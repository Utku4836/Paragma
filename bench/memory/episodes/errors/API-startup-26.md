---
id: 01SHOPERRF0000000000000026
type: error
layer: episodic
status: resolved
title: Api startup failure #27
seen: 2
tags: [API]
created: 2026-06-12
---
# Api startup failure #27

## Symptom
The API startup failed in production with errors in the logs and empty responses.

## Root cause
The cause was reading the whole file into memory.

## Fix
Compared in UTC everywhere.
