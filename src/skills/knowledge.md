---
name: paragma-knowledge
description: Use to look up or record project knowledge with Paragma - rules, decisions, errors and their fixes, notes, secrets.
---
# Knowledge in Paragma

## Look it up
- `pg find "<topic, question, error line or code name>"`: rules, decisions, errors, notes, code and secrets in one call, in any language. One search with the user's words beats several.
- `pg read <id>` (`e12`, `r3`, `d7`, `n40`; `shop:e12` in another project) or a path. What this conversation was shown in full lately comes back as one line: it is already in your context; `pg read` shows it again.
- `connected:` after a record (and after the best result of a search): what it replaced, notes on the same code, links, what was written in the same work. Read those before changing what it governs.
- `pg find "..." --in <project>` or `--all` for other projects.
- `pg doctor` lists the records that may be outdated (their code is gone or its signature changed: `pg write --confirm <id>` when still right) and what could be tidier (notes about the same thing, a note on unrelated topics, a folder with one note), each with the command that does it.

## Record it
- A rule from the user: `pg write --rule "<one sentence>" --group <Group>`. Tie it to code: end it with `(applies: payments/, pricing/vat.go)`.
- `(draft)` rules in rules.md are pg's, from a fix that kept coming back: not binding until `pg write --confirm <id>`.
- A decision: `pg write --decision "<what>" --why "<why>" --alternatives "<what else>"`. One that replaces another: `--supersedes d7`.
- A fixed error: `pg write --error "<error line as printed>" --cause "..." --fix "..."` (`--files a.go,b.go` when the line names none).
- The same rule, decision or error written again updates the one memory has. A record that seems to contradict another is pointed out: replace it with `--supersedes` (decisions) or `pg rm r3` (rules).
- An error seen often in many files is proposed as a rule: write it with `pg write --rule`.
- Anything else worth keeping: a note per topic, `pg write <category>/<topic> "..."` (a category folder holds many notes). A note that grew several topics: split it.
- A file the work will need again (a workbook, a contract, a deck, a PDF, an export): `pg write <category>/<topic> --file <path>` keeps it beside that note, with a card in its Attachments section (where it came from, when). The same file again is a new version, the old one kept. Not code, and not a file that holds a key (it is refused). Its parts: `pg read <category>/<file>.xlsx#Sheet1!A1:D20`, `...docx#<heading>`, `...pptx#3`; `pg find` searches inside it.
- In a note: a due date, `pg write <note> --due 2026-12-01` (or `+7d`; `none` takes it off; `pg ls --due` lists them); a table, `pg write <note> --table add "Jan | 1200"`, `--table sort <column>` (`-<column>`: highest first), `--table sum <column>` (a total row); to-dos as `- [ ] ...` lines, listed by `pg ls --todo`, done: `pg write --check n4.2`.
- Notes about code can say so in their properties: `files: [payments/refund.go]`, `symbols: [Refund]`.

## Secrets
- Never write keys, tokens or passwords in notes: they are masked and not kept.
- Keep one: `pg write --secret "<title>" <value>` (`-` reads the value from stdin). Find it: `pg find "<what it is for>"` gives `$NAME`. Use it: `pg run -- <command with $NAME>`. Its value never shows, and you do not need it.
- Do not read the project's .env file to get a value: use `$NAME` in `pg run`.

## Good to know
- Exit code 1: memory has nothing on it. Solve it, and record it if it was not obvious.
- Long text: `-` with stdin, or `--file PATH`.
