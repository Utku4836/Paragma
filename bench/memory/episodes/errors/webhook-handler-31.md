---
id: 01SHOPERRF0000000000000031
type: error
layer: episodic
status: resolved
title: Webhook handler failure #32
seen: 1
tags: [webhook]
created: 2026-08-21
---
# Webhook handler failure #32

## Symptom
The webhook handler failed in local runs with errors in the logs and missing rows.

## Root cause
The cause was time zones mixed in a comparison.

## Fix
Added the index in a forward migration.
