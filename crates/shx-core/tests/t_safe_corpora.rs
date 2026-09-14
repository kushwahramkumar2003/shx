//! T-SAFE-1 / T-SAFE-2 and the rule↔corpus meta-test (T-302).

use std::collections::BTreeSet;

use shx_core::{RULES, RiskClassifier, RiskLevel};

fn danger_corpus() -> &'static str {
    include_str!("../../../tests/corpus/risk_danger.txt")
}

fn benign_corpus() -> &'static str {
    include_str!("../../../tests/corpus/risk_benign.txt")
}

fn danger_lines() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (i, raw) in danger_corpus().lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((cmd, id)) = line.split_once('→') else {
            panic!("risk_danger.txt:{}: expected `command → rule.id`", i + 1);
        };
        out.push((cmd.trim().to_string(), id.trim().to_string()));
    }
    out
}

fn benign_lines() -> Vec<String> {
    benign_corpus()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// T-SAFE-1: every labeled danger line fires its rule; Danger rules are Danger.
#[test]
fn t_safe_1_zero_false_negatives() {
    let clf = RiskClassifier;
    let lines = danger_lines();
    assert!(
        lines.len() >= 40,
        "danger corpus has {} lines; need ≥ 40",
        lines.len()
    );
    let mut misses = Vec::new();
    for (cmd, expect_id) in &lines {
        let a = clf.assess(cmd);
        let hit = a.rules.iter().any(|r| r.0 == *expect_id);
        if !hit {
            misses.push(format!("{cmd}  (expected {expect_id}, got {:?})", a.rules));
            continue;
        }
        let rule = RULES
            .iter()
            .find(|r| r.id == expect_id)
            .unwrap_or_else(|| panic!("unknown rule id {expect_id}"));
        if rule.level == RiskLevel::Danger && a.level != RiskLevel::Danger {
            misses.push(format!(
                "{cmd}  rule {expect_id} is Danger but got {:?}",
                a.level
            ));
        }
    }
    assert!(
        misses.is_empty(),
        "T-SAFE-1 false negatives:\n{}",
        misses.join("\n")
    );
}

/// T-SAFE-2: < 10% of routine commands flagged above Safe.
#[test]
fn t_safe_2_benign_false_positive_budget() {
    let clf = RiskClassifier;
    let lines = benign_lines();
    assert!(
        lines.len() >= 80,
        "benign corpus has {} lines; need a real routine set",
        lines.len()
    );
    let mut flagged = Vec::new();
    for cmd in &lines {
        let a = clf.assess(cmd);
        if a.level > RiskLevel::Safe {
            flagged.push(format!("{cmd}  → {:?} {:?}", a.level, a.rules));
        }
    }
    let rate = flagged.len() as f64 / lines.len() as f64;
    assert!(
        rate < 0.10,
        "T-SAFE-2 FP rate {:.1}% (budget <10%); flagged:\n{}",
        rate * 100.0,
        flagged.join("\n")
    );
}

/// Adding a classifier rule without a danger-corpus line must fail.
#[test]
fn meta_every_rule_has_a_corpus_line() {
    let present: BTreeSet<String> = danger_lines().into_iter().map(|(_, id)| id).collect();
    let missing: Vec<&str> = RULES
        .iter()
        .map(|r| r.id)
        .filter(|id| !present.contains(*id))
        .collect();
    assert!(
        missing.is_empty(),
        "rules with no risk_danger.txt line: {missing:?}"
    );
}
