# Project memory

This project keeps its memory as Markdown files in `memory/`: rules, past errors and their fixes, decisions (ADRs), a journal and session notes.

- `memory/knowledge/` rules.md, architecture.md, stack.md, concepts/
- `memory/episodes/` errors/ (symptom, root cause, fix), decisions/, journal/
- `memory/work/` sessions/ for ongoing work, inbox.md

When to read
- At the start of a conversation, check `memory/work/sessions/` for unfinished work.
- When you need a rule, a decision or a known fix, look it up in `memory/`.
- When you hit an error, check `memory/episodes/errors/` before debugging.

When to write
- Multi-step work: keep a session note in `memory/work/sessions/<date>-<topic>.md` with Goal, Plan (checkboxes), Notes (decisions the user gives you) and Handoff.
- Before you stop: update the Handoff with where things stand and what is next.
- Fixed a non-obvious error: add a note to `memory/episodes/errors/`.
