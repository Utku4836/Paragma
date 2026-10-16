---
id: 01SHOPERR00000000000000003
type: error
layer: episodic
status: resolved
title: Flaky audit order test
sig: 'expected charge before refund'
seen: 2
tags: [tests, flaky]
created: 2026-04-23
updated: 2026-09-12
---
# Flaky audit order test

## Symptom
TestAuditOrder fails about 1 in 20 runs.

## Root cause
Two goroutines recorded entries; the test assumed order.

## Fix
Sort entries by time and action in the test; production code was fine.

## Files
- audit/audit_test.go
