---
id: 01JA3Z8K2MERRJWT00000000AA
type: error
layer: episodic
status: resolved
title: JWT exp alanı yanlış saat dilimi
sig: "token is expired by 3h0m0s"
seen: 1
tags: [auth, jwt]
created: 2026-10-13
updated: 2026-10-13
---
# JWT exp alanı yanlış saat dilimi

## Belirti
Refresh endpoint testleri `token is expired by 3h0m0s` ile düşüyor; yalnızca CI dışında.

## Kök neden
`time.Now()` yerel saatle (Europe/Istanbul) karşılaştırılıyordu, `exp` UTC idi.

## Çözüm
Tüm karşılaştırmalarda `time.Now().UTC()`; test saati `clock.Fixed` ile sabitlendi.
