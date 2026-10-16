# Results: Paragma vs no memory

Run on 2026-10-16 with Paragma at the end of phase 3 (MVP). Agent: Antigravity CLI with Gemini 3.8 Flash High. One run per cell. Setup, scenarios and hidden checks are described in [README.md](README.md).

**Summary.** With Paragma the agent got all three tasks fully right. Without memory it fixed the recurring error the wrong way and broke a project rule it could not know. On the recurring error, Paragma also cut the work sharply: 71% fewer tool calls, 87% fewer tokens processed and 80% less wall time.

## Scores

| Scenario | No memory | Paragma |
| --- | --- | --- |
| S1 recurring error | 80%: tests green, but the fix is wrong | **100%** |
| S2 continuation (2 conversations) | invalid run (see below) | **100%** |
| S3 project rules | 71%: no 30-day refund window | **100%** |

## Cost

| Scenario | Condition | Tool calls | Tokens processed | Uncached tokens | Wall time |
| --- | --- | --- | --- | --- | --- |
| S1 | no memory | 58 | 2,483,133 | 321,194 | 570 s |
| S1 | Paragma | **17** | **316,438** | **161,241** | **115 s** |
| S2, second conversation | Paragma | 37 | — | — | — |
| S3 | no memory | 37 | 1,312,232 | 267,403 | 395 s |
| S3 | Paragma | 46 | 1,972,619 | 279,925 | 495 s |

"Tokens processed" counts input, cached input, output and thinking over every model call, i.e. everything the agent had to read and write. In S2 the Paragma run used 79 tool calls over both conversations; its second conversation needed 37 to finish the work.

## What happened

**S1: recurring error.** `go test` fails on a Windows checkout because the catalog file has CRLF line endings and its pinned checksum is computed over the LF file.
- Without memory, the agent debugged for 58 tool calls. It made the tests pass by rewriting `testdata/catalog.json` with LF endings. The next Windows checkout breaks again, and the hidden test that loads a CRLF file fails.
- With Paragma, `pg run -- go test ./...` printed the recorded fix on the first failure: normalise line endings in `catalog.Load` and keep the pinned checksum. The agent applied it in 17 tool calls; all hidden checks pass.

**S2: continuation.** The user gave four decisions and a plan in conversation 1 ("do only step a"). Conversation 2 was a new agent profile with no access to the first transcript: "continue and finish".
- With Paragma, the first conversation saved a session (plan, decisions, handoff). The second started with `pg session resume` and finished steps b–d. All checks pass, including "discount before VAT" and "total never below zero", which exist only in conversation 1.
- The no-memory run is **invalid**. Twice, its second conversation searched the machine for the lost context, found this benchmark's directory, and ran the hidden tests against its own code. The leak detector (`why_flagged.py`) caught both runs and the result is excluded. An agent without memory searching outside its workspace for a lost conversation is itself the failure mode Paragma addresses.

**S3: project rules.** Add `Refund(...)` and follow the project's rules.
- Without memory, the code is clean and its own tests pass. It has no 30-day refund window and no `ErrRefundWindow`, because that rule exists only in the project memory (ADR 0014). Two of six hidden checks fail.
- With Paragma, the agent searched the rules and the decision, then implemented the window, the sentinel error, over-refund protection, auditing and `clock.Now()`. It spent about 25% more tool calls than the no-memory run, mostly reading memory; uncached tokens were about the same.

## Limits

- One run per cell, one model. Agent runs vary a lot between repetitions; treat the cost numbers as indicative.
- The scenarios were written by the Paragma author and chosen because memory matters in them. The hidden checks only test what the user asked for or what the project's memory states.
- The stronger baseline, the same memory as plain Markdown files (`files` condition), is in the harness but not in this run. In an earlier single S1 run (`PRELIMINARY.md`), plain files were also correct but needed 49 tool calls and 1.95M tokens, against 29 and 0.79M for Paragma.
- Isolation: every conversation runs in a fresh agent profile, in a random temp directory, with a clean environment, and a detector counts any access outside the workspace. In S2 that was still not enough to stop a determined agent; see above.
