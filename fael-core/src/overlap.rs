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
    let mut prefix = false;
    for x in a {
        for y in b {
            let (xt, yt) = (x.trim_end_matches('/'), y.trim_end_matches('/'));
            if xt == yt {
                return Some(OverlapClass::Exact);
            }
            if under(x, y) || under(y, x) {
                prefix = true;
            }
        }
    }
    prefix.then_some(OverlapClass::Prefix)
}

/// `x` is `y` or below it on a `/` boundary.
pub fn under(x: &str, y: &str) -> bool {
    let (x, y) = (x.trim_end_matches('/'), y.trim_end_matches('/'));
    x == y || x.strip_prefix(y).is_some_and(|r| r.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn overlap_is_class_is_some() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(overlap(&v(&["src/"]), &v(&["src/a.rs"])));
        assert!(!overlap(&v(&["src/a.rs"]), &v(&["src/b.rs"])));
    }
}
