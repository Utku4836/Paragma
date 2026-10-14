//go:build !windows

package fasttz

// Init is a no-op outside Windows, where loading the local zone is cheap.
func Init() {}
