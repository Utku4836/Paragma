---
id: 01SHOPERRF0000000000000014
type: error
layer: episodic
status: resolved
title: Metrics endpoint failure #15
seen: 3
tags: [metrics]
created: 2026-05-21
---
# Metrics endpoint failure #15

## Symptom
The metrics endpoint failed in CI with errors in the logs and wrong totals.

## Root cause
The cause was time zones mixed in a comparison.

## Fix
Added the index in a forward migration.
