---
id: 01SHOPDEC00140000000000000
type: decision
layer: episodic
status: active
title: "0014: Refund window of 30 days"
created: 2026-03-12
---
# 0014: Refund window of 30 days

## Context
Finance closes the books monthly and asked us to stop refunds older than a month. The provider allows 180 days.

## Decision
Refunds are accepted only within 30 days of payment.At (measured with clock.Now()). Past the window, payments.Refund returns the exported sentinel `ErrRefundWindow`. Partial refunds are fine while the total stays within the paid amount; each refund is audited as "refund".

## Alternatives
- 180 days like the provider: rejected by finance.
- Manual approval past 30 days: later, via the admin tool.

## Consequences
Documented in the rules; reviewers check it.
