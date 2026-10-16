---
id: 01SHOPERRF0000000000000003
type: error
layer: episodic
status: resolved
title: Rate limiter failure #4
seen: 2
tags: [rate]
created: 2026-03-24
---
# Rate limiter failure #4

## Symptom
The rate limiter failed in CI with errors in the logs and empty responses.

## Root cause
The cause was time zones mixed in a comparison.

## Fix
Fixed the page arithmetic and tested the last page.
