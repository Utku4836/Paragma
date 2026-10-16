---
id: 01SHOPERRF0000000000000036
type: error
layer: episodic
status: resolved
title: Search index failure #37
seen: 1
tags: [search]
created: 2026-07-24
---
# Search index failure #37

## Symptom
The search index failed in staging with errors in the logs and timeouts.

## Root cause
The cause was the provider changing a field name.

## Fix
Added a guard and a regression test.
