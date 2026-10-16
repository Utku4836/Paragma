---
id: 01SHOPERRF0000000000000034
type: error
layer: episodic
status: resolved
title: Nightly export failure #35
seen: 3
tags: [nightly]
created: 2026-03-27
---
# Nightly export failure #35

## Symptom
The nightly export failed in staging with errors in the logs and a panic.

## Root cause
The cause was a stale build cache in CI.

## Fix
Copied the slice before appending.
