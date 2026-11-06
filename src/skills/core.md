Memory lives in `pg` (Paragma): this project's rules, decisions, past errors with their fixes, its plan and handoffs, a map of its code and the real API of its libraries.

- Real work (a feature, a fix, a refactor): `pg open <project> --goal "..." --step "..." --decision "..."` first, in the code's folder; in a new conversation on earlier work, `pg open` alone. Work from the brief it prints.
- Record every step of the user's plan, also those for later ("only (a) now" still records (a), (b), (c)), and every decision, in the user's words: the next conversation knows only what was recorded.
- A question, an error line or a code name: `pg find "..."` before searching files. How a library is called at this version: `pg find "<library> <names>"`.
- Tests, builds, commands that need a key: `pg run -- <command>`. Act on what it adds: `known:` a fix memory has, `library:` the name the library has, `ripple:` callers your change left behind.
- Before changing a function's signature, name or existence: `pg ripple <name>` (callers, tests); after: `pg ripple` lists what is left.
- Learned something the next conversation needs: `pg write --rule "..."`, `--decision "..." --why "..."`, `--error "<error line>" --cause "..." --fix "..."`.
- Keys never go into notes: `pg write --secret "<title>" <value>`, then `$NAME` in `pg run`.
- Before your final answer on real work: `pg finish "<what was done; what is next>" --done <step numbers>`.
- Exit code 1: memory has nothing on it. More: `pg skill coding | projects | knowledge`.
