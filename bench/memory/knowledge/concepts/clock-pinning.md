---
id: 01SHOPCCLOCKPINNING0000000
type: concept
layer: semantic
title: Clock pinning
verified: 2026-09-01
---
# Clock pinning

Tests set clock.Now = func() time.Time { return fixed } and restore the previous function with t.Cleanup.
