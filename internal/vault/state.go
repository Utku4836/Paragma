package vault

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

// State is per-machine working state kept in .paragma/state.json (ignored
// by git): which branch is active when git does not decide, and which
// session is current on each branch.
type State struct {
	Branch   string            `json:"branch,omitempty"`
	Sessions map[string]string `json:"sessions,omitempty"` // branch -> session file address
}

func (v *Vault) statePath() string { return filepath.Join(v.Root, "state.json") }

// LoadState reads state.json; a missing or broken file is an empty state.
func (v *Vault) LoadState() *State {
	st := &State{Sessions: map[string]string{}}
	b, err := os.ReadFile(v.statePath())
	if err == nil {
		_ = json.Unmarshal(b, st)
	}
	if st.Sessions == nil {
		st.Sessions = map[string]string{}
	}
	return st
}

// SaveState writes state.json atomically.
func (v *Vault) SaveState(st *State) error {
	b, err := json.MarshalIndent(st, "", "  ")
	if err != nil {
		return err
	}
	return WriteFileAtomic(v.statePath(), b)
}

// WriteFileAtomic writes via a temp file in the same directory and a rename,
// so a reader never sees a half-written file.
func WriteFileAtomic(path string, data []byte) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".pg-*.tmp")
	if err != nil {
		return err
	}
	name := tmp.Name()
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		os.Remove(name)
		return err
	}
	if err := tmp.Close(); err != nil {
		os.Remove(name)
		return err
	}
	// Windows refuses to rename over a file another process holds open;
	// retry briefly before giving up.
	for i := 0; ; i++ {
		err = os.Rename(name, path)
		if err == nil || i >= 20 {
			break
		}
		time.Sleep(25 * time.Millisecond)
	}
	if err != nil {
		os.Remove(name)
	}
	return err
}

// GitBranch returns the git branch checked out in dir (read from .git/HEAD
// without running git), or "" when dir is not a git work tree or HEAD is
// detached.
func GitBranch(dir string) string {
	for d := dir; ; {
		g := filepath.Join(d, ".git")
		if st, err := os.Stat(g); err == nil {
			head := filepath.Join(g, "HEAD")
			if !st.IsDir() {
				// worktree or submodule: ".git" is a file "gitdir: <path>"
				b, err := os.ReadFile(g)
				if err != nil {
					return ""
				}
				gd := strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(string(b)), "gitdir:"))
				if !filepath.IsAbs(gd) {
					gd = filepath.Join(d, gd)
				}
				head = filepath.Join(gd, "HEAD")
			}
			b, err := os.ReadFile(head)
			if err != nil {
				return ""
			}
			s := strings.TrimSpace(string(b))
			if r, ok := strings.CutPrefix(s, "ref: refs/heads/"); ok {
				return r
			}
			return ""
		}
		parent := filepath.Dir(d)
		if parent == d {
			return ""
		}
		d = parent
	}
}

// Branch returns the active branch and whether git decided it.
func (v *Vault) Branch() (string, bool) {
	if !v.Global {
		if b := GitBranch(v.Base); b != "" {
			return b, true
		}
	}
	if st := v.LoadState(); st.Branch != "" {
		return st.Branch, false
	}
	return "main", false
}

// ---- project registry ----

// Project is one registered vault.
type Project struct {
	Name     string `json:"name"`
	Path     string `json:"path"` // the .paragma directory (or the vault root for repo-less projects)
	LastUsed string `json:"last_used,omitempty"`
}

// Registry is ~/.paragma/registry.json.
type Registry struct {
	Active   string    `json:"active,omitempty"`
	Projects []Project `json:"projects"`
}

func registryPath() string { return filepath.Join(GlobalRoot(), "registry.json") }

// LoadRegistry reads the registry; a missing file is an empty registry.
func LoadRegistry() *Registry {
	r := &Registry{}
	if b, err := os.ReadFile(registryPath()); err == nil {
		_ = json.Unmarshal(b, r)
	}
	return r
}

// Save writes the registry, sorted by name.
func (r *Registry) Save() error {
	sort.Slice(r.Projects, func(i, j int) bool { return r.Projects[i].Name < r.Projects[j].Name })
	b, err := json.MarshalIndent(r, "", "  ")
	if err != nil {
		return err
	}
	return WriteFileAtomic(registryPath(), b)
}

// Find returns the project with this name (case-insensitive).
func (r *Registry) Find(name string) *Project {
	for i := range r.Projects {
		if strings.EqualFold(r.Projects[i].Name, name) {
			return &r.Projects[i]
		}
	}
	return nil
}

// ByPath returns the project registered for a vault root.
func (r *Registry) ByPath(root string) *Project {
	for i := range r.Projects {
		if samePath(r.Projects[i].Path, root) {
			return &r.Projects[i]
		}
	}
	return nil
}

// Register adds or refreshes a project entry. The name defaults to the
// project directory's name and is made unique.
func (r *Registry) Register(name, root string) *Project {
	if p := r.ByPath(root); p != nil {
		p.LastUsed = time.Now().Format("2006-01-02")
		return p
	}
	if name == "" {
		name = filepath.Base(filepath.Dir(root))
		if filepath.Base(root) != DirName {
			name = filepath.Base(root)
		}
	}
	base, n := name, 2
	for r.Find(name) != nil {
		name = base + "-" + itoa(n)
		n++
	}
	r.Projects = append(r.Projects, Project{Name: name, Path: root, LastUsed: time.Now().Format("2006-01-02")})
	return &r.Projects[len(r.Projects)-1]
}

// Remove drops a project from the registry (files stay).
func (r *Registry) Remove(name string) bool {
	for i := range r.Projects {
		if strings.EqualFold(r.Projects[i].Name, name) {
			r.Projects = append(r.Projects[:i], r.Projects[i+1:]...)
			if strings.EqualFold(r.Active, name) {
				r.Active = ""
			}
			return true
		}
	}
	return false
}

// ActiveVault returns the vault root of the active project, if any.
func (r *Registry) ActiveVault() (string, error) {
	if r.Active == "" {
		return "", errors.New("no active project")
	}
	p := r.Find(r.Active)
	if p == nil {
		return "", errors.New("active project not registered")
	}
	return p.Path, nil
}

func itoa(n int) string {
	if n == 0 {
		return "0"
	}
	var b []byte
	for n > 0 {
		b = append([]byte{byte('0' + n%10)}, b...)
		n /= 10
	}
	return string(b)
}
