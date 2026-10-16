---
id: 01SHOPERR00000000000000011
type: error
layer: episodic
status: resolved
title: Config missing in local runs
sig: 'missing SHOP_DB_URL'
seen: 3
tags: [config, dev]
created: 2026-04-27
updated: 2026-06-28
---
# Config missing in local runs

## Symptom
Local runs fail with `missing SHOP_DB_URL`.

## Root cause
direnv was not allowed in the new clone.

## Fix
Run `direnv allow`; documented in README.

## Files
- README.md
