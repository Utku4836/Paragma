package cli

import (
	"encoding/json"
	"os"
	"path/filepath"
	"time"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/Utku-Private/Paragma/internal/ulid"
	"github.com/Utku-Private/Paragma/internal/vault"
	"github.com/spf13/cobra"
)

func (a *app) initCmd() *cobra.Command {
	var global bool
	c := &cobra.Command{
		Use:   "init [dir]",
		Short: "Create a vault (.paragma/) in dir (default: current directory)",
		Long: `Create a vault: .paragma/ with work/ (working memory), episodes/ (errors,
decisions, journal), knowledge/ (rules, architecture, stack), archive/.
Safe to re-run; existing files are kept.`,
		Example: "pg init\npg init --global",
		Args:    cobra.MaximumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			var root string
			if global {
				root = vault.GlobalRoot()
			} else {
				dir := "."
				if len(args) == 1 {
					dir = args[0]
				}
				abs, err := filepath.Abs(dir)
				if err != nil {
					return fail(ExitUsage, "%v", err)
				}
				root = filepath.Join(abs, vault.DirName)
			}
			v, created, err := vault.Init(root, ulid.New, time.Now().Format("2006-01-02"))
			if err != nil {
				return fail(ExitInternal, "init: %v", err)
			}
			db, err := index.Open(v)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			defer db.Close()
			st, err := db.Sync()
			if err != nil {
				return fail(ExitInternal, "sync: %v", err)
			}
			if a.jsonOut {
				return a.emitJSON(map[string]any{"vault": v.Root, "created": created, "files": st.Files})
			}
			if created {
				a.printf("ok created %s (%d notes)\n", filepath.ToSlash(v.Root), st.Files)
			} else {
				a.printf("ok exists %s (%d notes)\n", filepath.ToSlash(v.Root), st.Files)
			}
			return nil
		},
	}
	c.Flags().BoolVar(&global, "global", false, "create the global vault (~/.paragma)")
	return c
}

func (a *app) reindexCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "reindex",
		Short: "Rebuild the index from the Markdown files",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			sc := a.scope
			if sc == "all" {
				sc = ""
			}
			v, err := vault.Find(a.vaultFlag, sc)
			if err != nil {
				return fail(ExitNoVault, "%v", err)
			}
			start := time.Now()
			db, err := index.Reset(v)
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			defer db.Close()
			st, err := db.Sync()
			if err != nil {
				return fail(ExitInternal, "sync: %v", err)
			}
			secs := db.Count(`SELECT COUNT(*) FROM sections`)
			if a.jsonOut {
				return a.emitJSON(map[string]any{"files": st.Files, "sections": secs, "ms": time.Since(start).Milliseconds()})
			}
			a.printf("ok reindexed %d files, %d sections in %dms\n", st.Files, secs, time.Since(start).Milliseconds())
			return nil
		},
	}
}

func (a *app) statusCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "status",
		Short: "Vault location, counts and index health",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			files := s.res.Files()
			layers := map[string]int{}
			tokens := 0
			fmErr := 0
			for _, f := range files {
				layers[f.Layer]++
				tokens += f.Tokens
				if f.FMError != "" {
					fmErr++
				}
			}
			db := s.db
			secs := db.Count(`SELECT COUNT(*) FROM sections`)
			tasksAll := db.Count(`SELECT COUNT(*) FROM tasks`)
			tasksOpen := db.Count(`SELECT COUNT(*) FROM tasks WHERE state NOT IN ('x','X','-')`)
			links, _ := db.AllLinks()
			broken := 0
			for _, l := range links {
				if !linkResolves(s.res, l) {
					broken++
				}
			}
			if a.jsonOut {
				return a.emitJSON(map[string]any{
					"vault": s.v.Root, "global": s.v.Global, "files": len(files), "layers": layers,
					"sections": secs, "tokens": tokens, "tasks_open": tasksOpen, "tasks": tasksAll,
					"links": len(links), "broken_links": broken, "frontmatter_errors": fmErr,
				})
			}
			a.printf("vault %s\n", s.v.Display())
			a.printf("files %d (working %d, episodic %d, semantic %d, other %d)  sections %d  %s\n",
				len(files), layers["working"], layers["episodic"], layers["semantic"],
				len(files)-layers["working"]-layers["episodic"]-layers["semantic"], secs, textutil.FmtTokens(tokens))
			a.printf("tasks %d open / %d  links %d (%d broken)  frontmatter errors %d\n", tasksOpen, tasksAll, len(links), broken, fmErr)
			return nil
		},
	}
}

func linkResolves(r *addr.Resolver, l index.LinkRow) bool {
	p, err := addr.Parse(l.Target)
	if err != nil {
		return false
	}
	if p.File == "" && p.ULID == "" && p.Block == "" {
		return false
	}
	if p.File == "" && p.ULID == "" {
		// same-file reference: [[#Heading]] or [[^blk]]
		f, err := r.DB.FileByID(l.SrcFile)
		if err != nil {
			return false
		}
		p.File = f.Addr()
	}
	_, err = r.Resolve(p)
	return err == nil
}

func (a *app) emitJSON(v any) error {
	enc := json.NewEncoder(a.stdout)
	enc.SetEscapeHTML(false)
	return enc.Encode(v)
}

var _ = os.Getwd
