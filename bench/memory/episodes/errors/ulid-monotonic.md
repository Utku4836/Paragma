---
id: 01SHOPERR00000000000000016
type: error
layer: episodic
status: resolved
title: Duplicate ULIDs in a batch
sig: 'UNIQUE constraint failed: orders.id'
seen: 1
tags: [ids]
created: 2026-06-26
updated: 2026-09-20
---
# Duplicate ULIDs in a batch

## Symptom
Bulk import failed with duplicate IDs.

## Root cause
ULIDs generated in the same millisecond with a non-monotonic source.

## Fix
Use a monotonic entropy source.

## Files
- ids/ulid.go
