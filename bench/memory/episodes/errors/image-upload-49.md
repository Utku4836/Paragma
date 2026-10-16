---
id: 01SHOPERRF0000000000000049
type: error
layer: episodic
status: resolved
title: Image upload failure #50
seen: 1
tags: [image]
created: 2026-06-12
---
# Image upload failure #50

## Symptom
The image upload failed in CI with errors in the logs and timeouts.

## Root cause
The cause was a nil map in the config struct.

## Fix
Streamed the file line by line.
