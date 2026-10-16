package space

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"time"
)

// Registry is <home>/spaces.json: the default space and, per work
// directory, which space it uses and which project is current in it.
type Registry struct {
	Default string               `json:"default,omitempty"`
	Dirs    map[string]*DirState `json:"dirs,omitempty"`
	home    string
}

// DirState is what pg remembers about one work directory.
type DirState struct {
	Path     string            `json:"path"`
	Space    string            `json:"space,omitempty"`    // "" = the default space
	Projects map[string]string `json:"projects,omitempty"` // space -> current project
	Used     string            `json:"used,omitempty"`
}

func registryPath(home string) string { return filepath.Join(home, "spaces.json") }

// LoadRegistry reads spaces.json; a missing or broken file is empty.
func LoadRegistry(home string) *Registry {
	r := &Registry{home: home}
	if b, err := os.ReadFile(registryPath(home)); err == nil {
		_ = json.Unmarshal(b, r)
	}
	if r.Dirs == nil {
		r.Dirs = map[string]*DirState{}
	}
	return r
}

// Save writes spaces.json atomically.
func (r *Registry) Save() error {
	b, err := json.MarshalIndent(r, "", "  ")
	if err != nil {
		return err
	}
	return WriteFileAtomic(registryPath(r.home), b)
}

// DefaultSpace is the configured default, or "main".
func (r *Registry) DefaultSpace() string {
	if r.Default != "" {
		return r.Default
	}
	return DefaultName
}

func dirKey(dir string) string {
	k := filepath.ToSlash(filepath.Clean(dir))
	if runtime.GOOS == "windows" {
		k = strings.ToLower(k)
	}
	return k
}

// Dir returns the state of a work directory (an empty state when unknown;
// it is not added until Touch).
func (r *Registry) Dir(dir string) *DirState {
	if d := r.Dirs[dirKey(dir)]; d != nil {
		return d
	}
	return &DirState{Path: dir}
}

// Touch returns the state of dir, adding it if needed.
func (r *Registry) Touch(dir string) *DirState {
	k := dirKey(dir)
	d := r.Dirs[k]
	if d == nil {
		d = &DirState{Path: filepath.ToSlash(dir)}
		r.Dirs[k] = d
	}
	if d.Projects == nil {
		d.Projects = map[string]string{}
	}
	d.Used = time.Now().Format("2006-01-02")
	return d
}

// WorkDir is the directory pg keys its per-directory state on: the root of
// the git work tree containing dir, else dir itself.
func WorkDir(dir string) string {
	abs, err := filepath.Abs(dir)
	if err != nil {
		abs = dir
	}
	for d := abs; ; {
		if _, err := os.Stat(filepath.Join(d, ".git")); err == nil {
			return d
		}
		parent := filepath.Dir(d)
		if parent == d {
			return abs
		}
		d = parent
	}
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
