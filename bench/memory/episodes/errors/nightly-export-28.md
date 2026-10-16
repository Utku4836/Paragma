---
id: 01SHOPERRF0000000000000028
type: error
layer: episodic
status: resolved
title: Nightly export failure #29
seen: 1
tags: [nightly]
created: 2026-03-10
---
# Nightly export failure #29

## Symptom
The nightly export failed in staging with errors in the logs and missing rows.

## Root cause
The cause was a context cancelled too early.

## Fix
Derived the context from the request instead of a timeout.
