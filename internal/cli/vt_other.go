//go:build !windows

package cli

// enableVT: terminals outside Windows understand ANSI escapes already.
func enableVT() {}
