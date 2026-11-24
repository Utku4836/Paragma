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
- The brief: goal, open steps, the last handoff in full and earlier ones in one line, what changed since, other conversations still at work, a conversation that stopped without a handoff and what it did (`stopped without a handoff`), after compaction what this conversation did before, how the project runs (`how it runs here`; on a new computer from the project note), rules, decisions and errors for the goal, likely code, secrets by title. And now and then: steps left open through several handoffs (still wanted? `pg steps --defer` or `--drop`), drafts waiting two weeks for a cause, an archived note read again (bring it back with `pg mv`). Nothing is said twice in a conversation.
- A short question or one-off task needs no project: `pg find` searches every project.

## While working
- More steps: `pg open --step "..."`. A changed goal: `pg open --goal "..."`. All steps with their numbers: `pg steps` (`--all` with the done ones); one gone stale or wrong: `pg steps --drop 7` or `pg steps --edit 7 "new text"`; left for later on purpose: `pg steps --defer 7` (`--resume 7` brings it back). A step's number never changes. `pg finish` names the steps it closed: one closed by mistake, `pg steps --reopen 7`.
- Other conversations on the project: take the step you work on, `pg steps --take 7`, and the others see it taken (until you finish or go quiet for an hour); one taken by another is marked `(taken by ...)`, leave it. A short note for them: `pg tell "I am changing refund.go"`; theirs come to you once, with your next pg command. With hooks, an edit to a file another conversation still at work changed waits once, with who and when: check with it first.
- What pg does by itself, inside memory only, each undone with one command: a handoff for a conversation that stopped without one (written from what the log says it did; its own `pg finish` replaces it), a draft record of an error a test run fixed or of a flaky test, a draft rule from a fix that keeps coming back, a dated line on a record whose fix did not help, the commands that pass here in the project note (`How it runs`; a section you changed is yours and stays). `pg undo` lists them with numbers; `pg undo <number>` takes one back. One that gets undone often only says what it would write (`pg doctor --experts`). pg never deletes or moves a note, changes your text or touches code by itself.
- A question only the user can answer, and the work waits on it (an address, a document, a choice): `pg ask "..."` keeps it; the brief lists the open ones under "waiting on the user", so it is neither asked twice nor forgotten. When the user answers: `pg ask <number> "..."`; several at once: `pg ask --json '[{"question": "2", "answer": "..."}, {"question": "3", "answer": "..."}]'`.
- Inside a project's own folder in memory (`~/.paragma/projects/<name>`), commands work on that project without `-p`.
- The user works in the same memory with `pg` alone (an interface in their terminal): their changes show as `you` in the activity and as `changed by the user`; a note they changed is the newest word on it. A note from them (`pg tell` there) comes as `from the user`.
- Decisions, rules, errors as you learn them: `pg skill knowledge`.

## Finish
- `pg finish "<what was done; what is next>" --done 1,2 --left "<new step for later>"`: ticks off the steps you finished (their numbers from the brief or `pg steps`, words only that step has, or `--done all`; one that closes nothing is said), adds new steps for later, writes this conversation's handoff with what it wrote, the code it changed and the last test run. The steps you took (`pg steps --take`) and leave open are named in the handoff as worked on, and you are asked whether they are done. Steps not done stay open for the next conversation. Finishing again replaces this conversation's handoff; other conversations' are kept. A handoff that says what was done but not what is next, while steps are open, is pointed out: the next conversation starts from it.

## Notes and folders
- A note: `pg write <folder>/<name> "text"` (`--append` adds below; `-` reads stdin, `--file PATH` a file). A folder: `pg write <folder>/`. In another project: `pg write <project>/<path> "..."`.
- One topic per note, named for it: `kargo/yurtici-anlasmasi`, `kargo/ucretler`, `kargo/iade-kodu`, not one `kargo/durum` holding everything. A folder is a category with many notes; a new topic is a new note, not another section appended to one. Split a note that has come to hold several topics.
- Code and scripts do not go in the memory folder (it goes to GitHub with the notes, and .md files there count as notes): keep them in a folder of their own. A document the work needs again (a workbook, a contract, a deck, a PDF) is kept with its note: `pg write <topic>/<note> --file <path>` (`pg skill knowledge`).
- `pg open` links the folder it runs in to the project. `pg link` is for the rest (the folders of a project, one that moved: `--replace`, one taken out: `--remove`).
- What is where: `pg ls`, `pg ls <folder>`; projects: `pg ls` outside a project. The to-dos of every note: `pg ls --todo`; due dates: `pg ls --due`.
- Rename or move a note, folder or project: `pg mv <from> <to>` (`<folder>/` moves into it). Ids stay, `[[links]]` to it follow (also `[[folder/name]]`).
- `pg rm <id | path | folder/ | project>` moves it to `.archive/` (`pg mv .archive/<path> <path>` brings it back); `--hard` deletes it.
- A path's first part names this project's folder when it has one, else a project of that name: `pg write` says so when the note went to another project.
- pg says a file of its own is damaged: `pg doctor` finds it and makes it again (notes are never touched).

## Memory synced
- The user may keep memory in one place, the same on every computer: GitHub, another git host, a cloud folder (OneDrive, Google Drive, Dropbox, iCloud) or WebDAV, with `pg sync` or the Sync section of `pg` (their screen). What you write then goes there by itself; never wait for it or sync by hand.
- A note changed on two computers keeps the other one beside it as `<name>.conflict.md`: the user chooses in `pg` (Sync); asked to do it yourself, join what both say in the note, then `pg rm` the copy.
- `pg sync` says where it goes and how the last sync went. Connect, move, disconnect, back up or restore only when the user asks: `pg sync connect --help`, `pg sync move <where>` (checked before the old place is left), `pg sync backup [<folder>]`, `pg sync backups`, `pg sync restore <zip>` (joined with the memory; `--replace --yes` only when the user said to replace it; `--check` first shows what it would do). A place that already holds a memory is answered by the user's choice only (`--there update|join|replace|take`); never pause, resume or keep projects off the sync (`pg sync pause`, `pg sync projects --skip`) unless asked.
