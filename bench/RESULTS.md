# Results

Agent: Antigravity CLI with Gemini 3.8 Flash High. One run per cell. Setup, scenarios and hidden checks are described in [README.md](README.md). Raw logs are not committed; `evaluate.py` and `why_flagged.py` reproduce the tables from a runs directory.

Three conditions:
- **No memory:** the repository only.
- **Plain notes:** the same 149 notes as Markdown files in `memory/`, Obsidian-style, plus an `AGENTS.md` saying where things are and when to read and write them. The agent reads them with its own tools.
- **Paragma:** the same notes behind `pg`, plus the `AGENTS.md` section written by `pg install agents-md`.

## Summary

- **Memory decides correctness.** Without memory the agent fixed the recurring error the wrong way and missed a project rule it could not know. With the same knowledge, as plain notes or through Paragma, every task was fully right.
- **Paragma vs plain notes is an efficiency question, and the answer depends on the task.**
  - Recurring error: Paragma clearly ahead. `pg run` puts the recorded fix next to the failure.
  - Project rules: Paragma processed fewer tokens but took longer.
  - Continuation: plain notes were slightly cheaper.
- At this memory size (149 notes, about 17k tokens) reading plain files is still affordable. The quick tests below add a ten times larger memory: there plain notes cost twice as much, Paragma the same.

## Quick tests (2026-10-17, after phase 6)

The coding scenarios above take 10 to 20 minutes each, most of it writing code. The quick tests measure only the memory part: how fast the agent gets the facts it needs. Paragma here is the phase-6 version (spaces, complete context packs). Memories: the small one used above (149 notes, about 17k tokens) and a large one from `genmem_big.py` (1,539 notes, about 186k tokens: the same notes plus a busy year of background that mentions refund, payment, checksum and discount in other contexts).

**Agent questions** (`quick_agent.py`). Five questions answered from memory without changing code: the refund rules, the cause and fix of the catalog checksum failure, test conventions, time and time zones, migration rules and a past migration problem. One run per cell; Antigravity, Gemini 3.8 Flash High.

| Memory | Condition | Correct facts | Tool calls | Tokens processed | Time |
| --- | --- | --- | --- | --- | --- |
| small | plain notes | 17/17 | 42 | 1.14M | 281 s |
| small | Paragma | 17/17 | **19** | **0.40M** | **173 s** |
| large | plain notes | 17/17 | 60 | 2.17M | 591 s |
| large | Paragma | 17/17 | **12** | **0.32M** | **163 s** |

Both conditions always answered correctly. With Paragma the agent needed 2 to 5 times fewer tool calls and 3 to 7 times fewer tokens, and its cost did not grow with the memory: on the large memory most answers came from a single `pg context` call. With plain notes the cost grew with the memory, from 1.14M to 2.17M tokens.

**Retrieval without an agent** (`quick.py`). Ten questions with known facts. For Paragma, the agent's first call (`pg context <topic>` or `pg error find <error>`); for plain notes, `grep -rn` for the key word, then reading the matching files, most matches first, until every fact has been seen.

| Memory | Condition | Facts in the first call(s) | Calls | Tokens read |
| --- | --- | --- | --- | --- |
| small | plain notes | 24/24 | 15 | 7.8k |
| small | Paragma | 24/24 | 10 | 12.4k |
| large | plain notes | 24/24 | 75 | 84.7k |
| large | Paragma | 24/24 | 10 | 13.1k |

On the small memory Paragma reads more tokens, because the first context pack of a conversation also carries all project rules (about 900 tokens). It still needs fewer calls, and a call is what costs an agent: every turn re-reads the whole conversation. On the large memory one grep word can match hundreds of lines (the time-zone question needed 63 file reads); Paragma's answer size stays the same.

Limits: one run per cell, one model, questions written by the Paragma author. The plain-notes retrieval model is optimistic (it knows the right grep word) and simple (real agents also open files by name).

## Correctness

| Scenario | No memory | Plain notes | Paragma |
| --- | --- | --- | --- |
| S1 recurring error | 80%: tests green, wrong fix | 100% | 100% |
| S2 continuation (2 conversations) | invalid run (see below) | 100% | 100% |
| S3 project rules | 71%: no 30-day refund window | 100% | 100% |

## Cost

Paragma was run three times: at the end of phase 3, at the end of phase 5 (instructions add `pg related` and `pg context`) and after phase 6 (spaces and projects, complete context packs, `pg start`/`resume`). All runs are shown because the differences between them are a fair picture of run-to-run variance. All phase-6 runs passed every check.

| Scenario | Condition | Tool calls | Tokens processed | Uncached tokens | Wall time |
| --- | --- | --- | --- | --- | --- |
| S1 | no memory | 58 | 2.48M | 321k | 570 s |
| S1 | plain notes | 54 | 2.35M | 285k | 385 s |
| S1 | Paragma (phase 3) | **17** | **0.32M** | **161k** | **115 s** |
| S1 | Paragma (phase 5) | 36 | 1.09M | 230k | 279 s |
| S1 | Paragma (phase 6) | 29 | 0.64M | 211k | 228 s |
| S2 | plain notes | 73 (31 in 2nd conv.) | 2.72M | 472k | 846 s |
| S2 | Paragma (phase 3) | 79 (37) | 3.15M | 462k | 921 s |
| S2 | Paragma (phase 5) | 83 (39) | 2.90M | 523k | 815 s |
| S2 | Paragma (phase 6) | 78 (38) | 2.58M | 500k | 746 s |
| S3 | no memory | 37 | 1.31M | 267k | 395 s |
| S3 | plain notes | 64 | 3.31M | 305k | 545 s |
| S3 | Paragma (phase 3) | 46 | 1.97M | 280k | 495 s |
| S3 | Paragma (phase 5) | 60 | 2.70M | 349k | 879 s |
| S3 | Paragma (phase 6) | 65 | 2.45M | 371k | 483 s |

"Tokens processed" counts input, cached input, output and thinking over every model call, i.e. everything the agent read and wrote. The no-memory S3 run is cheap because it never looked for rules it did not know existed, and it is wrong.

## What happened

**S1: recurring error.** `go test` fails on a Windows checkout: the catalog file has CRLF line endings and its pinned checksum is computed over the LF file.
- No memory: tests made green by rewriting the test data with LF endings. The next Windows checkout breaks again.
- Plain notes: the agent found `memory/episodes/errors/catalog-checksum-crlf.md` by grepping and applied the right fix (15 memory file reads).
- Paragma: `pg run -- go test ./...` printed the recorded fix on the first failure. Fewest tool calls and tokens in both runs.

**S2: continuation.** The user gives four decisions and a plan in conversation 1 ("do only step a"). Conversation 2 has a fresh agent profile with no access to the first transcript: "continue and finish".
- Plain notes and Paragma both saved the decisions in conversation 1 and finished correctly in conversation 2. Paragma's session commands (start, plan, log, checkpoint, task done) cost more calls than writing one Markdown session note.
- The no-memory run is **invalid**. Twice its second conversation searched the machine for the lost context, found this benchmark's directory and ran the hidden tests against its own code. The leak detector caught both runs and they are excluded. That an agent without memory goes looking outside its workspace for a lost conversation is itself the failure mode both memory conditions avoid.

**S3: project rules.** Add `Refund(...)` and follow the project's rules.
- No memory: clean code, but no 30-day refund window and no `ErrRefundWindow`. That rule exists only in the project memory.
- Plain notes and Paragma: both implemented every rule. Paragma processed 18–40% fewer tokens than plain notes, but the phase-5 run took longer in wall time.

## Limits

- One run per cell and one model. The two Paragma runs differ by up to 2x in the same scenario, so differences smaller than that are not meaningful.
- The scenarios were written by the Paragma author and chosen because memory matters in them.
- The memory is 149 notes. Plain notes stay workable at this size; larger memories were not tested.
- Isolation: every conversation runs in a fresh agent profile, in a random temp directory, with a clean environment. A detector flags access outside the workspace; flagged runs were inspected, and only the no-memory S2 runs were excluded. The plain-notes S1 run read its own background task logs, which is allowed.
