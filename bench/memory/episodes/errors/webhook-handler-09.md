---
id: 01SHOPERRF0000000000000009
type: error
layer: episodic
status: resolved
title: Webhook handler failure #10
seen: 3
tags: [webhook]
created: 2026-05-14
---
# Webhook handler failure #10

## Symptom
The webhook handler failed in staging with errors in the logs and wrong totals.

## Root cause
The cause was an unbounded channel filling up.

## Fix
Copied the slice before appending.
