---
id: 01JA3Z8K2MDEC0007000000AA
type: decision
layer: episodic
status: active
title: "0007: WAL modu"
created: 2026-09-02
supersedes: [[0003-journal-mode]]
---
# 0007: WAL modu

## Bağlam
Paralel testlerde kilitlenme: [[sqlite-database-locked]].

## Karar
SQLite her açılışta WAL moduna alınır, `busy_timeout=5000`.

## Alternatifler
- Postgres'e geçiş: test altyapısı için ağır.
- Testleri seri çalıştırmak: CI süresi 3 katına çıkıyor.

## Sonuçlar
`-wal` ve `-shm` dosyaları `.gitignore`'a eklendi.
