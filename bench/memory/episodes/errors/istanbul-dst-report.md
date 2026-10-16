---
id: 01SHOPERR00000000000000007
type: error
layer: episodic
status: resolved
title: Daily report shifted by an hour
sig: 'report window 23:00-23:00'
seen: 1
tags: [time]
created: 2026-05-11
updated: 2026-07-19
---
# Daily report shifted by an hour

## Symptom
The daily sales report covered 23:00 to 23:00.

## Root cause
Report used UTC midnight instead of Istanbul midnight.

## Fix
Compute the window in Europe/Istanbul, then convert to UTC.

## Files
- reports/daily.go
