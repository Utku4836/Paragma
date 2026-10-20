package cli

// Which conversation is calling. An agent runs every tool call in a fresh
// shell, so pg's parent changes from call to call, but the first ancestor
// that is not a shell is the agent itself and stays the same for the whole
// conversation. That process (name and pid) keys what pg remembers having
// shown, so a second agent working at the same time is not told "shown
// above" about things it never saw.

import (
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
)

// shells and launchers between the agent and pg
var shellNames = map[string]bool{
	"pwsh": true, "powershell": true, "cmd": true, "bash": true, "sh": true, "zsh": true, "fish": true, "dash": true,
	"ksh": true, "tcsh": true, "csh": true, "env": true, "timeout": true, "conhost": true, "winpty": true, "mintty": true,
	"git-bash": true, "nu": true, "xonsh": true, "sudo": true, "nice": true, "time": true,
}

var (
	sessionOnce sync.Once
	sessionKey  string
)

// conversation returns a key for the calling conversation, "" when it
// cannot be told ($PARAGMA_SESSION overrides).
func conversation() string {
	sessionOnce.Do(func() {
		if k := os.Getenv("PARAGMA_SESSION"); k != "" {
			sessionKey = k
			return
		}
		pid := os.Getppid()
		for i := 0; i < 12 && pid > 1; i++ {
			name, ppid, ok := processInfo(pid)
			if !ok {
				return
			}
			base := strings.ToLower(strings.TrimSuffix(filepath.Base(name), filepath.Ext(name)))
			if !shellNames[base] {
				sessionKey = base + ":" + strconv.Itoa(pid)
				return
			}
			pid = ppid
		}
	})
	return sessionKey
}
