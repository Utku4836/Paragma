#!/usr/bin/env bash
# One short agent conversation: a question about the project, answered from
# memory, no code changes. Used by quick_agent.py.
#
#   bench/ask.sh <files|paragma> <memory-dir> <prompt-file> <out-dir>
#
# Same isolation as run.sh: fresh repo copy in a random temp directory,
# memory for the condition, a fresh agent profile, a clean environment.
set -u
B=$(cd "$(dirname "$0")" && pwd)
COND=$1
MEM=$2
PROMPT=$3
W=$(mkdir -p "$4" && cd "$4" && pwd)
MODEL=${MODEL:-gemini-3.8-flash-high}
TO=${AGENT_TIMEOUT:-300}
TMPROOT=$(cygpath -u "$LOCALAPPDATA")/Temp
WS=$(mktemp -d "$TMPROOT/ws-XXXXXXXX")
R=$WS/repo
mkdir -p "$W"
cp -r "$B/fixture/shopkit" "$R"
EXTRA=()
APATH="/c/Windows/system32:/c/Windows:/c/Windows/System32/WindowsPowerShell/v1.0:/c/Program Files/Go/bin:/c/Program Files/Git/cmd:$(dirname "$(command -v agy)")"
case $COND in
files)
	cp -r "$MEM" "$R/memory"
	rm -f "$R/memory/config.toml" "$R/memory/.gitignore"
	cp "$B/agents/files.AGENTS.md" "$R/AGENTS.md"
	;;
paragma)
	bash "$B/to_space.sh" "$MEM" "$WS/pghome"
	cp "$B/agents/paragma.AGENTS.md" "$R/AGENTS.md"
	mkdir -p "$WS/bin" && cp "$B/../bin/pg.exe" "$WS/bin/"
	APATH="$WS/bin:$APATH"
	EXTRA=(PARAGMA_HOME="$(cygpath -w "$WS/pghome")")
	;;
esac
(cd "$R" && git init -q && git -c core.autocrlf=false add -A && git -c user.name=bench -c user.email=bench@example.com -c core.autocrlf=false commit -qm "initial")
AH=$(mktemp -d "$TMPROOT/p-XXXXXXXX")
mkdir -p "$AH/.gemini/antigravity-cli"
cp -r "$HOME/.gemini/config" "$AH/.gemini/" 2>/dev/null
cp "$HOME/.gemini/antigravity-cli/settings.json" "$HOME/.gemini/antigravity-cli/installation_id" "$AH/.gemini/antigravity-cli/" 2>/dev/null
AHW=$(cygpath -w "$AH")
start=$(date +%s)
for try in 1 2 3; do
(cd "$R" && env -i PATH="$APATH" SYSTEMROOT="C:\WINDOWS" WINDIR="C:\WINDOWS" COMSPEC="C:\WINDOWS\system32\cmd.exe" PATHEXT=".COM;.EXE;.BAT;.CMD" \
	TEMP="$(cygpath -w "$TMPROOT")" TMP="$(cygpath -w "$TMPROOT")" LOCALAPPDATA="$LOCALAPPDATA" APPDATA="$LOCALAPPDATA\..\Roaming" PROGRAMDATA="C:\ProgramData" \
	USERPROFILE="$AHW" HOME="$AHW" "${EXTRA[@]}" \
	/usr/bin/timeout "$TO" "$(command -v agy)" -p "$(cat "$PROMPT")" --model "$MODEL" \
	--output-format stream-json --dangerously-skip-permissions --print-timeout "$((TO - 20))s" \
	>"$W/conv1.jsonl" 2>"$W/conv1.err")
rc=$?
# agents started at the same moment sometimes get a login prompt; retry
grep -q "Authentication required" "$W/conv1.err" || break
sleep $((try * 20))
done
echo "$rc $(($(date +%s) - start))" >"$W/timing.txt"
echo "$WS" >"$W/workspace.txt"
(cd "$R" && git status --porcelain | grep -v " Microsoft/$") >"$W/changed.txt"
rm -rf "$AH" "$WS"
