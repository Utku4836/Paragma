---
id: 01SHOPERRF0000000000000017
type: error
layer: episodic
status: resolved
title: Webhook handler failure #18
seen: 3
tags: [webhook]
created: 2026-05-14
---
# Webhook handler failure #18

## Symptom
The webhook handler failed in staging with errors in the logs and missing rows.

## Root cause
The cause was a nil map in the config struct.

## Fix
Bounded the retries and closed the channel on exit.
