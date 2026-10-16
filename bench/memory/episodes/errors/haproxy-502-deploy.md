---
id: 01SHOPERR00000000000000008
type: error
layer: episodic
status: resolved
title: 502s during deploy
sig: '502 Bad Gateway'
seen: 2
tags: [ops]
created: 2026-07-14
updated: 2026-06-28
---
# 502s during deploy

## Symptom
Short bursts of 502 during deploys.

## Root cause
Nodes stopped before HAProxy marked them down.

## Fix
Drain for 30s via the admin socket before stopping.

## Files
- deploy/rollout.sh
