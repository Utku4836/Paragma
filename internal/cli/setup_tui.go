package cli

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/Utku4836/Paragma/internal/agents"
	"golang.org/x/term"
)

// The interactive pg setup screen.

const (
	cAccent = "\x1b[38;2;139;92;246m"
	cShadow = "\x1b[38;2;76;52;140m"
	cDim    = "\x1b[38;2;130;130;130m"
	cText   = "\x1b[38;2;230;230;230m"
	cOK     = "\x1b[38;2;120;200;140m"
	cReset  = "\x1b[0m"
	cBold   = "\x1b[1m"
)

var bannerRows = []string{
	"██████╗  █████╗ ██████╗  █████╗  ██████╗ ███╗   ███╗ █████╗ ",
	"██╔══██╗██╔══██╗██╔══██╗██╔══██╗██╔════╝ ████╗ ████║██╔══██╗",
	"██████╔╝███████║██████╔╝███████║██║  ███╗██╔████╔██║███████║",
	"██╔═══╝ ██╔══██║██╔══██╗██╔══██║██║   ██║██║╚██╔╝██║██╔══██║",
	"██║     ██║  ██║██║  ██║██║  ██║╚██████╔╝██║ ╚═╝ ██║██║  ██║",
	"╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝ ╚═╝     ╚═╝╚═╝  ╚═╝",
}

// banner colours the blocks in the accent and the box-drawing shadow darker.
func banner(width int) string {
	if width < 64 {
		return "  " + cBold + cAccent + "PARAGMA" + cReset + "\n"
	}
	var b strings.Builder
	for _, row := range bannerRows {
		b.WriteString("  ")
		cur := ""
		for _, r := range row {
			col := cShadow
			if r == '█' {
				col = cAccent
			}
			if r == ' ' {
				col = ""
			}
			if col != cur && col != "" {
				b.WriteString(col)
				cur = col
			}
			b.WriteRune(r)
		}
		b.WriteString(cReset + "\n")
	}
	return b.String()
}

type screen int

const (
	scAgents screen = iota
	scScope
	scConversations
	scConfirm
)

type setupModel struct {
	agents  []*agents.Agent
	convs   []agents.Conversation
	sc      screen
	cur     int // cursor in the current list
	top     int // first visible row of the conversation list
	agent   *agents.Agent
	target  string // file to write
	where   string // human description of the target
	back    screen
	message string
	width   int
	height  int
	quit    bool
}

// short shows a path with ~ for the home folder.
func short(p string) string {
	if h := home(); h != "" && strings.HasPrefix(strings.ToLower(p), strings.ToLower(h)) {
		p = "~" + p[len(h):]
	}
	return filepath.ToSlash(p)
}

func since(t time.Time) string {
	if t.IsZero() {
		return ""
	}
	d := time.Since(t)
	switch {
	case d < time.Hour:
		return "just now"
	case d < 24*time.Hour:
		return fmt.Sprintf("%dh ago", int(d.Hours()))
	case d < 48*time.Hour:
		return "yesterday"
	case d < 30*24*time.Hour:
		return fmt.Sprintf("%d days ago", int(d.Hours()/24))
	}
	return t.Format("2006-01-02")
}

func fit(s string, n int) string {
	r := []rune(s)
	if len(r) > n {
		if n <= 3 {
			return string(r[:n])
		}
		return string(r[:n-3]) + "..."
	}
	return s + strings.Repeat(" ", n-len(r))
}

func (m *setupModel) listLen() int {
	switch m.sc {
	case scAgents:
		return len(m.agents)
	case scScope:
		return 3
	case scConversations:
		return len(m.convs)
	}
	return 0
}

func (m *setupModel) view() string {
	var b strings.Builder
	b.WriteString("\n" + banner(m.width) + "\n")
	line := func(sel bool, s string) {
		if sel {
			b.WriteString("  " + cAccent + "› " + cText + cBold + s + cReset + "\n")
		} else {
			b.WriteString("    " + cText + s + cReset + "\n")
		}
	}
	hint := ""
	switch m.sc {
	case scAgents:
		b.WriteString("  " + cDim + "Connect Paragma to an agent on this computer" + cReset + "\n\n")
		if len(m.agents) == 0 {
			b.WriteString("  No supported agent found (Claude Code, Codex, Antigravity, OpenCode).\n")
		}
		for i, ag := range m.agents {
			state := ""
			if hasSection(ag.GlobalFile) {
				state = cOK + "  ✓ every conversation" + cReset
			}
			line(i == m.cur, fit(ag.Name, 13)+cDim+fit(strings.Join(ag.Apps, " · "), 16)+short(ag.Home)+cReset+state)
		}
		hint = "↑↓ move   enter select   q quit"
	case scScope:
		b.WriteString("  " + cBold + cText + m.agent.Name + cReset + "\n\n")
		line(m.cur == 0, fit("Every conversation", 22)+cDim+short(m.agent.GlobalFile)+cReset)
		line(m.cur == 1, fit("One conversation", 22)+cDim+"installs into that conversation's folder"+cReset)
		line(m.cur == 2, "Back")
		hint = "↑↓ move   enter select   esc back"
	case scConversations:
		b.WriteString("  " + cBold + cText + m.agent.Name + cReset + cDim + " · pick a conversation (Paragma goes into its folder)" + cReset + "\n\n")
		if len(m.convs) == 0 {
			b.WriteString("  No conversations found.\n")
		}
		rows := m.height - 16
		if rows < 5 {
			rows = 5
		}
		if m.cur < m.top {
			m.top = m.cur
		}
		if m.cur >= m.top+rows {
			m.top = m.cur - rows + 1
		}
		tw := 38
		dw := m.width - tw - 22
		if dw < 20 {
			dw = 20
		}
		for i := m.top; i < len(m.convs) && i < m.top+rows; i++ {
			c := m.convs[i]
			mark := "  "
			if hasSection(filepath.Join(c.Dir, m.agent.ProjectFile)) {
				mark = cOK + "✓ " + cReset
			}
			line(i == m.cur, fit(c.Title, tw)+" "+mark+cDim+fit(short(c.Dir), dw)+" "+since(c.Updated)+cReset)
		}
		if len(m.convs) > rows {
			b.WriteString(fmt.Sprintf("\n  %s%d of %d%s\n", cDim, m.cur+1, len(m.convs), cReset))
		}
		hint = "↑↓ move   enter select   esc back"
	case scConfirm:
		b.WriteString("  " + cBold + cText + "Install Paragma for " + m.agent.Name + " " + m.where + "?" + cReset + "\n\n")
		b.WriteString("  " + cDim + "Adds a \"Memory (Paragma)\" section to " + cReset + cText + short(m.target) + cReset + "\n")
		b.WriteString("  " + cDim + "The rest of the file stays as it is; running setup again only updates the section." + cReset + "\n")
		hint = "y install   n back"
	}
	if m.message != "" {
		b.WriteString("\n  " + m.message + cReset + "\n")
	}
	b.WriteString("\n  " + cDim + hint + cReset + "\n")
	return b.String()
}

// key handles one key: up, down, enter, back, yes, no, quit.
func (m *setupModel) key(k string) {
	n := m.listLen()
	switch k {
	case "up":
		if m.cur > 0 {
			m.cur--
		}
		return
	case "down":
		if m.cur < n-1 {
			m.cur++
		}
		return
	case "quit":
		if m.sc == scAgents {
			m.quit = true
			return
		}
		k = "back"
	}
	switch m.sc {
	case scAgents:
		if k == "enter" && n > 0 {
			m.agent = m.agents[m.cur]
			m.sc, m.cur, m.message = scScope, 0, ""
		}
	case scScope:
		switch {
		case k == "back" || (k == "enter" && m.cur == 2):
			m.sc, m.cur = scAgents, 0
		case k == "enter" && m.cur == 0:
			m.target, m.where, m.back = m.agent.GlobalFile, "in every conversation", scScope
			m.sc = scConfirm
		case k == "enter" && m.cur == 1:
			m.convs = filterTemp(m.agent.Conversations())
			m.sc, m.cur, m.top = scConversations, 0, 0
		}
	case scConversations:
		switch {
		case k == "back":
			m.sc, m.cur = scScope, 1
		case k == "enter" && n > 0:
			c := m.convs[m.cur]
			m.target = filepath.Join(c.Dir, m.agent.ProjectFile)
			m.where = "in " + short(c.Dir)
			m.back, m.sc = scConversations, scConfirm
		}
	case scConfirm:
		switch k {
		case "yes":
			changed, err := installSection(m.target)
			switch {
			case err != nil:
				m.message = "\x1b[38;2;230;100;100m✗ " + err.Error()
			case changed:
				m.message = cOK + "✓ " + m.agent.Name + ": wrote " + short(m.target)
			default:
				m.message = cOK + "✓ " + m.agent.Name + ": already set up in " + short(m.target)
			}
			m.sc, m.cur = scAgents, indexOf(m.agents, m.agent)
		case "no", "back":
			m.sc = m.back
			if m.back == scScope {
				m.cur = 0
			}
		}
	}
}

func indexOf(as []*agents.Agent, a *agents.Agent) int {
	for i, x := range as {
		if x == a {
			return i
		}
	}
	return 0
}

// filterTemp drops conversations that ran in temporary folders.
func filterTemp(cs []agents.Conversation) []agents.Conversation {
	tmp := strings.ToLower(filepath.Clean(os.TempDir()))
	var out []agents.Conversation
	for _, c := range cs {
		if !strings.HasPrefix(strings.ToLower(filepath.Clean(c.Dir)), tmp) {
			out = append(out, c)
		}
	}
	return out
}

func (a *app) setupTUI() error {
	enableVT()
	fd := int(os.Stdin.Fd())
	old, err := term.MakeRaw(fd)
	if err != nil {
		return fail(ExitInternal, "terminal: %v", err)
	}
	out := a.rawOut
	defer func() {
		term.Restore(fd, old)
		fmt.Fprint(out, "\x1b[?25h")
	}()
	m := &setupModel{agents: agents.Installed()}
	fmt.Fprint(out, "\x1b[?25l")
	buf := make([]byte, 16)
	for !m.quit {
		m.width, m.height, _ = term.GetSize(int(os.Stdout.Fd()))
		if m.width == 0 {
			m.width, m.height = 100, 30
		}
		fmt.Fprint(out, "\x1b[H\x1b[2J"+strings.ReplaceAll(m.view(), "\n", "\r\n"))
		n, err := os.Stdin.Read(buf)
		if err != nil {
			return nil
		}
		m.key(keyName(buf[:n]))
	}
	fmt.Fprint(out, "\x1b[H\x1b[2J")
	return nil
}

func keyName(b []byte) string {
	s := string(b)
	switch {
	case s == "\x1b[A" || s == "k" || s == "\x1bOA":
		return "up"
	case s == "\x1b[B" || s == "j" || s == "\x1bOB":
		return "down"
	case s == "\r" || s == "\n" || s == "\r\n":
		return "enter"
	case s == "\x1b" || s == "\x7f" || s == "\b" || s == "h":
		return "back"
	case s == "y" || s == "Y":
		return "yes"
	case s == "n" || s == "N":
		return "no"
	case s == "q" || s == "Q" || s == "\x03":
		return "quit"
	}
	return ""
}
