"""Quick retrieval test: does the first memory call answer the question?

No agent, a few seconds. For each question the scenario's facts are known;
the test measures how much reading it takes to have all of them in front of
the agent:

- Paragma: the call an agent makes first (pg context <topic>, or pg error
  find <error line> for an error). Facts found, output tokens, calls (1, or
  2 when a follow-up read is needed).
- Plain notes: what an agent does with Markdown files: grep -rn the key word
  (all matching lines are output), then read the matching files, most hits
  first, until every fact has been seen. Calls = 1 grep + files read.

    python bench/quick.py [memory-dir ...]     (default: bench/memory and, if present, bench/memory-big)
"""
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
PG = os.path.join(HERE, "..", "bin", "pg.exe" if os.name == "nt" else "pg")

# name, Paragma's first call, plain-notes grep word, facts (regexes)
QUESTIONS = [
    ("refund rules (S3)", ["context", "refund"], "refund",
     [r"30 days", r"ErrRefundWindow", r"audit\.Record", r"ErrInvalidAmount|exceed the paid amount"]),
    ("catalog checksum error (S1)", ["error", "find", "catalog_test.go:8: catalog checksum mismatch: got 8c77cb095cbd want 4ff52f25fc9d"], "checksum mismatch",
     [r"(?i)line endings|\\r\\n", r"gitattributes", r"(?i)do not change ExpectedChecksum"]),
    ("database locked error", ["error", "find", "database is locked (5)"], "database is locked",
     [r"WAL", r"busy_timeout"]),
    ("how to write tests", ["context", "writing tests"], "test",
     [r"(?i)table tests", r"t\.Cleanup", r"in-package fakes", r"(?i)regression test"]),
    ("VAT rounding", ["context", "vat rounding"], "rounding",
     [r"(?i)half up", r"5000\)/10000"]),
    ("money amounts", ["context", "money amounts"], "kuruş",
     [r"int64", r"float64"]),
    ("discount codes", ["context", "discount codes"], "discount",
     [r"(?i)server side"]),
    ("payment provider", ["context", "payment provider"], "provider",
     [r"Iyzico", r"(?i)fake"]),
    ("time zones", ["context", "time zone utc"], "UTC",
     [r"Europe/Istanbul", r"clock\.Now"]),
    ("migrations", ["context", "migrations"], "migration",
     [r"(?i)forward only", r"(?i)duplicate migration"]),
]


def tokens(s):
    return (len(s) * 2 + 6) // 7


def found(text, facts):
    return [bool(re.search(f, text)) for f in facts]


def paragma(home, args, facts):
    env = dict(os.environ, PARAGMA_HOME=home)
    env.pop("PARAGMA_PROJECT", None)
    subprocess.run([PG, "prime"], env=env, capture_output=True)  # a new conversation
    out = subprocess.run([PG] + args, env=env, capture_output=True, text=True, encoding="utf-8").stdout
    return found(out, facts), tokens(out), 1


def plain(mem, word, facts):
    lines, hits = [], {}
    for root, _, files in os.walk(mem):
        for fn in files:
            if not fn.endswith(".md"):
                continue
            p = os.path.join(root, fn)
            text = open(p, encoding="utf-8").read()
            for i, l in enumerate(text.splitlines(), 1):
                if word.lower() in l.lower():
                    rel = os.path.relpath(p, mem).replace("\\", "/")
                    lines.append(f"{rel}:{i}:{l}")
                    hits[p] = hits.get(p, 0) + 1
    seen = "\n".join(lines)
    cost, calls = tokens(seen), 1
    have = found(seen, facts)
    for p in sorted(hits, key=lambda p: -hits[p]):
        if all(have):
            break
        text = open(p, encoding="utf-8").read()
        cost += tokens(text)
        calls += 1
        have = [h or g for h, g in zip(have, found(text, facts))]
    return have, cost, calls


def main():
    mems = sys.argv[1:] or [m for m in (os.path.join(HERE, "memory"), os.path.join(HERE, "memory-big")) if os.path.isdir(m)]
    for mem in mems:
        home = tempfile.mkdtemp(prefix="pgq-")
        subprocess.run(["bash", os.path.join(HERE, "to_space.sh"), mem, home], check=True)
        n = sum(1 for _, _, fs in os.walk(mem) for f in fs if f.endswith(".md"))
        print(f"\n## {os.path.basename(mem)} ({n} notes)\n")
        print(f"| question | Paragma facts | tokens | calls | plain notes facts | tokens | calls |")
        print(f"|---|---|---|---|---|---|---|")
        tot = {"pf": 0, "pt": 0, "pc": 0, "nf": 0, "nt": 0, "nc": 0, "all": 0}
        for name, args, word, facts in QUESTIONS:
            pf, pt, pc = paragma(home, args, facts)
            nf, nt, nc = plain(mem, word, facts)
            tot["pf"] += sum(pf); tot["pt"] += pt; tot["pc"] += pc
            tot["nf"] += sum(nf); tot["nt"] += nt; tot["nc"] += nc
            tot["all"] += len(facts)
            miss = lambda f, h: "" if all(h) else " (missing: " + ", ".join(x for x, y in zip(f, h) if not y) + ")"
            print(f"| {name} | {sum(pf)}/{len(facts)}{miss(facts, pf)} | {pt:,} | {pc} | {sum(nf)}/{len(facts)}{miss(facts, nf)} | {nt:,} | {nc} |")
        print(f"| **total** | **{tot['pf']}/{tot['all']}** | **{tot['pt']:,}** | **{tot['pc']}** | **{tot['nf']}/{tot['all']}** | **{tot['nt']:,}** | **{tot['nc']}** |")
        shutil.rmtree(home, ignore_errors=True)


main()
