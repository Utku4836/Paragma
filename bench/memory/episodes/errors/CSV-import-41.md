---
id: 01SHOPERRF0000000000000041
type: error
layer: episodic
status: resolved
title: Csv import failure #42
seen: 3
tags: [CSV]
created: 2026-08-21
---
# Csv import failure #42

## Symptom
The CSV import failed in production with errors in the logs and a panic.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Added the index in a forward migration.
