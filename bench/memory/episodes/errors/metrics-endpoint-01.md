---
id: 01SHOPERRF0000000000000001
type: error
layer: episodic
status: resolved
title: Metrics endpoint failure #2
seen: 2
tags: [metrics]
created: 2026-03-12
---
# Metrics endpoint failure #2

## Symptom
The metrics endpoint failed in CI with errors in the logs and wrong totals.

## Root cause
The cause was a stale build cache in CI.

## Fix
Copied the slice before appending.
