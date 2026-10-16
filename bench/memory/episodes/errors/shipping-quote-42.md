---
id: 01SHOPERRF0000000000000042
type: error
layer: episodic
status: resolved
title: Shipping quote failure #43
seen: 1
tags: [shipping]
created: 2026-08-13
---
# Shipping quote failure #43

## Symptom
The shipping quote failed in local runs with errors in the logs and wrong totals.

## Root cause
The cause was a missing index on orders.customer_id.

## Fix
Cleared the cache and keyed it on go.sum.
