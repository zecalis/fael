//! The scope rule shared by the board and the contracts a chunk start prints.

/// SPEC §2: two scopes overlap when a path of one is a prefix of a path of the other on a
/// `/` boundary (`src/` covers `src/a.rs`, `src/a` does not cover `src/ab.rs`).
pub fn overlap(a: &[String], b: &[String]) -> bool {
    a.iter()
        .any(|x| b.iter().any(|y| under(x, y) || under(y, x)))
}

/// `x` is `y` or below it on a `/` boundary.
pub fn under(x: &str, y: &str) -> bool {
    let (x, y) = (x.trim_end_matches('/'), y.trim_end_matches('/'));
    x == y || x.strip_prefix(y).is_some_and(|r| r.starts_with('/'))
}
