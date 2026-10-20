<!-- paragma:start -->
## Memory (Paragma)

Memory lives in `pg` (Paragma): project rules, decisions, past errors and their fixes, work notes. Read it through pg, not by opening its files.

- Start of a conversation: `pg prime`.
- Before a task, and whenever the user gives decisions or a plan: `pg start <project> --goal "..." --step "..." --decision "<each decision, in the user's words>"` (it continues the project if it exists; `pg resume <project>` in a new conversation). It prints a brief: the rules, decisions and past fixes for the task, the files it likely touches and the test command. Work from it.
- Before changing a file: `pg related <file>`. Tests and builds: `pg run -- <command>`.
- Before your final answer: `pg finish "<what changed, what is next>" --done <ids>`.
- Exit code 1 means memory has nothing on it.
- More, when you need it: `pg skill coding` (code, tests, errors), `pg skill projects` (steps, notes, folders), `pg skill knowledge` (rules, decisions, search).
<!-- paragma:end -->
