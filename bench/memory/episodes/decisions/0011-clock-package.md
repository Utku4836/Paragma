---
id: 01SHOPDEC00110000000000000
type: decision
layer: episodic
status: active
title: "0011: Replaceable clock"
created: 2026-08-19
---
# 0011: Replaceable clock

## Context
Time-dependent tests were flaky around midnight.

## Decision
All library code calls clock.Now(); tests pin it.

## Alternatives
- Pass time as an argument everywhere: too invasive.

## Consequences
Documented in the rules; reviewers check it.
