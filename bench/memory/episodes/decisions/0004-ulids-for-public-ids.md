---
id: 01SHOPDECF0000000000000004
type: decision
layer: episodic
status: active
title: "0004: ULIDs for public IDs"
created: 2026-02-18
---
# 0004: ULIDs for public IDs

## Context
We needed a consistent answer for ulids for public ids across the API and the admin tools.

## Decision
ULIDs for public IDs. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
