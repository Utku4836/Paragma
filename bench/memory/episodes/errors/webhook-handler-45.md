---
id: 01SHOPERRF0000000000000045
type: error
layer: episodic
status: resolved
title: Webhook handler failure #46
seen: 3
tags: [webhook]
created: 2026-03-22
---
# Webhook handler failure #46

## Symptom
The webhook handler failed in CI with errors in the logs and missing rows.

## Root cause
The cause was a stale build cache in CI.

## Fix
Cleared the cache and keyed it on go.sum.
