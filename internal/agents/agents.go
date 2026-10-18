// Package agents finds the AI coding agents installed on this computer,
// their conversations (title, folder, last use) and where each reads its
// instructions. Each agent stores things its own way; the readers here only
// read (SQLite files are opened read-only) and skip what they cannot parse.
package agents

import (
	"bufio"
	"bytes"
	"database/sql"
	"encoding/json"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"

	_ "modernc.org/sqlite"
)

// Agent is one agent CLI (and its desktop app, which shares its data).
type Agent struct {
	ID          string   // claude, codex, antigravity, opencode
	Name        string   // Claude Code
	Home        string   // its data folder
	Apps        []string // "CLI", "desktop", "IDE"
	GlobalFile  string   // instructions read in every conversation
	ProjectFile string   // file name read from a conversation's folder
	conv        func(a *Agent) []Conversation
}

// Conversation is one conversation of an agent.
type Conversation struct {
	Title   string
	Dir     string
	Updated time.Time
	App     string // which app it came from, when the agent has several
}

func home() string {
	h, _ := os.UserHomeDir()
	return h
}

func exists(p string) bool {
	_, err := os.Stat(p)
	return err == nil
}

func onPath(names ...string) bool {
	for _, n := range names {
		if _, err := exec.LookPath(n); err == nil {
			return true
		}
	}
	return false
}

// all lists every agent pg knows, installed or not.
func all() []*Agent {
	h := home()
	roaming, _ := os.UserConfigDir() // %AppData% on Windows
	local, _ := os.UserCacheDir()    // %LocalAppData% on Windows
	claude := &Agent{ID: "claude", Name: "Claude Code", Home: filepath.Join(h, ".claude"),
		GlobalFile: filepath.Join(h, ".claude", "CLAUDE.md"), ProjectFile: "CLAUDE.md", conv: claudeConversations}
	if onPath("claude") {
		claude.Apps = append(claude.Apps, "CLI")
	}
	if exists(filepath.Join(local, "AnthropicClaude")) || exists(filepath.Join(local, "Claude")) || exists(filepath.Join(roaming, "Claude")) || exists("/Applications/Claude.app") {
		claude.Apps = append(claude.Apps, "desktop")
	}
	codex := &Agent{ID: "codex", Name: "Codex", Home: filepath.Join(h, ".codex"),
		GlobalFile: filepath.Join(h, ".codex", "AGENTS.md"), ProjectFile: "AGENTS.md", conv: codexConversations}
	if onPath("codex") {
		codex.Apps = append(codex.Apps, "CLI")
	}
	if exists(filepath.Join(h, ".codex", ".desktop-created")) || exists("/Applications/Codex.app") {
		codex.Apps = append(codex.Apps, "desktop")
	}
	ag := &Agent{ID: "antigravity", Name: "Antigravity", Home: filepath.Join(h, ".gemini"),
		GlobalFile: filepath.Join(h, ".gemini", "AGENTS.md"), ProjectFile: "AGENTS.md", conv: antigravityConversations}
	if onPath("agy") || exists(filepath.Join(h, ".gemini", "antigravity-cli")) {
		ag.Apps = append(ag.Apps, "CLI")
	}
	if exists(filepath.Join(h, ".gemini", "antigravity")) || exists(filepath.Join(local, "Programs", "Antigravity")) || exists("/Applications/Antigravity.app") {
		ag.Apps = append(ag.Apps, "IDE")
	}
	oc := &Agent{ID: "opencode", Name: "OpenCode", Home: filepath.Join(h, ".local", "share", "opencode"),
		GlobalFile: filepath.Join(h, ".config", "opencode", "AGENTS.md"), ProjectFile: "AGENTS.md", conv: opencodeConversations}
	if onPath("opencode") || exists(filepath.Join(h, ".config", "opencode")) {
		oc.Apps = append(oc.Apps, "CLI")
	}
	return []*Agent{claude, codex, ag, oc}
}

// Known returns every agent pg can set up, installed or not.
func Known() []*Agent { return all() }

// Installed returns the agents found on this computer.
func Installed() []*Agent {
	var out []*Agent
	for _, a := range all() {
		if len(a.Apps) > 0 || exists(a.Home) {
			if len(a.Apps) == 0 {
				a.Apps = []string{"CLI"}
			}
			out = append(out, a)
		}
	}
	return out
}

// Find returns the agent with this id (installed or not).
func Find(id string) *Agent {
	for _, a := range all() {
		if a.ID == strings.ToLower(id) {
			return a
		}
	}
	return nil
}

// Conversations returns the agent's conversations, newest first, whose
// folders still exist.
func (a *Agent) Conversations() []Conversation {
	cs := a.conv(a)
	var out []Conversation
	for _, c := range cs {
		if c.Dir == "" || !exists(c.Dir) {
			continue
		}
		c.Title = strings.Join(strings.Fields(c.Title), " ")
		if c.Title == "" {
			c.Title = "(untitled)"
		}
		out = append(out, c)
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].Updated.After(out[j].Updated) })
	return out
}

func openRO(path string) *sql.DB {
	if !exists(path) {
		return nil
	}
	db, err := sql.Open("sqlite", "file:"+filepath.ToSlash(path)+"?mode=ro&_pragma=busy_timeout(2000)")
	if err != nil {
		return nil
	}
	return db
}

// newest returns the last match of a glob (state_5.sqlite after state_4 …).
func newest(pattern string) string {
	m, _ := filepath.Glob(pattern)
	sort.Strings(m)
	if len(m) == 0 {
		return ""
	}
	return m[len(m)-1]
}

func cleanDir(d string) string {
	d = strings.TrimPrefix(d, `\\?\`)
	return filepath.Clean(filepath.FromSlash(d))
}

/// ---- Claude Code: ~/.claude/projects/<folder>/<session>.jsonl ----

func claudeConversations(a *Agent) []Conversation {
	files, _ := filepath.Glob(filepath.Join(a.Home, "projects", "*", "*.jsonl"))
	var out []Conversation
	for _, p := range files {
		st, err := os.Stat(p)
		if err != nil {
			continue
		}
		dir, title := claudeSession(p)
		out = append(out, Conversation{Title: title, Dir: dir, Updated: st.ModTime()})
	}
	return out
}

// claudeSession reads a session file's folder and title: the last title the
// app set (custom-title), else the first thing the user typed.
func claudeSession(p string) (dir, title string) {
	f, err := os.Open(p)
	if err != nil {
		return "", ""
	}
	defer f.Close()
	r := bufio.NewReaderSize(f, 1<<20)
	first := ""
	for {
		line, err := r.ReadBytes('\n')
		if len(line) > 0 {
			switch {
			case bytes.Contains(line, []byte(`"type":"custom-title"`)):
				var e struct{ CustomTitle string }
				if json.Unmarshal(line, &e) == nil && e.CustomTitle != "" {
					title = e.CustomTitle
				}
			case (dir == "" || first == "") && len(line) < 1<<20:
				var e struct {
					Cwd     string
					Type    string
					IsMeta  bool
					Message struct {
						Content json.RawMessage
					}
				}
				if json.Unmarshal(line, &e) != nil {
					break
				}
				if dir == "" && e.Cwd != "" {
					dir = cleanDir(e.Cwd)
				}
				if first == "" && e.Type == "user" && !e.IsMeta {
					var s string
					if json.Unmarshal(e.Message.Content, &s) == nil && s != "" && !strings.HasPrefix(s, "<") &&
						!strings.HasPrefix(s, "This session is being continued") {
						first = s
					}
				}
			}
		}
		if err != nil {
			break
		}
	}
	if title == "" {
		title = firstLine(first)
	}
	return dir, title
}

// firstLine is the first non-empty line of a message, without markdown marks.
func firstLine(s string) string {
	for _, l := range strings.Split(s, "\n") {
		if l = strings.TrimSpace(strings.TrimLeft(strings.TrimSpace(l), "#>*-")); l != "" {
			return l
		}
	}
	return ""
}

// ---- Codex: ~/.codex/state_N.sqlite, table threads ----

func codexConversations(a *Agent) []Conversation {
	db := openRO(newest(filepath.Join(a.Home, "state_*.sqlite")))
	if db == nil {
		return nil
	}
	defer db.Close()
	rows, err := db.Query(`SELECT title, cwd, updated_at FROM threads WHERE archived = 0`)
	if err != nil {
		return nil
	}
	defer rows.Close()
	var out []Conversation
	for rows.Next() {
		var t, cwd string
		var up int64
		if rows.Scan(&t, &cwd, &up) == nil {
			out = append(out, Conversation{Title: firstLine(t), Dir: cleanDir(cwd), Updated: time.Unix(up, 0)})
		}
	}
	return out
}

// ---- Antigravity: ~/.gemini/antigravity{,-cli}/conversation_summaries.db ----

func antigravityConversations(a *Agent) []Conversation {
	var out []Conversation
	for _, src := range []struct{ dir, app string }{{"antigravity-cli", "CLI"}, {"antigravity", "IDE"}} {
		db := openRO(filepath.Join(a.Home, src.dir, "conversation_summaries.db"))
		if db == nil {
			continue
		}
		rows, err := db.Query(`SELECT title, workspace_uris, last_modified_time FROM conversation_summaries`)
		if err != nil {
			db.Close()
			continue
		}
		for rows.Next() {
			var t, uris string
			var mod any
			if rows.Scan(&t, &uris, &mod) != nil {
				continue
			}
			var us []string
			json.Unmarshal([]byte(uris), &us)
			dir := ""
			if len(us) > 0 {
				dir = fromFileURI(us[0])
			}
			up := anyTime(mod)
			out = append(out, Conversation{Title: t, Dir: dir, Updated: up, App: src.app})
		}
		rows.Close()
		db.Close()
	}
	return out
}

func fromFileURI(u string) string {
	p, err := url.Parse(u)
	if err != nil || p.Scheme != "file" {
		return ""
	}
	path := p.Path
	if len(path) > 2 && path[0] == '/' && path[2] == ':' {
		path = path[1:] // /c:/Users -> c:/Users
	}
	return cleanDir(path)
}

// ---- OpenCode: ~/.local/share/opencode/opencode.db ----

func opencodeConversations(a *Agent) []Conversation {
	db := openRO(filepath.Join(a.Home, "opencode.db"))
	if db == nil {
		return nil
	}
	defer db.Close()
	var out []Conversation
	for _, table := range []string{"session_v2", "session"} {
		rows, err := db.Query(`SELECT COALESCE(title,''), directory, time_updated FROM ` + table + ` WHERE parent_id IS NULL`)
		if err != nil {
			continue
		}
		for rows.Next() {
			var t, dir string
			var up int64
			if rows.Scan(&t, &dir, &up) == nil {
				out = append(out, Conversation{Title: t, Dir: cleanDir(dir), Updated: time.UnixMilli(up)})
			}
		}
		rows.Close()
	}
	return out
}

// anyTime reads a SQLite datetime that the driver returns as a time or text.
func anyTime(v any) time.Time {
	switch x := v.(type) {
	case time.Time:
		return x
	case string:
		for _, layout := range []string{"2006-01-02 15:04:05.999999999-07:00", time.RFC3339Nano, "2006-01-02 15:04:05"} {
			if t, err := time.Parse(layout, x); err == nil {
				return t
			}
		}
	case []byte:
		return anyTime(string(x))
	}
	return time.Time{}
}
