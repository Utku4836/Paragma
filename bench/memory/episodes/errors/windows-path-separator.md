---
id: 01SHOPERR00000000000000013
type: error
layer: episodic
status: resolved
title: Golden file paths fail on Windows
sig: 'open testdata\golden/x.json'
seen: 1
tags: [windows, tests]
created: 2026-07-20
updated: 2026-09-28
---
# Golden file paths fail on Windows

## Symptom
Golden tests fail on Windows with mixed separators.

## Root cause
Paths were joined with string concatenation.

## Fix
Use filepath.Join everywhere.

## Files
- internal/golden/golden.go
