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
    assert_eq!(
        e,
        Experience {
            fixed: 3,             // A sha · B (#N) · C commit
            fixed_from_review: 1, // B
            closed_with_check: 1, // A
            repeats_with_check: 1,
            edits_after_close_with_check: 1, // s2 on a.rs, not d.rs
            fix_commits: 4,
            fix_commits_linked: 3,
        }
    );
}
