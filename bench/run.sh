#!/usr/bin/env bash
# Run one benchmark cell.
#
#   bench/run.sh <scenario> <none|files|paragma> <rep> [out-root]
#
# Builds a fresh copy of the fixture repo, adds the memory for the condition,
# runs the agent on the scenario's prompt(s) (prompt2 starts a NEW
# conversation), then runs the hidden tests on a copy of the result.
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
R=$W/repo

rm -rf "$W"
mkdir -p "$W"
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
	cp -r "$B/memory" "$R/.paragma"
	cp "$B/agents/paragma.AGENTS.md" "$R/AGENTS.md"
	;;
*)
	echo "unknown condition $COND" >&2
	exit 2
	;;
esac
(cd "$R" && git init -q && git add -A && git -c user.name=bench -c user.email=bench@example.com -c core.autocrlf=false commit -qm "initial")

export PATH="/c/Program Files/Go/bin:$PATH"
export PARAGMA_HOME="$W/pghome"
if [ "$COND" = paragma ]; then
	export PATH="$B/../bin:$PATH"
fi

for i in 1 2; do
	P=$SD/prompt$i.txt
	[ -f "$P" ] || break
	start=$(date +%s)
	(cd "$R" && timeout "$TO" agy -p "$(cat "$P")" --model "$MODEL" --output-format stream-json \
		--dangerously-skip-permissions --print-timeout "$((TO - 30))s" >"$W/conv$i.jsonl" 2>"$W/conv$i.err")
	echo "$i $? $(($(date +%s) - start))" >>"$W/timing.txt"
done

# ---- checks on a copy, so the agent's repo stays as it left it
C=$W/check
rm -rf "$C"
cp -r "$R" "$C"
(cd "$R" && go test ./... >"$W/own_tests.txt" 2>&1; echo "exit $?" >>"$W/own_tests.txt")
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
echo "done $SC $COND $REP"
