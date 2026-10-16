---
id: 01SHOPERRF0000000000000048
type: error
layer: episodic
status: resolved
title: Invoice pdf failure #49
seen: 3
tags: [invoice]
created: 2026-07-25
---
# Invoice pdf failure #49

## Symptom
The invoice PDF failed in CI with errors in the logs and timeouts.

## Root cause
The cause was a missing index on orders.customer_id.

## Fix
Derived the context from the request instead of a timeout.
