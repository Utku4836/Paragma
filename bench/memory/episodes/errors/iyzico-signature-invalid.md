---
id: 01SHOPERR00000000000000002
type: error
layer: episodic
status: resolved
title: Provider signature invalid
sig: 'iyzico: invalid signature'
seen: 1
tags: [payments, config]
created: 2026-08-11
updated: 2026-07-11
---
# Provider signature invalid

## Symptom
Charges fail in staging with `invalid signature` after the key rotation.

## Root cause
The staging secret in the environment still had the old value; the API caches it at startup.

## Fix
Restart the API after rotating secrets; added a startup log line with the key id.

## Files
- cmd/api/main.go
