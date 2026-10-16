---
id: 01SHOPERR00000000000000018
type: error
layer: episodic
status: resolved
title: CI uses old Go
sig: 'go: go.mod requires go >= 1.24'
seen: 1
tags: [ci]
created: 2026-04-26
updated: 2026-09-15
---
# CI uses old Go

## Symptom
CI failed after bumping go.mod.

## Root cause
setup-go pinned 1.22.

## Fix
Read the version from go.mod (go-version-file).

## Files
- .github/workflows/ci.yml
