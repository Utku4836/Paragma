---
id: 01SHOPERRF0000000000000018
type: error
layer: episodic
status: resolved
title: Image upload failure #19
seen: 3
tags: [image]
created: 2026-09-18
---
# Image upload failure #19

## Symptom
The image upload failed in CI with errors in the logs and empty responses.

## Root cause
The cause was an off-by-one in pagination.

## Fix
Rotated the certificate and added an expiry alert.
