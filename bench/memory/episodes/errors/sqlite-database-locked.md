---
id: 01SHOPERR00000000000000001
type: error
layer: episodic
status: resolved
title: SQLite database is locked
sig: 'database is locked (5)'
seen: 3
tags: [sqlite]
created: 2026-04-27
updated: 2026-06-21
---
# SQLite database is locked

## Symptom
Parallel admin tool tests fail with `database is locked (5)`.

## Root cause
WAL was off; readers blocked during writes.

## Fix
Open with `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;`.

## Files
- admin/db/open.go
