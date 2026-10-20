<p align="center">
  <img src=".github/assets/banner.png" width="100%" alt="Paragma">
</p>

<p align="center">
  <a href="#install">Install</a> &nbsp;·&nbsp;
  <a href="#connect-your-agent">Connect your agent</a> &nbsp;·&nbsp;
  <a href="#results">Results</a> &nbsp;·&nbsp;
  <a href="#how-it-works">How it works</a>
</p>

Paragma is a local memory for AI coding agents. It keeps a project's rules, decisions, past errors and work notes as Markdown on your computer, and hands the agent what it needs in one command instead of a dozen searches. One binary, `pg`. No server, no network, no model.

## Why

An agent starts every conversation from zero. It rediscovers the same project rules, repeats a fix that was already found, and loses the decisions you gave it yesterday. Notes help, but an agent reading files has to search, open and re-open until it trusts it has seen everything, and every extra step re-reads the whole conversation.

Paragma answers in one step:

- **Known errors come back with their fix.** `pg run -- go test ./...` prints the recorded cause and fix next to a failure it has seen before.
- **A topic in one call.** `pg context refund` returns the matching rules, decisions and past fixes, grouped by note, and ends by saying that nothing else matches, so the agent stops searching.
- **Work outlives the conversation.** `pg start` records the goal, the steps and your decisions. In a new conversation `pg resume` brings them back, together with the code that changed since.

## Results

The same knowledge reached two ways: as plain Markdown notes the agent reads with its own tools, or through Paragma. Each run starts from a fresh copy of a small Go project; numbers are plain notes → Paragma.

| Agent and model | Task | Tool calls | Tokens processed |
| --- | --- | --- | --- |
| Antigravity, Gemini 3.8 Flash | 5 questions, 1,539 notes | 60 → **12** | 2.17M → **0.32M** |
| Antigravity, Gemini 3.8 Flash | 5 questions, 149 notes | 42 → **19** | 1.14M → **0.40M** |
| OpenCode, DeepSeek V4.1 Flash | 5 questions, 1,539 notes | 58 → **17** | 0.48M → **0.16M** |
| OpenCode, GLM-5.3 Flash | 5 questions, 1,539 notes | 27 → **7** | 0.30M → **0.10M** |
| OpenCode, DeepSeek V4.1 Flash | 3 coding tasks, 1,539 notes | 154 → **117** | 3.10M → **1.08M** |
| OpenCode, GLM-5.3 Flash | 3 coding tasks, 1,539 notes | 113 → **96** | 1.84M → **1.32M** |

Correctness was the same both ways in every row: the same facts in the answers, and every hidden test passed in the coding tasks. With Paragma the agents needed up to 7 times fewer tokens and up to 5 times fewer tool calls, and the cost stayed flat as the memory grew tenfold.

The coding tasks are a recurring error whose obvious fix is wrong, a feature finished in a second conversation that only knows what the first one recorded, and a change governed by project rules. Without memory, agents failed parts of each. OpenCode runs used each model's highest reasoning effort. Setup and full numbers: [bench/RESULTS.md](bench/RESULTS.md).

## Install

```bash
npm install -g paragma
```

This installs `pg` for Windows, macOS or Linux (x64 and arm64). You can also download `pg` from the [releases](https://github.com/Utku4836/Paragma/releases) and put it on your `PATH`, or build it with Go:

```bash
go install github.com/Utku4836/Paragma/cmd/pg@latest
```

Memory lives in `~/.paragma`, outside your repositories. The only file Paragma writes into a project is the instructions section, and only when you set up that project's conversations.

## Connect your agent

```bash
pg setup
```

`pg setup` finds the agents on your computer, including their desktop apps, and adds a short "Memory (Paragma)" section where each one reads its instructions. Choose **every conversation**, or pick **one conversation** from the agent's own history to set up only that conversation's folder. Running it again updates the section and leaves the rest of the file alone.

| Agent | Apps | Every conversation | One conversation |
| --- | --- | --- | --- |
| Claude Code | CLI, desktop | `~/.claude/CLAUDE.md` | `CLAUDE.md` in its folder |
| Codex | CLI, desktop | `~/.codex/AGENTS.md` | `AGENTS.md` in its folder |
| Antigravity | CLI, IDE | `~/.gemini/AGENTS.md` | `AGENTS.md` in its folder |
| OpenCode | CLI | `~/.config/opencode/AGENTS.md` | `AGENTS.md` in its folder |

The results above were measured with OpenCode, earlier ones with Antigravity. In scripts: `pg setup codex --global` or `pg setup claude --dir .`

## What the agent does with it

The section `pg setup` writes is short: when to call `pg`. The details are in three skills the agent opens when it needs them, `pg skill coding`, `pg skill projects` and `pg skill knowledge`, also kept as files in `~/.paragma/skills/`. `pg help` lists every command.

```bash
pg prime                         # start of a conversation: what memory holds
pg start payments --goal "Card payments" --step "Webhook" --step "Tests" --decision "Discount before VAT"
                                 # prints a brief: rules, decisions, past fixes, likely files, test command
pg tree                          # the code: files, their declarations, the rules and errors about each
pg related payments/refund.go    # before changing a file: its rules, decisions, past fixes, last commits
pg context "refund"              # rules, decisions and fixes on a topic, in one call
pg read payments/refund.go#Refund  # one function, not the whole file
pg run -- go test ./...          # failing tests: new, still failing or flaky; the known fix
pg done t2f4                     # mark a step done; prints the next one in full
pg finish "what changed, next"   # before the final answer: closes steps, checks rules, writes the handoff
pg resume payments               # in a new conversation: goal, decisions, open steps, changed code
pg error record --from-last --cause "CRLF checkout" --fix "Normalise line endings before hashing"
```

`pg start` on a project that exists continues it. `pg related` without a path lists the files you changed with the rules that apply to them, and notes follow files renamed in git. Notes, folders and whole projects are renamed or moved with `pg mv`, and many `pg` calls can run at once.

## How it works

- **Spaces and projects.** Notes are plain Markdown in `~/.paragma/Spaces/<space>/<project>/`. A space works like an Obsidian vault: search and links stay inside it. Open the folder in any editor.
- **One index.** A SQLite full-text index next to the notes is updated on every call and can be deleted at any time. Parallel calls wait for each other's writes instead of failing.
- **Addresses.** Every result starts with an address the agent can pass back: `shopkit/rules^r003`, `payments/payments#Decisions`, `shopkit/errors/catalog-checksum-crlf`.
- **Local.** Paragma never opens a network connection. Memory stays on your computer and out of git.

## Development

```bash
go test ./...
python bench/quick.py          # retrieval test without an agent, about a minute
```

## License

[MIT](LICENSE)
