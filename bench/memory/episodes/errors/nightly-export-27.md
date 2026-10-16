---
id: 01SHOPERRF0000000000000027
type: error
layer: episodic
status: resolved
title: Nightly export failure #28
seen: 3
tags: [nightly]
created: 2026-06-14
---
# Nightly export failure #28

## Symptom
The nightly export failed in local runs with errors in the logs and a panic.

## Root cause
The cause was a nil map in the config struct.

## Fix
Added the index in a forward migration.
