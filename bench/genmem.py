"""Generate the shared project memory used by the benchmark.

The memory describes the fictional `shopkit` project after several months of
work: rules, architecture, ~40 recorded errors, ~20 decisions, concepts,
archived sessions and a journal. A handful of notes matter for the benchmark
scenarios; the rest is realistic background an agent has to sift through.

Usage: python bench/genmem.py <out-dir>   (writes <out-dir>/... as a .paragma tree)
"""
import os
import random
import sys

random.seed(7)
OUT = sys.argv[1]


def write(rel, text):
    p = os.path.join(OUT, rel)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w", encoding="utf-8", newline="\n") as f:
        f.write(text.strip() + "\n")


def fm(**kv):
    lines = ["---"]
    for k, v in kv.items():
        if isinstance(v, list):
            v = "[" + ", ".join(v) + "]"
        lines.append(f"{k}: {v}")
    lines.append("---")
    return "\n".join(lines)


# ---------------------------------------------------------------- config
write("config.toml", 'version = 1\nbudget = 800\nread_limit = 1500\nstale_days = 90\nhints = true\nlang = "en"\n')
write(".gitignore", "index.db\nindex.db-*\nlock\nstate.json\n")

# ---------------------------------------------------------------- rules
rules = {
    "Money": [
        "Amounts are int64 kuruş everywhere. Never float64 for money, not even in tests.",
        "Rounding is half up, done once, at the last step (see pricing.WithTax).",
        "Refunds are allowed only within 30 days of the payment (payment.At). Past that, return payments.ErrRefundWindow. Decided in [[0014-refund-window]].",
        "Partial refunds are allowed; the sum of refunds must never exceed the paid amount (return ErrInvalidAmount).",
        "Every money movement (charge, refund, payout) calls audit.Record(ctx, action, paymentID) after the provider call succeeds. The action names are \"charge\", \"refund\", \"payout\".",
        "Discount codes are validated server side only; the client never computes prices.",
    ],
    "Time": [
        "Never call time.Now() in library code. Use clock.Now() so tests can pin time.",
        "Store and compare times in UTC; convert to Europe/Istanbul only for display.",
        "Durations in config are Go duration strings (\"30s\"), never bare integers.",
    ],
    "Errors": [
        "Wrap errors with the operation name: fmt.Errorf(\"refund: %w\", err).",
        "Sentinel errors are exported vars named ErrXxx in the package that returns them.",
        "Do not log and return the same error; return it, the caller logs.",
        "Provider errors are wrapped, never compared by string.",
    ],
    "Testing": [
        "Table tests for anything with more than two cases.",
        "Tests that depend on time pin clock.Now and restore it with t.Cleanup.",
        "No network in unit tests; payment providers are faked with in-package fakes.",
        "Golden files live in testdata/ and are compared after normalising line endings.",
        "Every bug fix lands with a regression test named after the symptom.",
    ],
    "Code style": [
        "Package names are singular and short: cart, not carts.",
        "Exported functions have a doc comment starting with the name.",
        "No init() functions except for registering metrics.",
        "Keep handlers thin: validation and mapping only, logic in packages.",
        "Configuration is read once in main and passed down; no global config reads.",
    ],
    "Data": [
        "Migrations are forward only and live in migrations/NNNN_name.sql.",
        "Every table has created_at and updated_at in UTC.",
        "SQLite is opened with WAL and busy_timeout=5000 (see [[sqlite-database-locked]]).",
        "IDs are ULIDs as text, never autoincrement integers in APIs.",
    ],
    "Ops": [
        "Feature flags live in config/flags.yaml; removing a flag needs a cleanup task.",
        "Secrets come from the environment only; never commit .env files.",
        "Releases are tagged vMAJOR.MINOR.PATCH from main after CI is green.",
        "Logs are JSON lines; never print request bodies (PII).",
    ],
}
body = [fm(id="01SHOPRULES0000000000000AA", type="rule", layer="semantic", title="Rules", verified="2026-09-28"), "# Rules", "", "Project rules. One rule per callout; follow them unless the user says otherwise."]
n = 1
for sec, rs in rules.items():
    body.append(f"\n## {sec}\n")
    for r in rs:
        body.append(f"> [!rule] {r} ^r{n:03d}\n")
        n += 1
write("knowledge/rules.md", "\n".join(body))

write("knowledge/architecture.md", f"""
{fm(id="01SHOPARCH00000000000000AA", type="note", layer="semantic", title="Architecture", verified="2026-09-20", tags=["architecture"])}
# Architecture

shopkit is a library used by the shop API service and the admin tools. It has no HTTP code of its own.

## Packages

- catalog: loads the product catalog JSON and verifies it against a pinned sha256.
- cart: cart lines, subtotal and total (VAT included).
- pricing: tax and price arithmetic in kuruş.
- payments: charges and refunds through a Provider interface (Iyzico in production, fakes in tests).
- audit: append-only log of money movements, shipped to the data warehouse nightly.
- clock: the replaceable time source.

## Money flow

Cart.Total -> payments.Charge -> provider -> audit.Record("charge"). Refunds go the other way and are audited as "refund".

## Catalog

The catalog file is generated by the merchandising tool and committed to the repo. Its checksum is pinned in catalog.ExpectedChecksum so that a truncated or edited file is caught at startup. The checksum is computed over the file with LF line endings, which is what CI (Linux) checks out.

## Deployment

The API service embeds shopkit and runs on two VMs behind HAProxy. Config comes from environment variables.
""")

write("knowledge/stack.md", f"""
{fm(id="01SHOPSTACK0000000000000AA", type="note", layer="semantic", title="Stack", verified="2026-08-30")}
# Stack

## Language
Go 1.24, standard library first. No ORM.

## Storage
SQLite (modernc, no cgo) for the admin tools, PostgreSQL 16 for the API service.

## CI
GitHub Actions on ubuntu-latest: go vet, go test -race ./..., golangci-lint. Developers work on Windows and macOS.

## Payments
Iyzico in production; the Provider interface keeps it swappable.
""")

concepts = {
    "kurus-amounts": ("Kuruş amounts", "All money is stored as int64 kuruş (1 TL = 100 kuruş). This avoids float rounding and makes sums exact. Display code divides by 100 and formats with two decimals."),
    "vat": ("VAT", "Turkey's standard VAT is 20%. pricing.TaxRateBP holds it in basis points. Some categories (books) are 10%; per-category rates are not implemented yet."),
    "refund-window": ("Refund window", "A refund is accepted only within 30 days of the original payment, measured from payment.At with clock.Now(). The provider itself allows 180 days, but finance closes the books monthly and decided on 30 days."),
    "audit-log": ("Audit log", "audit.Record keeps an append-only list of money movements. Finance reconciles it against provider reports every night; a missing entry shows up as a reconciliation break."),
    "idempotency": ("Idempotency keys", "Charges carry an idempotency key derived from the cart ID so that a retried request does not charge twice. Refunds use the payment ID plus a refund sequence number."),
    "catalog-checksum": ("Catalog checksum", "The catalog checksum protects against partially deployed catalog files. It is pinned in code and must match the LF version of the file."),
    "feature-flags": ("Feature flags", "Flags are booleans in config/flags.yaml read at startup. A flag that has been on for 30 days gets a cleanup task."),
    "ulid": ("ULIDs", "IDs are ULIDs: sortable by time, URL safe, 26 characters. They are generated in Go, never in the database."),
    "fake-providers": ("Fake providers", "Tests use in-package fakes of the payment Provider that record calls. They never hit the network."),
    "clock-pinning": ("Clock pinning", "Tests set clock.Now = func() time.Time { return fixed } and restore the previous function with t.Cleanup."),
    "haproxy": ("HAProxy setup", "Two API VMs sit behind HAProxy with health checks on /healthz every 2s. Draining a node takes 30s."),
    "nightly-export": ("Nightly export", "A cron job exports audit entries and orders to the warehouse at 03:00 Istanbul time."),
    "discount-codes": ("Discount codes", "Marketing creates discount codes in the admin tool. Codes are case insensitive and single use per customer."),
    "currency": ("Currency", "Only TRY is supported. Multi-currency was discussed and postponed (see decisions)."),
    "rate-limits": ("Rate limits", "The provider allows 50 requests per second per merchant; the client retries 429 with exponential backoff up to 3 times."),
}
for slug, (title, text) in concepts.items():
    write(f"knowledge/concepts/{slug}.md", f"{fm(id='01SHOPC' + slug.upper().replace('-', '')[:19].ljust(19, '0'), type='concept', layer='semantic', title=title, verified='2026-09-01')}\n# {title}\n\n{text}\n")

# ---------------------------------------------------------------- errors
errors = [
    # (slug, title, sig, symptom, cause, fix, files, seen, tags)
    ("catalog-checksum-crlf", "Catalog checksum mismatch on Windows", "catalog checksum mismatch: got <hex> want <hex>",
     "`go test ./catalog` fails with `catalog checksum mismatch` on Windows machines only; CI on Linux is green.",
     "Git on Windows (core.autocrlf=true) checks out testdata/catalog.json with CRLF line endings. The pinned checksum is computed over the LF file, so the bytes differ.",
     "Normalise line endings in catalog.Load before hashing and decoding: `b = bytes.ReplaceAll(b, []byte(\"\\r\\n\"), []byte(\"\\n\"))`. Also add `testdata/** -text` to .gitattributes. Do NOT change ExpectedChecksum: CI computes it over the LF file and would break.",
     ["catalog/catalog.go", ".gitattributes"], 2, ["catalog", "windows", "tests"]),
    ("sqlite-database-locked", "SQLite database is locked", "database is locked (5)",
     "Parallel admin tool tests fail with `database is locked (5)`.",
     "WAL was off; readers blocked during writes.", "Open with `PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;`.", ["admin/db/open.go"], 3, ["sqlite"]),
    ("iyzico-signature-invalid", "Provider signature invalid", "iyzico: invalid signature",
     "Charges fail in staging with `invalid signature` after the key rotation.", "The staging secret in the environment still had the old value; the API caches it at startup.",
     "Restart the API after rotating secrets; added a startup log line with the key id.", ["cmd/api/main.go"], 1, ["payments", "config"]),
    ("flaky-audit-order", "Flaky audit order test", "expected charge before refund",
     "TestAuditOrder fails about 1 in 20 runs.", "Two goroutines recorded entries; the test assumed order.", "Sort entries by time and action in the test; production code was fine.", ["audit/audit_test.go"], 2, ["tests", "flaky"]),
    ("vat-rounding-off-by-one", "VAT rounding off by one kuruş", "total 2398 want 2399",
     "Totals were one kuruş short for some prices.", "Integer division truncated instead of rounding half up.", "Use (net*rate+5000)/10000 as in pricing.WithTax.", ["pricing/tax.go"], 1, ["pricing", "rounding"]),
    ("golangci-timeout", "golangci-lint timeout in CI", "context deadline exceeded",
     "Lint job fails with a timeout after 5 minutes.", "Cache key changed every run because go.sum was not part of it.", "Include go.sum in the cache key; set timeout to 10m.", [".github/workflows/ci.yml"], 1, ["ci"]),
    ("migration-order", "Migrations applied out of order", "no such column: updated_at",
     "Fresh databases fail at startup with `no such column`.", "Two migrations had the same number 0012.", "Renumbered to 0013; CI now checks for duplicate migration numbers.", ["migrations/"], 1, ["migrations"]),
    ("istanbul-dst-report", "Daily report shifted by an hour", "report window 23:00-23:00",
     "The daily sales report covered 23:00 to 23:00.", "Report used UTC midnight instead of Istanbul midnight.", "Compute the window in Europe/Istanbul, then convert to UTC.", ["reports/daily.go"], 1, ["time"]),
    ("haproxy-502-deploy", "502s during deploy", "502 Bad Gateway",
     "Short bursts of 502 during deploys.", "Nodes stopped before HAProxy marked them down.", "Drain for 30s via the admin socket before stopping.", ["deploy/rollout.sh"], 2, ["ops"]),
    ("json-number-precision", "Prices lose precision in admin export", "price 34900.000001",
     "Exported CSV shows prices like 34900.000001.", "The exporter decoded JSON numbers into float64.", "Decode into json.Number, then int64.", ["admin/export.go"], 1, ["money"]),
    ("race-cart-lines", "Data race on cart lines", "WARNING: DATA RACE",
     "go test -race reports a race in cart tests.", "A test shared one Cart between parallel subtests.", "Create a cart per subtest.", ["cart/cart_test.go"], 1, ["tests", "race"]),
    ("env-not-loaded", "Config missing in local runs", "missing SHOP_DB_URL",
     "Local runs fail with `missing SHOP_DB_URL`.", "direnv was not allowed in the new clone.", "Run `direnv allow`; documented in README.", ["README.md"], 3, ["config", "dev"]),
    ("refund-double-audit", "Refund audited twice", "expected 1 audit entry, got 2",
     "A refund created two audit entries.", "Audit was recorded before the provider call and again in the retry path.", "Record once, after the provider call succeeds.", ["payments/refund.go"], 1, ["payments", "audit"]),
    ("windows-path-separator", "Golden file paths fail on Windows", "open testdata\\golden/x.json",
     "Golden tests fail on Windows with mixed separators.", "Paths were joined with string concatenation.", "Use filepath.Join everywhere.", ["internal/golden/golden.go"], 1, ["windows", "tests"]),
    ("redis-timeout-cart", "Cart cache timeouts", "i/o timeout",
     "Intermittent `i/o timeout` from the cart cache.", "Redis maxclients reached during load tests.", "Pool size 20 per instance; alert at 80% clients.", ["cache/redis.go"], 1, ["redis"]),
    ("decimal-tax-books", "Wrong VAT for books", "vat 20% on category books",
     "Books were charged 20% VAT.", "Per-category rates are not implemented.", "Temporary: books excluded from the shop until category rates land.", ["pricing/tax.go"], 1, ["pricing"]),
    ("ulid-monotonic", "Duplicate ULIDs in a batch", "UNIQUE constraint failed: orders.id",
     "Bulk import failed with duplicate IDs.", "ULIDs generated in the same millisecond with a non-monotonic source.", "Use a monotonic entropy source.", ["ids/ulid.go"], 1, ["ids"]),
    ("utf8-bom-catalog", "Catalog JSON with BOM", "invalid character 'ï'",
     "Catalog failed to parse after editing in Notepad.", "Notepad saved a UTF-8 BOM.", "Strip the BOM in catalog.Load; merchandising tool exports without BOM.", ["catalog/catalog.go"], 1, ["catalog"]),
    ("ci-go-version", "CI uses old Go", "go: go.mod requires go >= 1.24",
     "CI failed after bumping go.mod.", "setup-go pinned 1.22.", "Read the version from go.mod (go-version-file).", [".github/workflows/ci.yml"], 1, ["ci"]),
    ("payout-weekend", "Payouts scheduled on weekends", "payout date is Saturday",
     "Merchant payouts landed on Saturdays.", "Business-day logic ignored weekends.", "Move to the next Monday; holidays are a known gap.", ["payouts/schedule.go"], 1, ["payouts"]),
]
for i, (slug, title, sig, symptom, cause, fix, files, seen, tags) in enumerate(errors):
    filelist = "\n".join(f"- {f}" for f in files)
    write(f"episodes/errors/{slug}.md", f"""
{fm(id=f"01SHOPERR{i:017d}", type="error", layer="episodic", status="resolved", title=title, sig="'" + sig.replace("'", "''") + "'", seen=str(seen), tags=tags, created=f"2026-0{random.randint(4, 9)}-{random.randint(10, 28)}", updated=f"2026-0{random.randint(6, 9)}-{random.randint(10, 28)}")}
# {title}

## Symptom
{symptom}

## Root cause
{cause}

## Fix
{fix}

## Files
{filelist}
""")

# filler errors so the episodic layer has realistic bulk
areas = ["admin tool", "API startup", "nightly export", "search index", "image upload", "email sender", "invoice PDF", "stock sync", "webhook handler", "metrics endpoint", "session store", "rate limiter", "CSV import", "order status job", "shipping quote"]
causes = ["a nil map in the config struct", "an expired TLS certificate on the staging host", "a missing index on orders.customer_id", "a goroutine leak in the retry loop", "the provider changing a field name", "an unbounded channel filling up", "time zones mixed in a comparison", "a stale build cache in CI", "a context cancelled too early", "a slice aliasing bug when appending", "an off-by-one in pagination", "reading the whole file into memory"]
fixes = ["Added a guard and a regression test.", "Rotated the certificate and added an expiry alert.", "Added the index in a forward migration.", "Bounded the retries and closed the channel on exit.", "Mapped both field names until the provider finishes the migration.", "Switched to a bounded worker pool.", "Compared in UTC everywhere.", "Cleared the cache and keyed it on go.sum.", "Derived the context from the request instead of a timeout.", "Copied the slice before appending.", "Fixed the page arithmetic and tested the last page.", "Streamed the file line by line."]
for i in range(60):
    a = random.choice(areas)
    c = random.choice(causes)
    f = random.choice(fixes)
    slug = f"{a.replace(' ', '-')}-{i:02d}"
    title = f"{a.capitalize()} failure #{i+1}"
    write(f"episodes/errors/{slug}.md", f"""
{fm(id=f"01SHOPERRF{i:016d}", type="error", layer="episodic", status="resolved", title=title, seen=str(random.randint(1, 3)), tags=[a.split()[0]], created=f"2026-0{random.randint(3, 9)}-{random.randint(10, 28)}")}
# {title}

## Symptom
The {a} failed in {random.choice(['staging', 'production', 'CI', 'local runs'])} with errors in the logs and {random.choice(['timeouts', 'empty responses', 'a panic', 'wrong totals', 'missing rows'])}.

## Root cause
The cause was {c}.

## Fix
{f}
""")

# ---------------------------------------------------------------- decisions
decisions = [
    ("0014-refund-window", "Refund window of 30 days", "Finance closes the books monthly and asked us to stop refunds older than a month. The provider allows 180 days.",
     "Refunds are accepted only within 30 days of payment.At (measured with clock.Now()). Past the window, payments.Refund returns the exported sentinel `ErrRefundWindow`. Partial refunds are fine while the total stays within the paid amount; each refund is audited as \"refund\".",
     "- 180 days like the provider: rejected by finance.\n- Manual approval past 30 days: later, via the admin tool."),
    ("0007-wal-mode", "SQLite in WAL mode", "Parallel tests locked the database.", "Open SQLite with WAL and busy_timeout=5000.", "- Serial tests: CI 3x slower."),
    ("0009-int64-kurus", "Money as int64 kuruş", "Float rounding produced one-kuruş differences in reports.", "All amounts are int64 kuruş; formatting divides by 100.", "- decimal library: extra dependency, slower."),
    ("0011-clock-package", "Replaceable clock", "Time-dependent tests were flaky around midnight.", "All library code calls clock.Now(); tests pin it.", "- Pass time as an argument everywhere: too invasive."),
    ("0012-audit-every-movement", "Audit every money movement", "Finance found refunds missing from reconciliation.", "Charge, refund and payout call audit.Record after the provider succeeds.", "- Audit in the provider adapter: misses fakes in tests."),
    ("0015-catalog-checksum", "Pin the catalog checksum", "A half-deployed catalog file served empty prices.", "catalog.Load verifies sha256 against ExpectedChecksum computed over the LF file.", "- Signature file: more moving parts."),
]
fill_dec = ["Structured logging with slog", "HAProxy instead of nginx", "Feature flags in YAML", "ULIDs for public IDs", "Iyzico as payment provider", "SQLite for admin tools", "Golden files for exports", "Weekly dependency updates", "Postgres 16 for the API", "Retry policy for provider calls", "Monthly key rotation", "Only TRY for now", "Single repo for API and tools", "golangci-lint with default rules"]
for slug, title, ctx, dec, alts in decisions:
    num = slug[:4]
    write(f"episodes/decisions/{slug}.md", f"""
{fm(id=f"01SHOPDEC{num}0000000000000", type="decision", layer="episodic", status="active", title=f'"{num}: {title}"', created=f"2026-0{random.randint(3, 9)}-{random.randint(10, 28)}")}
# {num}: {title}

## Context
{ctx}

## Decision
{dec}

## Alternatives
{alts}

## Consequences
Documented in the rules; reviewers check it.
""")
used = {int(d[0][:4]) for d in decisions}
num = 1
for t in fill_dec:
    while num in used:
        num += 1
    used.add(num)
    s = f"{num:04d}-" + t.lower().replace(" ", "-").replace("/", "-")
    write(f"episodes/decisions/{s}.md", f"""
{fm(id=f"01SHOPDECF{num:016d}", type="decision", layer="episodic", status="active", title=f'"{num:04d}: {t}"', created=f"2026-0{random.randint(2, 9)}-{random.randint(10, 28)}")}
# {num:04d}: {t}

## Context
We needed a consistent answer for {t.lower()} across the API and the admin tools.

## Decision
{t}. Revisit if it starts to cost more than a day per quarter.

## Alternatives
- Keep the status quo: inconsistent between services.
- A heavier tool: not justified at our size.
""")

# ---------------------------------------------------------------- archived sessions and journal
topics = ["Catalog import", "Iyzico integration", "VAT rounding fix", "Admin export", "Audit reconciliation", "HAProxy rollout", "Payout schedule", "Rate limiter", "Golden files", "CI cache", "Stock sync", "Invoice PDF", "Discount codes admin", "Order status job", "Shipping quotes", "Email templates", "Search index", "Webhook retries", "Session store", "Metrics endpoint", "CSV import", "Image upload", "Stock alerts", "Customer export"]
for i, t in enumerate(topics):
    d = f"2026-0{random.randint(4, 9)}-{random.randint(10, 28)}"
    tasks = "\n".join(f"- [x] {s} ^ta{i:02d}{j}" for j, s in enumerate(["Investigate current state", "Implement the change", "Tests", "Docs"]))
    write(f"archive/work/sessions/{d}-{t.lower().replace(' ', '-')}.md", f"""
{fm(id=f"01SHOPSES{i:017d}", type="session", layer="working", status="closed", title=t, branch="main", created=d)}
# {t}

## Goal
{t}: finish the work and leave the repo green.

## Plan
{tasks}

## Notes
- 10:15 started from the previous handoff
- 14:40 tests green

## Handoff
Done; see the journal entry of {d}.
""")
for m in ["2026-04", "2026-05", "2026-06", "2026-07", "2026-08", "2026-09"]:
    days = sorted({random.randint(1, 28) for _ in range(8)})
    parts = [fm(type="journal", layer="episodic"), f"# {m}"]
    for dd in days:
        parts.append(f"\n## {m}-{dd:02d}\n")
        for _ in range(random.randint(1, 3)):
            parts.append(f"- {random.randint(9, 18):02d}:{random.choice(['05', '20', '35', '50'])} {random.choice(topics).lower()}: {random.choice(['merged', 'reviewed', 'deployed to staging', 'tests fixed', 'started'])}")
    write(f"episodes/journal/{m}.md", "\n".join(parts))

write("work/inbox.md", f"{fm(type='note', layer='working', title='Inbox')}\n# Inbox\n\n- [ ] Per-category VAT rates ^tin01\n- [ ] Remove flag new_checkout after 2026-11-01 ^tin02\n")
print("memory written to", OUT)
