//! Secret redaction (frozen [`Redactor`] trait).
//!
//! Matches 04-SAFETY.md §3.1. A hit is replaced with `«redacted:kind»`.
//! [`Cow::Borrowed`] when nothing matched.

use std::borrow::Cow;

/// Text that has been passed through a [`Redactor`].
///
/// Borrowed when nothing matched (no allocation); owned when secrets were
/// masked.
pub type Redacted<'a> = Cow<'a, str>;

/// Masks known secret shapes in text destined for a backend or the store.
///
/// Implementations must be pure and must not panic on arbitrary input.
pub trait Redactor {
    /// Return `Cow::Borrowed` when `input` is unchanged.
    fn redact<'a>(&self, input: &'a str) -> Redacted<'a>;
}

/// Identity redactor (tests, `--offline` fixtures that must not mask).
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact<'a>(&self, input: &'a str) -> Redacted<'a> {
        Cow::Borrowed(input)
    }
}

/// Production redactor: API keys, env assignments, connection strings, PEMs, JWTs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SecretRedactor;

impl Redactor for SecretRedactor {
    fn redact<'a>(&self, input: &'a str) -> Redacted<'a> {
        redact_secrets(input)
    }
}

struct Hit {
    start: usize,
    end: usize,
    kind: &'static str,
}

fn redact_secrets(input: &str) -> Cow<'_, str> {
    let Some(first) = next_hit(input, 0) else {
        return Cow::Borrowed(input);
    };
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut hit = first;
    loop {
        out.push_str(&input[i..hit.start]);
        out.push_str("«redacted:");
        out.push_str(hit.kind);
        out.push('»');
        i = hit.end;
        match next_hit(input, i) {
            Some(h) => hit = h,
            None => {
                out.push_str(&input[i..]);
                break;
            }
        }
    }
    Cow::Owned(out)
}

fn next_hit(s: &str, from: usize) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for finder in FINDERS {
        if let Some(h) = finder(s, from) {
            best = Some(match best {
                None => h,
                Some(b) if h.start < b.start => h,
                Some(b) if h.start == b.start && h.end > b.end => h,
                Some(b) => b,
            });
        }
    }
    best
}

type Finder = fn(&str, usize) -> Option<Hit>;

const FINDERS: &[Finder] = &[
    find_akia,
    find_sk,
    find_github,
    find_aiza,
    find_slack,
    find_authorization,
    find_env_assign,
    find_connection,
    find_private_key,
    find_jwt,
];

fn find_from(s: &str, from: usize, needle: &str) -> Option<usize> {
    s[from..].find(needle).map(|rel| from + rel)
}

fn take_alnum(s: &str) -> usize {
    s.bytes().take_while(|b| b.is_ascii_alphanumeric()).count()
}

fn take_alnum_us_hyphen(s: &str) -> usize {
    s.bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count()
}

fn find_akia(s: &str, from: usize) -> Option<Hit> {
    let mut search = from;
    while let Some(start) = find_from(s, search, "AKIA") {
        let n = s[start + 4..]
            .bytes()
            .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            .count();
        if n >= 16 {
            return Some(Hit {
                start,
                end: start + 4 + n,
                kind: "aws-key",
            });
        }
        search = start + 1;
    }
    None
}

fn find_sk(s: &str, from: usize) -> Option<Hit> {
    let mut search = from;
    while let Some(start) = find_from(s, search, "sk-") {
        let n = take_alnum(&s[start + 3..]);
        if n >= 20 {
            return Some(Hit {
                start,
                end: start + 3 + n,
                kind: "api-key",
            });
        }
        search = start + 1;
    }
    None
}

fn find_github(s: &str, from: usize) -> Option<Hit> {
    for prefix in ["ghp_", "gho_", "github_pat_"] {
        let mut search = from;
        while let Some(start) = find_from(s, search, prefix) {
            let n = take_alnum_us_hyphen(&s[start + prefix.len()..]);
            if n >= 8 {
                return Some(Hit {
                    start,
                    end: start + prefix.len() + n,
                    kind: "github-token",
                });
            }
            search = start + 1;
        }
    }
    None
}

fn find_aiza(s: &str, from: usize) -> Option<Hit> {
    let mut search = from;
    while let Some(start) = find_from(s, search, "AIza") {
        let n = take_alnum_us_hyphen(&s[start + 4..]);
        if n >= 35 {
            return Some(Hit {
                start,
                end: start + 4 + n,
                kind: "google-key",
            });
        }
        search = start + 1;
    }
    None
}

fn find_slack(s: &str, from: usize) -> Option<Hit> {
    for prefix in ["xoxb-", "xoxa-", "xoxp-", "xoxr-", "xoxs-"] {
        if let Some(start) = find_from(s, from, prefix) {
            let n = take_alnum_us_hyphen(&s[start + prefix.len()..]);
            return Some(Hit {
                start,
                end: start + prefix.len() + n,
                kind: "slack-token",
            });
        }
    }
    None
}

fn find_authorization(s: &str, from: usize) -> Option<Hit> {
    let rel = s[from..].to_ascii_lowercase().find("authorization")?;
    let start = from + rel;
    let rest = &s.as_bytes()[start + "authorization".len()..];
    let mut i = 0;
    while i < rest.len() && rest[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < rest.len() && rest[i] == b':' {
        i += 1;
    }
    while i < rest.len() && rest[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < rest.len() && (rest[i] == b'"' || rest[i] == b'\'') {
        i += 1;
    }
    let val_start = i;
    while i < rest.len() && !rest[i].is_ascii_whitespace() && rest[i] != b'"' && rest[i] != b'\'' {
        i += 1;
    }
    if i == val_start {
        return None;
    }
    let scheme = rest[val_start..i].eq_ignore_ascii_case(b"bearer")
        || rest[val_start..i].eq_ignore_ascii_case(b"basic")
        || rest[val_start..i].eq_ignore_ascii_case(b"token");
    if scheme {
        while i < rest.len() && rest[i].is_ascii_whitespace() {
            i += 1;
        }
        while i < rest.len()
            && !rest[i].is_ascii_whitespace()
            && rest[i] != b'"'
            && rest[i] != b'\''
        {
            i += 1;
        }
    }
    Some(Hit {
        start,
        end: start + "authorization".len() + i,
        kind: "auth",
    })
}

fn is_secret_env_name(name: &str) -> bool {
    let u = name.to_ascii_uppercase();
    u.ends_with("KEY")
        || u.ends_with("TOKEN")
        || u.ends_with("SECRET")
        || u.ends_with("PASSWORD")
        || u.ends_with("PWD")
}

fn find_env_assign(s: &str, from: usize) -> Option<Hit> {
    let bytes = s.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if !is_env_name_start(bytes[i]) {
            i += 1;
            continue;
        }
        let name_start = i;
        i += 1;
        while i < bytes.len() && is_env_name_char(bytes[i]) {
            i += 1;
        }
        let name = &s[name_start..i];
        if i < bytes.len() && bytes[i] == b'=' && is_secret_env_name(name) {
            i += 1;
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q && bytes[i] != b'\n' {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == q {
                    i += 1;
                }
            } else {
                while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            return Some(Hit {
                start: name_start,
                end: i,
                kind: "env",
            });
        }
        i += 1;
    }
    None
}

fn is_env_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_env_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn find_connection(s: &str, from: usize) -> Option<Hit> {
    let mut search = from;
    while let Some(start) = find_from(s, search, "://") {
        let after = start + 3;
        let rest = &s[after..];
        if let Some(at) = rest.find('@') {
            let creds = &rest[..at];
            if creds.contains(':') && !creds.contains('/') {
                return Some(Hit {
                    start: after,
                    end: after + at,
                    kind: "connection",
                });
            }
        }
        search = start + 1;
    }
    None
}

fn find_private_key(s: &str, from: usize) -> Option<Hit> {
    let start = find_from(s, from, "-----BEGIN ")?;
    let rest = &s[start..];
    if !rest.to_ascii_uppercase().contains("PRIVATE KEY-----") {
        return None;
    }
    let end_tag = "-----END ";
    let rel_end = rest.find(end_tag)?;
    let after = &rest[rel_end + end_tag.len()..];
    let dash = after.find("-----")?;
    Some(Hit {
        start,
        end: start + rel_end + end_tag.len() + dash + 5,
        kind: "private-key",
    })
}

fn find_jwt(s: &str, from: usize) -> Option<Hit> {
    let mut search = from;
    while let Some(start) = find_from(s, search, "eyJ") {
        let rest = &s[start..];
        let Some((h, rest)) = jwt_part(rest) else {
            search = start + 1;
            continue;
        };
        if !rest.starts_with('.') {
            search = start + 1;
            continue;
        }
        let rest = &rest[1..];
        let Some((p, rest)) = jwt_part(rest) else {
            search = start + 1;
            continue;
        };
        if !rest.starts_with('.') {
            search = start + 1;
            continue;
        }
        let rest = &rest[1..];
        let Some((sig, _)) = jwt_part(rest) else {
            search = start + 1;
            continue;
        };
        if h >= 8 && p >= 8 && sig >= 8 {
            return Some(Hit {
                start,
                end: start + h + 1 + p + 1 + sig,
                kind: "jwt",
            });
        }
        search = start + 1;
    }
    None
}

fn jwt_part(s: &str) -> Option<(usize, &str)> {
    let n = s
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        .count();
    if n == 0 {
        return None;
    }
    Some((n, &s[n..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_borrows_without_allocating() {
        let r = NoopRedactor;
        let input = "sk-TESTFAKE0000000000000000";
        match r.redact(input) {
            Cow::Borrowed(s) => assert_eq!(s, input),
            Cow::Owned(_) => panic!("noop redactor must not allocate"),
        }
    }

    #[test]
    fn secret_redactor_borrows_when_clean() {
        let input = "git status && cargo test";
        match SecretRedactor.redact(input) {
            Cow::Borrowed(s) => assert_eq!(s, input),
            Cow::Owned(_) => panic!("clean input must not allocate"),
        }
    }

    #[test]
    fn masks_openai_and_aws() {
        let out =
            SecretRedactor.redact("export A=sk-TESTFAKE0000000000000000 AKIAIOSFODNN7EXAMPLE00");
        assert!(out.contains("«redacted:api-key»"), "{out}");
        assert!(
            out.contains("«redacted:aws-key»") || out.contains("«redacted:env»"),
            "{out}"
        );
    }

    #[test]
    fn no_panic_on_arbitrary_input() {
        let r = SecretRedactor;
        for s in ["", "\0", "eyJ", "://:@", "-----BEGIN ", "sk-", "AKIA"] {
            let _ = r.redact(s);
        }
    }
}
