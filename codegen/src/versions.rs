//! Language target versions: what the version in `rust#1.70.0` means, and
//! which versions a generator handles.
//!
//! A declared version is a **minimum**: the oldest toolchain the generated
//! code must build on. A generator registers at its **floor**, the oldest
//! version it generates for, and handles every version from there up: a
//! declared version gets the generator with the newest floor at or below it,
//! which receives the declared version in its request.
//!
//! Output and support only change at a generator's **boundaries** (its floor,
//! and each version listed in [`Versions::changes`]), so conformance proves
//! every version by running at each boundary. A feature supported only from
//! some version on says so with [`Support::from`](crate::features::Support::from).
//!
//! See the *Language target versions* proposal in the docs.

use std::cmp::Ordering;
use std::fmt;

/// A language or toolchain version: dotted numbers, compared numerically,
/// with missing parts as zero, so `1.70` is `1.70.0` and `5.0` is `5`.
#[derive(Debug, Clone)]
pub struct LangVersion(Vec<u64>);

impl LangVersion {
    /// `None` unless `text` is dotted numbers (`1.70.0`, `5.0`, `3`).
    pub fn parse(text: &str) -> Option<Self> {
        let parts = text
            .split('.')
            .map(|part| part.parse::<u64>().ok().filter(|_| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())))
            .collect::<Option<Vec<_>>>()?;
        Some(Self(parts))
    }

    fn part(&self, i: usize) -> u64 {
        self.0.get(i).copied().unwrap_or(0)
    }

    /// This version cut to `other`'s precision: `5.9.3` against `5.9` is `5.9`.
    fn to_precision_of(&self, other: &LangVersion) -> LangVersion {
        LangVersion((0..other.0.len()).map(|i| self.part(i)).collect())
    }
}

impl Ord for LangVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (0..self.0.len().max(other.0.len()))
            .map(|i| self.part(i).cmp(&other.part(i)))
            .find(|o| o.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for LangVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for LangVersion {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for LangVersion {}

impl fmt::Display for LangVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(u64::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

/// A version written by a generator about itself. Its own constants, so a
/// malformed one is a bug there: conformance checks every one with
/// [`Versions::problems`].
fn known(text: &'static str) -> LangVersion {
    LangVersion::parse(text).unwrap_or_else(|| panic!("`{text}` isn't a version"))
}

/// Which versions of its language a generator handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versions {
    /// The oldest version it generates for: what it registers under, and
    /// what `comline new` scaffolds.
    pub floor: &'static str,
    /// Every version above the floor where its output or support changes,
    /// oldest first. Between two boundaries the output is the same.
    pub changes: &'static [&'static str],
    /// The newest toolchain its output has been verified against, to the
    /// precision written: `5.9` covers every `5.9.x`.
    pub verified_up_to: &'static str,
    /// Whether the language promises newer toolchains build older code, as
    /// Rust's does (editions aside). Then a declared version past
    /// [`verified_up_to`](Self::verified_up_to) is still verified.
    pub forward_compatible: bool,
}

impl Versions {
    /// Every boundary, the floor first.
    pub fn boundaries(&self) -> Vec<&'static str> {
        std::iter::once(self.floor).chain(self.changes.iter().copied()).collect()
    }

    /// The boundary `version` generates as: the newest at or below it.
    /// `None` below the floor.
    pub fn boundary_for(&self, version: &LangVersion) -> Option<&'static str> {
        self.boundaries().into_iter().rev().find(|b| known(b) <= *version)
    }

    /// Whether generating for `version` is verified: within the verified
    /// range, or past it in a forward-compatible language.
    pub fn is_verified(&self, version: &LangVersion) -> bool {
        let ceiling = known(self.verified_up_to);
        self.forward_compatible || version.to_precision_of(&ceiling) <= ceiling
    }

    /// What's inconsistent about these versions, given the generator's
    /// `support` table: an unparsable version, boundaries out of order or past
    /// what's verified, or a support gate at a version that isn't a boundary.
    /// Empty when all is well.
    pub fn problems(&self, support: crate::features::SupportTable) -> Vec<String> {
        let all = self.boundaries().into_iter().chain([self.verified_up_to]);
        let unparsable: Vec<String> = all
            .filter(|v| LangVersion::parse(v).is_none())
            .map(|v| format!("`{v}` isn't a version"))
            .collect();
        if !unparsable.is_empty() {
            return unparsable;
        }
        let mut problems = Vec::new();
        let boundaries = self.boundaries();
        for pair in boundaries.windows(2) {
            if known(pair[0]) >= known(pair[1]) {
                problems.push(format!("boundary `{}` doesn't come after `{}`", pair[1], pair[0]));
            }
        }
        let newest = boundaries.last().expect("the floor is a boundary");
        if known(newest) > known(self.verified_up_to) {
            problems.push(format!(
                "boundary `{newest}` is past `verified_up_to` (`{}`): a boundary is a version conformance runs at",
                self.verified_up_to
            ));
        }
        for (feature, support) in support {
            if let Some(since) = support.since {
                match LangVersion::parse(since.version) {
                    None => problems.push(format!("`{}`'s gate `{}` isn't a version", feature.id(), since.version)),
                    Some(gate) if !boundaries.iter().any(|b| known(b) == gate) => problems.push(format!(
                        "`{}` is gated at `{}`, which isn't a boundary - list it in `changes`",
                        feature.id(),
                        since.version
                    )),
                    Some(_) => {}
                }
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::{Feature, Support, SupportTable};

    fn v(text: &str) -> LangVersion {
        LangVersion::parse(text).unwrap()
    }

    #[test]
    fn versions_compare_numerically_with_missing_parts_as_zero() {
        assert_eq!(v("1.70"), v("1.70.0"));
        assert!(v("1.9.0") < v("1.70.0"));
        assert!(v("5.0") < v("5.4"));
        assert!(v("2") > v("1.99.99"));
        assert_eq!(v("1.70").to_string(), "1.70");
    }

    #[test]
    fn only_dotted_numbers_parse() {
        for bad in ["", "1.", ".1", "1.x", "v1.70", "1.70.0-beta", "+1"] {
            assert!(LangVersion::parse(bad).is_none(), "`{bad}` parsed");
        }
    }

    const RUST: Versions = Versions {
        floor: "1.70.0",
        changes: &["1.75.0", "1.81.0"],
        verified_up_to: "1.90.0",
        forward_compatible: true,
    };

    #[test]
    fn a_version_generates_as_the_newest_boundary_at_or_below_it() {
        assert_eq!(RUST.boundary_for(&v("1.69")), None);
        assert_eq!(RUST.boundary_for(&v("1.70")), Some("1.70.0"));
        assert_eq!(RUST.boundary_for(&v("1.80.3")), Some("1.75.0"));
        assert_eq!(RUST.boundary_for(&v("2.0")), Some("1.81.0"));
    }

    #[test]
    fn past_the_ceiling_is_verified_only_in_a_forward_compatible_language() {
        assert!(RUST.is_verified(&v("1.99")));
        let ts = Versions { floor: "5.0", changes: &[], verified_up_to: "5.9", forward_compatible: false };
        assert!(ts.is_verified(&v("5.9.3")));
        assert!(!ts.is_verified(&v("6.0")));
    }

    #[test]
    fn a_gate_must_be_a_boundary() {
        const TABLE: SupportTable = &[
            (Feature::Connections, Support::supported().from("1.75.0", "async fn in traits")),
            (Feature::Unions, Support::supported().from("1.76.0", "nothing in particular")),
        ];
        assert_eq!(
            RUST.problems(TABLE),
            ["`unions` is gated at `1.76.0`, which isn't a boundary - list it in `changes`"]
        );
    }

    #[test]
    fn boundaries_go_up_and_stay_verified() {
        let bad = Versions { floor: "1.70.0", changes: &["1.70", "2.0"], verified_up_to: "1.90", forward_compatible: true };
        assert_eq!(
            bad.problems(&[]),
            [
                "boundary `1.70` doesn't come after `1.70.0`",
                "boundary `2.0` is past `verified_up_to` (`1.90`): a boundary is a version conformance runs at",
            ]
        );
        let unparsable = Versions { floor: "one", ..RUST };
        assert_eq!(unparsable.problems(&[]), ["`one` isn't a version"]);
    }
}
