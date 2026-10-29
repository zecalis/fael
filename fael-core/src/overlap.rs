//! The scope rule shared by the board and the contracts a chunk start prints.

/// How two scopes share files (decision 01M5ABTD: pure path math, never a
/// guess — `Exact` when a path names the same file, else `Prefix` when a dir
/// covers the other's file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapClass {
    Exact,
    Prefix,
}

impl OverlapClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            OverlapClass::Exact => "exact",
            OverlapClass::Prefix => "prefix",
        }
    }
}

/// SPEC §2: two scopes overlap when a path of one is a prefix of a path of the other on a
/// `/` boundary (`src/` covers `src/a.rs`, `src/a` does not cover `src/ab.rs`).
pub fn overlap(a: &[String], b: &[String]) -> bool {
    overlap_class(a, b).is_some()
}

/// The class of the overlap, if any: `Exact` wins as soon as one path names
/// the same file on both sides, else `Prefix` for a dir covering a file.
pub fn overlap_class(a: &[String], b: &[String]) -> Option<OverlapClass> {
    overlap_how(a, b).map(|(c, _)| c)
}

/// The overlap with its witness path, if any: `Exact` returns the shared
/// path (the `a` side), else `Prefix` returns the deeper path (the file the
/// dir covers) — what the board shows as `overlaps_how[].path`.
pub fn overlap_how(a: &[String], b: &[String]) -> Option<(OverlapClass, String)> {
    for x in a {
        for y in b {
            let (xt, yt) = (x.trim_end_matches('/'), y.trim_end_matches('/'));
            if xt == yt {
                return Some((OverlapClass::Exact, x.clone()));
            }
        }
    }
    let mut witness: Option<String> = None;
    for x in a {
        for y in b {
            if under(x, y) || under(y, x) {
                let deeper = if x.len() >= y.len() { x } else { y };
                if witness.is_none() {
                    witness = Some(deeper.clone());
                }
            }
        }
    }
    witness.map(|p| (OverlapClass::Prefix, p))
}

/// `x` is `y` or below it on a `/` boundary.
pub fn under(x: &str, y: &str) -> bool {
    let (x, y) = (x.trim_end_matches('/'), y.trim_end_matches('/'));
    x == y || x.strip_prefix(y).is_some_and(|r| r.starts_with('/'))
}

/// `x` is `y` or below it, or `y` is a file and `x` is its test beside it in the same
/// directory (`a.ts` → `a.test.ts` / `a.spec.ts` / `a_test.ts`): a chunk that edits a file
/// edits its test, so that is no "outside declared scope".
pub fn within(x: &str, y: &str) -> bool {
    if under(x, y) {
        return true;
    }
    let (dx, fx) = x.rsplit_once('/').unwrap_or(("", x));
    let (dy, fy) = y.rsplit_once('/').unwrap_or(("", y));
    let (Some((sx, ex)), Some((sy, ey))) = (fx.rsplit_once('.'), fy.rsplit_once('.')) else {
        return false;
    };
    dx == dy
        && ex == ey
        && [".test", ".spec", "_test"]
            .iter()
            .any(|t| sx.strip_suffix(t) == Some(sy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn within_takes_a_test_beside_a_scope_file() {
        assert!(within("src/series.test.ts", "src/series.ts"));
        assert!(within("src/series.spec.ts", "src/series.ts"));
        assert!(within("src/series_test.ts", "src/series.ts"));
        assert!(within("src/a/b.rs", "src/a/"));
        assert!(!within("src/other.test.ts", "src/series.ts"));
        assert!(!within("lib/series.test.ts", "src/series.ts"));
        assert!(!within("src/series.test.js", "src/series.ts"));
        // a scope dir never pairs; only a file does
        assert!(!within("src/x.test.ts", "src/x"));
        assert!(!within("src/a.test.b", "src/a.b/"));
    }

    fn v(x: &[&str]) -> Vec<String> {
        x.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn class_names_exact_prefix_or_none() {
        use OverlapClass::{Exact, Prefix};
        assert_eq!(
            overlap_class(&v(&["src/a.rs"]), &v(&["src/a.rs"])),
            Some(Exact)
        );
        assert_eq!(overlap_class(&v(&["src/"]), &v(&["src/"])), Some(Exact));
        assert_eq!(
            overlap_class(&v(&["src/"]), &v(&["src/a.rs"])),
            Some(Prefix)
        );
        assert_eq!(overlap_class(&v(&["src/a.rs"]), &v(&["src"])), Some(Prefix));
        // an exact path anywhere wins over a prefix elsewhere
        assert_eq!(
            overlap_class(&v(&["src/", "docs/b.md"]), &v(&["src/x.rs", "docs/b.md"])),
            Some(Exact)
        );
        assert_eq!(overlap_class(&v(&["src/a.rs"]), &v(&["src/b.rs"])), None);
        assert_eq!(overlap_class(&v(&["src/a"]), &v(&["src/ab.rs"])), None);
        assert_eq!(overlap_class(&v(&[]), &v(&["src/"])), None);
    }

    #[test]
    fn how_names_the_witness_path() {
        use OverlapClass::{Exact, Prefix};
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            overlap_how(&v(&["src/a.rs"]), &v(&["src/a.rs"])),
            Some((Exact, "src/a.rs".to_string()))
        );
        // prefix: the deeper path (the file), whichever side holds it
        assert_eq!(
            overlap_how(&v(&["src/"]), &v(&["src/a.rs"])),
            Some((Prefix, "src/a.rs".to_string()))
        );
        assert_eq!(
            overlap_how(&v(&["src/a.rs"]), &v(&["src/"])),
            Some((Prefix, "src/a.rs".to_string()))
        );
        assert_eq!(overlap_how(&v(&["src/a.rs"]), &v(&["src/b.rs"])), None);
    }

    #[test]
    fn overlap_is_class_is_some() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(overlap(&v(&["src/"]), &v(&["src/a.rs"])));
        assert!(!overlap(&v(&["src/a.rs"]), &v(&["src/b.rs"])));
    }
}
