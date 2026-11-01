"""A rehearsal of the agent tests without a model: on the same fixture,
memory and tasks as bench/agent_eval.py, plays the pg calls a good agent
would make and checks that their output holds what the agent needs, with
how many calls and tokens it took. Cheap and repeatable: run it before
spending a model's quota.

    python bench/rehearsal.py [--show]      (--show prints every output)

Needs Go on PATH for the recurring-error scenario.
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
EXE = ".exe" if os.name == "nt" else ""
PG = os.path.join(ROOT, "target", "release", "pg" + EXE)
sys.path.insert(0, HERE)
import to_v2  # noqa: E402

# the questions of agent_eval.py, asked the way its instructions tell an agent to
QUESTIONS = {
    "refund": ("refund rules and decisions", [r"30.?day", r"ErrRefundWindow", r"audit", r"ErrInvalidAmount|exceed|paid amount"]),
    "checksum": ("catalog_test.go:8: catalog checksum mismatch: got 8c77cb095cbd want 4ff52f25fc9d",
                 [r"CRLF|line ending", r"gitattributes", r"ExpectedChecksum"]),
    "tests": ("conventions for writing tests", [r"table", r"t\.Cleanup|clock", r"fake|network", r"regression"]),
    "time": ("time and time zones", [r"clock\.Now", r"UTC", r"Istanbul"]),
    "migrations": ("database migrations rules and past problems", [r"forward", r"numbered|NNNN|number", r"duplicate|0012"]),
}


def tokens(s):
    return (len(s) * 2 + 6) // 7


class Agent:
    """One conversation: its pg calls and what they printed."""

    def __init__(self, home, repo, name):
        self.env = dict(os.environ, PARAGMA_HOME=home, PARAGMA_SESSION=name, NO_COLOR="1")
        self.env.pop("PARAGMA_PROJECT", None)
        self.repo, self.calls, self.tokens, self.out = repo, 0, 0, []

    def pg(self, *args):
        r = subprocess.run([PG, *args], cwd=self.repo, env=self.env, capture_output=True, text=True, encoding="utf-8")
        text = r.stdout + r.stderr
        self.calls += 1
        self.tokens += tokens(text)
        self.out.append(f"$ pg {' '.join(args)}\n{text}")
        return text


def workspace(memory):
    ws = tempfile.mkdtemp(prefix="pgreh-")
    repo, home = os.path.join(ws, "repo"), os.path.join(ws, "home")
    shutil.copytree(os.path.join(HERE, "fixture", "shopkit"), repo)
    if memory:
        to_v2.convert(memory, home, "shopkit")
    git = ["git", "-c", "core.autocrlf=false", "-c", "user.name=b", "-c", "user.email=b@b"]
    subprocess.run(["git", "init", "-q"], cwd=repo, check=True)
    subprocess.run(git + ["add", "-A"], cwd=repo, check=True)
    subprocess.run(git + ["commit", "-qm", "initial"], cwd=repo, check=True)
    return ws, repo, home


def check(name, text, facts):
    hit = [bool(re.search(f, text, re.I | re.S)) for f in facts]
    missing = [f for f, h in zip(facts, hit) if not h]
    return sum(hit), len(facts), missing


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--show", action="store_true")
    a = ap.parse_args()
    big = os.path.join(HERE, "memory-big")
    rows = []

    # questions: one search each
    for q, (ask, facts) in QUESTIONS.items():
        ws, repo, home = workspace(big)
        ag = Agent(home, repo, "q")
        text = ag.pg("find", "--in", "shopkit", ask)
        rows.append((f"q:{q}", *check(q, text, facts), ag.calls, ag.tokens, ag.out))
        shutil.rmtree(ws, ignore_errors=True)

    # s1: the recurring error. open, run the tests: the known fix must show
    ws, repo, home = workspace(big)
    f = os.path.join(repo, "catalog", "testdata", "catalog.json")
    data = open(f, "rb").read().replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
    open(f, "wb").write(data)
    ag = Agent(home, repo, "s1")
    brief = ag.pg("open", "shopkit", "--goal", "go test ./... fails: find the cause and fix it properly")
    run = ag.pg("run", "--", "go test ./...")
    rows.append(("s1 recurring error", *check("s1", brief + run, [r"CRLF|line ending", r"gitattributes", r"known: e"]),
                 ag.calls, ag.tokens, ag.out))
    shutil.rmtree(ws, ignore_errors=True)

    # s2: continuation. The first conversation records the plan and decisions,
    # the second must get them back from pg open alone
    ws, repo, home = workspace(None)
    a1 = Agent(home, repo, "s2a")
    a1.pg("open", "shop-discounts", "--goal", "Discounts in the cart",
          "--step", "(a) pricing/discount.go: Percent(p int), Fixed(amount int64), Amount(net int64) int64",
          "--step", "(b) cart: field Discount *pricing.Discount, applied in Total()",
          "--step", "(c) table tests for (a) and (b)", "--step", "(d) README section about discounts")
    for d in ["Two kinds: a percentage discount and a fixed amount discount in kuruş",
              "A percentage discount can be at most 50%", "The discount is applied to the net subtotal before VAT",
              "The cart total never goes below zero"]:
        a1.pg("write", "--decision", d)
    a1.pg("finish", "Step (a) done: pricing/discount.go with Percent, Fixed, Amount. Next: (b), (c), (d).", "--done", "1")
    a2 = Agent(home, repo, "s2b")
    brief = a2.pg("open")
    rows.append(("s2 continuation (2nd)", *check("s2", brief, [r"\(b\) cart", r"\(c\) table tests", r"\(d\) README", r"50%",
                                                                r"before VAT", r"below zero", r"Step \(a\) done"]),
                 a1.calls + a2.calls, a1.tokens + a2.tokens, a1.out + a2.out))
    shutil.rmtree(ws, ignore_errors=True)

    # s3: project rules. The brief for the refund task must carry the refund rules
    ws, repo, home = workspace(big)
    ag = Agent(home, repo, "s3")
    brief = ag.pg("open", "shopkit", "--goal", "Add refund support to the payments package: func Refund(ctx, p Provider, pay *Payment, amount int64) error")
    rows.append(("s3 project rules", *check("s3", brief, [r"30.?day", r"ErrRefundWindow", r"audit", r"exceed|paid amount|ErrInvalidAmount",
                                                          r"int64", r"table"]),
                 ag.calls, ag.tokens, ag.out))
    shutil.rmtree(ws, ignore_errors=True)

    print("| task | facts | pg calls | tokens read | missing |")
    print("|---|---|---|---|---|")
    for name, got, total, missing, calls, toks, _ in rows:
        print(f"| {name} | {got}/{total} | {calls} | {toks:,} | {', '.join(missing)} |")
    if a.show:
        for name, *_, out in rows:
            print(f"\n===== {name}")
            print("\n".join(out))


if __name__ == "__main__":
    main()
