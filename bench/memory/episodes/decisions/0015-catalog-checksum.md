---
id: 01SHOPDEC00150000000000000
type: decision
layer: episodic
status: active
title: "0015: Pin the catalog checksum"
created: 2026-08-19
---
# 0015: Pin the catalog checksum

## Context
A half-deployed catalog file served empty prices.

## Decision
catalog.Load verifies sha256 against ExpectedChecksum computed over the LF file.

## Alternatives
- Signature file: more moving parts.

## Consequences
Documented in the rules; reviewers check it.
