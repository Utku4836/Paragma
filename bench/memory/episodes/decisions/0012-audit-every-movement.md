---
id: 01SHOPDEC00120000000000000
type: decision
layer: episodic
status: active
title: "0012: Audit every money movement"
created: 2026-07-17
---
# 0012: Audit every money movement

## Context
Finance found refunds missing from reconciliation.

## Decision
Charge, refund and payout call audit.Record after the provider succeeds.

## Alternatives
- Audit in the provider adapter: misses fakes in tests.

## Consequences
Documented in the rules; reviewers check it.
