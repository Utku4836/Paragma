---
id: 01SHOPERRF0000000000000043
type: error
layer: episodic
status: resolved
title: Nightly export failure #44
seen: 1
tags: [nightly]
created: 2026-08-23
---
# Nightly export failure #44

## Symptom
The nightly export failed in local runs with errors in the logs and a panic.

## Root cause
The cause was an off-by-one in pagination.

## Fix
Bounded the retries and closed the channel on exit.
