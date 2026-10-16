---
id: 01SHOPERR00000000000000005
type: error
layer: episodic
status: resolved
title: golangci-lint timeout in CI
sig: 'context deadline exceeded'
seen: 1
tags: [ci]
created: 2026-08-13
updated: 2026-07-28
---
# golangci-lint timeout in CI

## Symptom
Lint job fails with a timeout after 5 minutes.

## Root cause
Cache key changed every run because go.sum was not part of it.

## Fix
Include go.sum in the cache key; set timeout to 10m.

## Files
- .github/workflows/ci.yml
