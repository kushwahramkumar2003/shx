//! Secret redaction (frozen `Redactor` trait).
//!
//! The full pattern set lands in T-303. This module ships the trait and a
//! no-op implementation so memory and backends can compile against it now.

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

/// Identity redactor. Used until T-303 lands the pattern set.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopRedactor;

impl Redactor for NoopRedactor {
    fn redact<'a>(&self, input: &'a str) -> Redacted<'a> {
        Cow::Borrowed(input)
    }
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
}
