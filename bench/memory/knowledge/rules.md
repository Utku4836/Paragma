---
id: 01SHOPRULES0000000000000AA
type: rule
layer: semantic
title: Rules
verified: 2026-09-28
---
# Rules

Project rules. One rule per callout; follow them unless the user says otherwise.

## Money

> [!rule] Amounts are int64 kuruş everywhere. Never float64 for money, not even in tests. ^r001

> [!rule] Rounding is half up, done once, at the last step (see pricing.WithTax). ^r002

> [!rule] Refunds are allowed only within 30 days of the payment (payment.At). Past that, return payments.ErrRefundWindow. Decided in [[0014-refund-window]]. ^r003

> [!rule] Partial refunds are allowed; the sum of refunds must never exceed the paid amount (return ErrInvalidAmount). ^r004

> [!rule] Every money movement (charge, refund, payout) calls audit.Record(ctx, action, paymentID) after the provider call succeeds. The action names are "charge", "refund", "payout". ^r005

> [!rule] Discount codes are validated server side only; the client never computes prices. ^r006


## Time

> [!rule] Never call time.Now() in library code. Use clock.Now() so tests can pin time. ^r007

> [!rule] Store and compare times in UTC; convert to Europe/Istanbul only for display. ^r008

> [!rule] Durations in config are Go duration strings ("30s"), never bare integers. ^r009


## Errors

> [!rule] Wrap errors with the operation name: fmt.Errorf("refund: %w", err). ^r010

> [!rule] Sentinel errors are exported vars named ErrXxx in the package that returns them. ^r011

> [!rule] Do not log and return the same error; return it, the caller logs. ^r012

> [!rule] Provider errors are wrapped, never compared by string. ^r013


## Testing

> [!rule] Table tests for anything with more than two cases. ^r014

> [!rule] Tests that depend on time pin clock.Now and restore it with t.Cleanup. ^r015

> [!rule] No network in unit tests; payment providers are faked with in-package fakes. ^r016

> [!rule] Golden files live in testdata/ and are compared after normalising line endings. ^r017

> [!rule] Every bug fix lands with a regression test named after the symptom. ^r018


## Code style

> [!rule] Package names are singular and short: cart, not carts. ^r019

> [!rule] Exported functions have a doc comment starting with the name. ^r020

> [!rule] No init() functions except for registering metrics. ^r021

> [!rule] Keep handlers thin: validation and mapping only, logic in packages. ^r022

> [!rule] Configuration is read once in main and passed down; no global config reads. ^r023


## Data

> [!rule] Migrations are forward only and live in migrations/NNNN_name.sql. ^r024

> [!rule] Every table has created_at and updated_at in UTC. ^r025

> [!rule] SQLite is opened with WAL and busy_timeout=5000 (see [[sqlite-database-locked]]). ^r026

> [!rule] IDs are ULIDs as text, never autoincrement integers in APIs. ^r027


## Ops

> [!rule] Feature flags live in config/flags.yaml; removing a flag needs a cleanup task. ^r028

> [!rule] Secrets come from the environment only; never commit .env files. ^r029

> [!rule] Releases are tagged vMAJOR.MINOR.PATCH from main after CI is green. ^r030

> [!rule] Logs are JSON lines; never print request bodies (PII). ^r031
