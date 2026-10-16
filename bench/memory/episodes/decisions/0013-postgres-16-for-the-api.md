---
id: 01SHOPDECF0000000000000013
type: decision
layer: episodic
status: active
title: "0013: Postgres 16 for the API"
created: 2026-04-10
---
# 0013: Postgres 16 for the API

## Context
We needed a consistent answer for postgres 16 for the api across the API and the admin tools.

## Decision
Postgres 16 for the API. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
