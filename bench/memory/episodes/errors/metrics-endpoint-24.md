---
id: 01SHOPERRF0000000000000024
type: error
layer: episodic
status: resolved
title: Metrics endpoint failure #25
seen: 2
tags: [metrics]
created: 2026-08-21
---
# Metrics endpoint failure #25

## Symptom
The metrics endpoint failed in staging with errors in the logs and timeouts.

## Root cause
The cause was a slice aliasing bug when appending.

## Fix
Added a guard and a regression test.
