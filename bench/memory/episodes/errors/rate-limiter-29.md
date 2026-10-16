---
id: 01SHOPERRF0000000000000029
type: error
layer: episodic
status: resolved
title: Rate limiter failure #30
seen: 1
tags: [rate]
created: 2026-09-16
---
# Rate limiter failure #30

## Symptom
The rate limiter failed in staging with errors in the logs and a panic.

## Root cause
The cause was a missing index on orders.customer_id.

## Fix
Compared in UTC everywhere.
