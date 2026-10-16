"""Grow the benchmark memory about ten times, for the large-memory run.

Runs genmem.py into <out-dir>, then adds about 1,400 background notes: errors,
decisions, concepts, archived sessions and journal months from a busy year of
shopkit. Many of them mention the scenarios' words (refund, payment,
checksum, line endings, discount) in other contexts, as a real memory would,
so finding the one note that matters takes more than a grep. The notes the
scenarios need are the same as in the small memory.

Usage: python bench/genmem_big.py <out-dir>
"""
import os
import random
import subprocess
import sys

OUT = sys.argv[1]
HERE = os.path.dirname(os.path.abspath(__file__))
subprocess.run([sys.executable, os.path.join(HERE, "genmem.py"), OUT], check=True)
random.seed(11)


def write(rel, text):
    p = os.path.join(OUT, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w", encoding="utf-8", newline="\n") as f:
        f.write(text.strip() + "\n")


PKGS = ["cart", "pricing", "payments", "catalog", "audit", "orders", "shipping", "invoice", "stock", "search",
        "images", "webhook", "admin", "export", "notify", "auth", "metrics", "nightly", "reports", "returns"]
FILES = {p: [f"{p}/{p}.go", f"{p}/{p}_test.go", f"{p}/store.go", f"{p}/handler.go"] for p in PKGS}

# error templates: symptom, cause, fix. {p} package, {w} a scenario word used in passing
ERRORS = [
    ("{P} report totals off by one kuruş", "The {p} report summed rounded line totals.", "Sum raw amounts, round once at the end."),
    ("{P} CSV export opens as one line in Excel", "The export wrote LF line endings and Excel on Windows expected CRLF.", "Write CRLF in the CSV writer only; keep LF everywhere else."),
    ("Flaky {p} test at midnight", "The test used the wall clock and crossed a day boundary.", "Pin the clock in the test with clock.Set and t.Cleanup."),
    ("{P} handler returns 500 on empty body", "json.Decoder returned io.EOF and the handler did not map it.", "Map io.EOF to 400 with a clear message."),
    ("{P} cache serves stale data after deploy", "The cache key did not include the schema version.", "Add the schema version to the cache key."),
    ("Duplicate {p} rows after retry", "The retry path inserted again instead of upserting.", "Use the idempotency key and an upsert."),
    ("{P} migration fails on fresh database", "The migration referenced a column created in a later file.", "Reorder: create the column first."),
    ("{P} checksum differs between CI and laptops", "Generated files embedded the build time.", "Strip the timestamp before hashing the generated file."),
    ("{P} refund-like reversal posted twice", "The reversal job ran on two nodes.", "Take the nightly lock before running the job."),
    ("{P} payment status stuck in pending", "The provider callback was rejected by the signature check after key rotation.", "Restart after rotating secrets; log the key id at startup."),
    ("{P} discount badge shows wrong percent", "The template divided integers.", "Format the percent from the stored basis points."),
    ("{P} list endpoint is slow", "An N+1 query loaded items one by one.", "Load items with one IN query."),
    ("{P} timezone shown as UTC to users", "The view did not convert to Europe/Istanbul.", "Convert only in the view layer."),
    ("{P} file upload truncated", "The request body limit was 1 MB.", "Raise the limit for this route to 10 MB."),
    ("{P} job leaks goroutines", "A ticker was never stopped.", "Stop the ticker with defer."),
]
DEC = [
    ("Use a queue for {p} side effects", "Synchronous side effects slowed the {p} handlers.", "Side effects of {p} go through the outbox table and a worker.", "Inline calls: kept failing under load."),
    ("Cursor pagination for {p}", "Offset pagination of {p} lists got slow past page 50.", "List endpoints of {p} use opaque cursors.", "Offsets: too slow."),
    ("{P} keeps its own read model", "Joins across packages made {p} queries slow.", "{P} maintains a denormalised read table updated by events.", "Views: hard to index."),
    ("Soft delete for {p}", "Support needed to restore deleted {p} records.", "{P} rows get deleted_at instead of being removed.", "Hard delete plus backups: restores took hours."),
    ("Rate limit for {p} endpoints", "Bots hammered the {p} endpoints.", "Token bucket per API key, 20 req/s for {p}.", "IP limits: shared NATs."),
]
CONCEPTS = [
    ("{p} lifecycle", "A {p} record goes through draft, active and archived. Only active records are visible to customers; archived ones stay for reports."),
    ("{p} events", "Every change to {p} emits an event into the outbox table; workers fan them out to search, notify and reports."),
    ("{p} ownership", "The {p} package is owned by the commerce team; changes to its public API need a review from them."),
]

n = 0
# ~900 more errors
for i in range(900):
    p = random.choice(PKGS)
    title, cause, fix = random.choice(ERRORS)
    title = title.format(p=p, P=p.capitalize())
    slug = f"{p}-{title.lower().split()[1] if len(title.split()) > 1 else 'issue'}-{i:03d}".replace(" ", "-")
    slug = "".join(ch if ch.isalnum() or ch == "-" else "-" for ch in slug)
    f = random.choice(FILES[p])
    seen = random.choice([1, 1, 1, 2, 3])
    month = random.randint(1, 9)
    write(f"episodes/errors/{slug}.md", f"""
---
type: error
layer: episodic
status: resolved
title: {title}
sig: "{p}: {title.lower()}"
seen: {seen}
tags: [{p}]
created: 2026-{month:02d}-{random.randint(1, 28):02d}
---
# {title}

## Symptom
{title}. Seen in {random.choice(['staging', 'production', 'CI', 'a developer laptop'])}.

## Root cause
{cause.format(p=p, P=p.capitalize())}

## Fix
{fix.format(p=p, P=p.capitalize())}

## Files
- {f}
""")
    n += 1

# ~250 more decisions (numbers after the existing ones)
for i in range(250):
    p = random.choice(PKGS)
    t, ctx, dec, alt = random.choice(DEC)
    num = 100 + i
    t = t.format(p=p, P=p.capitalize())
    slug = f"{num:04d}-" + "".join(ch if ch.isalnum() else "-" for ch in t.lower()).strip("-")
    write(f"episodes/decisions/{slug}.md", f"""
---
type: decision
layer: episodic
status: active
title: "{num:04d}: {t}"
created: 2026-0{random.randint(1, 9)}-{random.randint(10, 28)}
---
# {num:04d}: {t}

## Context
{ctx.format(p=p, P=p.capitalize())}

## Decision
{dec.format(p=p, P=p.capitalize())}

## Alternatives
- {alt}

## Consequences
Applies to the {p} package; reviewers check it.
""")
    n += 1

# ~60 concepts
for p in PKGS:
    for t, text in CONCEPTS:
        t = t.format(p=p)
        slug = "".join(ch if ch.isalnum() else "-" for ch in t.lower())
        write(f"knowledge/concepts/{slug}.md", f"""
---
type: concept
layer: semantic
title: {t.capitalize()}
verified: 2026-08-01
---
# {t.capitalize()}

{text.format(p=p)}
""")
        n += 1

# ~180 archived sessions
for i in range(180):
    p = random.choice(PKGS)
    topic = random.choice(["cleanup", "speed-up", "new endpoint", "bug hunt", "refactor", "export", "tests", "docs"])
    d = f"2026-0{random.randint(1, 9)}-{random.randint(10, 28)}"
    tasks = "\n".join(f"- [x] {s} ^tb{i:03d}{j}" for j, s in enumerate(["Investigate", "Change", "Tests", "Docs"]))
    write(f"archive/work/sessions/{d}-{p}-{topic.replace(' ', '-')}-{i:03d}.md", f"""
---
type: session
layer: working
status: closed
title: {p.capitalize()} {topic}
created: {d}
---
# {p.capitalize()} {topic}

## Goal
{topic.capitalize()} in the {p} package.

## Plan
{tasks}

## Notes
- touched {random.choice(FILES[p])}; payment, refund and discount code untouched

## Handoff
Done; nothing open.
""")
    n += 1

print(f"added {n} notes")
