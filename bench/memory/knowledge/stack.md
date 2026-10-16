---
id: 01SHOPSTACK0000000000000AA
type: note
layer: semantic
title: Stack
verified: 2026-08-30
---
# Stack

## Language
Go 1.24, standard library first. No ORM.

## Storage
SQLite (modernc, no cgo) for the admin tools, PostgreSQL 16 for the API service.

## CI
GitHub Actions on ubuntu-latest: go vet, go test -race ./..., golangci-lint. Developers work on Windows and macOS.

## Payments
Iyzico in production; the Provider interface keeps it swappable.
