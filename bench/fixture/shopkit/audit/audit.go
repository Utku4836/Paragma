// Package audit records money movements.
package audit

import (
	"context"
	"sync"
	"time"

	"example.com/shopkit/clock"
)

// Entry is one audit record.
type Entry struct {
	Action string
	Ref    string
	At     time.Time
}

var (
	mu      sync.Mutex
	entries []Entry
)

// Record appends an audit entry.
func Record(ctx context.Context, action, ref string) {
	mu.Lock()
	defer mu.Unlock()
	entries = append(entries, Entry{Action: action, Ref: ref, At: clock.Now()})
}

// Entries returns a copy of the log (tests).
func Entries() []Entry {
	mu.Lock()
	defer mu.Unlock()
	return append([]Entry(nil), entries...)
}

// Reset clears the log (tests).
func Reset() {
	mu.Lock()
	defer mu.Unlock()
	entries = nil
}
