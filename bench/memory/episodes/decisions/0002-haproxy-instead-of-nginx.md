---
id: 01SHOPDECF0000000000000002
type: decision
layer: episodic
status: active
title: "0002: HAProxy instead of nginx"
created: 2026-04-15
---
# 0002: HAProxy instead of nginx

## Context
We needed a consistent answer for haproxy instead of nginx across the API and the admin tools.

## Decision
HAProxy instead of nginx. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
