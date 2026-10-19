package cli

import (
	"fmt"
	"path/filepath"
	"strings"

	"github.com/Utku4836/Paragma/internal/addr"
	"github.com/Utku4836/Paragma/internal/index"
	"github.com/spf13/cobra"
)

// Commands that change part of an existing note: append, write, insert,
// edit, section add, link, keywords.

func (a *app) appendCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "append <addr> <text|->",
		Short: "Add text at the end of a note, heading, block or line range",
		Long: `Add text at the end of the target. For a heading the text goes after the
heading's own paragraphs, before its sub-headings. Long text: pass "-" and
pipe it on stdin, or use --file PATH (PowerShell 5.1 pipes are not UTF-8).`,
		Example: `pg append rules#Database "> [!rule] Open SQLite in WAL mode"
pg append architecture#Middleware --file note.md`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			addrArgs, textArgs := a.splitAddrArgs(args)
			text, err := a.inputText(textArgs, true)
			if err != nil {
				return err
			}
			s, t, err := a.resolve(addrArgs)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				after := t.To
				switch t.Kind {
				case addr.KindFile:
					after = len(n.lines)
				case addr.KindSection:
					after = lastLine(n.lines, t.Section.Line, t.Section.BodyEnd)
				}
				n.lines, _ = place(n.lines, after, splitText(text))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
}

// splitAddrArgs separates the address from the text in "<addr> <text...>"
// commands. Normally the address is the first argument; when an unquoted
// heading with spaces was split by the shell, the longest prefix of the
// arguments that resolves is taken as the address.
func (a *app) splitAddrArgs(args []string) (string, []string) {
	if len(args) <= 1 || !strings.Contains(args[0], "#") {
		return args[0], args[1:]
	}
	if _, _, err := a.resolve(args[0]); err == nil {
		return args[0], args[1:]
	}
	for j := min(len(args)-1, 6); j >= 1; j-- {
		cand := strings.Join(args[:j+1], " ")
		if _, _, err := a.resolve(cand); err == nil {
			return cand, args[j+1:]
		}
	}
	return args[0], args[1:]
}

func (a *app) writeCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "write <addr> <text|->",
		Short: "Replace the body of a heading, a block, a line range or a whole note",
		Long: `Replace the target's text. A heading keeps its heading line and its
sub-headings; only its own paragraphs are replaced. A whole note keeps its
front matter. A block keeps its ^id even if the new text omits it.
Use --if-hash with the hash from pg read to refuse if someone else changed
the note meanwhile.`,
		Example: `pg write stack#Backend "Go 1.24, chi, sqlc"
pg write auth-refactor^t004 "- [ ] Tests for refresh endpoint ^t004"
pg write architecture#Deploy - --if-hash 3f2a9c1e`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			addrArgs, textArgs := a.splitAddrArgs(args)
			text, err := a.inputText(textArgs, false)
			if err != nil {
				return err
			}
			s, t, err := a.resolve(addrArgs)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				block := splitText(text)
				switch t.Kind {
				case addr.KindFile:
					body := n.lines[:t.From-1]
					n.lines = append(append([]string(nil), body...), block...)
				case addr.KindSection:
					from, to := t.Section.Line+1, lastLine(n.lines, t.Section.Line+1, t.Section.BodyEnd)
					if to >= from {
						n.lines = replaceRange(n.lines, from, to, nil)
					}
					n.lines, _ = place(n.lines, t.Section.Line, block)
				case addr.KindBlock:
					block = keepBlockID(block, t.Block)
					n.lines = replaceRange(n.lines, t.From, t.To, block)
				case addr.KindLines:
					n.lines = replaceRange(n.lines, t.From, t.To, block)
				}
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
}

// keepBlockID appends ^id to a rewritten block that lost it: on the first
// line of a list item, else on the last line.
func keepBlockID(block []string, id string) []string {
	if len(block) == 0 {
		return block
	}
	for _, l := range block {
		if strings.Contains(l, "^"+id) {
			return block
		}
	}
	out := append([]string(nil), block...)
	i := len(out) - 1
	if isListLine(out[0]) {
		i = 0
	}
	out[i] += " ^" + id
	return out
}

func (a *app) insertCmd() *cobra.Command {
	var before, after bool
	c := &cobra.Command{
		Use:   "insert <addr> --before|--after <text|->",
		Short: "Insert text right before or after a block, heading or line range",
		Example: `pg insert rules^r004 --after "> [!rule] Never log tokens ^r005"
pg insert stack:12 --before "## Runtime"`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if before == after {
				return fail(ExitUsage, "give exactly one of --before or --after")
			}
			addrArgs, textArgs := a.splitAddrArgs(args)
			text, err := a.inputText(textArgs, true)
			if err != nil {
				return err
			}
			s, t, err := a.resolve(addrArgs)
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				at := t.To
				if before {
					at = t.From - 1
				}
				n.lines, _ = place(n.lines, at, splitText(text))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr())
				return nil
			})
		},
	}
	c.Flags().BoolVar(&before, "before", false, "insert before the target")
	c.Flags().BoolVar(&after, "after", false, "insert after the target")
	return c
}

func (a *app) editCmd() *cobra.Command {
	var find, repl string
	var all bool
	c := &cobra.Command{
		Use:   "edit <addr> --find TEXT --replace TEXT",
		Short: "Replace exact text inside a note, heading, block or line range",
		Long: `Targeted find/replace limited to the addressed part. Fails with exit 3 if
the text is not there and exit 6 if it occurs more than once (use --all).`,
		Example: `pg edit stack --find "Go 1.22" --replace "Go 1.24"
pg edit rules^r002 --find "mock" --replace "fake" --all`,
		Args: cobra.MinimumNArgs(1),
		RunE: func(cmd *cobra.Command, args []string) error {
			if find == "" {
				return fail(ExitUsage, "--find is required")
			}
			s, t, err := a.resolve(strings.Join(args, " "))
			if err != nil {
				return err
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				from, to := t.From, t.To
				region := strings.Join(n.lines[from-1:to], "\n")
				c := strings.Count(region, find)
				switch {
				case c == 0:
					return fail(ExitNotFound, "not found in %s: %q", t.Addr(), find)
				case c > 1 && !all:
					return fail(ExitValidation, "%q occurs %d times in %s; narrow the address or add --all", find, c, t.Addr())
				}
				region = strings.Replace(region, find, repl, map[bool]int{true: -1, false: 1}[all])
				n.lines = replaceRange(n.lines, from, to, strings.Split(region, "\n"))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(fmt.Sprintf("%s (%d replaced)", t.Addr(), map[bool]int{true: c, false: 1}[all]))
				return nil
			})
		},
	}
	c.Flags().StringVar(&find, "find", "", "exact text to find")
	c.Flags().StringVar(&repl, "replace", "", "replacement text")
	c.Flags().BoolVar(&all, "all", false, "replace every occurrence")
	return c
}

func (a *app) sectionCmd() *cobra.Command {
	c := &cobra.Command{
		Use:   "section",
		Short: "Heading operations: section add",
	}
	c.AddCommand(&cobra.Command{
		Use:   "add <addr> <heading> [text|-]",
		Short: "Add a heading under a note or heading (at the end of it)",
		Example: `pg section add architecture "Caching"
pg section add architecture#Layers "Queue" "Workers read from Redis streams."`,
		Args: cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			heading := strings.TrimSpace(args[1])
			text, err := a.inputText(args[2:], false)
			if err != nil {
				return err
			}
			if t.Kind == addr.KindBlock || t.Kind == addr.KindLines {
				return fail(ExitUsage, "section add takes a note or a heading address")
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				level, after, parentPath := 2, len(n.lines), ""
				if t.Kind == addr.KindSection && !t.Section.IsTitle {
					level, after, parentPath = min(t.Section.Level+1, 6), t.Section.End, t.Section.Path
				} else if !hasTitle(s, t.File) {
					level = 1
				}
				block := []string{strings.Repeat("#", level) + " " + heading}
				if text != "" {
					block = append(block, "")
					block = append(block, splitText(text)...)
				}
				n.lines, _ = place(n.lines, lastLine(n.lines, 1, after), block)
				if err := a.save(n); err != nil {
					return err
				}
				p := heading
				if parentPath != "" {
					p = parentPath + "/" + heading
				}
				a.ok(t.File.Addr() + "#" + p)
				return nil
			})
		},
	})
	return c
}

func (a *app) linkCmd() *cobra.Command {
	var kind string
	c := &cobra.Command{
		Use:   "link <from-addr> <to-addr>",
		Short: "Add a [[link]] (optionally typed: fixes, supersedes, ...) at the end of the source",
		Example: `pg link 0007-wal-mode sqlite-database-locked --kind fixes
pg link auth-refactor#Notes architecture#Middleware`,
		Args: cobra.ExactArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			_, dst, err := a.resolve(args[1])
			if err != nil {
				return err
			}
			target := shortRef(s, dst.File)
			if sub := strings.TrimPrefix(dst.Addr(), dst.File.Addr()); sub != "" && !strings.HasPrefix(sub, ":") {
				target += sub
			}
			line := "- [[" + target + "]]"
			if kind != "" {
				line = strings.ToLower(kind) + ":: [[" + target + "]]"
			}
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				after := t.To
				switch t.Kind {
				case addr.KindFile:
					after = len(n.lines)
				case addr.KindSection:
					after = lastLine(n.lines, t.Section.Line, t.Section.BodyEnd)
				}
				n.lines, _ = place(n.lines, after, []string{line})
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.Addr() + " -> " + target)
				return nil
			})
		},
	}
	c.Flags().StringVar(&kind, "kind", "", "link type written as kind:: [[target]] (fixes, supersedes, ...)")
	return c
}

// shortRef is the shortest name that still resolves to f: its file name if
// unique, else its path.
func shortRef(s *store, f *index.File) string {
	base := filepath.Base(f.Addr())
	if g, err := s.res.ResolveFile(base); err == nil && g.ID == f.ID {
		return base
	}
	return f.Addr()
}

func (a *app) keywordsCmd() *cobra.Command {
	return &cobra.Command{
		Use:   "keywords <addr> <keyword>[, keyword...]",
		Short: "Add search keywords to a note: synonyms, the other language, words a future search will use",
		Long: `Add keywords to a note's front matter. They are searched like the
note's text, so a note written as "database is locked" can also be found
by "kilitlenme" or "sqlite busy". Use it after a search missed a note that
existed, or when recording something under one name that others will
look for under another.`,
		Example: `pg keywords sqlite-database-locked "kilitlenme, sqlite busy, lock contention"`,
		Args:    cobra.MinimumNArgs(2),
		RunE: func(cmd *cobra.Command, args []string) error {
			s, t, err := a.resolve(args[0])
			if err != nil {
				return err
			}
			add := keywordList(strings.Join(args[1:], ","))
			return a.withLock(s, func() error {
				n, err := a.load(s, t.File)
				if err != nil {
					return err
				}
				var have []string
				for _, l := range n.lines {
					if strings.HasPrefix(l, "keywords:") {
						have = keywordList(strings.Trim(strings.TrimSpace(strings.TrimPrefix(l, "keywords:")), "[]"))
						break
					}
				}
				all := keywordList(strings.Join(append(have, add...), ","))
				for i, k := range all {
					all[i] = strings.Trim(k, `"'`)
				}
				n.lines = setFront(n.lines, "keywords", yamlList(all))
				if err := a.save(n); err != nil {
					return err
				}
				a.ok(t.File.Addr() + " keywords: " + strings.Join(all, ", "))
				return nil
			})
		},
	}
}
