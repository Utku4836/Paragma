package cli

import (
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/Utku4836/Paragma/internal/agents"
)

var reANSI = regexp.MustCompile("\x1b\\[[0-9;]*m")

// plain is a screen without its colours.
func plain(m *setupModel) string { return reANSI.ReplaceAllString(m.view(), "") }

func TestSetupScreens(t *testing.T) {
	dir := t.TempDir()
	codex := agents.Find("codex")
	codex.GlobalFile = filepath.Join(dir, "global", "AGENTS.md")
	codex.Apps = []string{"CLI", "desktop"}
	m := &setupModel{agents: []*agents.Agent{codex}, width: 100, height: 30}

	v := plain(m)
	for _, want := range []string{"██████╗", "Codex", "CLI · desktop", "q quit"} {
		if !strings.Contains(v, want) {
			t.Errorf("agent list lacks %q:\n%s", want, v)
		}
	}
	// every conversation: global file
	m.key("enter")
	if m.sc != scScope || !strings.Contains(plain(m), "Every conversation") {
		t.Fatalf("scope screen:\n%s", plain(m))
	}
	m.key("enter")
	if m.sc != scConfirm || !strings.Contains(plain(m), "in every conversation?") {
		t.Fatalf("confirm screen:\n%s", plain(m))
	}
	m.key("yes")
	b, err := os.ReadFile(codex.GlobalFile)
	if err != nil || !strings.Contains(string(b), "## Memory (Paragma)") {
		t.Fatalf("global file: %v %s", err, b)
	}
	if m.sc != scAgents || !strings.Contains(plain(m), "✓ Codex: wrote") || !strings.Contains(plain(m), "✓ every conversation") {
		t.Fatalf("after install:\n%s", plain(m))
	}

	// one conversation: its folder
	proj := filepath.Join(dir, "shop")
	os.MkdirAll(proj, 0o755)
	m.key("enter")
	m.sc, m.convs = scConversations, []agents.Conversation{{Title: "Refund support", Dir: proj, Updated: time.Now().Add(-48 * time.Hour)}}
	if v := plain(m); !strings.Contains(v, "Refund support") || !strings.Contains(v, "days ago") {
		t.Fatalf("conversations:\n%s", v)
	}
	m.key("enter")
	m.key("no") // back to the list, nothing written
	if _, err := os.Stat(filepath.Join(proj, "AGENTS.md")); err == nil || m.sc != scConversations {
		t.Fatal("no must not write")
	}
	m.key("enter")
	m.key("yes")
	if !hasSection(filepath.Join(proj, "AGENTS.md")) {
		t.Fatal("folder file not written")
	}
	m.key("quit")
	if !m.quit {
		t.Fatal("q on the agent list quits")
	}
}
