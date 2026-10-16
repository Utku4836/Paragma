// Package clock is the replaceable time source. Code must call clock.Now(),
// never time.Now(), so tests can pin the time.
package clock

import "time"

// Now returns the current time in UTC. Tests replace it.
var Now = func() time.Time { return time.Now().UTC() }
