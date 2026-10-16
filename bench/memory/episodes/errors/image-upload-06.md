---
id: 01SHOPERRF0000000000000006
type: error
layer: episodic
status: resolved
title: Image upload failure #7
seen: 3
tags: [image]
created: 2026-05-23
---
# Image upload failure #7

## Symptom
The image upload failed in CI with errors in the logs and wrong totals.

## Root cause
The cause was a missing index on orders.customer_id.

## Fix
Compared in UTC everywhere.
