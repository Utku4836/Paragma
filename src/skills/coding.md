---
name: paragma-coding
description: Use when changing code with Paragma memory - finding where things are, the rules and past errors for a function, running tests, recording a fix.
---
# Coding with Paragma

## Find your way
- The brief of `pg open` already lists the rules and past errors for the task, the likely code, the test command and what changed since the last conversation. Start from it.
- The map: `pg tree` (folders and files with their functions; `[1 rule: r3; 1 error: e12]` marks code memory has notes on), `pg tree <folder>/`, `pg tree <file>` (every function with its lines).
- One function instead of a whole file: `pg read <file>#Function` (`Type.Method` for a method), or `pg read <file>:40-60`. It shows the notes about that function too.
- A code name or a file in `pg find` (`pg find Refund`, `pg find payments/refund.go`) finds the code itself first, with its notes.
- Shaders and queries kept in strings (`/* wgsl */` or `glsl`, `sql`, `gql` tags) are code too: `pg find groundAt` finds `sea.ts#SEA_SHADER.groundAt`. So are SQL, GraphQL and Protobuf files, Dockerfile stages and CI jobs.
- Documents have sections: `docs:` in the brief and `[doc]` in `pg find` name the one to read; `pg read README.md#Install` reads only it.

## Before and after a change
- What reaches a function before you change it: `pg ripple Refund` (or `pg ripple payments/refund.go#Refund`): who calls it, which tests reach it, the notes about it. `pg read <file>#Function` shows the callers and tests too.
- After changing a signature, renaming or removing something: `pg ripple` lists what changed since the last commit and the callers left as they were (`not updated:`), and the tests to run. A shader struct, GraphQL or Protobuf type or SQL table whose fields changed lists the other code that names it or its old fields (possible).
- `pg run`, the hooks after an edit and `pg finish` say it by themselves once (`ripple: ...`). Update those callers, or say in the handoff why not.

## Libraries
- Not sure how a library is called at the version this project locks? `pg find "<library> <names>"` (`pg find "tree_sitter Parser set_language"`): the real signature, its doc and example, the feature it needs and whether it is on, deprecation, its source file. From the source already on this computer (Rust, npm, Go, Python), no guessing from older versions.
- `pg read <path>` (`pg read tree_sitter::Parser::set_language`) shows it with its code.
- A build that fails on a name the library does not have: `pg run` adds `library: ... has no X; it has Y` with the right signature.
- The brief: `libraries:` the ones the goal names, `upgraded:` what changed in what this code calls after an upgrade.

## Test
- `pg run -- <test command>` instead of running it directly: failing tests marked new, failing already or flaky; each error line with the function it points at, the values an assertion compared (long ones cut to where they differ), the project's own stack frames, and the fix memory has (from this project or another). The same errors as your last run are said in one line. Fixed tests are named too. It exits with the command's exit code.
- `how it runs here:` in the brief: the test, build and check commands that passed in this project. Use them as they are.
- The brief names the tests to run first for the likely code (`go test ./payments/`).
- Any command that needs a key: `pg run -- curl -H "Authorization: Bearer $STRIPE_TEST" ...`. `$NAME` is put in on every system; the value never shows.

## Errors
- `pg run` shows the output made short (errors whole, warnings grouped, repeats and progress counted, a library's frames counted); a failure names the file with all of it and the lines of its errors: read only those.
- A failure memory knows: `pg run` prints `known: e12 ... fix: ...`. Read it: `pg read e12`.
- Fixed something that was not obvious: `pg write --error "<the error line as printed>" --cause "..." --fix "..."`. Without `--files`, the file and function in the error line are linked. The same error again is counted, not duplicated. When a test that failed passes, `pg run` prints this command with the change that fixed it filled in (the functions changed after the failing run): say what was wrong and how, then run it.
- `may be outdated since it was written: ...` on a note: the code it names is gone, or its signature changed. Check it before relying on it; still right: `pg write --confirm <id>`, else change or remove it. `pg doctor` lists every such note.

## Before you finish
- `pg finish "<what was done; what is next>"` lists the functions you changed and the rules and notes about them (and about the code that calls them). Check your change against those rules.

Projects and handoffs: `pg skill projects`. Rules and decisions: `pg skill knowledge`.
