<!-- paragma:start -->
## Memory (Paragma)

Memory lives in `pg` (Paragma): project notes, rules, decisions, past errors and their fixes. Pull only what you need, through pg; do not open its files.

START
- New conversation: `pg prime`. Continuing earlier work: `pg resume <project>`.
- The user gives decisions, requirements or a plan, or the work may not end in this conversation: BEFORE writing code, `pg start <project> --goal "..." --step "..." --decision "<each decision, in the user's words>"`. What is not recorded is lost to the next conversation.

FIND
- Where things are in the code, and the rules and past errors about each file: `pg tree [dir]`, instead of listing and opening files.
- A topic or task: `pg context "<topic>"` (every matching rule, decision and fix in one call). Before changing a file: `pg related <path>`. An error: `pg error find "<error line>"`. A fact: `pg search "<words>"`, then `pg read <addr>`.
- Tests and builds through `pg run -- <cmd>`: the known fix on failure, the rules for the files you changed on success.

RECORD (in the current project)
- Step done: `pg done <id>`. New step: `pg plan "..."`. Finding: `pg log "..."`. New decision from the user: `pg decide "..."`. Rule from the user: `pg rule add "..."`.
- Fixed a non-obvious error: `pg error record --from-last --cause "..." --fix "..."`.
- Your own notes: `pg new note "<title>" --dir <folder>`; see the folder with `pg tree --notes`.
- BEFORE your final answer, when anything is left or another conversation may continue: `pg checkpoint "<where it stands, what is next>"`.

RULES
- Exit code 1 means memory has nothing on it: solve it yourself; record it if it was not obvious.
- Long text: `-` and stdin, or `--file PATH`. Anything else: `pg help`.
<!-- paragma:end -->
