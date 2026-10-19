package cli

import (
	"strings"

	"github.com/Utku4836/Paragma/internal/index"
	"github.com/Utku4836/Paragma/internal/textutil"
)

// The standard sections of project, error and decision notes.

// sectionNames are the headings pg writes and recognises, per language.
// Recognition accepts every language so a space can mix them.
var sectionNames = map[string]map[string]string{
	"goal":      {"en": "Goal", "tr": "Hedef"},
	"plan":      {"en": "Plan", "tr": "Plan"},
	"notes":     {"en": "Log", "tr": "Günlük"},
	"handoff":   {"en": "Handoff", "tr": "Devir"},
	"symptom":   {"en": "Symptom", "tr": "Belirti"},
	"cause":     {"en": "Root cause", "tr": "Kök neden"},
	"fix":       {"en": "Fix", "tr": "Çözüm"},
	"files":     {"en": "Files", "tr": "Dosyalar"},
	"context":   {"en": "Context", "tr": "Bağlam"},
	"decision":  {"en": "Decision", "tr": "Karar"},
	"options":   {"en": "Alternatives", "tr": "Alternatifler"},
	"outcome":   {"en": "Consequences", "tr": "Sonuçlar"},
	"decisions": {"en": "Decisions", "tr": "Kararlar"},
}

// extra spellings that count as the same section
var sectionAliases = map[string][]string{
	"goal":    {"Amaç", "Objective"},
	"notes":   {"Notes", "Notlar", "Log"},
	"handoff": {"Checkpoint"},
	"plan":    {"Tasks", "Görevler"},
}

func (a *app) sec(s *store, key string) string {
	lang := s.v.Cfg.Lang
	if lang != "tr" {
		lang = "en"
	}
	return sectionNames[key][lang]
}

// findSection returns the top-level section of a file whose heading is one of
// key's spellings (level 2 under a title, or level 1 otherwise).
func findSection(s *store, f *index.File, key string) *index.Section {
	secs, err := s.res.Sections(f)
	if err != nil {
		return nil
	}
	names := map[string]bool{}
	for _, n := range sectionNames[key] {
		names[foldName(n)] = true
	}
	for _, n := range sectionAliases[key] {
		names[foldName(n)] = true
	}
	for _, sec := range secs {
		if sec.IsTitle || sec.Level == 0 {
			continue
		}
		if names[foldName(sec.Path)] {
			return sec
		}
	}
	return nil
}

func foldName(s string) string { return strings.TrimSpace(textutil.Fold(s)) }

// sectionBody returns the non-blank lines of a section.
func (a *app) sectionBody(s *store, f *index.File, key string) []string {
	sec := findSection(s, f, key)
	if sec == nil {
		return nil
	}
	lines, err := fileLines(s, f)
	if err != nil {
		return nil
	}
	var out []string
	for _, l := range sliceLines(lines, sec.Line+1, sec.BodyEnd) {
		if !isBlank(l) {
			out = append(out, l)
		}
	}
	return out
}

// appendToSection appends block at the end of section key in n, creating
// the section (at the end of the note) if it is missing.
func (a *app) appendToSection(n *note, f *index.File, key string) func(block []string) {
	return func(block []string) {
		sec := findSection(n.s, f, key)
		if sec == nil {
			n.lines, _ = place(n.lines, lastLine(n.lines, 1, len(n.lines)), append([]string{"## " + a.sec(n.s, key), ""}, block...))
			return
		}
		n.lines, _ = place(n.lines, lastLine(n.lines, sec.Line, sec.BodyEnd), block)
	}
}

// fillSection puts body right under the heading line in lines.
func fillSection(lines []string, heading string, body []string) []string {
	for i, l := range lines {
		if l == heading {
			out := append([]string(nil), lines[:i+1]...)
			out = append(out, body...)
			return append(out, lines[i+1:]...)
		}
	}
	return lines
}

func hasTitle(s *store, f *index.File) bool {
	secs, _ := s.res.Sections(f)
	for _, sec := range secs {
		if sec.IsTitle {
			return true
		}
	}
	return false
}
