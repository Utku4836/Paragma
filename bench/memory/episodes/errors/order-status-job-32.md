---
id: 01SHOPERRF0000000000000032
type: error
layer: episodic
status: resolved
title: Order status job failure #33
seen: 3
tags: [order]
created: 2026-04-27
---
# Order status job failure #33

## Symptom
The order status job failed in production with errors in the logs and missing rows.

## Root cause
The cause was a context cancelled too early.

## Fix
Compared in UTC everywhere.
