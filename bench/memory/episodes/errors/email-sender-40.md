---
id: 01SHOPERRF0000000000000040
type: error
layer: episodic
status: resolved
title: Email sender failure #41
seen: 1
tags: [email]
created: 2026-06-12
---
# Email sender failure #41

## Symptom
The email sender failed in production with errors in the logs and a panic.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Fixed the page arithmetic and tested the last page.
