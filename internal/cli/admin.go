package cli

import (
	"encoding/json"
	"errors"
	"path/filepath"
	"time"

	"github.com/Utku-Private/Paragma/internal/addr"
	"github.com/Utku-Private/Paragma/internal/index"
	"github.com/Utku-Private/Paragma/internal/space"
	"github.com/Utku-Private/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

func (a *app) reindexCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "reindex",
		Short: "Rebuild the space's index from the Markdown files",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			v, err := space.Find(a.home(), a.spaceFlag, a.workDir())
			if err != nil {
				if errors.Is(err, space.ErrNoSpace) {
					return fail(ExitNoSpace, "%v", err)
				}
				return fail(ExitInternal, "%v", err)
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
				return a.emitJSON(map[string]any{"space": v.Name, "files": st.Files, "sections": secs, "ms": time.Since(start).Milliseconds()})
			}
			a.printf("ok reindexed space %s: %d files, %d sections in %dms\n", v.Name, st.Files, secs, time.Since(start).Milliseconds())
			return nil
		},
	}
}

func (a *app) statusCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "status",
		Short: "Space, current project, counts and index health",
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
					"space": s.v.Name, "root": s.v.Root, "project": s.project, "projects": s.v.Projects(), "files": len(files), "layers": layers,
					"sections": secs, "tokens": tokens, "tasks_open": tasksOpen, "tasks": tasksAll,
					"links": len(links), "broken_links": broken, "frontmatter_errors": fmErr,
				})
			}
			cur := s.project
			if cur == "" {
				cur = "none (pg resume <project>)"
			}
			a.printf("space %s  %s  projects %d  current project here: %s\n", s.v.Name, filepath.ToSlash(s.v.Root), len(s.v.Projects()), cur)
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
