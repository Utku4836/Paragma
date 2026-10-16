---
id: 01SHOPERRF0000000000000053
type: error
layer: episodic
status: resolved
title: Image upload failure #54
seen: 1
tags: [image]
created: 2026-05-19
---
# Image upload failure #54

## Symptom
The image upload failed in production with errors in the logs and a panic.

## Root cause
The cause was a nil map in the config struct.

## Fix
Added the index in a forward migration.
