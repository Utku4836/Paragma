---
id: 01SHOPERRF0000000000000004
type: error
layer: episodic
status: resolved
title: Metrics endpoint failure #5
seen: 1
tags: [metrics]
created: 2026-04-19
---
# Metrics endpoint failure #5

## Symptom
The metrics endpoint failed in production with errors in the logs and empty responses.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Cleared the cache and keyed it on go.sum.
