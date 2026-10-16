---
id: 01SHOPERRF0000000000000035
type: error
layer: episodic
status: resolved
title: Session store failure #36
seen: 3
tags: [session]
created: 2026-06-13
---
# Session store failure #36

## Symptom
The session store failed in staging with errors in the logs and empty responses.

## Root cause
The cause was a context cancelled too early.

## Fix
Derived the context from the request instead of a timeout.
