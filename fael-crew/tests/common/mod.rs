//! Shared by the integration tests: these drive the `fael` binary, which hands the crew verbs to
//! `fael-crew`. Cargo builds only this package's own binary for a test, so `fael` is looked up
//! next to it — present after `cargo test --workspace`.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// Normalise `chunk start` stdout for the start golden (start_gates.rs):
/// fixture ULIDs, RFC3339 times, calendar days, 40-hex shas and the scratch
/// paths — the same bless pattern as the hook replay test.
pub fn normalise_start(out: &str, tmp: &std::path::Path, ids: &[(String, &str)]) -> String {
    let mut s = out.to_string();
    for cand in [
        tmp.to_string_lossy().into_owned(),
        tmp.join("repo").to_string_lossy().into_owned(),
    ] {
        s = s.replace(&cand, "<TMP>");
    }
    if let Ok(c) = tmp.canonicalize() {
        s = s.replace(&c.to_string_lossy().into_owned(), "<TMP>");
    }
    // Full ids first, then their unique prefixes (the board shortens to the
    // shortest unique prefix; the fixture's share 8 chars, diverge at 9).
    for (orig, fixed) in ids {
        s = s.replace(orig, fixed);
    }
    for (orig, fixed) in ids {
        for len in (9..orig.len()).rev() {
            if let Some(o) = orig.get(..len) {
                s = s.replace(o, fixed);
            }
        }
    }
    s = squash_times(s);
    s = squash_hex(s);
    s
}

/// Two passes: full RFC3339 stamps first, then bare calendar days.
/// Byte-wise (patterns are ASCII) reassembled as bytes, so multi-byte
/// UTF-8 (`·`, `—`, `→`) passes through untouched.
fn squash_times(s: String) -> String {
    let first = replace_bytes(s.as_bytes(), stamp_len, "<TIME>");
    let second = replace_bytes(&first, date_len, "<DATE>");
    String::from_utf8(second).expect("ascii replacements keep utf-8")
}

fn replace_bytes(b: &[u8], at: impl Fn(&[u8], usize) -> Option<usize>, rep: &str) -> Vec<u8> {
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(len) = at(b, i) {
            o.extend_from_slice(rep.as_bytes());
            i += len;
        } else {
            o.push(b[i]);
            i += 1;
        }
    }
    o
}

fn digits(b: &[u8], i: usize, n: usize) -> bool {
    b.len() >= i + n && b[i..i + n].iter().all(|c| c.is_ascii_digit())
}

/// `YYYY-MM-DDTHH:MM:SS(.frac)?Z` at `i`: its length, else None.
fn stamp_len(b: &[u8], i: usize) -> Option<usize> {
    if !(digits(b, i, 4)
        && b.get(i + 4) == Some(&b'-')
        && digits(b, i + 5, 2)
        && b.get(i + 7) == Some(&b'-')
        && digits(b, i + 8, 2)
        && b.get(i + 10) == Some(&b'T')
        && digits(b, i + 11, 2)
        && b.get(i + 13) == Some(&b':')
        && digits(b, i + 14, 2)
        && b.get(i + 16) == Some(&b':')
        && digits(b, i + 17, 2))
    {
        return None;
    }
    let mut len = 19;
    if b.get(i + 19) == Some(&b'.') {
        let mut k = i + 20;
        while b.get(k).is_some_and(|c| c.is_ascii_digit()) {
            k += 1;
        }
        if k == i + 20 {
            return None;
        }
        len = k - i;
    }
    if b.get(i + len) != Some(&b'Z') {
        return None;
    }
    Some(len + 1)
}

/// `YYYY-MM-DD` at `i` with non-digit boundaries: its length, else None.
fn date_len(b: &[u8], i: usize) -> Option<usize> {
    if !(digits(b, i, 4)
        && b.get(i + 4) == Some(&b'-')
        && digits(b, i + 5, 2)
        && b.get(i + 7) == Some(&b'-')
        && digits(b, i + 8, 2))
    {
        return None;
    }
    if i > 0 && b[i - 1].is_ascii_digit() {
        return None;
    }
    if b.get(i + 10).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(10)
}

/// 40 lowercase hex (a commit sha) with non-hex boundaries → `<SHA>`.
fn squash_hex(s: String) -> String {
    let b = s.as_bytes();
    let is_hex = |c: &u8| c.is_ascii_digit() || (b'a'..=b'f').contains(c);
    let at = |b: &[u8], i: usize| {
        (b.len() >= i + 40
            && b[i..i + 40].iter().all(is_hex)
            && (i == 0 || !is_hex(&b[i - 1]))
            && b.get(i + 40).is_none_or(|c| !is_hex(c)))
        .then_some(40)
    };
    String::from_utf8(replace_bytes(b, at, "<SHA>")).expect("ascii replacements keep utf-8")
}
/// The `fael` binary built beside `fael-crew`, with a freshness guard.
/// A partial build (`cargo test -p fael-crew`) leaves a stale `target/debug/fael`
/// whose `plan --help` prints the old verbs while `fael-crew` is fresh —
/// the dispatch mismatch wastes a debug session. Fail fast here instead.
pub fn fael_bin() -> PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(probe_fael).clone()
}

fn probe_fael() -> PathBuf {
    let crew = PathBuf::from(env!("CARGO_BIN_EXE_fael-crew"));
    let fael = crew.with_file_name(format!("fael{}", std::env::consts::EXE_SUFFIX));
    assert!(
        fael.is_file(),
        "{} is missing — run `cargo test --workspace` (or `cargo build -p fael`) first",
        fael.display()
    );
    // Freshness by content, not mtime: the test links the current
    // `fael-core`, so every verb it knows must appear in the `fael`
    // binary's help. A stale `fael` prints the old list — rebuild both.
    // The probe is a fael run like any other: scratch state dir, so it never
    // lands in the developer's real usage.jsonl (fael:01M4F3G0).
    let out = Command::new(&fael)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join("fael-crew-probe"),
        )
        .args(["plan", "--help"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    // `show` is an alias for `export`: named in PLAN_VERBS but not in the usage line.
    let missing: Vec<&&str> = fael_core::work::PLAN_VERBS
        .iter()
        .filter(|v| **v != "show" && !out.contains(**v))
        .collect();
    assert!(
        missing.is_empty(),
        "stale {} — `fael plan --help` lacks {missing:?} (partial build: only fael-crew rebuilt); run `cargo test --workspace` (or `cargo build --workspace`) first",
        fael.display()
    );
    fael
}
