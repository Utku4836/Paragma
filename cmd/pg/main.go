// Command pg is the Paragma CLI: agent memory backed by Markdown files and a
// SQLite full-text index.
package main

import (
	"os"
	"runtime/pprof"

	"github.com/Utku-Private/Paragma/internal/cli"
	"github.com/Utku-Private/Paragma/internal/fasttz"
)

func main() {
	fasttz.Init()
	// PARAGMA_CPUPROFILE=out.prof writes a CPU profile (development aid).
	if p := os.Getenv("PARAGMA_CPUPROFILE"); p != "" {
		if f, err := os.Create(p); err == nil {
			pprof.StartCPUProfile(f)
			code := cli.Execute(os.Args[1:], os.Stdout, os.Stderr, os.Stdin)
			pprof.StopCPUProfile()
			f.Close()
			os.Exit(code)
		}
	}
	os.Exit(cli.Execute(os.Args[1:], os.Stdout, os.Stderr, os.Stdin))
}
