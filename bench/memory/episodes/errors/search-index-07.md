---
id: 01SHOPERRF0000000000000007
type: error
layer: episodic
status: resolved
title: Search index failure #8
seen: 1
tags: [search]
created: 2026-04-17
---
# Search index failure #8

## Symptom
The search index failed in production with errors in the logs and timeouts.

## Root cause
The cause was a missing index on orders.customer_id.

## Fix
Rotated the certificate and added an expiry alert.
