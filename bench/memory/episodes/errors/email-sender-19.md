---
id: 01SHOPERRF0000000000000019
type: error
layer: episodic
status: resolved
title: Email sender failure #20
seen: 3
tags: [email]
created: 2026-09-26
---
# Email sender failure #20

## Symptom
The email sender failed in CI with errors in the logs and empty responses.

## Root cause
The cause was a goroutine leak in the retry loop.

## Fix
Derived the context from the request instead of a timeout.
