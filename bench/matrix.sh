#!/usr/bin/env bash
# Run the whole benchmark matrix with limited parallelism.
#
#   bench/matrix.sh [reps] [parallel] [out-root]
#
# Cells are interleaved (scenario x condition x rep) so that a slow period of
# the model API hits every condition alike.
B=$(cd "$(dirname "$0")" && pwd)
REPS=${1:-3}
PAR=${2:-3}
OUT=${3:-$B/runs}
mkdir -p "$OUT"
{
	for rep in $(seq 1 "$REPS"); do
		for sc in s1-recurring-error s2-continuation s3-project-rules; do
			for cond in none files paragma; do
				echo "$sc $cond $rep"
			done
		done
	done
} >"$OUT/cells.txt"
xargs -P "$PAR" -L 1 bash -c '[ -f "'"$OUT"'/$0/$1-$2/repo/go.mod" ] && exit 0; bash "'"$B"'/run.sh" "$0" "$1" "$2" "'"$OUT"'" >>"'"$OUT"'/progress.log" 2>&1' <"$OUT/cells.txt"
echo "matrix finished $(date)" >>"$OUT/progress.log"
