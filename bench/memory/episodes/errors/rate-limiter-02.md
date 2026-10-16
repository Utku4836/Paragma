---
id: 01SHOPERRF0000000000000002
type: error
layer: episodic
status: resolved
title: Rate limiter failure #3
seen: 1
tags: [rate]
created: 2026-08-19
---
# Rate limiter failure #3

## Symptom
The rate limiter failed in local runs with errors in the logs and a panic.

## Root cause
The cause was an off-by-one in pagination.

## Fix
Rotated the certificate and added an expiry alert.
