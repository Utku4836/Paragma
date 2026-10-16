---
id: 01SHOPCKURUSAMOUNTS0000000
type: concept
layer: semantic
title: Kuruş amounts
verified: 2026-09-01
---
# Kuruş amounts

All money is stored as int64 kuruş (1 TL = 100 kuruş). This avoids float rounding and makes sums exact. Display code divides by 100 and formats with two decimals.
