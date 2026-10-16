---
id: 01SHOPERRF0000000000000023
type: error
layer: episodic
status: resolved
title: Search index failure #24
seen: 2
tags: [search]
created: 2026-04-20
---
# Search index failure #24

## Symptom
The search index failed in production with errors in the logs and wrong totals.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Bounded the retries and closed the channel on exit.
