Memory lives in `pg` (Paragma): this project's rules, decisions, past errors with their fixes, its plan and handoffs between conversations, and a map of its code.

- Real work (a feature, a fix, a refactor): `pg open <project> --goal "..." --step "..."` first, in the code's folder. It continues the project if it exists; in a new conversation on earlier work run `pg open` alone. Give each step of the user's plan in their words. Work from the brief it prints.
- A question about the project: `pg find "<topic, error line or code name>"` before searching files.
- Tests, builds, and commands that need a key: `pg run -- <command>`. It says what is new, already failing or flaky, and gives the fix memory knows.
- Learned something the next conversation needs: `pg write --rule "..."`, `pg write --decision "..." --why "..."`, `pg write --error "<error line>" --cause "..." --fix "..."`.
- Keys and passwords never go into notes: `pg write --secret "<title>" <value>`, then `$NAME` in `pg run`.
- Before your final answer on real work: `pg finish "<what was done; what is next>" --done <numbers of the steps you finished>`.
- Exit code 1 means memory has nothing on it. More: `pg skill coding`, `pg skill projects`, `pg skill knowledge`.
