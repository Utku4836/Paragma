---
id: 01SHOPERRF0000000000000050
type: error
layer: episodic
status: resolved
title: Session store failure #51
seen: 1
tags: [session]
created: 2026-07-17
---
# Session store failure #51

## Symptom
The session store failed in staging with errors in the logs and a panic.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Mapped both field names until the provider finishes the migration.
