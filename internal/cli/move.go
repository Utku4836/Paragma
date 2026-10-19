package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/spf13/cobra"
)

// Moving, archiving and deleting whole notes.

func (a *app) mvCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "mv <addr> <new-name>",
		Short: "Rename or move a note and rewrite the [[links]] that point to it",
		Long: `Rename a note. A bare name keeps the directory; a name with "/" is a path
inside the space. Every [[link]] to the note is rewritten.`,
		Example: "pg mv auth-notes auth\npg mv scratch knowledge/concepts/token-rotation",
		Args:    cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			if t.Kind != addr.KindFile {
				return fail(ExitUsage, "mv moves whole notes; give a note address")
			}
			dest := strings.TrimSuffix(filepath.ToSlash(args[1]), ".md")
			if !strings.Contains(dest, "/") {
				dest = filepath.ToSlash(filepath.Dir(t.File.Path)) + "/" + dest
			}
			dest = strings.TrimPrefix(dest, "./") + ".md"
			return a.withLock(s, func() error {
				return a.move(s, t.File, dest, true)
			})
		},
	}
}

// move renames f to dest (relative to the space root) and, if rewrite, updates links.
func (a *app) move(s *store, f *index.File, dest string, rewrite bool) error {
	dp := filepath.Join(s.v.Root, filepath.FromSlash(dest))
	if _, err := os.Stat(dp); err == nil {
		return fail(ExitConflict, "%s already exists", strings.TrimSuffix(dest, ".md"))
	}
	// collect links to f before the move, while they still resolve
	type fix struct {
		src *index.File
		old string
	}
	var fixes []fix
	if rewrite {
		ls, err := s.db.LinksTo(fileKeys(f))
		if err != nil {
			return fail(ExitInternal, "%v", err)
		}
		seen := map[string]bool{}
		for _, l := range ls {
			src, err := s.db.FileByID(l.SrcFile)
			if err != nil {
				continue
			}
			tt, err := resolveLink(s.res, src, l.Target)
			if err != nil || tt.File.ID != f.ID {
				continue
			}
			raw := l.Target
			if i := strings.IndexAny(raw, "#^"); i >= 0 {
				raw = raw[:i]
			}
			if raw == "" || seen[src.Path+"\x00"+raw] {
				continue
			}
			seen[src.Path+"\x00"+raw] = true
			fixes = append(fixes, fix{src, raw})
		}
	}
	if a.dryRun {
		a.printf("would move %s -> %s and rewrite %d links\n", f.Addr(), strings.TrimSuffix(dest, ".md"), len(fixes))
		return nil
	}
	if err := os.MkdirAll(filepath.Dir(dp), 0o755); err != nil {
		return fail(ExitInternal, "%v", err)
	}
	if err := os.Rename(filepath.Join(s.v.Root, filepath.FromSlash(f.Path)), dp); err != nil {
		return fail(ExitInternal, "move: %v", err)
	}
	changed := []string{f.Path, dest}
	newRef := strings.TrimSuffix(filepath.Base(dest), ".md")
	if err := a.reindex(s, changed...); err != nil {
		return err
	}
	if g, err := s.res.ResolveFile(newRef); err != nil || g.Path != dest {
		newRef = strings.TrimSuffix(dest, ".md")
	}
	updated := map[string]bool{}
	for _, fx := range fixes {
		srcPath := fx.src.Path
		if srcPath == f.Path {
			srcPath = dest
		}
		g, err := s.res.ResolveFile(strings.TrimSuffix(srcPath, ".md"))
		if err != nil {
			continue
		}
		n, err := a.load(s, g)
		if err != nil {
			return err
		}
		re := regexp.MustCompile(`\[\[` + regexp.QuoteMeta(fx.old) + `([\]|#^])`)
		for i, l := range n.lines {
			n.lines[i] = re.ReplaceAllString(l, "[["+newRef+"$1")
		}
		if err := a.save(n); err != nil {
			return err
		}
		updated[srcPath] = true
	}
	msg := strings.TrimSuffix(dest, ".md")
	if len(updated) > 0 {
		msg += fmt.Sprintf(" (links updated in %d notes)", len(updated))
	}
	a.ok(msg)
	return nil
}

func (a *app) archiveCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "archive <addr>",
		Short:   "Move a note to its project's archive/ (kept, searchable with --all, ranked low)",
		Example: "pg archive old-deploy",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			if t.Kind != addr.KindFile {
				return fail(ExitUsage, "archive works on whole notes")
			}
			if space.Archived(t.File.Path) {
				return fail(ExitValidation, "%s is already archived", t.File.Addr())
			}
			return a.withLock(s, func() error { return a.archiveFile(s, t.File) })
		},
	}
}

// archiveFile marks a note archived and moves it under its project's
// archive/, keeping the rest of its path so links by name keep resolving.
func (a *app) archiveFile(s *store, f *index.File) error {
	n, err := a.load(s, f)
	if err != nil {
		return err
	}
	n.lines = setFront(n.lines, "status", "archived")
	if err := a.save(n); err != nil {
		return err
	}
	dest := "archive/" + f.Path
	if p := space.ProjectOf(f.Path); p != "" {
		dest = p + "/archive/" + strings.TrimPrefix(f.Path, p+"/")
	}
	return a.move(s, f, dest, false)
}

func (a *app) rmCmd() *cobra.Command {
	var hard bool
	c := &cobra.Command{
		Use:     "rm <addr> --hard",
		Short:   "Delete a note for good (prefer pg archive)",
		Example: "pg rm scratch --hard",
		Args:    cobra.ExactArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if !hard {
				return fail(ExitUsage, "rm deletes for good; pass --hard, or keep it with: pg archive %s", args[0])
			}
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			if t.Kind != addr.KindFile {
				return fail(ExitUsage, "rm deletes whole notes; to remove part of one use pg write")
			}
			bl, _ := a.backlinks(s, t)
			return a.withLock(s, func() error {
				if a.dryRun {
					a.printf("would delete %s (%d links to it would break)\n", t.File.Addr(), len(bl))
					return nil
				}
				if err := os.Remove(filepath.Join(s.v.Root, filepath.FromSlash(t.File.Path))); err != nil {
					return fail(ExitInternal, "%v", err)
				}
				if err := a.reindex(s, t.File.Path); err != nil {
					return err
				}
				msg := "removed " + t.File.Addr()
				if len(bl) > 0 {
					msg += fmt.Sprintf(" (%d links to it are now broken)", len(bl))
				}
				a.ok(msg)
				return nil
			})
		},
	}
	c.Flags().BoolVar(&hard, "hard", false, "really delete the file")
	return c
}
