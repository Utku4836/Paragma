---
id: 01SHOPERRF0000000000000055
type: error
layer: episodic
status: resolved
title: Admin tool failure #56
seen: 3
tags: [admin]
created: 2026-07-27
---
# Admin tool failure #56

## Symptom
The admin tool failed in production with errors in the logs and missing rows.

## Root cause
The cause was a nil map in the config struct.

## Fix
Added a guard and a regression test.
