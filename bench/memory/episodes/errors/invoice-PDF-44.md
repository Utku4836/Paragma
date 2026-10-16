---
id: 01SHOPERRF0000000000000044
type: error
layer: episodic
status: resolved
title: Invoice pdf failure #45
seen: 2
tags: [invoice]
created: 2026-03-21
---
# Invoice pdf failure #45

## Symptom
The invoice PDF failed in staging with errors in the logs and a panic.

## Root cause
The cause was a goroutine leak in the retry loop.

## Fix
Switched to a bounded worker pool.
