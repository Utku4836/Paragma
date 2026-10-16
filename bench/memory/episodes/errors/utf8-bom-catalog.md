---
id: 01SHOPERR00000000000000017
type: error
layer: episodic
status: resolved
title: Catalog JSON with BOM
sig: 'invalid character ''ï'''
seen: 1
tags: [catalog]
created: 2026-09-24
updated: 2026-08-12
---
# Catalog JSON with BOM

## Symptom
Catalog failed to parse after editing in Notepad.

## Root cause
Notepad saved a UTF-8 BOM.

## Fix
Strip the BOM in catalog.Load; merchandising tool exports without BOM.

## Files
- catalog/catalog.go
