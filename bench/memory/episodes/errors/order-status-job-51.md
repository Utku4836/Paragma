---
id: 01SHOPERRF0000000000000051
type: error
layer: episodic
status: resolved
title: Order status job failure #52
seen: 1
tags: [order]
created: 2026-05-27
---
# Order status job failure #52

## Symptom
The order status job failed in local runs with errors in the logs and a panic.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Cleared the cache and keyed it on go.sum.
