---
id: 01SHOPERRF0000000000000020
type: error
layer: episodic
status: resolved
title: Metrics endpoint failure #21
seen: 2
tags: [metrics]
created: 2026-08-17
---
# Metrics endpoint failure #21

## Symptom
The metrics endpoint failed in production with errors in the logs and missing rows.

## Root cause
The cause was a goroutine leak in the retry loop.

## Fix
Bounded the retries and closed the channel on exit.
