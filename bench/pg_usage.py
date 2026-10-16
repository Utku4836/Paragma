"""How did agents use memory? Two reports over the runs' transcripts:

1. Per pg subcommand: calls, failures, output size; failed calls; repeats.
2. Per run and conversation: the call at which the scenario's key fact first
   reached the agent, and how many memory lookups it still made after that
   ("lookups after the fact": the agent not trusting what it had). Also
   direct reads of memory files and pg help calls.

    python bench/pg_usage.py <runs-dir> [<runs-dir>...]
"""
import json
import os
import re
import sys
from collections import Counter, defaultdict

PG = re.compile(r"(?:^|[;&|(]\s*)pg(?:\.exe)?\s+([^;&|]+)")

# the fact each scenario hinges on, as it appears in memory output
FACT = {
    "s1-recurring-error": re.compile(r"(?i)line endings|crlf"),
    "s2-continuation": re.compile(r"(?i)before vat|at most 50|50%"),
    "s3-project-rules": re.compile(r"(?i)30 days"),
}

# pg subcommands that look memory up (as opposed to running tests or writing)
LOOKUP = {"search", "read", "peek", "outline", "context", "related", "find", "grep", "links", "backlinks",
          "neighbors", "graph", "recent", "rule list", "error find", "help", "prime", "resume",
          "session resume", "session status", "project status", "project list", "index", "marks", "task list",
          "task show", "task next"}

MEM_PATH = re.compile(r"(?i)([\\/](memory|\.paragma|pghome|spaces)[\\/])")


def tokens(s):
    return (len(s) * 2 + 6) // 7


def subcommand(args):
    w = args.split()
    sub = w[0] if w else ""
    if sub in ("session", "task", "error", "rule", "decision", "project", "branch", "help", "install", "space") and len(w) > 1:
        sub += " " + w[1]
    return sub


def steps(path):
    for line in open(path, encoding="utf-8", errors="replace"):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        su = e.get("step_update") or {}
        if su.get("state") != "DONE" or su.get("step_type") != "tool":
            continue
        info = su.get("tool_info") or {}
        if info.get("name") in ("manage_task", "schedule"):
            continue  # the agent's own background bookkeeping
        yield info


calls = []  # (run, conv, cmdline, sub, out_tokens, failed, out)
effort = []  # per run/conv
for root in sys.argv[1:]:
    for sc in sorted(os.listdir(root)):
        sdir = os.path.join(root, sc)
        if not os.path.isdir(sdir):
            continue
        for run in sorted(os.listdir(sdir)):
            if not os.path.isdir(os.path.join(sdir, run)):
                continue
            for conv in ("conv1.jsonl", "conv2.jsonl"):
                p = os.path.join(sdir, run, conv)
                if not os.path.exists(p):
                    continue
                name = f"{os.path.basename(root)}/{sc}/{run}"
                total = help_calls = mem_files = 0
                fact_at = None
                after = 0
                for i, info in enumerate(steps(p), 1):
                    total += 1
                    params = info.get("parameters") or {}
                    cmd = params.get("CommandLine") or ""
                    out = info.get("output") or ""
                    pgs = [m.group(1).strip() for m in PG.finditer(cmd)]
                    lookup = False
                    for args in pgs:
                        sub = subcommand(args)
                        failed = bool(re.search(r"^error: |exit status [1-9]|no results", out, re.M))
                        calls.append((name, conv, args, sub, tokens(out), failed, out))
                        if sub.startswith("help") or args.endswith("--help"):
                            help_calls += 1
                        if sub in LOOKUP or sub.startswith("help"):
                            lookup = True
                    if not pgs and MEM_PATH.search(json.dumps(params)):
                        mem_files += 1
                        lookup = True
                    if fact_at is not None and lookup:
                        after += 1
                    if fact_at is None and sc in FACT and FACT[sc].search(out):
                        fact_at = i
                effort.append((name, conv, total, fact_at, after, mem_files, help_calls))

by = defaultdict(list)
for c in calls:
    by[c[3]].append(c)
print(f"{len(calls)} pg calls in {len({c[0] for c in calls})} runs\n")
if calls:
    print(f"{'command':<22} {'calls':>5} {'failed':>6} {'avg out tokens':>15} {'max':>6}")
    for sub, cs in sorted(by.items(), key=lambda kv: -len(kv[1])):
        outs = [c[4] for c in cs]
        print(f"{sub:<22} {len(cs):>5} {sum(c[5] for c in cs):>6} {sum(outs) / len(outs):>15.0f} {max(outs):>6}")

    print("\nfailed or empty calls:")
    for c in calls:
        if c[5]:
            first = [l for l in c[6].splitlines() if l.strip()][:2]
            print(f"  {c[0]} {c[1]}: pg {c[2][:90]}\n      -> {' | '.join(first)[:160]}")

    print("\nrepeated identical calls within a conversation:")
    rep = Counter((c[0], c[1], c[2]) for c in calls)
    for (run, conv, args), n in rep.most_common():
        if n > 1:
            print(f"  {n}x {run} {conv}: pg {args[:100]}")

print("\nmemory effort per conversation (tool calls without the agent's task bookkeeping)")
print(f"{'run':<48} {'conv':<6} {'calls':>5} {'fact at':>7} {'lookups after':>13} {'memory files':>12} {'help':>4}")
for name, conv, total, fact_at, after, mem_files, help_calls in effort:
    fa = "-" if fact_at is None else str(fact_at)
    print(f"{name:<48} {conv[:5]:<6} {total:>5} {fa:>7} {after:>13} {mem_files:>12} {help_calls:>4}")
