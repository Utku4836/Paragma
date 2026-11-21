<p align="center">
  <img src=".github/assets/banner.png" width="100%" alt="Paragma">
</p>

<p align="center">
  <b>Project memory for AI coding agents.</b><br>
  Rules, decisions, past errors with their fixes, and the handoff between conversations: kept as Markdown on your computer, found in one call.
</p>

<p align="center">
  <a href="#install">Install</a> &nbsp;·&nbsp;
  <a href="#what-the-agent-sees">What the agent sees</a> &nbsp;·&nbsp;
  <a href="#results">Results</a> &nbsp;·&nbsp;
  <a href="#for-you-pg">For you</a> &nbsp;·&nbsp;
  <a href="#commands">Commands</a> &nbsp;·&nbsp;
  <a href="#how-it-works">How it works</a>
</p>

---

## Install

```bash
npm install -g paragma
pg setup
```

That is all. `npm` fetches the build of `pg` for your computer (Windows, macOS or Linux, on x64 or arm64) and checks it against the release's SHA-256 sums. It needs Node 18 or later. `pg setup` then opens a small screen where you pick your coding agents and where Paragma should be read: in every conversation, or only in one project's folder.

A newer npm may warn that `paragma` has install scripts not covered by `allowScripts`. That is fine: the first `pg` call fetches the binary instead.

<details>
<summary>Without npm</summary>

Download `pg` for your platform from the [releases](https://github.com/Utku4836/Paragma/releases), put it on your `PATH` and run `pg setup`. Each release lists the SHA-256 of every file in `SHA256SUMS`.

</details>

Your memory lives in `~/.paragma`, outside your repositories. Paragma works offline; the search model is built into `pg`. Only `pg git`, once you connect it, talks to GitHub.

## Why

A coding agent starts every conversation from zero. It rediscovers the project's rules, tries the obvious fix for an error that was already solved the other way, and forgets the decisions you gave it yesterday. Notes help, but an agent reading files searches, opens and re-opens until it trusts it has seen enough, and each step re-reads the whole conversation.

Paragma gives the agent what it needs in one step:

- **The work continues.** `pg open` returns the goal, the open steps, the last conversation's handoff, and the rules, decisions and past errors that match the work.
- **Known errors come back with their fix.** `pg run -- go test ./...` reads the test output, says which failures are new, still failing or flaky, and prints the recorded cause and fix next to a failure seen before, in this project or another. The numbers of tests are the test tool's own when it says them (node, cargo, pytest, jest, vitest, mocha, dotnet); `pg run --json` gives them to a script.
- **Memory knows the code.** Notes are linked to files and functions, follow them when they are renamed, and say when the code they describe has changed since.

## What the agent sees

A new conversation on earlier work runs `pg open` and starts from this:

```text
$ pg open
ok shop (continued)
goal: Refunds through the new payment provider
steps: 1 open, 1 done
- [ ] 2. Move refunds to it
last handoff (2026-11-01 17:43Z · shop):
Provider client added and tested. Refunds still use the old provider.
matching the goal and steps:
r1 rule General: Money is int64 cents, never float.
d1 decision: Refunds over 500 EUR need a second approval
e1 error: payments/refund_test.go:41: refund window: got 29 days, want 30
stack: go
tests: pg run -- go test ./...
likely code: payments/refund.go#Refund
its tests first: pg run -- go test ./payments/
```

When the refund test fails again, the agent does not have to find the cause a second time:

```text
$ pg find "refund window off by one day"
[e1 error] errors/payments-refund-test-go-refund-window
## Cause
The window was counted from the order, not from delivery
## Fix
Count from the delivery date
```

## Results

We measured whether agents do the same work with less effort when the same knowledge comes through Paragma instead of plain Markdown files. Every run starts from a fresh copy of a small Go project and a memory of 1,539 notes (rules, decisions, recorded errors, journals). Both conditions hold identical knowledge and equivalent instructions; only the access differs. Hidden tests, which the agent never sees, check the result.

| Agent and model | Tasks | Score, files → Paragma | Tokens processed | Tool calls | Time |
| --- | --- | --- | --- | --- | --- |
| Antigravity, Gemini 3.8 Flash | 5 questions | 5/5 → 5/5 | 1.95M → **0.32M** | 76 → **22** | 8.6 → **3.2** min |
| Antigravity, Gemini 3.8 Flash | 3 coding tasks | 3/3 → 3/3 | 6.92M → **3.60M** | 173 → **129** | 46 → **18** min |
| OpenCode, GLM-5.3 Flash | 2 coding tasks | 2/2 → 2/2 | 1.12M → **0.54M** | 87 → **58** | 15 → **7** min |

In every pair of runs, the same task with the same model, Paragma used fewer tokens. Over all runs plain files needed **2.2 times the tokens**, 1.6 times the tool calls and 2.5 times the time, with the same scores.

The coding tasks are a recurring error whose obvious fix is wrong, a feature finished in a second conversation that only knows what the first one recorded, and a change governed by project rules. Each cell ran once, so small differences are not established. Setup, scenarios and every number: [bench/RESULTS.md](bench/RESULTS.md).

## Connect your agent

```bash
pg setup
```

```text
  ██████╗  █████╗ ██████╗  █████╗  ██████╗ ███╗   ███╗ █████╗
  ...
  memory for coding agents · v0.2.0

  Connect Paragma to an agent

  › Claude Code   CLI · desktop     ✓ every conversation
    Codex         CLI · desktop
    Antigravity   CLI · IDE
    OpenCode      CLI

  ↑↓ move   enter choose   q quit
```

`pg setup` finds the agents on your computer, including their desktop apps, and adds a short "Memory (Paragma)" section where each one reads its instructions. Choose **every conversation**, **one conversation** from the agent's own history (Paragma then goes into that conversation's folder), or **this folder**. Esc goes back a step. The rest of the file is left as it is. After every update, `pg` brings what it set up to the new version by itself, before its first command: its sections (only its own lines; what you wrote, also inside the section, stays), its hooks and its MCP server entries. `pg setup --update` does it at once and says what changed.

| Agent | Every conversation | One folder |
| --- | --- | --- |
| Claude Code | `~/.claude/CLAUDE.md` | `CLAUDE.md` |
| Codex | `~/.codex/AGENTS.md` | `AGENTS.md` |
| Antigravity | `~/.gemini/AGENTS.md` | `AGENTS.md` |
| OpenCode | `~/.config/opencode/AGENTS.md` | `AGENTS.md` |

In scripts: `pg setup codex --global`, `pg setup claude --dir .`, `pg setup all --global`, `pg setup --list`, `pg setup --update`.

**MCP.** Apps that start MCP servers can run Paragma as one: the app gets the project's brief when a conversation starts and pg's commands as tools, which helps most in apps without a shell or hooks. In `pg setup` choose **MCP** and an app (Claude Code, Claude Desktop, Codex, Antigravity, Cursor; for every conversation), or `pg setup all --mcp`; `--no-mcp` takes it out. Where Paragma's hooks already give the brief, the server does not repeat it. Every tool takes a `project` and a `budget`; `pg_batch` runs several calls in one, and tool calls the app sends together run side by side.

## For you: `pg`

`pg` alone, in a terminal, opens the same memory for you, by keyboard and mouse. Every change you make there is a pg command underneath: the agents see it as they see each other's work, and what pg does by itself still undoes with one key.

- **Home**: the goal, the next steps, the notes due soon, the notes you opened last, the agents at work and the last handoff.
- **Notes**: the folders and notes as a tree, the chosen one shown formatted beside it, with the notes that link to it. Drag a note onto a folder to move it; drop a file on the window to keep it with a note.
- **A note editor with the usual keys**: copy, cut and paste with the system clipboard, undo, selection with Shift or the mouse, find, line numbers. Today's date (Ctrl+D), tables (Ctrl+T; Tab goes to the next cell and lines the columns up), to-dos (Ctrl+Enter), `[[` offers the notes to link to. Several notes open in tabs. When an agent changed the note meanwhile, saving asks: merge both, take theirs, or keep yours.
- **Ctrl+K**: every command and note by a part of its name. **/** searches the memory as you type.
- **Plan**, **Files** kept with notes, **Agents** (who is at work, the steps they took; tell them all something), **Activity** (what the agents and pg did).
- **Sync**: sign in to GitHub, find your memory or choose a repository, the `.env` and file settings, **Transfer** to another repository. Notes not sent yet are marked; a note changed on two computers is merged in the editor or one side chosen.
- **Setup**: which agents have Paragma's instructions, hooks and MCP server, each turned on or off, and **Update everything** to this version.

What agents write shows on the screen as it happens.

## Commands

The section `pg setup` writes is short: when to call `pg`. The details are in three skills the agent reads when it needs them (`pg skill coding`, `pg skill projects`, `pg skill knowledge`, also kept as files in `~/.paragma/skills/`), and `pg <command> --help` explains every option.

```bash
pg open shop --goal "Refunds through the new provider" --step "Add the client" --step "Move refunds" \
             --decision "Refunds over 500 EUR need a second approval"
                                     # start or continue the work; prints the brief
pg find "refund window"              # a topic, a question, an error line, a file or a function
pg read payments/refund.go#Refund    # one function with the notes about it; also a note, or file:10-20
pg run -- go test ./...              # what fails, what is new, flaky or fixed, and the known fix
pg write --rule "Money is int64 cents, never float."
pg write --decision "..." --why "..." --alternatives "..."
pg write --error "<error line>" --cause "..." --fix "..."
pg write payments/provider --file notes.md      # a note; long text from a file or stdin (-)
pg write money/budget --file budget.xlsx --attach   # a file kept with the note; pg read budget.xlsx#Sheet1!A1:D9
pg write money/budget --due 2026-12-01          # also --table add|sort|sum, --check n4.2 for a to-do
pg finish --done 1 "what changed, what is next"  # tick off steps, leave the handoff
pg steps                             # the steps with their numbers (--take 4, --defer 4, --drop 4, --edit 4 "text")
pg tell "I am changing refund.go"    # a note the other conversations on the project see once
pg ask "Which address on invoices?"  # a question that waits on the user; the brief lists it until pg ask 1 "..."
pg ask --json '[{"question": "1", "answer": "..."}]'      # many answers at once
pg ripple                            # what a change reaches: callers left as they were, tests to run
pg ls --code                         # the code: files and symbols, with the notes about them marked
pg batch '[["find", "refund"], ["read", "e12"]]'   # several commands in one call; lookups run side by side
pg undo                              # what pg did by itself, and one taken back
pg ls                                # projects, or the notes of one (--todo, --due); also pg mv, pg rm, pg doctor
```

**What pg does by itself.** Inside memory only, small steps that each undo with `pg undo`: it writes the handoff of a conversation that stopped without one, from what the log says it did; when a failing test passes and memory has no fix for its error, it writes a draft record with the change that fixed it, which the agent keeps by saying why. A test that fails and passes with the same code gets a draft record as flaky; a known fix that keeps coming back in several conversations becomes a draft rule, not binding until confirmed; a fix that did not help gets a dated line on its record; the commands that pass are kept in the project note for other computers. One that gets undone often only says what it would write. At `pg finish`, the steps the conversation took and left open are named in its handoff as worked on, and it is asked whether they are done. With hooks, an edit to a file another conversation still at work changed waits once, with who and when. pg never deletes or moves a note, changes your text or touches code by itself. `pg doctor --experts` shows what each part of pg said, how often the agent acted on it, and how that weighs it now.

Several conversations on one project see each other: who is at work, the steps another has taken, the notes they left, and what a conversation that stopped without a handoff did. Many `pg` calls can run at once, from several conversations, on one project or many. A write is whole or not there at all, also when its process is killed halfway. `pg rm` moves notes to `.archive/` instead of deleting them. pg's own index files are made again from the notes when they are damaged (kept aside as `.broken`); `pg doctor` also looks for damage deep inside them.

### Secrets

API keys and tokens never need to appear in a conversation. Each project has its own `<project>.env`; a secret is saved under a title, and the agent uses it by name:

```bash
pg write --secret "Npm publish token" -      # the value from stdin; shown only as npm_***
pg run -- npm publish --token '$NPM_PUBLISH_TOKEN'
```

`pg run` puts the value in place itself, on every platform, and prints `$NPM_PUBLISH_TOKEN` again wherever the value would appear in the output. Keys pasted into notes are masked and not kept. In a test with 50 keys and ten requests in plain words, the right key came first every time and was used through `pg run` without its value appearing in any output.

### Backup and sync

```bash
pg git
```

Opens a small screen: sign in to GitHub (with the GitHub CLI or the sign-in git already has, else in the browser with a code), then choose where the memory goes: a new private repository (`paragma-memory`, of your account or of one of its organizations), or one you have, in a `Paragma/` folder on its own branch or on its main branch. Choosing a public repository asks first. The `.env` files go only if you say so, and never to a public repository.

From then on every change goes there by itself, a few seconds after it is made, in the background: the agent never waits for it. What other computers sent comes in the same way. On a new computer, `pg git` finds the memory in your repositories (your own and your organizations') and restores it, joined with whatever is already there. The indexes and the short-lived state in `.pg/` never go; they are rebuilt.

Run `pg git` again to see the account, the repository and the last sync, to change the `.env` setting, to move the memory to another repository (other computers follow by themselves) or to disconnect. Without a terminal: `pg git connect --new`, `pg git status`, `pg git sync`, `pg git env on|off`, `pg git move --repo owner/name`, `pg git disconnect`.

## How it works

- **Plain Markdown.** Each project is a folder in `~/.paragma/projects/`: `project.md` (goal, steps, handoffs), `rules.md`, `decisions/`, `errors/` and any notes you like. Open it in any editor; edits are picked up on the next call.
- **Hybrid search.** Full-text search (SQLite FTS5) and meaning search with a multilingual embedding model built into the binary ([potion-multilingual-128M](https://huggingface.co/minishlab/potion-multilingual-128M), reduced to Latin scripts and 4-bit), merged by rank. A question in Turkish finds a note written in English.
- **Code awareness.** Paragma reads 20 programming languages, the shading languages (WGSL, GLSL, HLSL), SQL, GraphQL, Protobuf, Dockerfiles, CI and compose files, Vue and Svelte: files, functions, types, what each function calls. Shaders and queries kept in strings (``/* wgsl */ `...` ``, ``sql`...` ``) are read as their language, and Markdown documents by their sections, so the brief and `pg find` name the section to read. Notes are linked to code by path and symbol, and record the state of the code when they were written, so a note says when it may be out of date.
- **What a change reaches.** `pg ripple` compares the code with the last commit: a function whose signature changed, renamed or removed, with the callers the change left as they were (those it is sure of apart from those found by name only, and the places that use it outside a function, such as a callback) and the tests that reach it; a shader struct, GraphQL or Protobuf type or SQL table whose fields changed, with the other code that names them. `pg run`, the hooks and `pg finish` say it by themselves; `pg ripple Refund` tells what reaches a function before changing it.
- **Libraries at their versions.** The libraries a project locks (Cargo.lock, node_modules, go.mod, a virtual environment) are read from the source already on the computer, when an agent asks: `pg find "serde_json Value get"` gives the real signature, doc, example, the feature it needs and deprecation at that version; a failed build that looked for a name a library does not have gets the right one from `pg run`; an upgrade is pointed out in the brief with what changed in what the code calls.
- **Records that merge.** Writing a rule, decision or error that is already known updates the existing one instead of adding a copy; a decision that contradicts an earlier one is pointed out.
- **A pure core.** What pg says is decided by a kernel that reads only facts (the event log, the runs, what was already said) and never a file or the clock: small experts each notice one thing (a conversation cut short, the commands that work, a note whose code is gone, notes that could be tidier), and attention chooses what fits the agent's context, never twice. Each kind of hint is weighted by how often agents acted on it; a note fades by its uses (ACT-R). Heavy looks over the whole memory run after `pg finish`, in the background.
- **One binary.** `pg` is written in Rust: no runtime, no server, and no network unless you connect `pg git` (it uses git and curl). The indexes next to the notes can be deleted at any time and are rebuilt.

## How it is tested

- **Unit tests** on Windows, macOS and Linux for every change; **end-to-end tests** of every command before each release.
- **Stress and robustness**, also before each release: every command from three conversations at once; notes in CRLF, BOM, UTF-16 and Latin-1, empty and very large files, binary files, symlink loops; the first code index built by twelve processes at the same time.
- **Search quality:** 70 questions in English, Turkish, Spanish, German, French and Portuguese against a memory of 1,335 notes (`bench/quality.py`), run before every change to ranking: 153 of the 168 expected facts are found, and nothing is returned for questions memory cannot answer.
- **Rehearsal:** the agent test tasks replayed step by step without a model (`bench/rehearsal.py`), to catch mistakes before spending a model's time.
- **Agent tests:** the results above. The project, the memory, the questions, the scenarios and their hidden tests are in [bench/](bench/).

## Building from source

```bash
bash tools/build-model/fetch.sh   # downloads the source model (about 500 MB) and builds model/
cargo build --release             # target/release/pg
cargo test --release
```

## License

[MIT](LICENSE). The embedded model, potion-multilingual-128M by Minish Lab, is MIT licensed; the tree-sitter grammars are MIT licensed; SQLite is in the public domain.
