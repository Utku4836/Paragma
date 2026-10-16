---
id: 01SHOPERRF0000000000000054
type: error
layer: episodic
status: resolved
title: Stock sync failure #55
seen: 1
tags: [stock]
created: 2026-05-21
---
# Stock sync failure #55

## Symptom
The stock sync failed in staging with errors in the logs and a panic.

## Root cause
The cause was a context cancelled too early.

## Fix
Fixed the page arithmetic and tested the last page.
