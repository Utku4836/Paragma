---
id: 01SHOPERR00000000000000004
type: error
layer: episodic
status: resolved
title: VAT rounding off by one kuruş
sig: 'total 2398 want 2399'
seen: 1
tags: [pricing, rounding]
created: 2026-05-12
updated: 2026-09-11
---
# VAT rounding off by one kuruş

## Symptom
Totals were one kuruş short for some prices.

## Root cause
Integer division truncated instead of rounding half up.

## Fix
Use (net*rate+5000)/10000 as in pricing.WithTax.

## Files
- pricing/tax.go
