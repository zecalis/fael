//! PLAN-fael-row-retire w1 (from PLAN-fael-decision-held chunk 2): an open
//! decision or note — any writer — on a file this session edited, changed
//! since the row was written, is named once, at the push after stop (here an
//! edit of a file no row is on), with bump / close / supersede ready to run.
//! Never at the edit before stop, never an issue, an unedited or unchanged
//! file, or a row already closed or superseded.

use super::{fael, fael_env, json};
use std::path::Path;

const S: &str = "2020-01-01T00:00:00Z";
const HELD: &str = "changed since it was written";

fn hook(d: &Path, event: &str, file: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":"{S}","files":["{file}"]}}"#,
        json(d)
    );
    let (ok, out, err) = fael(d, &["hook", event], &input);
    assert!(ok, "{err}");
    out
}

/// Change `file` on disk, then the session's edit hook for it.
fn edit(d: &Path, file: &str) -> String {
    std::fs::write(d.join(file), format!("fn {}() {{}}\n", fael_core::ulid())).unwrap();
    hook(d, "edit", file)
}

fn stop(d: &Path) {
    let input = format!(r#"{{"cwd":{},"session":"{S}","text":"done"}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}{err}");
}

/// File a `kind` row; `session` stamps it as that session's, `more` adds flags.
fn add(
    d: &Path,
    kind: &str,
    text: &str,
    file: &str,
    session: Option<&str>,
    more: &[&str],
) -> String {
    let (ok, out, err) = add_raw(d, kind, text, &[file], session, more);
    assert!(ok, "{err}");
    out.split_whitespace()
        .find(|w| w.starts_with("01"))
        .unwrap()
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string()
}

/// File a row on several files (comma-joined, like the CLI takes them).
fn add_files(d: &Path, kind: &str, text: &str, files: &[&str], session: Option<&str>) -> String {
    let (ok, out, err) = add_raw(d, kind, text, files, session, &[]);
    assert!(ok, "{err}");
    out.split_whitespace()
        .find(|w| w.starts_with("01"))
        .unwrap()
        .trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string()
}

/// File a row, keeping stdout and stderr apart (c2 prints held rows on stdout).
fn add_raw(
    d: &Path,
    kind: &str,
    text: &str,
    files: &[&str],
    session: Option<&str>,
    more: &[&str],
) -> (bool, String, String) {
    let envs: Vec<(&str, &str)> = session.map(|s| ("FAEL_SESSION", s)).into_iter().collect();
    let joined = files.join(",");
    let args = [&["add", kind, text, "--files", &joined][..], more].concat();
    fael_env(d, &args, "", &envs)
}

/// The short id `out` names `id` by — fael abbreviates, and no other row's
/// short id is a prefix of this one's. `None` when `out` never names it.
fn said_id(out: &str, id: &str) -> Option<String> {
    out.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|w| w.len() >= 8 && id.starts_with(w))
        .map(String::from)
}

fn decide(d: &Path, text: &str, file: &str, session: Option<&str>) -> String {
    add(d, "decision", text, file, session, &[])
}

fn repo() -> std::path::PathBuf {
    let d = super::repo();
    for f in ["src/a.rs", "src/b.rs"] {
        std::fs::write(d.join(f), "fn x() {}\n").unwrap();
    }
    d
}

/// c2: add on a file with a held decision prints its id with the three
/// verdicts ready. Two files, so editing one leaves the row live — a
/// one-file row goes dormant and is left out, like chunk done's (w2).
/// 01M5H5AQV: a session that edited a file with a moved decision gets no
/// held line at the edit, at Stop, or on its next push.
#[test]
fn no_held_line_on_the_push_after_stop() {
    let d = repo();
    decide(&d, "column widths hold still", "src/a.rs", Some(S));
    assert!(!edit(&d, "src/a.rs").contains(HELD));
    stop(&d);
    let out = hook(&d, "edit", "src/c.rs");
    assert!(!out.contains(HELD) && !out.contains("fael bump"), "{out}");
}

/// A `from: user` row says to ask the user first.
#[test]
fn a_from_user_row_says_ask_the_user() {
    let d = repo();
    let (ok, _, err) = add_raw(
        &d,
        "decision",
        "the user said so",
        &["src/a.rs", "src/b.rs"],
        None,
        &["--from", "user"],
    );
    assert!(ok, "{err}");
    edit(&d, "src/a.rs");
    let (ok, out, err) = add_raw(&d, "note", "n", &["src/a.rs"], Some(S), &[]);
    assert!(ok, "{err}");
    assert!(out.contains("ask them only before"), "{out}");
}

#[test]
fn add_on_a_file_with_a_held_decision_names_it_with_verdicts() {
    let d = repo();
    let id = add_files(
        &d,
        "decision",
        "two-file rule",
        &["src/a.rs", "src/b.rs"],
        Some("other"),
    );
    edit(&d, "src/a.rs");
    let (ok, out, err) = add_raw(&d, "decision", "new rule", &["src/a.rs"], Some(S), &[]);
    assert!(ok, "{err}");
    let short = said_id(&out, &id).unwrap_or_default();
    assert!(!short.is_empty(), "held id named: {out}");
    for want in [
        format!("fael bump {short}"),
        format!(r#"fael close {short} "now in `src/a.rs`""#),
        format!("--supersedes {short}"),
    ] {
        assert!(out.contains(&want), "{want}: {out}");
    }
}

/// c2: add on a clean file prints nothing extra.
#[test]
fn add_on_a_clean_file_prints_nothing_extra() {
    let d = repo();
    let (ok, out, err) = add_raw(&d, "decision", "fresh rule", &["src/a.rs"], Some(S), &[]);
    assert!(ok, "{err}");
    assert!(!out.contains(HELD), "{out}");
    assert!(!out.contains("fael bump"), "{out}");
}

/// c2 fix: `fael add --json` keeps stdout one JSON row per line — the held
/// text is a warning on stderr, never mixed into the JSONL stream.
#[test]
fn add_json_keeps_the_held_text_off_stdout() {
    let d = repo();
    let id = add_files(
        &d,
        "decision",
        "two-file rule",
        &["src/a.rs", "src/b.rs"],
        Some("other"),
    );
    edit(&d, "src/a.rs");
    let (ok, out, err) = fael_env(
        &d,
        &["add", "--json", "note", "json row", "--files", "src/a.rs"],
        "",
        &[("FAEL_SESSION", S)],
    );
    assert!(ok, "{err}");
    assert!(
        !out.is_empty() && out.lines().all(|l| l.starts_with('{')),
        "stdout is JSONL only: {out}"
    );
    assert!(
        said_id(&err, &id).is_some(),
        "held row named on stderr: {err}"
    );
}

/// c2 fix: the batch `--json -` stream stays JSONL too.
#[test]
fn add_json_batch_keeps_the_held_text_off_stdout() {
    let d = repo();
    let id = add_files(
        &d,
        "decision",
        "two-file rule",
        &["src/a.rs", "src/b.rs"],
        Some("other"),
    );
    edit(&d, "src/a.rs");
    let stdin = r#"[{"kind":"note","text":"batch row","files":["src/a.rs"]}]"#;
    let (ok, out, err) = fael_env(&d, &["add", "--json", "-"], stdin, &[("FAEL_SESSION", S)]);
    assert!(ok, "{err}");
    assert!(
        !out.is_empty() && out.lines().all(|l| l.starts_with('{')),
        "stdout is JSONL only: {out}"
    );
    assert!(
        said_id(&err, &id).is_some(),
        "held row named on stderr: {err}"
    );
}
