---
id: 01SHOPDECF0000000000000006
type: decision
layer: episodic
status: active
title: "0006: SQLite for admin tools"
created: 2026-07-17
---
# 0006: SQLite for admin tools

## Context
We needed a consistent answer for sqlite for admin tools across the API and the admin tools.

## Decision
SQLite for admin tools. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
