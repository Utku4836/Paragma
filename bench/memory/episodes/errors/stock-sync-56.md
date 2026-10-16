---
id: 01SHOPERRF0000000000000056
type: error
layer: episodic
status: resolved
title: Stock sync failure #57
seen: 1
tags: [stock]
created: 2026-08-23
---
# Stock sync failure #57

## Symptom
The stock sync failed in local runs with errors in the logs and missing rows.

## Root cause
The cause was a goroutine leak in the retry loop.

## Fix
Cleared the cache and keyed it on go.sum.
