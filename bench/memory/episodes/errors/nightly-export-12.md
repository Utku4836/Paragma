---
id: 01SHOPERRF0000000000000012
type: error
layer: episodic
status: resolved
title: Nightly export failure #13
seen: 3
tags: [nightly]
created: 2026-03-13
---
# Nightly export failure #13

## Symptom
The nightly export failed in staging with errors in the logs and missing rows.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Switched to a bounded worker pool.
