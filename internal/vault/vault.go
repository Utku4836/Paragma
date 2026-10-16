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
	Version     int    `toml:"version"`
	Budget      int    `toml:"budget"`
	ReadLimit   int    `toml:"read_limit"`
	StaleDays   int    `toml:"stale_days"`
	Hints       bool   `toml:"hints"`
	AutoPromote bool   `toml:"auto_promote"` // a fix seen 3 times becomes a rule
	Lang        string `toml:"lang"`         // language of section headings pg writes: en | tr
}

func DefaultConfig() Config {
	return Config{Version: 1, Budget: 800, ReadLimit: 1500, StaleDays: 90, Hints: true, Lang: "en", AutoPromote: true}
}

const configTemplate = `# Paragma vault settings
version = 1
budget = 800        # default output budget in tokens for list-like commands
read_limit = 1500   # pg read shows an outline instead when the target is bigger (override: --full)
stale_days = 90     # semantic notes not verified for this long are flagged [stale?]
hints = true        # print one-line "next:" hints
auto_promote = true # an error fix that worked 3 times becomes a rule in knowledge/rules.md
lang = "%s"         # language of section headings in notes pg creates: en | tr
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
	// outside any project directory: the project chosen with pg project switch
	if root, err := LoadRegistry().ActiveVault(); err == nil && isVault(root) {
		return Open(root)
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
func Init(root string, newID func() string, today, lang string) (*Vault, bool, error) {
	if lang != "tr" {
		lang = "en"
	}
	t := func(en, tr string) string {
		if lang == "tr" {
			return tr
		}
		return en
	}
	created := !isVault(root)
	for _, d := range layout {
		if err := os.MkdirAll(filepath.Join(root, filepath.FromSlash(d)), 0o755); err != nil {
			return nil, false, err
		}
	}
	// a vault starts empty: notes are created when something is written
	// (pg rule add creates rules.md, pg task add creates work/inbox.md, ...)
	files := map[string]string{
		"config.toml": fmt.Sprintf(configTemplate, lang),
		".gitignore":  "index.db\nindex.db-*\nlock\nstate.json\nlast-run.json\nmisses.log\n",
	}
	_, _ = t, newID
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

// IgnoreInRepo keeps the project memory out of git: it adds ".paragma/" to
// the .gitignore of the project directory when that directory is a git
// work tree (or already has a .gitignore). It reports whether it wrote.
func IgnoreInRepo(base string) (bool, error) {
	gi := filepath.Join(base, ".gitignore")
	_, gitErr := os.Stat(filepath.Join(base, ".git"))
	old, err := os.ReadFile(gi)
	if err != nil && !os.IsNotExist(err) {
		return false, err
	}
	if os.IsNotExist(err) && gitErr != nil {
		return false, nil // not a git repository: nothing to protect
	}
	for _, l := range strings.Split(string(old), "\n") {
		switch strings.TrimSpace(l) {
		case DirName, DirName + "/", "/" + DirName, "/" + DirName + "/":
			return false, nil
		}
	}
	text := string(old)
	if text != "" && !strings.HasSuffix(text, "\n") {
		text += "\n"
	}
	text += "# Paragma project memory (local, not shared)\n" + DirName + "/\n"
	return true, WriteFileAtomic(gi, []byte(text))
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
	case strings.HasPrefix(r, "work/branches/") && (strings.HasSuffix(r, "/branch.md") || strings.Count(r, "/") == 2):
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
