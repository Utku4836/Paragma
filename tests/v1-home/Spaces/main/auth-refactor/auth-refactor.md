---
id: 01JA4AUTHREFACTOR00000000AA
type: project
layer: working
status: active
title: Auth refactor
tags: [auth]
created: 2026-10-12
---
# Auth refactor

## Hedef
Cookie tabanlı oturumdan JWT + refresh token'a geç. Bkz [[refresh-token-rotasyonu]].

## Plan
- [x] Mevcut akışı çıkar ^t001
- [/] Refresh endpoint ^t002 !p1 @due(2026-10-20)
    - [x] Handler ^t003
    - [ ] Testler ^t004
- [!] Prod migration ^t005 @blocked(db erişimi)
- [ ] Eski cookie desteğini kaldır ^t006 #cleanup
- [?] Redis mi Postgres mi? ^t007

## Notlar
- 14:02 `middleware/auth.go` iki yerden çağrılıyor, bkz [[architecture#Middleware]]
- 15:10 timezone sorunu → [[jwt-exp-timezone]]

## Devir
Son checkpoint 2026-10-13 15:40. Refresh endpoint testleri kırmızı;
sebep saat dilimi, [[jwt-exp-timezone]] kaydına bak. Sıradaki: ^t004.
