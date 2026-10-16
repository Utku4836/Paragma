---
id: 01SHOPERR00000000000000006
type: error
layer: episodic
status: resolved
title: Migrations applied out of order
sig: 'no such column: updated_at'
seen: 1
tags: [migrations]
created: 2026-04-28
updated: 2026-09-11
---
# Migrations applied out of order

## Symptom
Fresh databases fail at startup with `no such column`.

## Root cause
Two migrations had the same number 0012.

## Fix
Renumbered to 0013; CI now checks for duplicate migration numbers.

## Files
- migrations/
