//! `experience` (PLAN-fael-experience-loop chunk 4), apart from
//! `experience.rs` to keep it under the file-size limit.

use super::experience::{Commit, Experience, experience};
use super::said::tests::rows;
use crate::Log;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn id(l: char) -> String {
    format!("01K{}", l.to_string().repeat(23))
}

fn issue(l: char, day: &str, file: &str, rest: &str) -> String {
    format!(
        "{{\"v\":1,\"id\":\"{}\",\"ts\":\"2026-{day}T00:10:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[\"{file}\"]{rest}}}\n",
        id(l)
    )
}

fn close(l: char, day: &str, text: &str) -> String {
    format!(
        "{{\"v\":1,\"id\":\"X{l}\",\"ts\":\"2026-{day}T00:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"{text}\",\"files\":[],\"ref\":\"{}\"}}\n",
        id(l)
    )
}

/// A: closed with a sha and a check, re-filed by F · B: filed by s1 after a
/// review finding on its file, closed with `(#282)` · C: closed in words, a
/// commit names it · D: closed with a number that is no sha, re-filed by G ·
/// E: closed with a sha before the first usage.
#[test]
fn fixes_checks_repeats_and_recall() {
    let log = Log {
        rows: rows(
            &(issue('A', "10-01", "a.rs", "")
                + &issue('B', "10-01", "b.rs", r#","session":"s1""#)
                + &issue('C', "10-01", "c.rs", "")
                + &issue('D', "10-01", "d.rs", "")
                + &issue('E', "09-01", "e.rs", "")
                + &issue(
                    'F',
                    "10-04",
                    "a.rs",
                    &format!(r#","supersedes":"{}""#, id('A')),
                )
                + &issue(
                    'G',
                    "10-04",
                    "d.rs",
                    &format!(r#","supersedes":"{}""#, id('D')),
                )),
        ),
        closes: rows(
            &(close(
                'A',
                "10-02",
                "`a.rs` looped → cap at 3; guard `tests/a.rs` (9bb1038)",
            ) + &close('B', "10-02", "fixed (#282)")
                + &close('C', "10-02", "no longer happens")
                + &close('D', "10-02", "wontfix, see 1234567")
                + &close('E', "09-02", "fixed in abc1234")),
        ),
        ..Log::default()
    };
    let line = |at: &str, s: &str, rest: &str| {
        format!(
            "{{\"ts\":\"2026-{at}.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/{s}.jsonl\",\"ids\":[],{rest}}}\n"
        )
    };
    let usage = line(
        "10-01T00:05:00",
        "s1",
        r#""event":"review","said":[{"kind":"finding","key":"b.rs"}]"#,
    ) + &line(
        "10-03T00:00:00",
        "s2",
        r#""event":"edit","files":["a.rs","d.rs"]"#,
    );
    let p = super::parse::parse(
        &usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let commit = |sha: &str, message: String| Commit {
        sha: sha.into(),
        message,
    };
    let commits = vec![
        commit("1111111a", "fix: loop cap (#282)".into()),
        commit("2222222b", format!("fix(x): c\n\nfael:{}", &id('C')[..8])),
        commit("9bb1038f", "fix: a".into()),
        commit("3333333c", "fix: nobody filed it".into()),
        commit("4444444d", "feat: not a fix (#282)".into()),
    ];
    let mut twice = commits.clone(); // a second worktree reads the same commits
    twice.extend(commits);
    let logs = HashMap::from([("/w/r".to_string(), log)]);
    let e = experience(&p, &logs, &HashMap::from([("/w/r".to_string(), twice)]));
    // closes before the label contract: `label` is `label_tests.rs`'s
    assert_eq!(
        Experience {
            label: Default::default(),
            ..e
        },
        Experience {
            fixed: 3,             // A sha · B (#N) · C commit
            fixed_from_review: 1, // B
            closed_with_check: 1, // A
            repeats_with_check: 1,
            edits_after_close_with_check: 1, // s2 on a.rs, not d.rs
            fix_commits: 4,
            fix_commits_linked: 3,
            ..Experience::default()
        }
    );
}

/// H: its close folded in by `fael compact`, with a sha and a check · I:
/// closed with `deadbeef` (no digit) · J: with `1234567` (no letter). Two
/// worktrees read the same log and the same commits: each counts once.
#[test]
fn folded_closes_sha_edges_and_worktrees() {
    let folded = r#","closed":{"id":"XH","ts":"2026-10-02T00:00:00Z","by":"w","text":"cap → guard `tests/h.rs` (9bb1038)"}"#;
    let log = || Log {
        rows: rows(
            &(issue('H', "10-01", "h.rs", folded)
                + &issue('I', "10-01", "i.rs", "")
                + &issue('J', "10-01", "j.rs", "")),
        ),
        closes: rows(
            &(close('I', "10-02", "fixed in deadbeef") + &close('J', "10-02", "fixed in 1234567")),
        ),
        ..Log::default()
    };
    let usage: String = ["/w/r", "/w/q"]
        .iter()
        .map(|repo| {
            format!(
                "{{\"ts\":\"2026-10-01T00:05:00.000Z\",\"repo\":\"{repo}\",\"client\":\"claude\",\"session\":\"/t/s.jsonl\",\"ids\":[],\"event\":\"read\"}}\n"
            )
        })
        .collect();
    let p = super::parse::parse(
        &usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let commits = || {
        vec![
            Commit {
                sha: "9bb1038f".into(),
                message: "fix: h".into(),
            },
            Commit {
                sha: "5555555e".into(),
                message: "fix: deadbeef".into(),
            },
        ]
    };
    let logs = HashMap::from([("/w/r".to_string(), log()), ("/w/q".to_string(), log())]);
    let all = HashMap::from([
        ("/w/r".to_string(), commits()),
        ("/w/q".to_string(), commits()),
    ]);
    let e = experience(&p, &logs, &all);
    // closes before the label contract: `label` is `label_tests.rs`'s
    assert_eq!(
        Experience {
            label: Default::default(),
            ..e
        },
        Experience {
            fixed: 1,             // H, from the folded close
            closed_with_check: 1, // H
            fix_commits: 2,
            fix_commits_linked: 1, // 9bb1038, named by H's folded close
            ..Experience::default()
        }
    );
}

/// PLAN-fael-label §7, locked before the code: (text, core, guard).
#[test]
fn close_shape_locks_the_label_table() {
    use super::experience::{Shape, close_shape, names_check};
    let table = [
        (
            "stale cache → invalidate on mtime; tried ttl; guard `fael/tests/a.rs`",
            true,
            true,
        ),
        (
            "config เก่าถูก cache -> invalidate ตอน mtime เปลี่ยน",
            true,
            false,
        ),
        ("a → b → c", true, false),
        ("→ fixed it", false, false),
        ("cause →   ", false, false),
        ("renamed `a -> b` in the doc", false, false),
        ("x → y; guard `Cargo.toml`", true, false),
        ("x → y; guard `run cargo test`", true, false),
        ("x -> y → z", true, false),
        ("`a → b` -> c", false, false),
        ("cause → fix `oops", true, false),
        ("stale `x → y` in the old note", false, false),
        ("example `src/a.rs` only", false, true),
        ("fixed in a1b2c3d", false, false),
        // an unpaired backtick is plain text for the guard too
        ("cause → fix `src/a.rs", true, false),
        // a cut span leaves a space: `-` and `>` around it never glue into an arrow
        ("a -`x`> b", false, false),
        // a stray backtick is no cause and no fix
        ("cause → `", false, false),
        ("`→ fixed it", false, false),
        // a double-backtick span holds a backtick (CommonMark)
        ("use `` a`b -> c `` here", false, false),
        ("x → y; guard ``fael/a.rs``", true, true),
        // a span with a `/` is a guard whatever it holds, a command too
        ("x → y; guard `scripts/file-size.sh --strict`", true, true),
    ];
    for (text, core, guard) in table {
        assert_eq!(close_shape(text), Shape { core, guard }, "{text}");
        // §7 rule 7: `closed_with_check` reads the same guard
        assert_eq!(names_check(&[text]), guard, "{text}");
    }
}

/// The taught close text, filled in on each branch, reads as a label core and
/// names its fix only through the optional `(#N)` — it asks for no sha; only
/// a `/` path makes it a guard (`names_check`). A `don't`
/// branch that cites a path with a `/` therefore reads as a guard too — the
/// known limit of a purely formal guard (PLAN-fael-capture-yield §5), kept
/// here so a change to either side shows.
#[test]
fn close_template_fills_into_a_label_core_and_a_fix() {
    use super::experience::{CLOSE_TEMPLATE, Shape, close_shape, names_fix};
    let fill = |path: &str, fix: &str| {
        CLOSE_TEMPLATE
            .replace("<cause>", "stale cache")
            .replace("<fix>", "invalidate on mtime")
            .replace("<what failed>", "a ttl")
            .replace("<test path>", path)
            .replace("<X>", "add a third matcher")
            .replace("<Y>", "sameName already exists")
            .replace("[; (#N)]", fix)
    };
    for (path, fix, guard) in [
        ("fael/tests/a.rs", "; (#12)", true),
        ("fael/tests/a.rs", "", true),
        ("none", "; (#12)", false),
        ("none", "", false),
    ] {
        let text = fill(path, fix);
        assert_eq!(close_shape(&text), Shape { core: true, guard }, "{text}");
        assert_eq!(names_fix(&text), !fix.is_empty(), "{text}");
    }
    assert!(
        !CLOSE_TEMPLATE.contains("sha"),
        "the template asks for no sha"
    );
    // unfilled, the placeholders name no fix
    assert!(!names_fix(CLOSE_TEMPLATE));
    // the don't branch citing a path with a `/` reads as a guard
    let dont = "x → y; don't edit `src/a.rs` because b; tried z; a1b2c3d4";
    assert_eq!(
        close_shape(dont),
        Shape {
            core: true,
            guard: true
        }
    );
}

/// PLAN-fael-fix-evidence §3, the one rule carry (`fix_reached`) and stats
/// read: (main commit message, close, issue id, fixed).
#[test]
fn cites_fix_locks_the_fix_evidence_table() {
    use super::experience::{cites_fix, new_close, resolve};
    // vela's two rows sharing 8 chars (fael:01M4HW99)
    let ids = [
        "01M4G1N16R2X00000000000000",
        "01M4G1N199XP00000000000000",
        "01M3MPZDGMG0PE0F2X4ZFHFDDY",
    ];
    let (a, z) = (ids[0], ids[2]);
    assert_eq!(resolve("01M4G1N1", ids), None, "ambiguous: never the first");
    assert_eq!(resolve("01M4G1N16", ids), Some(a));
    assert_eq!(resolve("01ZZZZZZ", ids), None);
    let picked = "fix: keep state (fael:01M3MPZD)\n\n(cherry picked from commit 22703f2)";
    let table = [
        ("fix: keep state (fael:01M3MPZD)", "x → y", z, true),
        (
            "feat: page (#405)\n\n* fix: x (fael:01M3MPZD)",
            "x → y",
            z,
            true,
        ), // squash
        (picked, "x → y", z, true),
        ("fixes 01M3MPZD in passing", "x → y", z, false), // not the token
        ("fix: x (fael:01m3mpzd)", "x → y", z, false),
        ("fix: x (fael:01M3MPZ)", "x → y", z, false), // under 8
        ("fix: x (fael:01M4G1N1)", "x → y", a, false), // ambiguous
        ("fix: x (fael:01M4G1N16R)", "x → y", a, true), // a longer prefix
        ("fix: x (fael:01M3MPZD)", "x → y", a, false), // another row's
        ("fix: x (#326)", "x → y; (#326)", a, true),
        ("fix: x (#3261)", "x → y; (#326)", a, false),
        ("fix: x (#326)", "x → y; #326", a, false), // not the token
        ("fix: x 22703f2", "x → y; 22703f2", a, false), // a sha never
    ];
    for (msg, close, id, want) in table {
        let is_id = |p: &str| resolve(p, ids) == Some(id);
        assert_eq!(cites_fix(msg, close, is_id), want, "{msg} / {close}");
    }
    // v0.40.1, the cutoff (fael:01M4HX74), in any zone
    assert!(new_close("2026-10-10T03:59:25.000Z") && new_close("2026-10-10T10:59:25+07:00"));
    assert!(!new_close("2026-10-10T03:59:24.999Z") && !new_close("not a time"));
}

/// Stats reads the same rule: K cited on main · L its sha on main (no
/// evidence) · M an ambiguous 8-char cite, then a longer one · N `(#77)` ·
/// O an old close naming a sha, on the old rule.
#[test]
fn a_new_close_is_fixed_only_by_a_cite_on_main() {
    let sibling = format!(
        "{{\"v\":1,\"id\":\"01KMMMMMZ{}\",\"ts\":\"2026-10-01T00:10:00Z\",\"by\":\"w\",\"kind\":\"note\",\"text\":\"t\",\"files\":[\"m.rs\"]}}\n",
        "Z".repeat(17)
    );
    let log = Log {
        rows: rows(
            &("KLMNO".chars())
                .map(|l| issue(l, "10-01", "k.rs", ""))
                .chain([sibling])
                .collect::<String>(),
        ),
        closes: rows(
            &(close('K', "10-11", "x → y")
                + &close('L', "10-11", "x → y; 9bb1038f")
                + &close('M', "10-11", "x → y")
                + &close('N', "10-11", "x → y; (#77)")
                + &close('O', "10-02", "fixed in abc1234")),
        ),
        ..Log::default()
    };
    let usage = "{\"ts\":\"2026-10-01T00:05:00.000Z\",\"repo\":\"/w/r\",\"client\":\"claude\",\"session\":\"/t/s.jsonl\",\"ids\":[],\"event\":\"read\"}\n";
    let p = super::parse::parse(
        usage,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let commit = |sha: &str, message: &str| Commit {
        sha: sha.into(),
        message: message.into(),
    };
    let mut commits = vec![
        commit("1111111a", "fix: k (fael:01KKKKKKKK)"),
        commit("9bb1038f", "fix: l"),
        commit("2222222b", "fix: m (fael:01KMMMMM)"),
        commit("3333333c", "feat: n (#77)"),
    ];
    let logs = HashMap::from([("/w/r".to_string(), log)]);
    let fixed = |commits: &[Commit]| {
        let all = HashMap::from([("/w/r".to_string(), commits.to_vec())]);
        experience(&p, &logs, &all).fixed
    };
    assert_eq!(fixed(&commits), 3, "K, N, O");
    commits.push(commit("4444444d", "fix: m (fael:01KMMMMMM)"));
    assert_eq!(fixed(&commits), 4, "M by its longer prefix");
}
