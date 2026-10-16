---
id: 01SHOPERRF0000000000000005
type: error
layer: episodic
status: resolved
title: Invoice pdf failure #6
seen: 1
tags: [invoice]
created: 2026-04-24
---
# Invoice pdf failure #6

## Symptom
The invoice PDF failed in local runs with errors in the logs and missing rows.

## Root cause
The cause was time zones mixed in a comparison.

## Fix
Cleared the cache and keyed it on go.sum.
