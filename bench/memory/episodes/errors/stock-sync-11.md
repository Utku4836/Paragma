---
id: 01SHOPERRF0000000000000011
type: error
layer: episodic
status: resolved
title: Stock sync failure #12
seen: 1
tags: [stock]
created: 2026-04-12
---
# Stock sync failure #12

## Symptom
The stock sync failed in production with errors in the logs and wrong totals.

## Root cause
The cause was an off-by-one in pagination.

## Fix
Compared in UTC everywhere.
