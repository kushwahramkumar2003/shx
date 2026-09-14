//! T-SAFE-4 — redaction corpus.

use std::borrow::Cow;

use shx_core::{Redactor, SecretRedactor};

fn corpus() -> &'static str {
    include_str!("../../../tests/corpus/redaction.txt")
}

fn pairs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, raw) in corpus().lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((left, right)) = line.split_once('→') else {
            panic!("redaction.txt:{}: expected `raw → expected`", i + 1);
        };
        out.push((left.trim().to_string(), right.trim().to_string()));
    }
    out
}

/// T-SAFE-4: every positive masks; every negative passes through.
#[test]
fn t_safe_4_corpus() {
    let r = SecretRedactor;
    let mut misses = Vec::new();
    for (raw, expect) in pairs() {
        let got = r.redact(&raw);
        if got.as_ref() != expect {
            misses.push(format!("in:  {raw}\nwant: {expect}\ngot:  {got}"));
        }
    }
    assert!(
        misses.is_empty(),
        "T-SAFE-4 mismatches:\n\n{}",
        misses.join("\n\n")
    );
}

#[test]
fn t_safe_4_cow_borrows_when_clean() {
    match SecretRedactor.redact("cargo test --workspace") {
        Cow::Borrowed(_) => {}
        Cow::Owned(s) => panic!("allocated on a clean string: {s}"),
    }
}
