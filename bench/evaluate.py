"""Score benchmark runs and write results.

    python bench/evaluate.py <runs-dir> [results.md]

For every run directory (<scenario>/<condition>-<rep>) it reads the agent's
stream-json logs, the hidden test results and a few file checks, and prints
a table per scenario plus per-condition averages.
"""
import json
import os
import re
import sys
from collections import defaultdict

RUNS = sys.argv[1]
OUT = sys.argv[2] if len(sys.argv) > 2 else None

HIDDEN_TOTAL = {"s1-recurring-error": 4, "s2-continuation": 4, "s3-project-rules": 6}


WINPATH = re.compile(r"[A-Za-z]:(?:\\|\|/)[^\s\"'|;,)]*")
ALLOWED_OUTSIDE = ("c:/program files/go", "c:/program")  # the Go toolchain; paths are cut at spaces


def outside_paths(text, ws):
    """Absolute paths in a tool call that are outside the run's workspace."""
    out = []
    for m in WINPATH.findall(text):
        p = m.replace("\\\\", "/").replace("\\", "/").lower()
        if ws and p.startswith(ws):
            continue
        if p.startswith(ALLOWED_OUTSIDE):
            continue
        out.append(p)
    return out


def conv_metrics(path, ws=""):
    m = dict(steps=0, tools=0, input=0, cached=0, output=0, thinking=0, seconds=0.0,
             pg=0, mem_reads=0, status="missing", asked=False, outside=0)
    if not os.path.exists(path):
        return m
    seen_tools = set()
    for line in open(path, encoding="utf-8", errors="replace"):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        if e.get("event") == "result":
            r = e["result"]
            m["status"] = r.get("status", "?")
            resp = r.get("response", "") or ""
            m["asked"] = resp.rstrip().endswith("?")
            continue
        su = e.get("step_update")
        if not su or su.get("state") != "DONE":
            continue
        u = su.get("usage") or {}
        m["input"] += u.get("input_tokens", 0)
        m["cached"] += u.get("cache_read_tokens", 0)
        m["output"] += u.get("output_tokens", 0)
        m["thinking"] += u.get("thinking_tokens", 0)
        m["seconds"] += su.get("duration_seconds", 0) or 0
        m["steps"] += 1
        if su.get("step_type") == "tool":
            key = su.get("step_index")
            if key in seen_tools:
                continue
            seen_tools.add(key)
            m["tools"] += 1
            info = su.get("tool_info") or {}
            params = json.dumps(info.get("parameters", {}), ensure_ascii=False)
            cmd = (info.get("parameters") or {}).get("CommandLine", "") or ""
            if info.get("name") == "manage_task":
                continue  # the agent's own background tasks; paths point into its own profile
            seen_text = (params + " " + (info.get("output") or "")).replace("\\\\", "/").replace("\\", "/").lower()
            own = ws.rsplit("/", 1)[-1] if ws else ""
            param_text = params.replace("\\\\", "/").replace("\\", "/").lower()
            foreign_ws = [w for w in re.findall(r"/(ws-[a-z0-9]{8})", param_text) if w != own]
            if outside_paths(params, ws) or "paragma/bench" in seen_text or foreign_ws or re.search(r"c:/users/[^/]+/.gemini/", seen_text):
                m["outside"] += 1
            if re.search(r"(^|[\s;&|(])pg(\.exe)?\s", cmd + " "):
                m["pg"] += 1
            elif "memory" in params.replace("\\\\", "/") or ".paragma" in params:
                m["mem_reads"] += 1
    return m


def hidden_results(path):
    passed, failed = set(), set()
    if not os.path.exists(path):
        return passed, failed
    for line in open(path, encoding="utf-8", errors="replace"):
        try:
            e = json.loads(line)
        except ValueError:
            continue
        t = e.get("Test", "")
        if not t.startswith("TestHidden_"):
            continue
        if e.get("Action") == "pass":
            passed.add(t)
        elif e.get("Action") == "fail":
            failed.add(t)
    return passed, failed


def extra_checks(scenario, run):
    """Scenario checks that are not Go tests. Returns {name: bool}."""
    repo = os.path.join(run, "repo")
    own = open(os.path.join(run, "own_tests.txt"), encoding="utf-8", errors="replace").read() if os.path.exists(os.path.join(run, "own_tests.txt")) else ""
    checks = {"own tests pass": own.strip().endswith("exit 0")}
    if scenario == "s2-continuation":
        readme = open(os.path.join(repo, "README.md"), encoding="utf-8", errors="replace").read().lower()
        checks["README discount section"] = "discount" in readme
        tests = ""
        for d in ("cart", "pricing"):
            for f in os.listdir(os.path.join(repo, d)):
                if f.endswith("_test.go"):
                    tests += open(os.path.join(repo, d, f), encoding="utf-8", errors="replace").read()
        checks["discount tests written"] = "Discount" in tests or "Percent(" in tests
        cart = open(os.path.join(repo, "cart", "cart.go"), encoding="utf-8", errors="replace").read()
        checks["API as specified"] = re.search(r"Discount\s+\*pricing\.Discount", cart) is not None
    return checks


rows = []
for scenario in sorted(os.listdir(RUNS)):
    sdir = os.path.join(RUNS, scenario)
    if not os.path.isdir(sdir):
        continue
    for run in sorted(os.listdir(sdir)):
        rdir = os.path.join(sdir, run)
        if not os.path.isdir(rdir) or "-" not in run:
            continue
        cond, rep = run.rsplit("-", 1)
        if not os.path.exists(os.path.join(rdir, "repo", "go.mod")):
            continue  # unfinished run (the repo copy is the last step)
        ws = ""
        wsf = os.path.join(rdir, "workspace.txt")
        if os.path.exists(wsf):
            w = open(wsf, encoding="utf-8").read().strip()
            if w.startswith("/") and len(w) > 2 and w[2] == "/":
                w = w[1] + ":" + w[2:]
            ws = w.replace("\\", "/").lower()
        convs = [conv_metrics(os.path.join(rdir, f"conv{i}.jsonl"), ws) for i in (1, 2)]
        convs = [c for c in convs if c["status"] != "missing"]
        tot = defaultdict(float)
        for c in convs:
            for k in ("steps", "tools", "input", "cached", "output", "thinking", "seconds", "pg", "mem_reads", "outside"):
                tot[k] += c[k]
        passed, failed = hidden_results(os.path.join(rdir, "hidden.json"))
        extras = extra_checks(scenario, rdir)
        total_hidden = HIDDEN_TOTAL.get(scenario, len(passed | failed))
        score_items = len(passed) + sum(extras.values())
        score_total = total_hidden + len(extras)
        rows.append(dict(scenario=scenario, cond=cond, rep=rep, convs=len(convs),
                         status="/".join(c["status"] for c in convs),
                         hidden=f"{len(passed)}/{total_hidden}", passed=sorted(passed),
                         extras=extras, score=score_items / score_total if score_total else 0,
                         tokens=int(tot["input"] + tot["cached"] + tot["output"] + tot["thinking"]),
                         uncached=int(tot["input"] + tot["output"] + tot["thinking"]),
                         **{k: tot[k] for k in ("steps", "tools", "seconds", "pg", "mem_reads", "outside")},
                         last_conv_tools=int(convs[-1]["tools"]) if convs else 0))

order = {"none": 0, "files": 1, "paragma": 2}
lines = []
p = lines.append
p("| scenario | condition | runs | score (mean) | full pass | tool calls | last conversation tool calls | tokens processed | uncached tokens | wall s | pg calls | memory file reads | outside-workspace calls |")
p("|---|---|---|---|---|---|---|---|---|---|---|---|---|")
agg = defaultdict(list)
for r in rows:
    agg[(r["scenario"], r["cond"])].append(r)


def mean(xs):
    return sum(xs) / len(xs) if xs else 0


for (sc, cond), rs in sorted(agg.items(), key=lambda kv: (kv[0][0], order.get(kv[0][1], 9))):
    full = sum(1 for r in rs if abs(r["score"] - 1) < 1e-9)
    p(f"| {sc} | {cond} | {len(rs)} | {mean([r['score'] for r in rs]):.0%} | {full}/{len(rs)} | "
      f"{mean([r['tools'] for r in rs]):.1f} | {mean([r['last_conv_tools'] for r in rs]):.1f} | {mean([r['tokens'] for r in rs]):,.0f} | {mean([r['uncached'] for r in rs]):,.0f} | "
      f"{mean([r['seconds'] for r in rs]):.0f} | {mean([r['pg'] for r in rs]):.1f} | {mean([r['mem_reads'] for r in rs]):.1f} | {sum(r['outside'] for r in rs):.0f} |")
p("")
p("Per run:")
p("")
p("| run | status | hidden tests | other checks | score | tool calls | tokens |")
p("|---|---|---|---|---|---|---|")
for r in sorted(rows, key=lambda r: (r["scenario"], order.get(r["cond"], 9), r["rep"])):
    ex = ", ".join(f"{k}: {'yes' if v else 'NO'}" for k, v in r["extras"].items())
    p(f"| {r['scenario']}/{r['cond']}-{r['rep']} | {r['status']} | {r['hidden']} | {ex} | {r['score']:.0%} | {r['tools']:.0f} | {r['tokens']:,} |")
text = "\n".join(lines)
print(text)
if OUT:
    with open(OUT, "w", encoding="utf-8") as f:
        f.write(text + "\n")
with open(os.path.join(RUNS, "rows.json"), "w", encoding="utf-8") as f:
    json.dump(rows, f, indent=1, default=str)
