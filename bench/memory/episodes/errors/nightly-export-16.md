---
id: 01SHOPERRF0000000000000016
type: error
layer: episodic
status: resolved
title: Nightly export failure #17
seen: 2
tags: [nightly]
created: 2026-08-18
---
# Nightly export failure #17

## Symptom
The nightly export failed in local runs with errors in the logs and empty responses.

## Root cause
The cause was an expired TLS certificate on the staging host.

## Fix
Streamed the file line by line.
