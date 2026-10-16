---
id: 01SHOPERRF0000000000000025
type: error
layer: episodic
status: resolved
title: Shipping quote failure #26
seen: 1
tags: [shipping]
created: 2026-06-15
---
# Shipping quote failure #26

## Symptom
The shipping quote failed in local runs with errors in the logs and a panic.

## Root cause
The cause was time zones mixed in a comparison.

## Fix
Streamed the file line by line.
