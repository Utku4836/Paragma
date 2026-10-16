---
id: 01SHOPDECF0000000000000020
type: decision
layer: episodic
status: active
title: "0020: golangci-lint with default rules"
created: 2026-02-12
---
# 0020: golangci-lint with default rules

## Context
We needed a consistent answer for golangci-lint with default rules across the API and the admin tools.

## Decision
golangci-lint with default rules. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
