---
id: 01JA3Z8K2MARCH0000000000AA
type: note
layer: semantic
title: Mimari
tags: [architecture, backend]
verified: 2026-09-20
---
# Mimari

Servis üç katmandan oluşur; her katman yalnızca altındakini çağırır.

## Katmanlar

### API

HTTP router (`chi`) ve gRPC sunucusu aynı handler katmanını paylaşır.
Handler'lar iş mantığı içermez, yalnızca doğrulama ve dönüşüm yapar.

### Depolama

SQLite, `internal/db` paketinde açılır. Bağlantı havuzu tek yazıcıya ayarlıdır.

> [!rule] DB açılışında WAL zorunlu ^r-wal
> `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;`

> [!rule] Migration'lar yalnızca `cmd/migrate` üzerinden çalışır.

## Middleware

Auth middleware iki yerden çağrılır: HTTP router ve gRPC interceptor.
Token doğrulaması yalnızca middleware katmanında yapılır, bkz [[rules#Güvenlik]].

- [x] Router entegrasyonu ^m1
- [/] gRPC interceptor ^m2

## Dağıtım

Fly.io üzerinde iki bölge: `ams` ve `fra`. Deploy `make deploy` ile yapılır.
Gizli anahtarlar `fly secrets` ile yönetilir, depoya asla girmez.
