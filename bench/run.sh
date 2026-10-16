#!/usr/bin/env bash
# Run one benchmark cell.
#
#   bench/run.sh <scenario> <none|files|paragma> <rep> [out-root]
#
# Builds a fresh copy of the fixture repo in a random temp directory (outside
# this repository, so the agent cannot wander into the hidden tests or other
# runs), adds the memory for the condition, runs the agent on the scenario's
# prompt(s) and then runs the hidden tests on a copy of the result.
#
# Each conversation gets a fresh Antigravity profile (its own HOME with only
# the login settings copied): prompt2 is a NEW conversation that cannot read
# the first one's transcript, so only the project's files carry knowledge.
#
# Environment: MODEL (default gemini-3.8-flash-high), AGENT_TIMEOUT (s).
set -u
B=$(cd "$(dirname "$0")" && pwd)
SC=$1
COND=$2
REP=$3
OUT=${4:-$B/runs}
MODEL=${MODEL:-gemini-3.8-flash-high}
TO=${AGENT_TIMEOUT:-900}
SD=$B/scenarios/$SC
W=$OUT/$SC/$COND-$REP
TMPROOT=$(cygpath -u "$LOCALAPPDATA")/Temp
WS=$(mktemp -d "$TMPROOT/ws-XXXXXXXX")
R=$WS/repo

rm -rf "$W"
mkdir -p "$W"
echo "$WS" >"$W/workspace.txt"
cp -r "$B/fixture/shopkit" "$R"
if [ -f "$SD/setup.sh" ]; then (cd "$R" && bash "$SD/setup.sh"); fi
case $COND in
none) ;;
files)
	cp -r "$B/memory" "$R/memory"
	rm -f "$R/memory/config.toml" "$R/memory/.gitignore"
	cp "$B/agents/files.AGENTS.md" "$R/AGENTS.md"
	;;
paragma)
	# memory lives outside the repo, in the Paragma home (a space with one project)
	bash "$B/to_space.sh" "$B/memory" "$WS/pghome"
	cp "$B/agents/paragma.AGENTS.md" "$R/AGENTS.md"
	;;
*)
	echo "unknown condition $COND" >&2
	exit 2
	;;
esac
(cd "$R" && git init -q && git -c core.autocrlf=false add -A && git -c user.name=bench -c user.email=bench@example.com -c core.autocrlf=false commit -qm "initial")

export PATH="/c/Program Files/Go/bin:$PATH"
export PARAGMA_HOME="$WS/pghome"
if [ "$COND" = paragma ]; then
	mkdir -p "$WS/bin"
	cp "$B/../bin/pg.exe" "$WS/bin/pg.exe"
	export PATH="$WS/bin:$PATH"
fi

for i in 1 2; do
	P=$SD/prompt$i.txt
	[ -f "$P" ] || break
	# fresh agent profile per conversation: login only, no history. It lives
	# away from the workspace and is deleted when the conversation ends, so a
	# later conversation cannot dig up an earlier transcript.
	AH=$(mktemp -d "$TMPROOT/p-XXXXXXXX")
	mkdir -p "$AH/.gemini/antigravity-cli"
	cp -r "$HOME/.gemini/config" "$AH/.gemini/" 2>/dev/null
	cp "$HOME/.gemini/antigravity-cli/settings.json" "$HOME/.gemini/antigravity-cli/installation_id" "$AH/.gemini/antigravity-cli/" 2>/dev/null
	AHW=$(cygpath -w "$AH")
	start=$(date +%s)
	# a clean environment: no PWD/OLDPWD or other variables that point at
	# this repository (an early run found the hidden tests that way)
	APATH="/c/Windows/system32:/c/Windows:/c/Windows/System32/WindowsPowerShell/v1.0:/c/Program Files/Go/bin:/c/Program Files/Git/cmd:$(dirname "$(command -v agy)")"
	EXTRA=()
	if [ "$COND" = paragma ]; then
		APATH="$WS/bin:$APATH"
		EXTRA=(PARAGMA_HOME="$(cygpath -w "$WS/pghome")")
	fi
	(cd "$R" && env -i PATH="$APATH" SYSTEMROOT="${SYSTEMROOT:-C:\\WINDOWS}" WINDIR="${WINDIR:-C:\\WINDOWS}" \
		COMSPEC="${COMSPEC:-C:\\WINDOWS\\system32\\cmd.exe}" PATHEXT="${PATHEXT:-.COM;.EXE;.BAT;.CMD}" \
		TEMP="$(cygpath -w "$TMPROOT")" TMP="$(cygpath -w "$TMPROOT")" LOCALAPPDATA="$LOCALAPPDATA" \
		APPDATA="${APPDATA:-$LOCALAPPDATA\\..\\Roaming}" PROGRAMDATA="${PROGRAMDATA:-C:\\ProgramData}" \
		USERPROFILE="$AHW" HOME="$AHW" "${EXTRA[@]}" \
		/usr/bin/timeout "$TO" "$(command -v agy)" -p "$(cat "$P")" --model "$MODEL" \
		--output-format stream-json --dangerously-skip-permissions --print-timeout "$((TO - 30))s" \
		>"$W/conv$i.jsonl" 2>"$W/conv$i.err")
	echo "$i $? $(($(date +%s) - start))" >>"$W/timing.txt"
	echo "$AH" >>"$W/profiles.txt"
	rm -rf "$AH"
	if [ ! -s "$W/conv$i.jsonl" ]; then
		echo "FAILED $SC $COND $REP: the agent produced no output in conversation $i" >&2
		rm -rf "$WS"
		exit 1
	fi
done

# ---- checks on a copy, so the agent's repo stays as it left it
C=$WS/check
cp -r "$R" "$C"
(cd "$R" && go test -count=1 ./... >"$W/own_tests.txt" 2>&1; echo "exit $?" >>"$W/own_tests.txt")
cp -r "$SD/hidden/." "$C/"
for opt in $(find "$C" -name "*.go.opt"); do
	target=${opt%.opt}
	pkg=$(dirname "$opt")
	if grep -lq "ErrRefundWindow" $(ls "$pkg"/*.go | grep -v _test.go) 2>/dev/null; then
		mv "$opt" "$target"
	else
		rm "$opt"
	fi
done
(cd "$C" && go test -count=1 -json ./... >"$W/hidden.json" 2>"$W/hidden.err")
(cd "$R" && git status --porcelain >"$W/changed.txt" && git diff >"$W/diff.patch")
# keep the agent's final repo (without the memory copy) next to the logs
mkdir -p "$W/repo"
(cd "$R" && tar cf - --exclude=.git --exclude=memory . ) | (cd "$W/repo" && tar xf -)
[ -d "$WS/pghome/Spaces" ] && (cd "$WS/pghome" && tar cf - --exclude=.pg Spaces) | (cd "$W/repo" && tar xf -)
cp -r "$R/memory/work" "$W/repo/memory-work" 2>/dev/null
rm -rf "$WS"
echo "done $SC $COND $REP"
