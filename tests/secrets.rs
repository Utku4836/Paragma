//! Secrets: many kept by title, the right one found by what an agent would
//! ask, used by pg run without the value ever showing.

use std::path::Path;
use std::process::Command;

fn pg(home: &Path, cwd: &Path, args: &[&str]) -> (i32, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_pg"))
        .args(args)
        .current_dir(cwd)
        .env("PARAGMA_HOME", home)
        .env("PARAGMA_PROJECT", "shop")
        .env("PARAGMA_SESSION", "secrets-test")
        .output()
        .unwrap();
    (
        o.status.code().unwrap_or(-1),
        format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
    )
}

const TITLES: &[&str] = &[
    "Stripe Live",
    "Stripe Test",
    "Stripe Webhook Signing",
    "GitHub Deploy",
    "GitHub Actions",
    "Npm Publish",
    "AWS Prod Access",
    "AWS Staging Access",
    "AWS Prod Secret",
    "Google Maps",
    "Google OAuth Client",
    "Firebase Admin",
    "Sentry DSN",
    "Datadog",
    "Slack Bot",
    "Slack Webhook",
    "Twilio Auth",
    "SendGrid",
    "Mailgun",
    "Postmark",
    "OpenAI",
    "Anthropic",
    "Cloudflare DNS",
    "Cloudflare Workers",
    "Vercel Deploy",
    "Netlify",
    "Heroku",
    "DigitalOcean",
    "Docker Hub",
    "PyPI Upload",
    "Crates Io",
    "Algolia Admin",
    "Algolia Search",
    "Mapbox",
    "Shopify Admin",
    "Shopify Storefront",
    "Iyzico Live",
    "Iyzico Sandbox",
    "PayPal Client",
    "Apple Push",
    "Firebase Messaging",
    "Supabase Service",
    "Supabase Anon",
    "Postgres Prod Password",
    "Postgres Staging Password",
    "Redis Prod",
    "MongoDB Atlas",
    "Elastic Cloud",
    "Jira",
    "Linear",
];

fn value(i: usize) -> String {
    format!("val{i:02}_{}", "Zq7".repeat(4 + i % 3))
}

#[test]
fn fifty_secrets_found_by_title_and_used_unseen() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let cwd = d.path();
    let made = Command::new(env!("CARGO_BIN_EXE_pg"))
        .args(["write", "shop/"])
        .env("PARAGMA_HOME", &home)
        .env_remove("PARAGMA_PROJECT")
        .output()
        .unwrap();
    assert!(made.status.success());
    assert!(home.join("projects/shop/shop.env").is_file());
    for (i, t) in TITLES.iter().enumerate() {
        let (c, out) = pg(&home, cwd, &["write", "--secret", t, &value(i)]);
        assert_eq!(c, 0, "{out}");
        assert!(!out.contains(&value(i)), "{out}");
    }

    // what an agent asks -> the title it must get first
    let asks = [
        ("stripe test key", "Stripe Test"),
        ("stripe live secret key", "Stripe Live"),
        ("stripe webhook signing secret", "Stripe Webhook Signing"),
        ("npm publish token", "Npm Publish"),
        ("aws staging access key", "AWS Staging Access"),
        ("sentry dsn", "Sentry DSN"),
        ("iyzico sandbox api key", "Iyzico Sandbox"),
        ("postgres staging password", "Postgres Staging Password"),
        ("supabase anon key", "Supabase Anon"),
        ("slack webhook url", "Slack Webhook"),
    ];
    for (q, want) in asks {
        let (_, out) = pg(&home, cwd, &["find", q]);
        let first = out.lines().find(|l| l.starts_with("[secret] ")).unwrap_or("");
        assert!(first.starts_with(&format!("[secret] {want} (")), "{q:?} -> {out}");
        for i in 0..TITLES.len() {
            assert!(!out.contains(&value(i)), "value leaked for {q:?}: {out}");
        }
    }

    // every secret reaches the command as itself, in the line and in the
    // environment, and its value never shows
    for (i, t) in TITLES.iter().enumerate() {
        let name = t.to_ascii_uppercase().replace(' ', "_");
        let check = if cfg!(windows) {
            format!("if \"${name}\"==\"{v}\" (if \"%{name}%\"==\"{v}\" echo match)", v = value(i))
        } else {
            format!("[ \"${name}\" = \"{v}\" ] && [ \"$(printenv {name})\" = \"{v}\" ] && echo match", v = value(i))
        };
        let (c, out) = pg(&home, cwd, &["run", "--", &check]);
        assert_eq!(c, 0, "{t}: {out}");
        assert!(out.contains("\nmatch"), "{t}: {out}");
        assert!(!out.contains(&value(i)), "{t} leaked: {out}");
    }
    let (_, out) = pg(&home, cwd, &["run", "--", "echo $STRIPE_LIVE and $IYZICO_LIVE"]);
    assert!(out.contains("output:\n$STRIPE_LIVE and $IYZICO_LIVE"), "{out}");

    // a key pasted into a note is masked and not kept: it needs a title
    let (_, out) = pg(&home, cwd, &["write", "notes/deploy", "deploy with AKIAIOSFODNN7EXAMPLE"]);
    assert!(out.contains("masked 1 secret (an AWS access key), not kept"), "{out}");
    let note = std::fs::read_to_string(home.join("projects/shop/notes/deploy.md")).unwrap();
    assert!(!note.contains("AKIAIOSFODNN7EXAMPLE"));
}
