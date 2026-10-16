---
id: 01SHOPERRF0000000000000058
type: error
layer: episodic
status: resolved
title: Search index failure #59
seen: 3
tags: [search]
created: 2026-04-22
---
# Search index failure #59

## Symptom
The search index failed in CI with errors in the logs and timeouts.

## Root cause
The cause was reading the whole file into memory.

## Fix
Streamed the file line by line.
