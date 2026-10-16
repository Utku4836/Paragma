| scenario | condition | runs | score (mean) | full pass | tool calls | last conversation tool calls | tokens processed | uncached tokens | wall s | pg calls | memory file reads | outside-workspace calls |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| s1-recurring-error | none | 1 | 80% | 0/1 | 92.0 | 92.0 | 4,940,978 | 399,878 | 801 | 0.0 | 0.0 | 0 |
| s1-recurring-error | files | 1 | 100% | 1/1 | 49.0 | 49.0 | 1,952,221 | 238,846 | 480 | 0.0 | 23.0 | 0 |
| s1-recurring-error | paragma | 1 | 100% | 1/1 | 29.0 | 29.0 | 785,360 | 185,617 | 220 | 15.0 | 0.0 | 0 |

Per run:

| run | status | hidden tests | other checks | score | tool calls | tokens |
|---|---|---|---|---|---|---|
| s1-recurring-error/none-1 | SUCCESS | 3/4 | own tests pass: yes | 80% | 92 | 4,940,978 |
| s1-recurring-error/files-1 | SUCCESS | 4/4 | own tests pass: yes | 100% | 49 | 1,952,221 |
| s1-recurring-error/paragma-1 | SUCCESS | 4/4 | own tests pass: yes | 100% | 29 | 785,360 |
