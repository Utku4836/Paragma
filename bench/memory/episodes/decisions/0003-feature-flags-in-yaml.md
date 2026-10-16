---
id: 01SHOPDECF0000000000000003
type: decision
layer: episodic
status: active
title: "0003: Feature flags in YAML"
created: 2026-06-24
---
# 0003: Feature flags in YAML

## Context
We needed a consistent answer for feature flags in yaml across the API and the admin tools.

## Decision
Feature flags in YAML. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
