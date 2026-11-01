---
id: 01JA3Z8K2MERRLOCK0000000AA
type: error
layer: episodic
status: resolved
title: SQLite database is locked
sig: "database is locked (5)"
sig_norm: "database is locked <n>"
seen: 3
tags: [sqlite, concurrency]
created: 2026-09-02
updated: 2026-10-01
verified: 2026-10-01
---
# SQLite database is locked

## Belirti
Paralel testlerde `database is locked (5)` hatası, özellikle `go test -race ./...` ile.

## Kök neden
WAL modu kapalıydı; yazma sırasında okuyucular kilitleniyordu.

## Çözüm
`PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;` açılışta, `internal/db/open.go` içinde.
Bkz karar: [[0007-wal-modu]].

## Dosyalar
- internal/db/open.go
