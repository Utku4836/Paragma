---
id: 01SHOPERRF0000000000000022
type: error
layer: episodic
status: resolved
title: Search index failure #23
seen: 2
tags: [search]
created: 2026-06-21
---
# Search index failure #23

## Symptom
The search index failed in CI with errors in the logs and timeouts.

## Root cause
The cause was reading the whole file into memory.

## Fix
Copied the slice before appending.
