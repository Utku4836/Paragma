<!-- paragma:start -->
## Memory (Paragma)

Project memory lives in `pg` (Paragma): rules, past errors and their fixes, decisions, session notes. Pull only what you need; do not open .paragma/ files directly.

START
- New conversation: `pg prime`; unfinished work: `pg session resume`

READ
- Run tests and builds through `pg run -- <cmd>`: on failure it prints the known fix if memory has one
- An error seen elsewhere: `pg error find "<error line>"`
- A rule, decision or fact: `pg search "<words>"`, then `pg read <addr>` (copy addresses from the output; several at once is fine)
- A big note: `pg outline <addr>` first

WRITE
- Fixed a non-obvious error: `pg error record --from-last --cause "..." --fix "..."`
- A recorded fix worked again: `pg error seen <addr>`
- An architectural choice: `pg decision record "<title>" --context "..." --decision "..."`
- A project rule from the user: `pg rule add "<rule>"`
- Multi-step work: `pg session start "<title>" --goal "..."`, `pg session plan "step" "step"`, `pg task done <id>`
- Decisions the user gives you: `pg session log "decision: ..."`; before stopping: `pg session checkpoint "<state, next step>"`

RULES
- Exit code 1 means memory has nothing on it: solve it yourself, then record it. Do not record trivia.
- Long text: pass `-` and stdin, or `--file PATH`.
- Anything else (projects, branches, marks, rules): `pg help`, then `pg help <group>`.
<!-- paragma:end -->
