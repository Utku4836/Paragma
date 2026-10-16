---
id: 01JA3Z8K2MRULES000000000AA
type: rule
layer: semantic
title: Rules
verified: 2026-10-01
---
# Rules

## Veritabanı

> [!rule] DB açılışında WAL ve busy_timeout zorunlu. ^r001
> promoted from [[sqlite-database-locked]]

> [!rule] Testler gerçek SQLite dosyası kullanır, mock yok. ^r002

## Güvenlik

> [!rule] JWT süreleri her zaman UTC ile hesaplanır. ^r003
> fixes:: [[jwt-exp-timezone]]

> [!warning] Refresh token'lar tek kullanımlıktır; tekrar kullanım tüm oturumu düşürür. ^r004

## Kod stili

- Hata mesajları küçük harfle başlar, nokta ile bitmez. ^r005
- Paket adları tekil: `user`, `users` değil. ^r006
