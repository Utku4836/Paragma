---
id: 01SHOPERR00000000000000015
type: error
layer: episodic
status: resolved
title: Wrong VAT for books
sig: 'vat 20% on category books'
seen: 1
tags: [pricing]
created: 2026-05-17
updated: 2026-06-28
---
# Wrong VAT for books

## Symptom
Books were charged 20% VAT.

## Root cause
Per-category rates are not implemented.

## Fix
Temporary: books excluded from the shop until category rates land.

## Files
- pricing/tax.go
