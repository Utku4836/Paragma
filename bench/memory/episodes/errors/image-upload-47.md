---
id: 01SHOPERRF0000000000000047
type: error
layer: episodic
status: resolved
title: Image upload failure #48
seen: 1
tags: [image]
created: 2026-05-14
---
# Image upload failure #48

## Symptom
The image upload failed in local runs with errors in the logs and a panic.

## Root cause
The cause was the provider changing a field name.

## Fix
Added a guard and a regression test.
