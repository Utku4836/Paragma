// Package vault finds, creates and describes Paragma vaults. A vault is a
// `.paragma/` directory; its notes live inside it and index.db sits next to
// them. The global vault is ~/.paragma (or $PARAGMA_HOME).
package vault

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
)

const DirName = ".paragma"

// Vault is an opened vault.
type Vault struct {
	Root   string // the .paragma directory, absolute
	Base   string // directory that contains .paragma (the project); == Root for the global vault
	Global bool
	Cfg    Config
}

// Config is .paragma/config.toml.
type Config struct {
	Version   int  `toml:"version"`
	Budget    int  `toml:"budget"`
	ReadLimit int  `toml:"read_limit"`
	StaleDays int  `toml:"stale_days"`
	Hints     bool `toml:"hints"`
}

func DefaultConfig() Config {
	return Config{Version: 1, Budget: 800, ReadLimit: 1500, StaleDays: 90, Hints: true}
}

const configTemplate = `# Paragma vault settings
version = 1
budget = 800        # default output budget in tokens for list-like commands
read_limit = 1500   # pg read shows an outline instead when the target is bigger (override: --full)
stale_days = 90     # semantic notes not verified for this long are flagged [stale?]
hints = true        # print one-line "next:" hints
`

// ErrNoVault is returned when no vault can be found.
var ErrNoVault = errors.New("no vault found")

// GlobalRoot returns the global vault directory.
func GlobalRoot() string {
	if h := os.Getenv("PARAGMA_HOME"); h != "" {
		abs, err := filepath.Abs(h)
		if err == nil {
			return abs
		}
		return h
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return DirName
	}
	return filepath.Join(home, DirName)
}

func isVault(root string) bool {
	st, err := os.Stat(filepath.Join(root, "config.toml"))
	return err == nil && !st.IsDir()
}

// Find locates the vault to use. explicit is --vault (project dir or the
// .paragma dir itself); scope is "project", "global" or "" (auto).
func Find(explicit, scope string) (*Vault, error) {
	groot := GlobalRoot()
	if scope == "global" {
		if !isVault(groot) {
			return nil, fmt.Errorf("%w: global vault %s not initialised (pg init --global)", ErrNoVault, groot)
		}
		return Open(groot)
	}
	if explicit == "" {
		explicit = os.Getenv("PARAGMA_VAULT")
	}
	if explicit != "" {
		abs, err := filepath.Abs(explicit)
		if err != nil {
			return nil, err
		}
		for _, cand := range []string{abs, filepath.Join(abs, DirName)} {
			if isVault(cand) {
				return Open(cand)
			}
		}
		return nil, fmt.Errorf("%w at %s", ErrNoVault, explicit)
	}
	cwd, err := os.Getwd()
	if err != nil {
		return nil, err
	}
	for dir := cwd; ; {
		cand := filepath.Join(dir, DirName)
		if !samePath(cand, groot) && isVault(cand) {
			return Open(cand)
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	if scope == "project" {
		return nil, fmt.Errorf("%w: no project vault above %s (pg init)", ErrNoVault, cwd)
	}
	if isVault(groot) {
		return Open(groot)
	}
	return nil, fmt.Errorf("%w: no .paragma above %s and no global vault (run: pg init)", ErrNoVault, cwd)
}

// Open opens the vault rooted at root (a .paragma directory).
func Open(root string) (*Vault, error) {
	root, _ = filepath.Abs(root)
	v := &Vault{Root: root, Base: filepath.Dir(root), Cfg: DefaultConfig()}
	if samePath(root, GlobalRoot()) {
		v.Global = true
		v.Base = root
	}
	if err := readConfig(filepath.Join(root, "config.toml"), &v.Cfg); err != nil {
		return nil, err
	}
	if v.Cfg.Budget <= 0 {
		v.Cfg.Budget = 800
	}
	if v.Cfg.ReadLimit <= 0 {
		v.Cfg.ReadLimit = 1500
	}
	if v.Cfg.StaleDays <= 0 {
		v.Cfg.StaleDays = 90
	}
	return v, nil
}

// DBPath is the index database location.
func (v *Vault) DBPath() string { return filepath.Join(v.Root, "index.db") }

// Display is a short human name for the vault location.
func (v *Vault) Display() string {
	if v.Global {
		return "~/" + DirName + " (global)"
	}
	if cwd, err := os.Getwd(); err == nil {
		if rel, err := filepath.Rel(cwd, v.Root); err == nil && !strings.HasPrefix(rel, "..") {
			return "./" + filepath.ToSlash(rel)
		}
	}
	return filepath.ToSlash(v.Root)
}

// Skip reports whether a path (relative to Root, slash separated) is not part
// of the note set.
func (v *Vault) Skip(rel string, isDir bool) bool {
	base := rel
	if i := strings.LastIndex(rel, "/"); i >= 0 {
		base = rel[i+1:]
	}
	if strings.HasPrefix(base, ".") {
		return true
	}
	if isDir {
		// repo-external projects live under the global vault but are vaults of their own
		return v.Global && rel == "projects"
	}
	if !strings.HasSuffix(strings.ToLower(base), ".md") {
		return true
	}
	return rel == "INDEX.md"
}

var layout = []string{
	"work/sessions",
	"episodes/errors",
	"episodes/decisions",
	"episodes/journal",
	"knowledge/concepts",
	"archive",
}

// Init creates a vault at root. It is idempotent: existing files are kept.
func Init(root string, newID func() string, today string) (*Vault, bool, error) {
	created := !isVault(root)
	for _, d := range layout {
		if err := os.MkdirAll(filepath.Join(root, filepath.FromSlash(d)), 0o755); err != nil {
			return nil, false, err
		}
	}
	files := map[string]string{
		"config.toml":               configTemplate,
		".gitignore":                "index.db\nindex.db-*\nlock\n",
		"knowledge/rules.md":        fmt.Sprintf("---\nid: %s\ntype: rule\nlayer: semantic\ntitle: Rules\ncreated: %s\nverified: %s\n---\n# Rules\n\nProject rules, one `> [!rule]` callout each.\n", newID(), today, today),
		"knowledge/architecture.md": fmt.Sprintf("---\nid: %s\ntype: note\nlayer: semantic\ntitle: Architecture\ncreated: %s\nverified: %s\n---\n# Architecture\n", newID(), today, today),
		"knowledge/stack.md":        fmt.Sprintf("---\nid: %s\ntype: note\nlayer: semantic\ntitle: Stack\ncreated: %s\nverified: %s\n---\n# Stack\n", newID(), today, today),
		"work/inbox.md":             fmt.Sprintf("---\nid: %s\ntype: note\nlayer: working\ntitle: Inbox\ncreated: %s\n---\n# Inbox\n", newID(), today),
	}
	for rel, content := range files {
		p := filepath.Join(root, filepath.FromSlash(rel))
		if _, err := os.Stat(p); err == nil {
			continue
		}
		if err := os.WriteFile(p, []byte(content), 0o644); err != nil {
			return nil, false, err
		}
	}
	v, err := Open(root)
	return v, created, err
}

// InferLayer guesses the memory layer from the top-level directory.
func InferLayer(rel string) string {
	top, rest, _ := strings.Cut(rel, "/")
	switch top {
	case "work":
		return "working"
	case "episodes":
		return "episodic"
	case "knowledge":
		return "semantic"
	case "archive":
		return InferLayer(rest)
	}
	return ""
}

// InferType guesses the note type from the path.
func InferType(rel string) string {
	r := strings.TrimPrefix(rel, "archive/")
	switch {
	case strings.HasPrefix(r, "episodes/errors/"):
		return "error"
	case strings.HasPrefix(r, "episodes/decisions/"):
		return "decision"
	case strings.HasPrefix(r, "episodes/journal/"):
		return "journal"
	case strings.HasPrefix(r, "work/sessions/"), strings.Contains(r, "/sessions/") && strings.HasPrefix(r, "work/branches/"):
		return "session"
	case strings.HasPrefix(r, "work/branches/") && strings.HasSuffix(r, "/branch.md"):
		return "branch"
	case r == "knowledge/rules.md":
		return "rule"
	case strings.HasPrefix(r, "knowledge/concepts/"):
		return "concept"
	}
	return "note"
}

func samePath(a, b string) bool {
	a, _ = filepath.Abs(a)
	b, _ = filepath.Abs(b)
	return strings.EqualFold(filepath.Clean(a), filepath.Clean(b))
}

// readConfig reads the flat `key = value` subset of TOML that config.toml
// uses. A full TOML library cost ~60ms of process start-up on Windows, which
// matters for a tool agents call dozens of times per session.
func readConfig(path string, c *Config) error {
	b, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("config.toml: %v", err)
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
		case "hints":
			c.Hints = val == "true"
		}
		if err != nil {
			return err
		}
	}
	return nil
}
