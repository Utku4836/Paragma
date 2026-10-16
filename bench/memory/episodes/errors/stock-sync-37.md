---
id: 01SHOPERRF0000000000000037
type: error
layer: episodic
status: resolved
title: Stock sync failure #38
seen: 3
tags: [stock]
created: 2026-07-26
---
# Stock sync failure #38

## Symptom
The stock sync failed in production with errors in the logs and a panic.

## Root cause
The cause was an unbounded channel filling up.

## Fix
Copied the slice before appending.
