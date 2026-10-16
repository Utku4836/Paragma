#!/usr/bin/env bash
# Lay out the benchmark memory (bench/memory, written by genmem.py in the
# plain-notes layout) as a Paragma space: <home>/Spaces/main/shopkit/.
#
#   bench/to_space.sh <memory-dir> <paragma-home>
set -eu
M=$1
P=$2/Spaces/main/shopkit
mkdir -p "$P"
cp "$M"/knowledge/*.md "$P/"
cp -r "$M/knowledge/concepts" "$P/concepts"
cp -r "$M/episodes/errors" "$P/errors"
cp -r "$M/episodes/decisions" "$P/decisions"
cp -r "$M/episodes/journal" "$P/journal"
[ -f "$M/work/inbox.md" ] && cp "$M/work/inbox.md" "$P/inbox.md"
if [ -d "$M/work/sessions" ]; then mkdir -p "$P/sessions" && cp -r "$M/work/sessions/." "$P/sessions/"; fi
if [ -d "$M/archive/work/sessions" ]; then mkdir -p "$P/archive/sessions" && cp -r "$M/archive/work/sessions/." "$P/archive/sessions/"; fi
