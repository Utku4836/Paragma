package cli

import (
	"fmt"
	"os"
	"path"
	"path/filepath"
	"regexp"
	"strings"
	"unicode"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/space"
	"github.com/Utku4836/Paragma/internal/textutil"
	"github.com/spf13/cobra"
)

// Moving, archiving and deleting whole notes.

func (a *app) mvCmd() *cobra.Command {
	return &cobra.Command{
		Use:     "mv <note|folder> <new name|path|folder/>",
		Aliases: []string{"rename", "move"},
		Short:   "Rename or move a note, a folder or a whole project; links to it are rewritten",
		Long: `Rename or move a note, a folder inside a project, or a project, in one
command. The first argument is a note address, a folder path (billing/old)
or a project name. The second is:
  a bare name      rename in place:            pg mv billing/webhook hooks
  a folder/        move into it, same name:    pg mv billing/hooks/raw-body billing/
  a path           move there (a path that does not start with a project
                   stays in the note's own project): pg mv billing/raw-body notes/raw-body
Renaming a project renames its folder and its project note, and keeps it the
current project. Every [[link]] that pointed at a moved note still does.`,
		Example: "pg mv auth-notes auth\npg mv billing/webhook billing/hooks\npg mv billing/hooks/raw-body billing/\npg mv billing invoicing",
		Args:    cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, err := a.main()
			if err != nil {
				return err
			}
			if dir := a.folderArg(s, args[0]); dir != "" {
				return a.withLock(s, func() error { return a.moveFolder(s, dir, args[1]) })
			}
			_, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			if t.Kind != addr.KindFile {
				return fail(ExitUsage, "mv moves whole notes and folders; give a note address or a folder")
			}
			dest := a.destPath(s, path.Dir(t.File.Path), strings.TrimSuffix(path.Base(t.File.Path), ".md"), args[1]) + ".md"
			return a.withLock(s, func() error { return a.move(s, t.File, dest, true) })
		},
	}
}

// folderArg is the folder (relative to the space root) that arg names: a
// project, a path inside the space, or a folder of the current project.
func (a *app) folderArg(s *store, arg string) string {
	arg = strings.Trim(filepath.ToSlash(arg), "/")
	for _, cand := range []string{arg, s.project + "/" + arg} {
		if cand == "" || strings.HasPrefix(cand, "/") || strings.Contains("/"+cand+"/", "/../") {
			continue
		}
		if st, err := os.Stat(filepath.Join(s.v.Root, filepath.FromSlash(cand))); err == nil && st.IsDir() && !strings.HasPrefix(path.Base(cand), ".") {
			return cand
		}
	}
	return ""
}

// destPath is where something now at dir/name goes for the second argument
// of pg mv (without .md): a bare name renames in place, a trailing slash
// or an existing folder moves into it, any other path is taken from the
// space root when it starts with a project, else from the source's project.
func (a *app) destPath(s *store, dir, name, arg string) string {
	arg = strings.TrimSuffix(strings.TrimPrefix(filepath.ToSlash(arg), "./"), ".md")
	inside := func(p string) string {
		top := strings.SplitN(p, "/", 2)[0]
		if s.v.HasProject(top) || space.ProjectOf(dir+"/x") == "" {
			return p
		}
		return space.ProjectOf(dir+"/x") + "/" + p
	}
	if strings.HasSuffix(arg, "/") {
		return strings.TrimPrefix(inside(strings.TrimSuffix(arg, "/"))+"/"+name, "/")
	}
	if !strings.Contains(arg, "/") {
		if d := a.folderArg(s, arg); d != "" && d != dir+"/"+name && (s.v.HasProject(arg) || d == path.Join(dir, arg)) {
			return d + "/" + name // an existing folder: move into it
		}
		return strings.TrimPrefix(path.Join(dir, arg), "./")
	}
	return inside(arg)
}

// moveFolder renames or moves a folder with every note in it; a project
// folder also renames its project note and stays the current project.
func (a *app) moveFolder(s *store, dir, arg string) error {
	parent, name := path.Dir(dir), path.Base(dir)
	if parent == "." {
		parent = ""
	}
	dest := a.destPath(s, parent, name, arg)
	if parent == "" && !strings.Contains(strings.Trim(filepath.ToSlash(arg), "/"), "/") {
		dest = strings.Trim(filepath.ToSlash(arg), "/") // a project gets a new name
	}
	if dest == dir || strings.HasPrefix(dest+"/", dir+"/") {
		return fail(ExitUsage, "cannot move %s into itself", dir)
	}
	if _, err := os.Stat(filepath.Join(s.v.Root, filepath.FromSlash(dest))); err == nil {
		return fail(ExitConflict, "%s already exists", dest)
	}
	isProject := parent == "" && !strings.Contains(dest, "/")
	if isProject && !space.ValidName(dest) {
		return fail(ExitUsage, "bad project name %q", dest)
	}
	var moves []movePair
	for _, f := range s.res.Files() {
		if !strings.HasPrefix(f.Path, dir+"/") {
			continue
		}
		np := dest + "/" + strings.TrimPrefix(f.Path, dir+"/")
		if isProject && f.Path == dir+"/"+name+".md" {
			np = dest + "/" + dest + ".md" // the project note follows the folder's name
		}
		moves = append(moves, movePair{f, np})
	}
	if err := a.moveNotes(s, moves, true, dest); err != nil {
		return err
	}
	if !a.dryRun {
		// the folder itself, with anything that is not a note
		src, dst := filepath.Join(s.v.Root, filepath.FromSlash(dir)), filepath.Join(s.v.Root, filepath.FromSlash(dest))
		if _, err := os.Stat(src); err == nil {
			moveRest(src, dst)
		}
	}
	if isProject && !a.dryRun {
		if f := projectNote(s, dest); f != nil && textutil.Fold(f.Title) == textutil.Fold(name) {
			// a title that was just the name follows the new name, in the same case
			title := dest
			if r := []rune(f.Title); len(r) > 0 && unicode.IsUpper(r[0]) {
				title = strings.ToUpper(dest[:1]) + dest[1:]
			}
			if n, err := a.load(s, f); err == nil {
				n.lines = setFront(n.lines, "title", yamlString(title))
				for i, l := range n.lines {
					if strings.TrimSpace(l) == "# "+f.Title {
						n.lines[i] = "# " + title
						break
					}
				}
				if err := a.save(n); err != nil {
					return err
				}
			}
		}
		reg := space.LoadRegistry(s.v.Home)
		for _, d := range reg.Dirs {
			if d.Projects[s.v.Name] == name {
				d.Projects[s.v.Name] = dest
			}
		}
		if err := reg.Save(); err != nil {
			return fail(ExitInternal, "%v", err)
		}
		if s.project == name {
			s.project, s.res.Prefer = dest, dest
		}
	}
	return nil
}

// moveRest moves what is left in src (files that are not notes, empty
// folders) to dst and removes src.
func moveRest(src, dst string) {
	filepath.WalkDir(src, func(p string, e os.DirEntry, err error) error {
		if err != nil || e.IsDir() {
			return nil
		}
		rel, _ := filepath.Rel(src, p)
		t := filepath.Join(dst, rel)
		os.MkdirAll(filepath.Dir(t), 0o755)
		os.Rename(p, t)
		return nil
	})
	os.MkdirAll(dst, 0o755)
	os.RemoveAll(src) // only empty folders are left
}

// move renames f to dest (relative to the space root) and, if rewrite,
// updates the links that point at it.
func (a *app) move(s *store, f *index.File, dest string, rewrite bool) error {
	return a.moveNotes(s, []movePair{{f, dest}}, rewrite, "")
}

type movePair struct {
	f    *index.File
	dest string // relative to the space root, with .md
}

// moveNotes moves notes on disk, re-indexes them and, if rewrite, rewrites
// every [[link]] to a moved note that no longer finds it. folder, when set,
// is what the ok line names (a moved folder).
func (a *app) moveNotes(s *store, moves []movePair, rewrite bool, folder string) error {
	if len(moves) == 0 {
		return fail(ExitNotFound, "no notes to move")
	}
	for _, m := range moves {
		if _, err := os.Stat(filepath.Join(s.v.Root, filepath.FromSlash(m.dest))); err == nil {
			return fail(ExitConflict, "%s already exists", strings.TrimSuffix(m.dest, ".md"))
		}
	}
	// the links to each moved note, collected while they still resolve
	type fix struct {
		src    string // path of the linking note before the move
		raw    string // the link target as written, without #/^ part
		target string // the moved note's new path
	}
	var fixes []fix
	if rewrite {
		seen := map[string]bool{}
		for _, m := range moves {
			ls, err := s.db.LinksTo(fileKeys(m.f))
			if err != nil {
				return fail(ExitInternal, "%v", err)
			}
			for _, l := range ls {
				src, err := s.db.FileByID(l.SrcFile)
				if err != nil {
					continue
				}
				if tt, err := resolveLink(s.res, src, l.Target); err != nil || tt.File.ID != m.f.ID {
					continue
				}
				raw := l.Target
				if i := strings.IndexAny(raw, "#^"); i >= 0 {
					raw = raw[:i]
				}
				if k := src.Path + "\x00" + raw; raw != "" && !seen[k] {
					seen[k] = true
					fixes = append(fixes, fix{src.Path, raw, m.dest})
				}
			}
		}
	}
	if a.dryRun {
		if len(moves) == 1 {
			a.printf("would move %s -> %s and check %d links\n", moves[0].f.Addr(), strings.TrimSuffix(moves[0].dest, ".md"), len(fixes))
		} else {
			a.printf("would move %d notes and check %d links\n", len(moves), len(fixes))
		}
		return nil
	}
	var changed []string
	newPath := map[string]string{}
	for _, m := range moves {
		dp := filepath.Join(s.v.Root, filepath.FromSlash(m.dest))
		if err := os.MkdirAll(filepath.Dir(dp), 0o755); err != nil {
			return fail(ExitInternal, "%v", err)
		}
		if err := os.Rename(filepath.Join(s.v.Root, filepath.FromSlash(m.f.Path)), dp); err != nil {
			return fail(ExitInternal, "move %s: %v", m.f.Addr(), err)
		}
		changed = append(changed, m.f.Path, m.dest)
		newPath[m.f.Path] = m.dest
	}
	if err := a.reindex(s, changed...); err != nil {
		return err
	}
	updated := map[string]bool{}
	for _, fx := range fixes {
		srcPath := fx.src
		if np, ok := newPath[srcPath]; ok {
			srcPath = np
		}
		src, err := s.res.ResolveFile(strings.TrimSuffix(srcPath, ".md"))
		if err != nil || src.Path != srcPath {
			continue
		}
		target, err := s.res.ResolveFile(strings.TrimSuffix(fx.target, ".md"))
		if err != nil {
			continue
		}
		if tt, err := resolveLink(s.res, src, fx.raw); err == nil && tt.File.ID == target.ID {
			continue // still finds it
		}
		n, err := a.load(s, src)
		if err != nil {
			return err
		}
		re := regexp.MustCompile(`\[\[` + regexp.QuoteMeta(fx.raw) + `([\]|#^])`)
		ref := shortRef(s, target)
		for i, l := range n.lines {
			n.lines[i] = re.ReplaceAllString(l, "[["+ref+"$1")
		}
		if err := a.save(n); err != nil {
			return err
		}
		updated[srcPath] = true
	}
	msg := strings.TrimSuffix(moves[0].dest, ".md")
	if folder != "" {
		msg = fmt.Sprintf("%s (%d notes)", folder, len(moves))
	}
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
