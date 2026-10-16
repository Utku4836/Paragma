---
id: 01SHOPERRF0000000000000057
type: error
layer: episodic
status: resolved
title: Order status job failure #58
seen: 2
tags: [order]
created: 2026-08-16
---
# Order status job failure #58

## Symptom
The order status job failed in production with errors in the logs and a panic.

## Root cause
The cause was time zones mixed in a comparison.

## Fix
Derived the context from the request instead of a timeout.
