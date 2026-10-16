<!-- paragma:start -->
## Memory (Paragma)

Project memory lives in `pg` (Paragma): rules, past errors and their fixes, decisions, session notes. Pull only what you need; do not open .paragma/ files directly.

START
- New conversation: `pg prime`; unfinished work: `pg session resume`

READ
- Run tests and builds through `pg run -- <cmd>`: on failure it prints the known fix if memory has one
- Before changing a file: `pg related <path>` (its rules, decisions, past errors)
- A topic or task: `pg context "<topic>"` (one call: matching rules, decisions, fixes)
- An error seen elsewhere: `pg error find "<error line>"`
- A specific fact: `pg search "<words>"`, then `pg read <addr>`

WRITE
- Fixed a non-obvious error: `pg error record --from-last --cause "..." --fix "..." --keywords "<other words for it>"`
- A search missed a note that exists: `pg keywords <addr> "<the words you searched>"`
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
