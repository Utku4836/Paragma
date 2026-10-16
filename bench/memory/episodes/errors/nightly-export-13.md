---
id: 01SHOPERRF0000000000000013
type: error
layer: episodic
status: resolved
title: Nightly export failure #14
seen: 2
tags: [nightly]
created: 2026-07-10
---
# Nightly export failure #14

## Symptom
The nightly export failed in staging with errors in the logs and empty responses.

## Root cause
The cause was a context cancelled too early.

## Fix
Rotated the certificate and added an expiry alert.
