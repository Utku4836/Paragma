"""Short agent test: real agent, questions answered from memory, no coding.

Each question in bench/questions/ is asked once per condition (plain notes,
Paragma) in an isolated workspace (bench/ask.sh); conversations run in
parallel. The answer is checked against the question's facts; the cost is
tool calls and tokens processed. About ten minutes instead of hours.

    python bench/quick_agent.py [--memory bench/memory-big] [--out bench/quick-runs] [--parallel 4] [--conds files,paragma] [--only refund,tests]
"""
import argparse
import concurrent.futures as cf
import json
import os
import re
import subprocess
import time

HERE = os.path.dirname(os.path.abspath(__file__))

# the facts a correct answer contains (ASCII, case-insensitive)
FACTS = {
    "refund": [r"30.?day", r"ErrRefundWindow", r"audit", r"ErrInvalidAmount|exceed|paid amount"],
    "checksum": [r"CRLF|line ending", r"gitattributes", r"(not|never|don.t).{0,40}(change|update|modify).{0,30}(ExpectedChecksum|checksum)|ExpectedChecksum.{0,60}(LF|CI)"],
    "tests": [r"table", r"t\.Cleanup|pin.{0,30}clock|clock", r"fake|no network", r"regression"],
    "time": [r"clock\.Now", r"UTC", r"Istanbul"],
    "migrations": [r"forward", r"NNNN|numbered|number", r"duplicate|same number|0012"],
}


def metrics(path):
    m = {"calls": 0, "tokens": 0, "answer": "", "status": "missing", "seconds": 0.0}
    if not os.path.exists(path):
        return m
    seen = set()
    for line in open(path, encoding="utf-8", errors="replace"):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if e.get("event") == "result":
            r = e.get("result") or {}
            m["status"] = r.get("status", "?")
            m["answer"] = r.get("response", "") or ""
            continue
        su = e.get("step_update") or {}
        if su.get("state") != "DONE":
            continue
        u = su.get("usage") or {}
        m["tokens"] += u.get("input_tokens", 0) + u.get("cache_read_tokens", 0) + u.get("output_tokens", 0) + u.get("thinking_tokens", 0)
        m["seconds"] += su.get("duration_seconds", 0) or 0
        if su.get("step_type") == "tool":
            info = su.get("tool_info") or {}
            if info.get("name") in ("manage_task", "schedule") or su.get("step_index") in seen:
                continue
            seen.add(su.get("step_index"))
            m["calls"] += 1
    return m


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--memory", default=os.path.join(HERE, "memory-big"))
    ap.add_argument("--out", default=os.path.join(HERE, "quick-runs"))
    ap.add_argument("--parallel", type=int, default=4)
    ap.add_argument("--conds", default="files,paragma")
    ap.add_argument("--only", default="")
    a = ap.parse_args()
    a.out, a.memory = os.path.abspath(a.out), os.path.abspath(a.memory)
    qs = sorted(f[:-4] for f in os.listdir(os.path.join(HERE, "questions")) if f.endswith(".txt"))
    if a.only:
        qs = [q for q in qs if q in a.only.split(",")]
    conds = a.conds.split(",")
    cells = [(q, c) for q in qs for c in conds]

    def run(cell):
        q, c = cell
        time.sleep(8 * (cells.index(cell) % a.parallel))  # staggered starts: parallel logins can fail
        out = os.path.join(a.out, q, c)
        if not os.path.exists(os.path.join(out, "timing.txt")):
            subprocess.run(["bash", os.path.join(HERE, "ask.sh"), c, a.memory, os.path.join(HERE, "questions", q + ".txt"), out])
        return cell

    with cf.ThreadPoolExecutor(a.parallel) as ex:
        for q, c in ex.map(run, cells):
            print(f"done {q} {c}", flush=True)

    print(f"\nmemory: {os.path.basename(a.memory)}\n")
    print("| question | condition | answer facts | tool calls | tokens processed | seconds | files changed |")
    print("|---|---|---|---|---|---|---|")
    tot = {c: [0, 0, 0, 0, 0.0] for c in conds}
    for q in qs:
        for c in conds:
            out = os.path.join(a.out, q, c)
            m = metrics(os.path.join(out, "conv1.jsonl"))
            hit = [bool(re.search(f, m["answer"], re.I | re.S)) for f in FACTS.get(q, [])]
            changed = open(os.path.join(out, "changed.txt"), encoding="utf-8").read().split() if os.path.exists(os.path.join(out, "changed.txt")) else []
            changed = [x for x in changed if x not in ("??", "M", "A", "D")]
            t = tot[c]
            t[0] += sum(hit); t[1] += len(hit); t[2] += m["calls"]; t[3] += m["tokens"]; t[4] += m["seconds"]
            print(f"| {q} | {c} | {sum(hit)}/{len(hit)} | {m['calls']} | {m['tokens']:,} | {m['seconds']:.0f} | {len(changed)} |")
    for c in conds:
        t = tot[c]
        print(f"| **total** | {c} | **{t[0]}/{t[1]}** | **{t[2]}** | **{t[3]:,}** | **{t[4]:.0f}** | |")


main()
