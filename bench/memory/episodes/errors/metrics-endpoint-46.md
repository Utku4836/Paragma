---
id: 01SHOPERRF0000000000000046
type: error
layer: episodic
status: resolved
title: Metrics endpoint failure #47
seen: 1
tags: [metrics]
created: 2026-03-17
---
# Metrics endpoint failure #47

## Symptom
The metrics endpoint failed in staging with errors in the logs and timeouts.

## Root cause
The cause was the provider changing a field name.

## Fix
Derived the context from the request instead of a timeout.
