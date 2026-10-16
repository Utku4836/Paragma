---
id: 01SHOPERRF0000000000000015
type: error
layer: episodic
status: resolved
title: Api startup failure #16
seen: 2
tags: [API]
created: 2026-06-25
---
# Api startup failure #16

## Symptom
The API startup failed in CI with errors in the logs and timeouts.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Cleared the cache and keyed it on go.sum.
