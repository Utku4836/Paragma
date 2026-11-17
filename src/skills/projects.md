---
name: paragma-projects
description: Use for work that spans steps or conversations with Paragma - opening or continuing a project, its plan, handoffs, other conversations on it, and organising its notes.
---
# Projects with Paragma

## Open
- `pg open <name> --goal "..." --step "..." --step "..." --decision "..."` in the code's folder: a new project, or the existing one continued. The folder is linked: later commands here know the project. Each `--decision` is kept as a decision record (`pg write --decision` for one with its reasons).
- Same name written another way (`Shop API`, `shop-api`) or a small typo continues the project. A name sharing only a word starts a new one and names the similar project: if it is the same work, `pg open <that name>`.
- A new conversation on earlier work: `pg open` alone in the code's folder (or its folder's name). Do not guess a new name.
- Give every step of the user's plan, also those for later conversations, and every decision, in their words with the names and signatures they gave: the next conversation only knows what was written.
- The brief: goal, open steps, the last handoff in full and earlier ones in one line, what changed since, other conversations still at work, a conversation that stopped without a handoff and what it did (`stopped without a handoff`), after compaction what this conversation did before, how the project runs (`how it runs here`), rules, decisions and errors for the goal, likely code, secrets by title. Nothing is said twice in a conversation.
- A short question or one-off task needs no project: `pg find` searches every project.

## While working
- More steps: `pg open --step "..."`. A changed goal: `pg open --goal "..."`. All steps with their numbers: `pg steps` (`--all` with the done ones); one gone stale or wrong: `pg steps --drop 7` or `pg steps --edit 7 "new text"`; left for later on purpose: `pg steps --defer 7` (`--resume 7` brings it back). A step's number never changes. `pg finish` names the steps it closed: one closed by mistake, `pg steps --reopen 7`.
- Other conversations on the project: take the step you work on, `pg steps --take 7`, and the others see it taken (until you finish or go quiet for an hour); one taken by another is marked `(taken by ...)`, leave it. A short note for them: `pg tell "I am changing refund.go"`; theirs come to you once, with your next pg command.
- A question only the user can answer, and the work waits on it (an address, a document, a choice): `pg ask "..."` keeps it; the brief lists the open ones under "waiting on the user", so it is neither asked twice nor forgotten. When the user answers: `pg answer <number> "..."`.
- Inside a project's own folder in memory (`~/.paragma/projects/<name>`), commands work on that project without `-p`.
- Decisions, rules, errors as you learn them: `pg skill knowledge`.

## Finish
- `pg finish "<what was done; what is next>" --done 1,2 --left "<new step for later>"`: ticks off the steps you finished (their numbers from the brief or `pg steps`, words only that step has, or `--done all`; one that closes nothing is said), adds new steps for later, writes this conversation's handoff with what it wrote, the code it changed and the last test run. Steps not done stay open for the next conversation. Finishing again replaces this conversation's handoff; other conversations' are kept.

## Notes and folders
- A note: `pg write <folder>/<name> "text"` (`--append` adds below; `-` reads stdin, `--file PATH` a file). A folder: `pg write <folder>/`. In another project: `pg write <project>/<path> "..."`.
- One topic per note, named for it: `kargo/yurtici-anlasmasi`, `kargo/ucretler`, `kargo/iade-kodu`, not one `kargo/durum` holding everything. A folder is a category with many notes; a new topic is a new note, not another section appended to one. Split a note that has come to hold several topics.
- Code, scripts and big files do not go in the memory folder (it goes to GitHub with the notes, and .md files there count as notes): keep them in a folder of their own and link it.
- The folders a project's code is in: `pg link` lists them; `pg link <folder>` links one (`pg open` does it for the folder it runs in), `--replace` when the code moved (the others are taken out), `pg link --remove <folder>` takes one out, `--remove --gone` the ones that no longer exist.
- What is where: `pg ls`, `pg ls <folder>`; projects: `pg ls` outside a project.
- Rename or move a note, folder or project: `pg mv <from> <to>` (`<folder>/` moves into it). Ids stay, `[[links]]` to it follow (also `[[folder/name]]`).
- `pg rm <id | path | folder/ | project>` moves it to `.archive/` (`pg mv .archive/<path> <path>` brings it back); `--hard` deletes it.
- A path's first part names this project's folder when it has one, else a project of that name: `pg write` says so when the note went to another project.
- pg says a file of its own is damaged: `pg doctor` finds it and makes it again (notes are never touched).

## Memory on GitHub
- The user may back up and sync memory with `pg git` (their screen). What you write then goes there by itself; never wait for it or sync by hand.
- `pg git status` says where it goes and how the last sync went. Connect, move or disconnect only when the user asks: `pg git connect --new`, `--repo owner/name`.
