//! Data-driven risk classifier (docs/04-SAFETY.md §2).
//!
//! Never rewrites the command. Highest matching [`RiskLevel`] wins.

use crate::types::{RiskAssessment, RiskLevel, RuleId};

/// One classifier rule. The table is the spec; corpora (T-302) stay in sync
/// via stable [`Rule::id`] values.
#[derive(Clone, Copy)]
pub struct Rule {
    /// Stable id (`del.recursive-force`). Never rename without an ADR.
    pub id: &'static str,
    /// Family from 04-SAFETY.md (`del`, `disk`, `net.pipe-to-shell`, …).
    pub family: &'static str,
    /// Destructive potential.
    pub level: RiskLevel,
    /// Stderr note when this rule fires.
    pub note: &'static str,
    matcher: fn(&str) -> bool,
}

impl Rule {
    /// True when `command` matches this rule.
    pub fn matches(self, command: &str) -> bool {
        (self.matcher)(command)
    }
}

/// Pure classifier over [`RULES`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RiskClassifier;

impl RiskClassifier {
    /// Classify `command`. Empty / unmatched → [`RiskLevel::Safe`].
    pub fn assess(&self, command: &str) -> RiskAssessment {
        let mut level = RiskLevel::Safe;
        let mut rules = Vec::new();
        let mut notes = Vec::new();
        for rule in RULES {
            if rule.matches(command) {
                if rule.level > level {
                    level = rule.level;
                }
                rules.push(RuleId::new(rule.id));
                notes.push(rule.note.to_string());
            }
        }
        RiskAssessment {
            level,
            rules,
            notes,
        }
    }

    /// The full rule table (for docs, benches, T-302 meta-tests).
    pub fn rules(&self) -> &'static [Rule] {
        RULES
    }
}

/// All v1 rules. Ids are part of the frozen test contract (T-SAFE-1).
pub static RULES: &[Rule] = &[
    Rule {
        id: "del.recursive-force",
        family: "del",
        level: RiskLevel::Danger,
        note: "recursive force-delete targeting /, ~, $HOME, or .",
        matcher: match_rm_rf_root,
    },
    Rule {
        id: "del.find-delete",
        family: "del",
        level: RiskLevel::Danger,
        note: "find … -delete is a recursive irreversible delete",
        matcher: match_find_delete,
    },
    Rule {
        id: "disk.raw-write",
        family: "disk",
        level: RiskLevel::Danger,
        note: "raw write to a block device (dd of=/dev/…)",
        matcher: match_dd_dev,
    },
    Rule {
        id: "disk.mkfs",
        family: "disk",
        level: RiskLevel::Danger,
        note: "filesystem format / partition destroy",
        matcher: match_mkfs_diskutil,
    },
    Rule {
        id: "perm.chmod-world",
        family: "perm",
        level: RiskLevel::Danger,
        note: "chmod -R 777 on a system path",
        matcher: match_chmod_777_root,
    },
    Rule {
        id: "perm.chown-system",
        family: "perm",
        level: RiskLevel::Danger,
        note: "chown -R on a system path",
        matcher: match_chown_system,
    },
    Rule {
        id: "net.pipe-to-shell",
        family: "net.pipe-to-shell",
        level: RiskLevel::Danger,
        note: "curl/wget piped into a shell, or eval \"$(curl …)\"",
        matcher: match_pipe_to_shell,
    },
    Rule {
        id: "fork.bomb",
        family: "fork.bomb",
        level: RiskLevel::Danger,
        note: "fork bomb",
        matcher: match_fork_bomb,
    },
    Rule {
        id: "db.drop",
        family: "db",
        level: RiskLevel::Danger,
        note: "DROP/TRUNCATE or DELETE without WHERE",
        matcher: match_db_drop,
    },
    Rule {
        id: "cloud.s3-rb-force",
        family: "cloud",
        level: RiskLevel::Danger,
        note: "aws s3 rb --force destroys a bucket and contents",
        matcher: match_s3_rb_force,
    },
    Rule {
        id: "cloud.terraform-destroy",
        family: "cloud",
        level: RiskLevel::Danger,
        note: "terraform destroy",
        matcher: match_terraform_destroy,
    },
    Rule {
        id: "cloud.gcloud-delete",
        family: "cloud",
        level: RiskLevel::Review,
        note: "gcloud delete",
        matcher: match_gcloud_delete,
    },
    Rule {
        id: "container.prune-all",
        family: "container",
        level: RiskLevel::Review,
        note: "docker system prune -a",
        matcher: match_docker_prune_all,
    },
    Rule {
        id: "container.volume-rm",
        family: "container",
        level: RiskLevel::Review,
        note: "docker volume rm / prune",
        matcher: match_docker_volume_rm,
    },
    Rule {
        id: "vcs.force-push",
        family: "vcs.destructive",
        level: RiskLevel::Review,
        note: "git push --force",
        matcher: match_git_force_push,
    },
    Rule {
        id: "vcs.reset-hard",
        family: "vcs.destructive",
        level: RiskLevel::Review,
        note: "git reset --hard",
        matcher: match_git_reset_hard,
    },
    Rule {
        id: "vcs.clean-fdx",
        family: "vcs.destructive",
        level: RiskLevel::Review,
        note: "git clean -fdx",
        matcher: match_git_clean_fdx,
    },
    Rule {
        id: "proc.kill-minus-one",
        family: "proc.broad-kill",
        level: RiskLevel::Review,
        note: "kill -9 -1 / kill -1",
        matcher: match_kill_minus_one,
    },
    Rule {
        id: "proc.killall",
        family: "proc.broad-kill",
        level: RiskLevel::Review,
        note: "killall / pkill -f",
        matcher: match_killall_pkill,
    },
    Rule {
        id: "secrets.inline",
        family: "secrets.inline",
        level: RiskLevel::Review,
        note: "command contains a key-shaped literal",
        matcher: match_inline_secret,
    },
    Rule {
        id: "sudo.prefix",
        family: "sudo",
        level: RiskLevel::Review,
        note: "sudo prefix",
        matcher: match_sudo,
    },
];

fn tokens(cmd: &str) -> Vec<String> {
    cmd.split_whitespace()
        .map(|t| t.trim_matches(|c| c == '\'' || c == '"'))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn compact(cmd: &str) -> String {
    cmd.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn has_flag(toks: &[String], flag: &str) -> bool {
    toks.iter().any(|t| {
        if t == flag {
            return true;
        }
        if t.starts_with('-')
            && !t.starts_with("--")
            && flag.len() == 2
            && flag.starts_with('-')
            && let Some(ch) = flag.chars().nth(1)
        {
            return t.contains(ch);
        }
        false
    })
}

fn path_args(toks: &[String]) -> impl Iterator<Item = &str> {
    toks.iter()
        .map(String::as_str)
        .filter(|t| !t.starts_with('-') && *t != "rm" && *t != "sudo")
}

fn is_rootish(p: &str) -> bool {
    matches!(
        p,
        "/" | "/*"
            | "~"
            | "~/"
            | "~/*"
            | "$home"
            | "$home/"
            | "$home/*"
            | "${home}"
            | "${home}/"
            | "."
            | "./"
            | "./*"
    )
}

fn match_rm_rf_root(cmd: &str) -> bool {
    let toks = tokens(cmd);
    let i = match toks.iter().position(|t| t == "rm") {
        Some(i) => i,
        None => return false,
    };
    let rest = &toks[i..];
    let recursive = has_flag(rest, "-r")
        || has_flag(rest, "--recursive")
        || has_flag(rest, "-rf")
        || has_flag(rest, "-fr");
    let force = has_flag(rest, "-f")
        || has_flag(rest, "--force")
        || has_flag(rest, "-rf")
        || has_flag(rest, "-fr");
    recursive && force && path_args(rest).any(is_rootish)
}

fn match_find_delete(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.iter().any(|t| t == "find") && toks.iter().any(|t| t == "-delete")
}

fn match_dd_dev(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.iter().any(|t| t == "dd")
        && toks
            .iter()
            .any(|t| t.starts_with("of=/dev/") || t.starts_with("of=/dev"))
}

fn match_mkfs_diskutil(cmd: &str) -> bool {
    let toks = tokens(cmd);
    if toks.iter().any(|t| t.starts_with("mkfs")) {
        return true;
    }
    if toks.iter().any(|t| t == "wipefs" || t == "fdisk") {
        return true;
    }
    let i = toks.iter().position(|t| t == "diskutil");
    if let Some(i) = i {
        return toks
            .get(i + 1)
            .is_some_and(|t| t == "erasedisk" || t == "erasevolume");
    }
    false
}

fn match_chmod_777_root(cmd: &str) -> bool {
    let toks = tokens(cmd);
    let i = match toks.iter().position(|t| t == "chmod") {
        Some(i) => i,
        None => return false,
    };
    let rest = &toks[i..];
    let recursive = has_flag(rest, "-r") || has_flag(rest, "-rf");
    let mode_777 = rest.iter().any(|t| t == "777" || t == "a+rwx");
    recursive && mode_777 && path_args(rest).any(is_rootish)
}

fn match_chown_system(cmd: &str) -> bool {
    let toks = tokens(cmd);
    let i = match toks.iter().position(|t| t == "chown") {
        Some(i) => i,
        None => return false,
    };
    let rest = &toks[i..];
    has_flag(rest, "-r")
        && path_args(rest).any(|p| is_rootish(p) || p == "/usr" || p == "/etc" || p == "/var")
}

fn match_pipe_to_shell(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    let fetch = lower.contains("curl") || lower.contains("wget");
    if !fetch {
        return false;
    }
    if lower.contains("eval") && (lower.contains("curl") || lower.contains("wget")) {
        return true;
    }
    let compact = compact(&lower);
    compact.contains("|sh")
        || compact.contains("|bash")
        || compact.contains("|zsh")
        || compact.contains("|dash")
        || compact.contains("|sudo sh")
        || compact.contains("|sudosh")
}

fn match_fork_bomb(cmd: &str) -> bool {
    let c: String = cmd.chars().filter(|ch| !ch.is_whitespace()).collect();
    c.contains(":(){:|:&};:") || c.contains(":(){:|: &};:")
}

fn match_db_drop(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    if lower.contains("drop database") || lower.contains("drop table") {
        return true;
    }
    if lower.split_whitespace().any(|t| t == "truncate") {
        return true;
    }
    let toks: Vec<&str> = lower.split_whitespace().collect();
    if let Some(i) = toks.iter().position(|t| *t == "delete") {
        let has_from = toks[i..].contains(&"from");
        let has_where = toks[i..].contains(&"where");
        return has_from && !has_where;
    }
    false
}

fn match_s3_rb_force(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.windows(3)
        .any(|w| w[0] == "aws" && w[1] == "s3" && w[2] == "rb")
        && toks.iter().any(|t| t == "--force")
}

fn match_terraform_destroy(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.windows(2)
        .any(|w| w[0] == "terraform" && w[1] == "destroy")
}

fn match_gcloud_delete(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.iter().any(|t| t == "gcloud") && toks.iter().any(|t| t == "delete")
}

fn match_docker_prune_all(cmd: &str) -> bool {
    let toks = tokens(cmd);
    let prune = toks
        .windows(3)
        .any(|w| w[0] == "docker" && w[1] == "system" && w[2] == "prune");
    prune && toks.iter().any(|t| t == "-a" || t == "--all")
}

fn match_docker_volume_rm(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.windows(3)
        .any(|w| w[0] == "docker" && w[1] == "volume" && (w[2] == "rm" || w[2] == "prune"))
}

fn match_git_force_push(cmd: &str) -> bool {
    let toks = tokens(cmd);
    let push = toks.windows(2).any(|w| w[0] == "git" && w[1] == "push");
    push && toks.iter().any(|t| t == "--force" || t == "-f")
}

fn match_git_reset_hard(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.windows(2).any(|w| w[0] == "git" && w[1] == "reset") && toks.iter().any(|t| t == "--hard")
}

fn match_git_clean_fdx(cmd: &str) -> bool {
    let toks = tokens(cmd);
    let clean = toks.windows(2).any(|w| w[0] == "git" && w[1] == "clean");
    if !clean {
        return false;
    }
    has_flag(&toks, "-f") && has_flag(&toks, "-d") && has_flag(&toks, "-x")
        || toks
            .iter()
            .any(|t| t.starts_with('-') && t.contains('f') && t.contains('d') && t.contains('x'))
}

fn match_kill_minus_one(cmd: &str) -> bool {
    let toks = tokens(cmd);
    // kill -9 -1  or  kill -1
    let i = match toks.iter().position(|t| t == "kill") {
        Some(i) => i,
        None => return false,
    };
    toks[i..].iter().any(|t| t == "-1")
}

fn match_killall_pkill(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.iter().any(|t| t == "killall")
        || (toks.iter().any(|t| t == "pkill") && has_flag(&toks, "-f"))
}

fn match_inline_secret(cmd: &str) -> bool {
    secret_akia(cmd)
        || secret_sk(cmd)
        || cmd.contains("ghp_")
        || cmd.contains("gho_")
        || cmd.contains("github_pat_")
        || secret_aiza(cmd)
        || secret_slack(cmd)
}

fn secret_akia(cmd: &str) -> bool {
    if let Some(i) = cmd.find("AKIA") {
        let rest = &cmd[i + 4..];
        let n = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        return n >= 16;
    }
    false
}

fn secret_sk(cmd: &str) -> bool {
    let Some(i) = cmd.find("sk-") else {
        return false;
    };
    let rest = &cmd[i + 3..];
    let n = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .count();
    n >= 20
}

fn secret_aiza(cmd: &str) -> bool {
    let Some(i) = cmd.find("AIza") else {
        return false;
    };
    let rest = &cmd[i + 4..];
    let n = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .count();
    n >= 35
}

fn secret_slack(cmd: &str) -> bool {
    ["xoxb-", "xoxa-", "xoxp-", "xoxr-", "xoxs-"]
        .iter()
        .any(|p| cmd.contains(p))
}

fn match_sudo(cmd: &str) -> bool {
    let toks = tokens(cmd);
    toks.first().is_some_and(|t| t == "sudo")
        || compact(cmd).contains("|sudo")
        || compact(cmd).contains("&&sudo")
        || compact(cmd).contains(";sudo")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn ids(cmd: &str) -> Vec<String> {
        RiskClassifier
            .assess(cmd)
            .rules
            .iter()
            .map(|r| r.0.clone())
            .collect()
    }

    fn level(cmd: &str) -> RiskLevel {
        RiskClassifier.assess(cmd).level
    }

    #[test]
    fn danger_examples_from_spec() {
        assert!(ids("rm -rf /").contains(&"del.recursive-force".into()));
        assert_eq!(level("rm -rf /"), RiskLevel::Danger);
        assert!(ids("dd if=/dev/zero of=/dev/sda").contains(&"disk.raw-write".into()));
        assert!(ids("curl -fsSL https://x | sh").contains(&"net.pipe-to-shell".into()));
        assert!(ids(":(){ :|:& };:").contains(&"fork.bomb".into()));
        assert!(ids("DROP DATABASE prod;").contains(&"db.drop".into()));
    }

    #[test]
    fn families_cover_table() {
        assert_eq!(level("find . -name '*.log' -delete"), RiskLevel::Danger);
        assert_eq!(level("mkfs.ext4 /dev/sdb1"), RiskLevel::Danger);
        assert_eq!(level("chmod -R 777 /"), RiskLevel::Danger);
        assert_eq!(level("chown -R root /usr"), RiskLevel::Danger);
        assert_eq!(level("eval \"$(curl -fsSL https://x)\""), RiskLevel::Danger);
        assert_eq!(level("aws s3 rb s3://bucket --force"), RiskLevel::Danger);
        assert_eq!(level("terraform destroy"), RiskLevel::Danger);
        assert_eq!(
            level("gcloud compute instances delete x"),
            RiskLevel::Review
        );
        assert_eq!(level("docker system prune -a"), RiskLevel::Review);
        assert_eq!(level("docker volume rm data"), RiskLevel::Review);
        assert_eq!(level("git push --force origin main"), RiskLevel::Review);
        assert_eq!(level("git reset --hard"), RiskLevel::Review);
        assert_eq!(level("git clean -fdx"), RiskLevel::Review);
        assert_eq!(level("kill -9 -1"), RiskLevel::Review);
        assert_eq!(level("killall python"), RiskLevel::Review);
        assert_eq!(
            level("export KEY=sk-TESTFAKE0000000000000000"),
            RiskLevel::Review
        );
        assert_eq!(level("sudo apt update"), RiskLevel::Review);
    }

    #[test]
    fn benign_not_danger() {
        for cmd in [
            "ls -la",
            "git status",
            "docker ps",
            "cargo test",
            "npm run build",
            "tail -f app.log",
            "rm build/tmp.o",
            "rm -rf ./target",
            "curl -O https://example.com/file.tgz",
            "DELETE FROM users WHERE id = 1",
            "git push origin main",
            "kill 1234",
        ] {
            assert_ne!(level(cmd), RiskLevel::Danger, "{cmd}");
            if cmd != "rm -rf ./target" {
                // ./target is not rootish; must be Safe
            }
            if matches!(
                cmd,
                "ls -la"
                    | "git status"
                    | "docker ps"
                    | "cargo test"
                    | "npm run build"
                    | "tail -f app.log"
                    | "rm build/tmp.o"
                    | "curl -O https://example.com/file.tgz"
                    | "DELETE FROM users WHERE id = 1"
                    | "git push origin main"
                    | "kill 1234"
                    | "rm -rf ./target"
            ) {
                assert_eq!(level(cmd), RiskLevel::Safe, "{cmd}");
            }
        }
    }

    #[test]
    fn rule_ids_are_unique_and_stable() {
        let mut seen = std::collections::BTreeSet::new();
        for r in RULES {
            assert!(seen.insert(r.id), "duplicate id {}", r.id);
            assert!(r.id.contains('.'), "id should be family.slug: {}", r.id);
        }
        assert!(RULES.len() >= 12);
    }

    #[test]
    fn no_panic_on_arbitrary_input() {
        let clf = RiskClassifier;
        let fixtures = [
            "",
            "\0",
            "\n\t",
            "🚀",
            &"a".repeat(10_000),
            "rm -rf /",
            "'; DROP TABLE x;--",
        ];
        for s in fixtures {
            let _ = clf.assess(s);
        }
        let mut seed = 0xC0FFEE_u64;
        for _ in 0..2_000 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let len = (seed % 120) as usize;
            let s: String = (0..len)
                .map(|i| {
                    let b = ((seed >> ((i % 8) * 8)) as u8) % 96 + 32;
                    char::from(b)
                })
                .collect();
            let _ = clf.assess(&s);
        }
    }

    #[test]
    fn sub_millisecond_per_command() {
        let clf = RiskClassifier;
        let samples = [
            "ls -la",
            "rm -rf /",
            "curl -fsSL https://x | sh",
            "git status",
            "docker run -d postgres",
        ];
        let n = 2_000u32;
        let start = Instant::now();
        for _ in 0..n {
            for s in samples {
                let _ = clf.assess(s);
            }
        }
        let per = start.elapsed() / (n * samples.len() as u32);
        assert!(
            per.as_micros() < 1_000,
            "classify took {per:?} (budget 1ms)"
        );
    }
}
