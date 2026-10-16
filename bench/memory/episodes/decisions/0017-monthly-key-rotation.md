---
id: 01SHOPDECF0000000000000017
type: decision
layer: episodic
status: active
title: "0017: Monthly key rotation"
created: 2026-03-25
---
# 0017: Monthly key rotation

## Context
We needed a consistent answer for monthly key rotation across the API and the admin tools.

## Decision
Monthly key rotation. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
