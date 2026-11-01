---
name: paragma-projects
description: Use for work that spans steps or conversations with Paragma - opening or continuing a project, its plan, handoffs, other conversations on it, and organising its notes.
---
# Projects with Paragma

## Open
- `pg open <name> --goal "..." --step "..." --step "..."` in the code's folder: a new project, or the existing one continued. The folder is linked: later commands here know the project.
- Same name written another way (`Shop API`, `shop-api`) or a small typo continues the project. A name sharing only a word starts a new one and names the similar project: if it is the same work, `pg open <that name>`.
- A new conversation on earlier work: `pg open` alone in the code's folder (or its folder's name). Do not guess a new name.
- Give every step of the user's plan, also those for later, in their words: the next conversation only knows what was written.
- The brief: goal, open steps, the last handoff in full and earlier ones in one line, what changed since, other conversations still at work, the stack and commands used here, rules, decisions and errors for the goal, likely code, secrets by title.
- A short question or one-off task needs no project: `pg find` searches every project.

## While working
- More steps: `pg open --step "..."`. A changed goal: `pg open --goal "..."`.
- Decisions, rules, errors as you learn them: `pg skill knowledge`.

## Finish
- `pg finish "<what was done; what is next>" --left "<step for later>"`: ticks off this conversation's steps (not other conversations'), adds the steps left, writes this conversation's handoff with what it wrote, the code it changed and the last test run. Other conversations' handoffs are kept.

## Notes and folders
- A note: `pg write <folder>/<name> "text"` (`--append` adds below; `-` reads stdin, `--file PATH` a file). A folder: `pg write <folder>/`. In another project: `pg write <project>/<path> "..."`.
- What is where: `pg ls`, `pg ls <folder>`; projects: `pg ls` outside a project.
- Rename or move a note, folder or project: `pg mv <from> <to>` (`<folder>/` moves into it). Ids stay, `[[links]]` follow.
- `pg rm <id | path | folder/ | project>` moves it to `.archive/` (`pg mv .archive/<path> <path>` brings it back); `--hard` deletes it.
