---
id: 01SHOPERR00000000000000014
type: error
layer: episodic
status: resolved
title: Cart cache timeouts
sig: 'i/o timeout'
seen: 1
tags: [redis]
created: 2026-07-21
updated: 2026-08-17
---
# Cart cache timeouts

## Symptom
Intermittent `i/o timeout` from the cart cache.

## Root cause
Redis maxclients reached during load tests.

## Fix
Pool size 20 per instance; alert at 80% clients.

## Files
- cache/redis.go
