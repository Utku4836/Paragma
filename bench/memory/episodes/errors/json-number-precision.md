---
id: 01SHOPERR00000000000000009
type: error
layer: episodic
status: resolved
title: Prices lose precision in admin export
sig: 'price 34900.000001'
seen: 1
tags: [money]
created: 2026-06-27
updated: 2026-07-13
---
# Prices lose precision in admin export

## Symptom
Exported CSV shows prices like 34900.000001.

## Root cause
The exporter decoded JSON numbers into float64.

## Fix
Decode into json.Number, then int64.

## Files
- admin/export.go
