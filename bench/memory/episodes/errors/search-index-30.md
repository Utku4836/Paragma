---
id: 01SHOPERRF0000000000000030
type: error
layer: episodic
status: resolved
title: Search index failure #31
seen: 1
tags: [search]
created: 2026-09-28
---
# Search index failure #31

## Symptom
The search index failed in CI with errors in the logs and a panic.

## Root cause
The cause was the provider changing a field name.

## Fix
Derived the context from the request instead of a timeout.
