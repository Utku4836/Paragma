---
id: 01SHOPERR00000000000000000
type: error
layer: episodic
status: resolved
title: Catalog checksum mismatch on Windows
sig: 'catalog checksum mismatch: got <hex> want <hex>'
seen: 2
tags: [catalog, windows, tests]
created: 2026-06-14
updated: 2026-09-11
---
# Catalog checksum mismatch on Windows

## Symptom
`go test ./catalog` fails with `catalog checksum mismatch` on Windows machines only; CI on Linux is green.

## Root cause
Git on Windows (core.autocrlf=true) checks out testdata/catalog.json with CRLF line endings. The pinned checksum is computed over the LF file, so the bytes differ.

## Fix
Normalise line endings in catalog.Load before hashing and decoding: `b = bytes.ReplaceAll(b, []byte("\r\n"), []byte("\n"))`. Also add `testdata/** -text` to .gitattributes. Do NOT change ExpectedChecksum: CI computes it over the LF file and would break.

## Files
- catalog/catalog.go
- .gitattributes
