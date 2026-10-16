// Package space finds, creates and describes Paragma spaces. All memory
// lives under one home directory (~/.paragma, or $PARAGMA_HOME):
//
//	~/.paragma/
//	  config.toml        settings shared by every space
//	  spaces.json        which space and project each work directory uses
//	  Spaces/<space>/    one space: its notes, grouped in project folders
//	    .pg/             the space's index and working files (disposable)
//	    <project>/       a project: <project>.md, rules.md, errors/, decisions/ …
//
// A space is what Obsidian calls a vault: search, links and the graph never
// cross from one space to another.
package space

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// DefaultName is the space created on first use.
const DefaultName = "main"

// MetaDir is the hidden per-space directory for the index and working files.
const MetaDir = ".pg"

// Space is an opened space.
type Space struct {
	Name string
	Root string // Spaces/<name>, absolute; notes live under it
	Home string // the Paragma home
	Cfg  Config
}

// Config is <home>/config.toml.
type Config struct {
	Version     int
	Budget      int
	ReadLimit   int
	StaleDays   int
	Hints       bool
	AutoPromote bool   // a fix seen 3 times becomes a rule
	Lang        string // language of section headings pg writes: en | tr
}

func DefaultConfig() Config {
	return Config{Version: 2, Budget: 800, ReadLimit: 1500, StaleDays: 90, Hints: true, Lang: "en", AutoPromote: true}
}

const configTemplate = `# Paragma settings (shared by every space)
version = 2
budget = 800        # default output budget in tokens for list-like commands
read_limit = 1500   # pg read shows an outline instead when the target is bigger (override: --full)
stale_days = 90     # semantic notes not verified for this long are flagged [stale?]
hints = true        # print one-line "next:" hints
auto_promote = true # an error fix that worked 3 times becomes a rule in the project's rules.md
lang = "en"         # language of section headings in notes pg creates: en | tr
`

// ErrNoSpace is returned when a named space does not exist.
var ErrNoSpace = errors.New("no such space")

// Home returns the Paragma home directory. override (the --home flag) wins
// over $PARAGMA_HOME.
func Home(override string) string {
	h := override
	if h == "" {
		h = os.Getenv("PARAGMA_HOME")
	}
	if h != "" {
		if abs, err := filepath.Abs(h); err == nil {
			return abs
		}
		return h
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return ".paragma"
	}
	return filepath.Join(home, ".paragma")
}

// SpacesDir is <home>/Spaces.
func SpacesDir(home string) string { return filepath.Join(home, "Spaces") }

var reName = regexp.MustCompile(`^[\p{L}\p{N}][\p{L}\p{N}._-]*$`)

// ValidName reports whether name can be a space or project folder name.
func ValidName(name string) bool { return reName.MatchString(name) && len(name) <= 64 }

// EnsureHome creates the home directory and its config file if missing.
func EnsureHome(home string) error {
	if err := os.MkdirAll(SpacesDir(home), 0o755); err != nil {
		return err
	}
	p := filepath.Join(home, "config.toml")
	if _, err := os.Stat(p); os.IsNotExist(err) {
		return os.WriteFile(p, []byte(configTemplate), 0o644)
	}
	return nil
}

// Exists reports whether a space folder exists.
func Exists(home, name string) bool {
	st, err := os.Stat(filepath.Join(SpacesDir(home), name))
	return err == nil && st.IsDir()
}

// Create makes a new, empty space. It is idempotent.
func Create(home, name string) (*Space, error) {
	if !ValidName(name) {
		return nil, fmt.Errorf("invalid space name %q (letters, digits, . _ -)", name)
	}
	if err := EnsureHome(home); err != nil {
		return nil, err
	}
	if err := os.MkdirAll(filepath.Join(SpacesDir(home), name, MetaDir), 0o755); err != nil {
		return nil, err
	}
	return Open(home, name)
}

// List returns the names of all spaces, sorted.
func List(home string) []string {
	es, err := os.ReadDir(SpacesDir(home))
	if err != nil {
		return nil
	}
	var out []string
	for _, e := range es {
		if e.IsDir() && !strings.HasPrefix(e.Name(), ".") {
			out = append(out, e.Name())
		}
	}
	sort.Strings(out)
	return out
}

// Open opens an existing space.
func Open(home, name string) (*Space, error) {
	if !Exists(home, name) {
		return nil, fmt.Errorf("%w: %s", ErrNoSpace, name)
	}
	s := &Space{Name: name, Root: filepath.Join(SpacesDir(home), name), Home: home, Cfg: DefaultConfig()}
	if err := readConfig(filepath.Join(home, "config.toml"), &s.Cfg); err != nil && !os.IsNotExist(errors.Unwrap(err)) {
		return nil, err
	}
	if s.Cfg.Budget <= 0 {
		s.Cfg.Budget = 800
	}
	if s.Cfg.ReadLimit <= 0 {
		s.Cfg.ReadLimit = 1500
	}
	if s.Cfg.StaleDays <= 0 {
		s.Cfg.StaleDays = 90
	}
	return s, os.MkdirAll(filepath.Join(s.Root, MetaDir), 0o755)
}

// Find picks the space for this invocation: the explicit name (--space),
// $PARAGMA_SPACE, the space mapped to the work directory, the default
// space. The default space is created on first use; any other must exist.
func Find(home, explicit, workDir string) (*Space, error) {
	name := explicit
	if name == "" {
		name = os.Getenv("PARAGMA_SPACE")
	}
	if name == "" {
		reg := LoadRegistry(home)
		name = reg.Dir(workDir).Space
		if name == "" {
			name = reg.DefaultSpace()
		}
	}
	if !Exists(home, name) {
		if name != DefaultName {
			return nil, fmt.Errorf("%w: %s (pg space list; pg space new %s)", ErrNoSpace, name, name)
		}
		return Create(home, name)
	}
	return Open(home, name)
}

// DBPath is the index database location.
func (s *Space) DBPath() string { return filepath.Join(s.Root, MetaDir, "index.db") }

// Meta returns a path inside the space's .pg directory.
func (s *Space) Meta(name string) string { return filepath.Join(s.Root, MetaDir, name) }

// Skip reports whether a path (relative to Root, slash separated) is not part
// of the note set.
func (s *Space) Skip(rel string, isDir bool) bool {
	base := rel
	if i := strings.LastIndex(rel, "/"); i >= 0 {
		base = rel[i+1:]
	}
	if strings.HasPrefix(base, ".") {
		return true
	}
	if isDir {
		return false
	}
	return !strings.HasSuffix(strings.ToLower(base), ".md")
}

// Projects lists the project folders of the space: its top-level
// directories.
func (s *Space) Projects() []string {
	es, err := os.ReadDir(s.Root)
	if err != nil {
		return nil
	}
	var out []string
	for _, e := range es {
		if e.IsDir() && !strings.HasPrefix(e.Name(), ".") {
			out = append(out, e.Name())
		}
	}
	sort.Strings(out)
	return out
}

// HasProject reports whether the project folder exists.
func (s *Space) HasProject(name string) bool {
	st, err := os.Stat(filepath.Join(s.Root, name))
	return name != "" && err == nil && st.IsDir()
}

// ProjectOf returns the project folder a note path belongs to ("" for notes
// at the space root).
func ProjectOf(rel string) string {
	if i := strings.Index(rel, "/"); i > 0 {
		return rel[:i]
	}
	return ""
}

// Archived reports whether a note path is inside an archive/ folder.
func Archived(rel string) bool {
	return strings.HasPrefix(rel, "archive/") || strings.Contains(rel, "/archive/")
}

// InferType guesses the note type from the path: the folders pg writes to
// (errors/, decisions/, journal/, concepts/), rules.md, and the project note
// <project>/<project>.md.
func InferType(rel string) string {
	segs := strings.Split(rel, "/")
	base := strings.ToLower(segs[len(segs)-1])
	for _, d := range segs[:len(segs)-1] {
		switch strings.ToLower(d) {
		case "errors":
			return "error"
		case "decisions":
			return "decision"
		case "journal":
			return "journal"
		case "concepts":
			return "concept"
		}
	}
	if base == "rules.md" {
		return "rule"
	}
	if len(segs) == 2 && strings.EqualFold(base, segs[0]+".md") {
		return "project"
	}
	return "note"
}

// LayerOf maps a note type to its memory layer.
func LayerOf(typ string) string {
	switch typ {
	case "error", "decision", "journal":
		return "episodic"
	case "rule", "concept":
		return "semantic"
	case "project", "session", "branch":
		return "working"
	}
	return ""
}

// InferLayer guesses the memory layer from the path.
func InferLayer(rel string) string { return LayerOf(InferType(rel)) }

// readConfig reads the flat `key = value` subset of TOML that config.toml
// uses. A full TOML library cost ~60ms of process start-up on Windows, which
// matters for a tool agents call dozens of times per session.
func readConfig(path string, c *Config) error {
	b, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("config.toml: %w", err)
	}
	for i, line := range strings.Split(string(b), "\n") {
		if j := strings.Index(line, "#"); j >= 0 {
			line = line[:j]
		}
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "[") {
			continue
		}
		k, val, ok := strings.Cut(line, "=")
		if !ok {
			return fmt.Errorf("config.toml:%d: expected key = value", i+1)
		}
		k, val = strings.TrimSpace(k), strings.Trim(strings.TrimSpace(val), `"`)
		num := func() (int, error) {
			n, err := strconv.Atoi(val)
			if err != nil {
				return 0, fmt.Errorf("config.toml:%d: %s must be a number", i+1, k)
			}
			return n, nil
		}
		switch k {
		case "version":
			c.Version, err = num()
		case "budget":
			c.Budget, err = num()
		case "read_limit":
			c.ReadLimit, err = num()
		case "stale_days":
			c.StaleDays, err = num()
		case "lang":
			c.Lang = val
		case "auto_promote":
			c.AutoPromote = val == "true"
		case "hints":
			c.Hints = val == "true"
		}
		if err != nil {
			return err
		}
	}
	return nil
}
