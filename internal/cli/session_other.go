//go:build !windows

package cli

import (
	"os"
	"os/exec"
	"strconv"
	"strings"
)

// processInfo returns a process's executable name and parent pid, from
// /proc where there is one (Linux), else from ps (macOS).
func processInfo(pid int) (string, int, bool) {
	if b, err := os.ReadFile("/proc/" + strconv.Itoa(pid) + "/stat"); err == nil {
		s := string(b)
		open, close := strings.IndexByte(s, '('), strings.LastIndexByte(s, ')')
		if open < 0 || close < open {
			return "", 0, false
		}
		f := strings.Fields(s[close+1:])
		if len(f) < 2 {
			return "", 0, false
		}
		ppid, _ := strconv.Atoi(f[1])
		return s[open+1 : close], ppid, true
	}
	out, err := exec.Command("ps", "-o", "ppid=,comm=", "-p", strconv.Itoa(pid)).Output()
	if err != nil {
		return "", 0, false
	}
	f := strings.Fields(string(out))
	if len(f) < 2 {
		return "", 0, false
	}
	ppid, _ := strconv.Atoi(f[0])
	return strings.Join(f[1:], " "), ppid, true
}
