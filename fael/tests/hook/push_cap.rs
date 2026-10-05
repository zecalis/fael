//! Read-push row cap (PLAN-fael-push-focus chunk 1): at most
//! `budget.push_rows` rows push, an open issue always shows, and the hidden
//! rows are counted by the exact call that reaches each class — the file, the
//! query's directory (same-dir), or the key (shared key).

use super::{fael, json, repo, strip_fh};
use std::path::Path;

/// An issue plus `n` decisions on `src/a.rs`; past `PUSH_HUB_ROWS` (8)
/// decisions the file is a hub.
fn seed(d: &Path, n: usize) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    for i in 0..n {
        let (ok, _, err) = fael(
            d,
            &[
                "add",
                "decision",
                &format!("decision {i}"),
                "--files",
                "src/a.rs",
            ],
            "",
        );
        assert!(ok, "{err}");
    }
}

fn read(d: &Path, session: Option<&str>) -> (bool, String) {
    read_file(d, session, "src/a.rs")
}

fn read_file(d: &Path, session: Option<&str>, file: &str) -> (bool, String) {
    let input = match session {
        Some(s) => format!(
            r#"{{"cwd":{},"session":"{s}","files":["{file}"]}}"#,
            json(d)
        ),
        None => format!(r#"{{"cwd":{},"files":["{file}"]}}"#, json(d)),
    };
    let (ok, out, _) = fael(d, &["hook", "read"], &input);
    (ok, out)
}

fn shown(out: &str) -> usize {
    // the Reply is one JSON line — row lines are `\n- [` once unescaped
    out.matches("- [").count()
}

#[test]
fn read_push_caps_at_five_with_next_call() {
    let d = repo();
    seed(&d, 7);
    let (ok, out) = read(&d, None);
    assert!(ok, "{out}");
    // 8 matching rows → 5 shown, the issue among them, plus one line
    // naming the exact next call (not the budget line)
    assert_eq!(shown(&out), 5, "{out}");
    assert!(out.contains("login loops"), "{out}");
    assert!(
        out.contains("… +3 more about this file — fael find --files src/a.rs"),
        "{out}"
    );
    assert!(!out.contains("narrow the filter"), "{out}");
}

/// PLAN-fael-say-gate chunk 6: a push whose rows fill `push_tokens` has no
/// budget for the stashed risk line — it is cut, not lost, the count line
/// still names the call to the cut rows, and a later push with room says it
/// once.
#[test]
fn a_full_budget_cuts_the_stashed_notice_not_the_count_and_a_later_push_says_it() {
    let d = repo();
    seed(&d, 7);
    // a session that starts after the seeded issue, or the issue clears the signal
    let s = "2099-01-01T00:00:00Z";
    let stop = format!(
        r#"{{"cwd":{},"session":"{s}","text":"the schema and the docs are out of sync"}}"#,
        json(&d)
    );
    assert!(fael(&d, &["hook", "stop"], &stop).0);
    let cfg = d.join(".fael/config.toml");
    std::fs::write(&cfg, "[budget]\npush_tokens = 60\n").unwrap();
    let (ok, out) = read(&d, Some(s));
    assert!(ok, "{out}");
    assert!(shown(&out) > 0 && shown(&out) < 8, "{out}");
    assert!(out.contains("fael find --files src/a.rs"), "{out}");
    assert!(!out.contains("possible problem"), "{out}");
    // room again: the cut rows and the kept notice come, once
    std::fs::write(&cfg, "[budget]\npush_tokens = 800\n").unwrap();
    let (ok, out) = read(&d, Some(s));
    assert!(ok && out.matches("possible problem").count() == 1, "{out}");
    assert!(out.contains("out of sync"), "{out}");
    let (ok, out) = read(&d, Some(s));
    assert!(ok && !out.contains("possible problem"), "{out}");
}

#[test]
fn read_push_omitted_rows_push_later_in_session() {
    let d = repo();
    seed(&d, 7);
    // omitted rows never reach seen, so later reads push them: 5 + 3
    let (ok, first) = read(&d, Some("cap1"));
    assert!(ok, "{first}");
    assert_eq!(shown(&first), 5, "{first}");
    assert!(first.contains("login loops"), "{first}");
    let (ok, second) = read(&d, Some("cap1"));
    assert!(ok, "{second}");
    assert_eq!(shown(&second), 3, "{second}");
    assert!(!second.contains("login loops"), "{second}");
    assert!(!second.contains("more about this file"), "{second}");
}

#[test]
fn read_push_hub_file_peeks_once_beside_now_rows() {
    let d = repo();
    seed(&d, 15);
    // 15 off-Focus decisions: any 5 by freshness is a guess, none at all says
    // nothing — the issue renders, a peek of 3, the rest one line naming the
    // exact call
    let (ok, out) = read(&d, None);
    assert!(ok, "{out}");
    assert_eq!(shown(&out), 1 + 3, "{out}");
    assert!(out.contains("login loops"), "{out}");
    assert!(out.contains("(4 of 16)"), "{out}");
    assert!(
        out.contains("… +12 more about this file — fael find --files src/a.rs"),
        "{out}"
    );
    // the peek rides the count line, once per file per session (01M42F5B): a
    // re-read drips no more rows and, the count told, the push is silent
    let (ok, first) = read(&d, Some("hub1"));
    assert!(
        ok && first.contains("… +12 more about this file"),
        "{first}"
    );
    let (ok, again) = read(&d, Some("hub1"));
    assert!(ok && !again.contains("context"), "{again}");
}

#[test]
fn read_push_zero_rows_means_budget_only() {
    let d = repo();
    seed(&d, 15);
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_rows = 0\n").unwrap();
    // no row cap, no hub cut: all 16 fit the default token budget, no omitted line
    let (ok, out) = read(&d, None);
    assert!(ok, "{out}");
    assert_eq!(shown(&out), 16, "{out}");
    assert!(!out.contains("more about this file"), "{out}");
}

#[test]
fn edit_hides_same_dir_neighbour_but_names_the_dir_call() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    for (text, f) in [("on a", "src/a.rs"), ("neighbour", "src/b.rs")] {
        let (ok, _, err) = fael(&d, &["add", "decision", text, "--files", f], "");
        assert!(ok, "{err}");
    }
    // stamped rows whose files still match earn no hint (PLAN-fael-file-hash
    // chunk 2) — strip `fh` so this still exercises the legacy retire ask
    strip_fh(&d, "on a");
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "edit"], &input);
    assert!(ok, "{out}");
    // the neighbour never renders, but the line names the exact call that does
    assert!(!out.contains("neighbour"), "{out}");
    assert!(
        out.contains("… +1 more in src/ — fael find --files src/"),
        "{out}"
    );
    let (ok, found, _) = fael(&d, &["find", "--files", "src/"], "");
    assert!(ok && found.contains("neighbour"), "{found}");
    // an edit push asks to retire a row the code outgrew; a read push never does
    assert!(out.contains("--supersedes "), "{out}");
    let (ok, read, _) = fael(&d, &["hook", "read"], &input);
    assert!(ok && !read.contains("--supersedes "), "{read}");
}

#[test]
fn budget_cut_names_the_dir_call_for_a_same_dir_now_row() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    for i in 0..6 {
        let (ok, _, err) = fael(
            &d,
            &[
                "add",
                "issue",
                &format!("aaaa issue number {i} on the main file with some words"),
                "--files",
                "src/a.rs",
            ],
            "",
        );
        assert!(ok, "{err}");
    }
    // an issue on the neighbour file is same-dir (tier 1) and Now — the budget
    // cuts it, so its count line must name the directory call, not the file call
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "NEIGHBOUR issue on b.rs",
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_tokens = 150\n").unwrap();
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "edit"], &input);
    assert!(ok, "{out}");
    // the budget cut a tier-1 row: the dir call reaches it, `--files <f>` does not
    assert!(
        out.contains("… +1 more in src/ — fael find --files src/"),
        "{out}"
    );
    assert!(!out.contains("more about this file"), "{out}");
    let (ok, found, _) = fael(&d, &["find", "--files", "src/"], "");
    assert!(ok && found.contains("NEIGHBOUR"), "{found}");
}

#[test]
fn read_push_drops_shared_key_rows_outside_focus() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // an exact hit on src/a.rs, and another file sharing its key (tier 2).
    // Different kinds, or self-heal would supersede one on the shared key.
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "on a",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
        ],
        "",
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "elsewhere",
            "--files",
            "lib/z.rs",
            "--key",
            "auth:session",
        ],
        "",
    );
    assert!(ok, "{err}");
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "read"], &input);
    assert!(ok, "{out}");
    // no session Focus holds the key: the sibling is neither pushed nor
    // counted (decision push:shared-key-siblings), `find --key` still has it
    assert!(!out.contains("elsewhere"), "{out}");
    assert!(!out.contains("#auth:session —"), "{out}");
    let (ok, found, _) = fael(&d, &["find", "--key", "auth:session"], "");
    assert!(ok && found.contains("elsewhere"), "{found}");
}

/// PLAN-fael-say-gate chunk 2: rows that all fit name no cut, so no count line.
#[test]
fn count_line_says_nothing_without_a_cut() {
    let d = repo();
    seed(&d, 2);
    let (ok, out) = read(&d, Some("c0"));
    assert!(ok && shown(&out) == 3, "{out}");
    assert!(!out.contains("more about"), "{out}");
}

/// PLAN-fael-say-gate chunk 2 (01M42F5B): the count line is said once per
/// file per session; a new session hears it again.
#[test]
fn count_line_is_said_once_per_file_per_session() {
    let d = repo();
    seed(&d, 15);
    let (_, first) = read(&d, Some("c1"));
    assert!(first.contains("… +12 more about this file"), "{first}");
    let (_, again) = read(&d, Some("c1"));
    assert!(!again.contains("more about"), "{again}");
    let (_, other) = read(&d, Some("c2"));
    assert!(other.contains("… +12 more about this file"), "{other}");
}

/// A decision on `file` whose title is shorter than its text: it has a body.
fn titled(d: &Path, file: &str) {
    std::fs::write(d.join(file), "// x\n").unwrap();
    let (ok, _, err) = fael(
        d,
        &[
            "add",
            "decision",
            "the cache keys include the tenant so two tenants never share a row",
            "--title",
            "cache keys include the tenant",
            "--files",
            file,
        ],
        "",
    );
    assert!(ok, "{err}");
}

/// PLAN-fael-say-gate chunk 2: rows with no body earn no `bodies:` line.
#[test]
fn bodies_line_says_nothing_without_a_body() {
    let d = repo();
    seed(&d, 1);
    let (ok, out) = read(&d, Some("b0"));
    assert!(ok && shown(&out) == 2, "{out}");
    assert!(!out.contains("bodies:"), "{out}");
}

/// PLAN-fael-say-gate chunk 2 (01M42F5B): the `bodies:` line is said once per
/// session, whatever file the next row with a body sits on.
#[test]
fn bodies_line_is_said_once_per_session() {
    let d = repo();
    titled(&d, "src/a.rs");
    titled(&d, "src/b.rs");
    let (_, a) = read_file(&d, Some("b1"), "src/a.rs");
    assert!(a.contains("bodies: fael find <id>"), "{a}");
    let (_, b) = read_file(&d, Some("b1"), "src/b.rs");
    assert!(shown(&b) == 1 && !b.contains("bodies:"), "{b}");
    let (_, b) = read_file(&d, Some("b2"), "src/b.rs");
    assert!(b.contains("bodies: fael find <id>"), "{b}");
}

/// Each count line is said once per what it counts: a read tells the file's
/// count, and a later edit of that file (edits keep the same-dir tier reads
/// skip) still says its new `… more in src/` line.
#[test]
fn a_new_count_line_is_said_after_the_file_count() {
    let d = repo();
    seed(&d, 15);
    std::fs::write(d.join("src/z.rs"), "// z\n").unwrap();
    for i in 0..10 {
        let (ok, _, err) = fael(
            &d,
            &["add", "decision", &format!("z {i}"), "--files", "src/z.rs"],
            "",
        );
        assert!(ok, "{err}");
    }
    let (_, first) = read(&d, Some("n1"));
    assert!(first.contains("more about this file"), "{first}");
    assert!(!first.contains("more in src/"), "{first}");
    let input = format!(
        r#"{{"cwd":{},"session":"n1","files":["src/a.rs"]}}"#,
        json(&d)
    );
    let (ok, edit, err) = fael(&d, &["hook", "edit"], &input);
    assert!(ok, "{err}");
    // no row left to say: the header rides the first count line still unsaid
    assert!(shown(&edit) == 0, "{edit}");
    assert!(
        edit.contains(r#""context":"fael mem for src/a.rs (0 of "#)
            && edit.contains("):\\n… +10 more in src/ — fael find --files src/\\n"),
        "{edit}"
    );
    assert!(!edit.contains("more about this file"), "{edit}");
}
