---
id: 01SHOPERRF0000000000000021
type: error
layer: episodic
status: resolved
title: Stock sync failure #22
seen: 1
tags: [stock]
created: 2026-03-18
---
# Stock sync failure #22

## Symptom
The stock sync failed in local runs with errors in the logs and a panic.

## Root cause
The cause was an unbounded channel filling up.

## Fix
Streamed the file line by line.
