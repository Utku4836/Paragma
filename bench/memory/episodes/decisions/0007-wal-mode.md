---
id: 01SHOPDEC00070000000000000
type: decision
layer: episodic
status: active
title: "0007: SQLite in WAL mode"
created: 2026-08-22
---
# 0007: SQLite in WAL mode

## Context
Parallel tests locked the database.

## Decision
Open SQLite with WAL and busy_timeout=5000.

## Alternatives
- Serial tests: CI 3x slower.

## Consequences
Documented in the rules; reviewers check it.
