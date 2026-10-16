<!-- paragma:start -->
## Memory (Paragma)

Memory lives in `pg` (Paragma): project notes, rules, decisions, past errors and their fixes. Pull only what you need, through pg; do not open its files.

START
- New conversation: `pg prime` (what memory holds, its projects). Continue earlier work: `pg resume <project>`.
- New multi-step work: `pg start <project> --goal "..." --step "..." --step "..." --decision "<what the user decided>"`, once; the project stays current.

READ
- Tests and builds through `pg run -- <cmd>`: on failure it prints the known fix; on success, rules for the files you changed.
- A topic or task: `pg context "<topic>"`: every matching rule, decision and fix in one call, and a last line saying what else exists.
- Before changing a file: `pg related <path>`. An error seen elsewhere: `pg error find "<error line>"`.
- A specific fact: `pg search "<words>"`, then `pg read <addr>`.

WRITE (goes to the current project)
- Step finished: `pg done <id>` (prints the next step). More steps: `pg plan "step" "step"`. Finding (also where things are in the code): `pg log "..."`. Decision from the user: `pg decide "..."`.
- Your own notes: `pg new note "<title>" --dir <folder>`; organise the project folder as you like.
- Fixed a non-obvious error: `pg error record --from-last --cause "..." --fix "..."`. A recorded fix worked again: `pg error seen <addr>`.
- Architectural choice: `pg decision record "<title>" --decision "..."`. Rule from the user: `pg rule add "<rule>"`.

RULES
- Exit code 1 means memory has nothing on it: solve it yourself, then record it. Do not record trivia.
- Long text: `-` and stdin, or `--file PATH`. Anything else: `pg help`.
<!-- paragma:end -->
