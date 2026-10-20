package cli

import (
	"unsafe"

	"golang.org/x/sys/windows"
)

var processTable map[int]windows.ProcessEntry32

// processInfo returns a process's executable name and parent pid.
func processInfo(pid int) (string, int, bool) {
	if processTable == nil {
		processTable = map[int]windows.ProcessEntry32{}
		snap, err := windows.CreateToolhelp32Snapshot(windows.TH32CS_SNAPPROCESS, 0)
		if err != nil {
			return "", 0, false
		}
		defer windows.CloseHandle(snap)
		var e windows.ProcessEntry32
		e.Size = uint32(unsafe.Sizeof(e))
		for err = windows.Process32First(snap, &e); err == nil; err = windows.Process32Next(snap, &e) {
			processTable[int(e.ProcessID)] = e
		}
	}
	e, ok := processTable[pid]
	if !ok {
		return "", 0, false
	}
	return windows.UTF16ToString(e.ExeFile[:]), int(e.ParentProcessID), true
}
