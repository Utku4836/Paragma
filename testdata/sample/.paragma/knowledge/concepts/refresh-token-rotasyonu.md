---
id: 01JA3Z8K2MCONCEPT0000000AA
type: concept
layer: semantic
title: Refresh token rotasyonu
tags: [auth, jwt]
verified: 2026-10-10
---
# Refresh token rotasyonu

Her refresh isteğinde yeni bir refresh token verilir, eskisi iptal edilir.
Çalınan bir token tekrar kullanılırsa token ailesinin tamamı geçersiz olur.

## Formül

Erişim token ömrü $t_a = 15\,dk$, refresh ömrü $t_r = 30\,gün$.

$$
P(\text{ele geçirme}) \approx 1 - e^{-\lambda t_a}
$$
