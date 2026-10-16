---
id: 01SHOPERRF0000000000000000
type: error
layer: episodic
status: resolved
title: Admin tool failure #1
seen: 3
tags: [admin]
created: 2026-07-20
---
# Admin tool failure #1

## Symptom
The admin tool failed in CI with errors in the logs and a panic.

## Root cause
The cause was an off-by-one in pagination.

## Fix
Rotated the certificate and added an expiry alert.
