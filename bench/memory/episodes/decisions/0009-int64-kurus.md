---
id: 01SHOPDEC00090000000000000
type: decision
layer: episodic
status: active
title: "0009: Money as int64 kuruş"
created: 2026-09-26
---
# 0009: Money as int64 kuruş

## Context
Float rounding produced one-kuruş differences in reports.

## Decision
All amounts are int64 kuruş; formatting divides by 100.

## Alternatives
- decimal library: extra dependency, slower.

## Consequences
Documented in the rules; reviewers check it.
