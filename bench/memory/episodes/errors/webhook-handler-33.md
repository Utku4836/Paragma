---
id: 01SHOPERRF0000000000000033
type: error
layer: episodic
status: resolved
title: Webhook handler failure #34
seen: 1
tags: [webhook]
created: 2026-07-10
---
# Webhook handler failure #34

## Symptom
The webhook handler failed in production with errors in the logs and empty responses.

## Root cause
The cause was a nil map in the config struct.

## Fix
Cleared the cache and keyed it on go.sum.
