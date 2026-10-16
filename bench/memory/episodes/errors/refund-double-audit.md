---
id: 01SHOPERR00000000000000012
type: error
layer: episodic
status: resolved
title: Refund audited twice
sig: 'expected 1 audit entry, got 2'
seen: 1
tags: [payments, audit]
created: 2026-04-16
updated: 2026-09-27
---
# Refund audited twice

## Symptom
A refund created two audit entries.

## Root cause
Audit was recorded before the provider call and again in the retry path.

## Fix
Record once, after the provider call succeeds.

## Files
- payments/refund.go
