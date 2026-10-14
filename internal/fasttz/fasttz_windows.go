// Package fasttz sets time.Local cheaply on Windows.
//
// Go resolves the local zone on Windows by walking registry MUI strings to
// find the zone's English name, which costs ~45ms on every process start.
// pg only needs the current UTC offset (for "today" and timestamps), so it
// installs a fixed zone built from GetTimeZoneInformation instead.
package fasttz

import (
	"syscall"
	"time"
)

const timeZoneIDDaylight = 2

// Init replaces time.Local with a fixed zone for the current offset. It must
// run before anything formats or computes a local time.
func Init() {
	var tzi syscall.Timezoneinformation
	id, err := syscall.GetTimeZoneInformation(&tzi)
	if err != nil {
		return
	}
	bias := tzi.Bias + tzi.StandardBias
	if id == timeZoneIDDaylight {
		bias = tzi.Bias + tzi.DaylightBias
	}
	time.Local = time.FixedZone("", -int(bias)*60)
}
