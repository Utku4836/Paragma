---
name: paragma-projects
description: Use for work that spans steps or conversations with Paragma - starting or continuing a project, recording the user's decisions, plan steps, handoffs, and organising project notes and folders.
---
# Projects with Paragma

## Start or continue
- `pg start <name> --goal "..." --step "..." --decision "..."`: a new project, or the existing one of that name (or the project that holds a note of that name) continued. Record every decision and every step of the user's plan (also the steps for later conversations), in their words with the names and signatures they gave, before writing code: the next conversation only knows what was recorded.
- A new conversation on earlier work: `pg resume <name>`: goal, decisions, open steps, handoff, code changed since the start, and the brief.

## While working
- `pg done <id> [--log "<finding>"]` marks a step done and prints the next one. More steps: `pg plan "step" "step"`.
- A new decision from the user: `pg decide "..."`. A finding worth keeping (also where things are in the code): `pg log "..."`.
- Steps in detail: `pg task list`, `pg task block <id> --by "..."`, `pg task ask <id> --q "..."`, `pg task note <id> "..."`.

## Finish
- `pg finish "<what changed, what is next>" --done <ids>`: closes steps, checks the changes against the rules, writes the handoff.
- `pg checkpoint "..."` rewrites only the handoff. `pg project close --summary "..."` when the project is over.

## Notes and folders
- Your own notes: `pg new note "<title>" --dir <folder>` (folders are created as needed).
- The project's folders: `pg tree --notes [folder]`.
- Rename or move a note, a folder or a whole project: `pg mv <from> <new name | path | folder/>`; links follow.
- `pg archive <note>` keeps it out of the way; `pg rm <note> --hard` deletes it.

All flags and examples: `pg skill projects --ref`.
