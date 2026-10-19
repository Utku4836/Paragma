package cli

import (
	"encoding/json"
	"os"
	"time"

	"github.com/Utku4836/Paragma/internal/space"
)

// shown remembers what pg printed recently (rules in a context pack, rule
// reminders after pg run) so it is not repeated in the same conversation.
// pg prime, run at the start of a conversation, forgets everything.
type shown map[string]int64

const shownWindow = 30 * time.Minute

func loadShown(s *store) shown {
	m := shown{}
	if b, err := os.ReadFile(s.v.Meta("shown.json")); err == nil {
		json.Unmarshal(b, &m)
	}
	return m
}

func (m shown) recent(key string, window time.Duration) bool {
	t, ok := m[key]
	return ok && time.Since(time.Unix(t, 0)) < window
}

func (m shown) mark(key string) { m[key] = time.Now().Unix() }

func (a *app) saveShown(s *store, m shown) {
	if a.dryRun {
		return
	}
	cut := time.Now().Add(-24 * time.Hour).Unix()
	for k, t := range m {
		if t < cut {
			delete(m, k)
		}
	}
	if b, err := json.Marshal(m); err == nil {
		space.WriteFileAtomic(s.v.Meta("shown.json"), b)
	}
}

func (a *app) forgetShown(s *store) {
	if !a.dryRun {
		os.Remove(s.v.Meta("shown.json"))
	}
}
