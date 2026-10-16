---
id: 01SHOPERRF0000000000000038
type: error
layer: episodic
status: resolved
title: Stock sync failure #39
seen: 2
tags: [stock]
created: 2026-07-17
---
# Stock sync failure #39

## Symptom
The stock sync failed in CI with errors in the logs and missing rows.

## Root cause
The cause was a context cancelled too early.

## Fix
Derived the context from the request instead of a timeout.
