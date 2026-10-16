---
id: 01SHOPCIDEMPOTENCY00000000
type: concept
layer: semantic
title: Idempotency keys
verified: 2026-09-01
---
# Idempotency keys

Charges carry an idempotency key derived from the cart ID so that a retried request does not charge twice. Refunds use the payment ID plus a refund sequence number.
