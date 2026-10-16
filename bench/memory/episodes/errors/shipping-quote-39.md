---
id: 01SHOPERRF0000000000000039
type: error
layer: episodic
status: resolved
title: Shipping quote failure #40
seen: 1
tags: [shipping]
created: 2026-06-13
---
# Shipping quote failure #40

## Symptom
The shipping quote failed in local runs with errors in the logs and wrong totals.

## Root cause
The cause was a goroutine leak in the retry loop.

## Fix
Cleared the cache and keyed it on go.sum.
