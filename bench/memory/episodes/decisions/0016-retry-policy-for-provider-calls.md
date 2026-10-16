---
id: 01SHOPDECF0000000000000016
type: decision
layer: episodic
status: active
title: "0016: Retry policy for provider calls"
created: 2026-07-22
---
# 0016: Retry policy for provider calls

## Context
We needed a consistent answer for retry policy for provider calls across the API and the admin tools.

## Decision
Retry policy for provider calls. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
