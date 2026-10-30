#!/usr/bin/env bash
# Downloads the source model and builds model/ (the files pg embeds).
#   bash tools/build-model/fetch.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
name=potion-multilingual-128M
src="$root/target/model-src/$name"
mkdir -p "$src"
for f in config.json tokenizer.json model.safetensors; do
	[ -s "$src/$f" ] || curl -sSfL -o "$src/$f" "https://huggingface.co/minishlab/$name/resolve/main/$f"
done
cargo run --release -q -p build-model -- "$src" "$root/model"
