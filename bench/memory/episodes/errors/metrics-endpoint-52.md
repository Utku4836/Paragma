---
id: 01SHOPERRF0000000000000052
type: error
layer: episodic
status: resolved
title: Metrics endpoint failure #53
seen: 3
tags: [metrics]
created: 2026-08-17
---
# Metrics endpoint failure #53

## Symptom
The metrics endpoint failed in staging with errors in the logs and empty responses.

## Root cause
The cause was a missing index on orders.customer_id.

## Fix
Added a guard and a regression test.
