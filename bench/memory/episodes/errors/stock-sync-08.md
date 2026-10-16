---
id: 01SHOPERRF0000000000000008
type: error
layer: episodic
status: resolved
title: Stock sync failure #9
seen: 2
tags: [stock]
created: 2026-05-10
---
# Stock sync failure #9

## Symptom
The stock sync failed in production with errors in the logs and wrong totals.

## Root cause
The cause was a slice aliasing bug when appending.

## Fix
Added the index in a forward migration.
