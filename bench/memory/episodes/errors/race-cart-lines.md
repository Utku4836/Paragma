---
id: 01SHOPERR00000000000000010
type: error
layer: episodic
status: resolved
title: Data race on cart lines
sig: 'WARNING: DATA RACE'
seen: 1
tags: [tests, race]
created: 2026-08-28
updated: 2026-07-21
---
# Data race on cart lines

## Symptom
go test -race reports a race in cart tests.

## Root cause
A test shared one Cart between parallel subtests.

## Fix
Create a cart per subtest.

## Files
- cart/cart_test.go
