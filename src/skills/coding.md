---
name: paragma-coding
description: Use when changing code with Paragma memory - finding where things are, the rules and past errors for a function, running tests, recording a fix.
---
# Coding with Paragma

## Find your way
- The brief of `pg open` already lists the rules and past errors for the task, the likely code, the test command and what changed since the last conversation. Start from it.
- The map: `pg tree` (folders and files with their functions; `[1 rule: r3; 1 error: e12]` marks code memory has notes on), `pg tree <folder>/`, `pg tree <file>` (every function with its lines).
- One function instead of a whole file: `pg read <file>#Function` (`Type.Method` for a method), or `pg read <file>:40-60`. It shows the notes about that function too.
- A code name or a file in `pg find` (`pg find Refund`, `pg find payments/refund.go`) finds the code itself first, with its notes.

## Test
- `pg run -- <test command>` instead of running it directly: failing tests marked new, failing already or flaky; each error line with the function it points at and the fix memory has (from this project or another). Fixed tests are named too. It exits with the command's exit code.
- The brief names the tests to run first for the likely code (`go test ./payments/`).
- Any command that needs a key: `pg run -- curl -H "Authorization: Bearer $STRIPE_TEST" ...`. `$NAME` is put in on every system; the value never shows.

## Errors
- A failure memory knows: `pg run` prints `known: e12 ... fix: ...`. Read it: `pg read e12`.
- Fixed something that was not obvious: `pg write --error "<the error line as printed>" --cause "..." --fix "..."`. Without `--files`, the file and function in the error line are linked. The same error again is counted, not duplicated.
- `(code changed since it was written: ...)` on a note: the code it describes changed after it was written. Check before relying on it.

## Before you finish
- `pg finish "<what was done; what is next>"` lists the functions you changed and the rules and notes about them (and about the code that calls them). Check your change against those rules.

Projects and handoffs: `pg skill projects`. Rules and decisions: `pg skill knowledge`.
