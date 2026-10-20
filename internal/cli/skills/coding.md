---
name: paragma-coding
description: Use when changing code with Paragma memory - finding where things are in the code, the rules and past errors for a file, running tests, fixing and recording errors.
---
# Coding with Paragma

## Find your way
- `pg start` / `pg resume` already printed a brief for the task: the rules, decisions and past fixes, the files likely involved and the test command. Work from it before searching.
- Where things are: `pg tree [dir]` lists the files with their declarations, and `[2 rules, 1 error]` where memory names a file.
- One declaration instead of a whole file: `pg outline <file>`, then `pg read <file>#Name` (`Type.Method` for a method) or `pg read <file>:10-40`.
- Before editing a file: `pg related <file>`: its rules and decisions first, then each past error with its fix, then its last commits.

## Test
- `pg run -- <test command>`: short output. On failure: the known fix, and which failing tests are new since the last run, failed before, or are flaky. On success: the rules for the files you changed.
- `--affected` (Go) tests only the packages you changed; `--full` prints all output.

## Errors
- A failure memory knows: `pg run` prints the fix. Any error line: `pg error find "<line>"`.
- Fixed something that was not obvious: `pg error record --from-last --cause "..." --fix "..."` (the files you changed are filled in).
- A recorded fix worked again: `pg error seen <addr>`. It did not: `pg error reopen <addr> --note "..."`.

## Before you finish
- `pg related` without a path: every changed file with the rules and decisions that apply to it.
- `pg finish "<what changed, what is next>" --done <ids>`.

All flags and examples: `pg skill coding --ref`.
