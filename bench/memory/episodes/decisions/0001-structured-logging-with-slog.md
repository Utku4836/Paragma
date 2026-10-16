---
id: 01SHOPDECF0000000000000001
type: decision
layer: episodic
status: active
title: "0001: Structured logging with slog"
created: 2026-02-24
---
# 0001: Structured logging with slog

## Context
We needed a consistent answer for structured logging with slog across the API and the admin tools.

## Decision
Structured logging with slog. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
