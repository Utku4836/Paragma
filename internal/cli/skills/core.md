Memory lives in `pg` (Paragma): project rules, decisions, past errors and their fixes, work notes.

- Given a task: `pg start <project> --goal "..." --step "..." --decision "..."` first (continues the project if it exists); on earlier work in a new conversation: `pg resume <project>`. No task yet: `pg prime`.
- Record one --decision per decision and one --step per step of the user's plan, in the user's words with the names they gave; "only (a) now" still records (a), (b), (c): the next conversation knows only what was recorded.
- Work from the brief it prints: rules, decisions and past fixes for the task and its likely files, the test command. Another file: `pg related <file>` before changing it.
- Tests and builds: `pg run -- <command>`. Before your final answer: `pg finish "<what changed, what is next>" --done <ids>`.
- Chain commands in one shell call when you can: `pg start ... ; pg run -- go test ./...`.
- Exit code 1 means memory has nothing on it. More: `pg skill coding`, `pg skill projects`, `pg skill knowledge`.
