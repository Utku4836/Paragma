---
id: 01SHOPERR00000000000000019
type: error
layer: episodic
status: resolved
title: Payouts scheduled on weekends
sig: 'payout date is Saturday'
seen: 1
tags: [payouts]
created: 2026-06-14
updated: 2026-09-23
---
# Payouts scheduled on weekends

## Symptom
Merchant payouts landed on Saturdays.

## Root cause
Business-day logic ignored weekends.

## Fix
Move to the next Monday; holidays are a known gap.

## Files
- payouts/schedule.go
