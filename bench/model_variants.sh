#!/usr/bin/env bash
# Builds pg with each model variant and measures it with bench/quality.py.
#   bash bench/model_variants.sh > bench/model-variants.md
# Leaves model/ as the last variant built; rebuild the chosen one after.
set -uo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/target/model-src/potion-multilingual-128M"
variants=(
	"latin-f16|--keep latin"
	"latin-i8|--keep latin --dtype i8"
	"latin-i4|--keep latin --dtype i4"
	"latin-i4-d192|--keep latin --dtype i4 --dims 192"
	"latin-i8-d128|--keep latin --dtype i8 --dims 128"
)
cd "$root"
for v in "${variants[@]}"; do
	name=${v%%|*}
	opts=${v#*|}
	echo "## $name ($opts)"
	rm -rf model
	# shellcheck disable=SC2086
	cargo run --release -q -p build-model -- "$src" model $opts 2>&1
	size=$(du -cb model/* | tail -1 | cut -f1)
	echo "model files: $((size / 1000000)) MB"
	cargo build --release -q 2>&1 | grep -E "^error" || true
	python bench/quality.py 2>&1 | sed -n '/^| system/,/^$/p'
	echo
done
