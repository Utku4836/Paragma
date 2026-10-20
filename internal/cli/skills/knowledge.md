---
name: paragma-knowledge
description: Use to look up or record project knowledge with Paragma - project rules, past decisions, how things are done here, facts from earlier work.
---
# Knowledge in Paragma

## Look it up
- A topic: `pg context "<topic>"`: every matching rule, decision and fix in one call, ending with what else exists. Prefer it to several searches.
- A fact: `pg search "<words>"`, then `pg read <addr> [<addr>...]`. Addresses: `note`, `note#Heading`, `note^id`, `note:10-20`.
- By metadata: `pg find --type decision --tag auth --since 7d`. Exact strings: `pg grep "<regex>"`. All rules: `pg rule list`.
- Links between notes: `pg links`, `pg backlinks`, `pg neighbors <note>`.

## Record it
- A rule from the user: `pg rule add "<rule>" --section <group>`.
- An architectural decision with its context: `pg decision record "<title>" --decision "..." --context "..."`; one that replaces another: `--supersedes <old>`.
- A fix that keeps working becomes a rule: `pg promote <addr> --text "<one sentence>"`.
- Words a later search will use (synonyms, the other language): `pg keywords <addr> "a, b"`.

## Good to know
- Exit code 1 means memory has nothing on it: solve it, and record it if it was not obvious.
- Long text: `-` and stdin, or `--file PATH` (PowerShell 5.1 pipes drop non-ASCII).
- Something already shown in this conversation comes back as one line; `--again` prints it in full.

All flags and examples: `pg skill knowledge --ref`.
