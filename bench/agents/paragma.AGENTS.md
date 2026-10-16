# Project memory (Paragma)

This project keeps its memory with the `pg` CLI: rules, past errors and their fixes, decisions (ADRs), a journal and session notes. Pull only what you need; do not open `.paragma/` files directly.

When to read
- At the start of a conversation: `pg session resume` (unfinished work) and `pg outline` (map of notes).
- When you need a rule, a decision or a known fix: `pg search "<words>"`, then `pg read <addr>` with addresses copied from the output (several at once is fine).
- When you hit an error: `pg search "<error text>"` before debugging.

When to write
- Multi-step work: `pg session start "<title>" --goal "..."`, then `pg session plan "step" "step" ...`; mark progress with `pg task done <id>`.
- Decisions the user gives you: `pg session log "decision: ..."`.
- Before you stop: `pg session checkpoint "<where things stand, what is next>"`.
- Fixed a non-obvious error: `pg new error "<title>"`, then `pg write <addr>#Fix "..."`.

More: `pg help`.
