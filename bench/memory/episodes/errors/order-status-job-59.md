---
id: 01SHOPERRF0000000000000059
type: error
layer: episodic
status: resolved
title: Order status job failure #60
seen: 1
tags: [order]
created: 2026-08-18
---
# Order status job failure #60

## Symptom
The order status job failed in local runs with errors in the logs and empty responses.

## Root cause
The cause was a missing index on orders.customer_id.

## Fix
Added a guard and a regression test.
