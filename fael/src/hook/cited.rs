//! The `cited` outcome (SPEC-fael-learn-loop §B): an id from the session's
//! seen list typed into a tool input or the closing reply. Written as a 0-byte
//! `outcome` usage line, never a push; each id once per session (a `^<id>`
//! mark in the seen list). The seen list also holds rows the agent filed or
//! found itself, so stats keeps only the ids a push said (or a pull showed
//! after a cut) — this side only finds the candidates.
//!
//! A `fael …` command is not a cite: `fael find <id>` is a pull, and stats
//! reads it from its own `found` line.

use super::asks::{UsageMeta, append_row};
use super::protocol::{Ctx, Event};
use super::state::{lock_seen, seen_path};
use super::usage::usage_row;
use serde_json::Value;
use std::io::Write;
use std::path::Path;

/// Shortest id prefix that counts, as the push prints it (`[01M45R3J]`).
const SHORT: usize = 8;
const ID_LEN: usize = 26;

/// A tool call's input as text, minus the shell segments that run `fael`.
pub(crate) fn haystack(input: &Value) -> String {
    let mut v = input.clone();
    if let Some(cmd) = input["command"].as_str() {
        v["command"] = without_fael(cmd).into();
    }
    v.to_string()
}

fn without_fael(cmd: &str) -> String {
    cmd.split(['\n', ';', '&', '|'])
        .filter(|seg| {
            // `FAEL_STATE_DIR=x fael find …`: skip the env words
            let w = seg.split_whitespace().find(|w| !w.contains('='));
            !w.is_some_and(|w| w == "fael" || w.ends_with("/fael"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Open issues a `git commit` tool input names (PLAN-fael-agent-ergonomics
/// chunk 5): the ids a commit-cite push line may say. A full id names
/// itself; a `SHORT` prefix names one only when no other open issue shares
/// it (the `candidates` rule) — and a closed or superseded issue never names
/// itself, so a commit that only re-states done work stays silent. `fael …`
/// shell segments are already out of the haystack, so `fael close <id>` is
/// no cite.
pub(crate) fn commit_cites(log: &crate::core::Log, input: &Value) -> Vec<String> {
    let cmd = input["command"].as_str().unwrap_or("");
    if !is_commit(cmd) {
        return vec![];
    }
    let text = haystack(input);
    let hide: std::collections::HashSet<&str> = crate::core::closed(log)
        .union(&crate::core::superseded(log))
        .copied()
        .collect();
    let open: Vec<&str> = log
        .rows
        .iter()
        .filter(|r| r.kind == "issue" && r.id.len() == ID_LEN && !hide.contains(r.id.as_str()))
        .map(|r| r.id.as_str())
        .collect();
    let mut out = vec![];
    for (i, id) in open.iter().enumerate() {
        let named = text.contains(*id)
            || (text.contains(&id[..SHORT])
                && open
                    .iter()
                    .enumerate()
                    .all(|(j, o)| j == i || o[..SHORT] != id[..SHORT]));
        if named {
            out.push(id.to_string());
        }
    }
    out
}

/// A `git commit` whose own subject declares a fix (`fix:` / `fix(`, the type
/// the agent typed — PLAN-fael-experience-loop chunk 5b; never a `fix(`
/// elsewhere in a script that also commits) and whose text names no row of
/// `log`, open or closed, by its `SHORT` prefix. Which issue the commit fixes
/// is never judged; a repo without conventional commits never matches.
pub(crate) fn fix_uncited(log: &crate::core::Log, input: &Value) -> bool {
    let cmd = input["command"].as_str().unwrap_or("");
    let fix = |s: &str| s.starts_with("fix:") || s.starts_with("fix(");
    if !commits(cmd).filter_map(subject).any(fix) {
        return false;
    }
    let text = haystack(input);
    !log.rows
        .iter()
        .any(|r| r.id.len() >= SHORT && text.contains(&r.id[..SHORT]))
}

/// The subject after a commit's first `-m`-ending flag (`-m`, `-am`, `-qm`):
/// its quote dropped, a `$(cat <<'EOF'` line skipped to the heredoc body.
// ponytail: `-F` / `--message` / a glued `-m"…"` read as no subject
fn subject(commit: &str) -> Option<&str> {
    let flag = commit
        .split_whitespace()
        .find(|w| w.starts_with('-') && !w.starts_with("--") && w.ends_with('m'))?;
    let rest = commit[offset(commit, flag) + flag.len()..].trim_start();
    let rest = rest.trim_start_matches(['"', '\'']);
    Some(match rest.strip_prefix("$(cat <<") {
        Some(h) => h[h.find('\n')? + 1..].trim_start(),
        None => rest,
    })
}

/// Byte offset of `part`, a slice of `whole`.
fn offset(whole: &str, part: &str) -> usize {
    part.as_ptr() as usize - whole.as_ptr() as usize
}

/// Each `git commit` in a shell command, from its `git` on: the first two bare
/// words of a `;`/`&`/`|`/newline-split segment, env assignments skipped.
fn commits(cmd: &str) -> impl Iterator<Item = &str> {
    cmd.split(['\n', ';', '&', '|']).filter_map(move |seg| {
        let mut words = seg.split_whitespace().filter(|w| !w.contains('='));
        let git = words
            .next()
            .filter(|w| *w == "git" || w.ends_with("/git"))?;
        (words.next() == Some("commit")).then(|| &cmd[offset(cmd, git)..])
    })
}

/// A shell command with a `git commit` segment.
pub(crate) fn is_commit(cmd: &str) -> bool {
    commits(cmd).next().is_some()
}

/// A tool event: the repo is resolved here, since a search with no files never
/// reaches `push`.
pub(crate) fn note_tool(e: &Event, input: &Value) {
    if e.session.as_deref().is_none_or(str::is_empty) || input.is_null() {
        return;
    }
    let cwd = e.cwd.as_deref().map_or_else(
        || std::env::current_dir().unwrap_or_default(),
        std::path::PathBuf::from,
    );
    let Ok(repo) = crate::repo_at(&cwd) else {
        return;
    };
    let session = e.session.as_deref().unwrap_or("");
    let agent = e.agent.as_deref().unwrap_or("");
    let client = e.client.as_deref().unwrap_or("neutral");
    note(client, session, agent, &repo.root, &haystack(input));
}

/// The closing reply, with the stop event's context already resolved.
pub(crate) fn note_reply(c: &Ctx, reply: &str) {
    note(&c.client, &c.session, &c.agent, &c.repo.root, reply);
}

fn note(client: &str, session: &str, agent: &str, root: &Path, text: &str) {
    if session.is_empty() {
        return;
    }
    let path = seen_path(session, agent, root);
    // the common case — nothing cited — costs one unlocked read
    let Ok(seen) = std::fs::read_to_string(&path) else {
        return;
    };
    if candidates(&seen, text).is_empty() {
        return;
    }
    let Some(mut f) = lock_seen(&path) else {
        return;
    };
    // re-read under the lock: a parallel hook may have marked them first
    let mut now = String::new();
    let _ = std::io::Read::read_to_string(&mut f, &mut now);
    let ids = candidates(&now, text);
    if ids.is_empty() {
        return;
    }
    let marks: String = ids.iter().map(|i| format!("^{i}\n")).collect();
    let _ = f.write_all(marks.as_bytes());
    let meta = UsageMeta {
        session: Some(session),
        agent: (!agent.is_empty()).then_some(agent),
        ..UsageMeta::default()
    };
    let mut row = usage_row(client, "outcome", root, "", &[], &meta);
    row["cited"] = ids.into();
    append_row(row);
}

/// The `dup` outcome (SPEC §B): a row was filed over `old`, and `old` was never
/// said to this session (not in its seen list — cut, or never pushed). Self-heal
/// proved the link (a supersede), so this is a fact, not a guess; a link the
/// session's own earlier row or a shown row explains is no dup. Empty session =
/// no-op. Call before the new row joins the seen list.
pub(crate) fn note_dup(session: &str, root: &Path, old: &str) {
    if session.is_empty() {
        return;
    }
    let seen = std::fs::read_to_string(seen_path(session, "", root)).unwrap_or_default();
    if seen.lines().any(|l| l == old) {
        return;
    }
    let meta = UsageMeta {
        session: Some(session),
        ..UsageMeta::default()
    };
    let mut row = usage_row("cli", "outcome", root, "", &[], &meta);
    row["dup"] = vec![old].into();
    append_row(row);
}

/// Row ids in the seen list the text names, that no `^<id>` mark covers yet:
/// a full id cites itself; a short prefix cites only when no other seen id
/// shares it — batch-filed siblings share the timestamp prefix, so a shared
/// prefix is ambiguous, never a cite.
fn candidates(seen: &str, text: &str) -> Vec<String> {
    let marked = |id: &str| seen.lines().any(|l| l.strip_prefix('^') == Some(id));
    let mut ids: Vec<&str> = vec![];
    for id in seen.lines() {
        let id_like = id.len() == ID_LEN && id.bytes().all(|b| b.is_ascii_alphanumeric());
        if id_like && !marked(id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut out: Vec<String> = vec![];
    for (i, id) in ids.iter().enumerate() {
        let named = text.contains(*id)
            || (text.contains(&id[..SHORT])
                && ids
                    .iter()
                    .enumerate()
                    .all(|(j, o)| j == i || o[..SHORT] != id[..SHORT]));
        if named {
            out.push(id.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_fael_command_is_no_cite_but_a_commit_message_is() {
        let t = |c: &str| haystack(&json!({"command": c}));
        assert!(!t("fael find 01M45R3J0A8Q39CQEH7YBCPA1N").contains("01M45R3J"));
        assert!(!t("cd x && FAEL_STATE_DIR=y /usr/bin/fael close 01M45R3J").contains("01M45R3J"));
        assert!(t("git commit -m 'fix per 01M45R3J'").contains("01M45R3J"));
        // a bare `fael` word later in a quoted message is no command
        assert!(t("echo 'x' && git commit -m \"a fael 01M45R3J\"").contains("01M45R3J"));
    }

    #[test]
    fn only_the_commits_own_subject_declares_a_fix() {
        let fix_subject = |c: &str| fix_uncited(&issue_log(&[], &[]), &json!({"command": c}));
        for c in [
            "git commit -m \"fix(hook): cap\"",
            "git add -A && git commit -qm 'fix: cap'",
            "git commit -am fix:cap",
            "git commit -m \"$(cat <<'EOF'\nfix(hook): cap\n\nbody\nEOF\n)\"",
        ] {
            assert!(fix_subject(c), "{c}");
        }
        // a later line, a script holding a fix commit as text, an echo, -F
        for c in [
            "git commit -m \"feat: x\n\nfix: later line\"",
            "cat > t.sh <<'EOF'\ngit commit -qm init\nrun \"git commit -m \\\"fix(a): x\\\"\"\nEOF",
            "echo \"fix(a): x\"; git commit -m init",
            "git commit -F msg.txt",
        ] {
            assert!(!fix_subject(c), "{c}");
        }
    }

    #[test]
    fn an_id_is_a_candidate_once_and_only_when_seen() {
        let id = "01M45R3J0A8Q39CQEH7YBCPA1N";
        let seen = format!("{id}\nk:x\n");
        assert_eq!(candidates(&seen, "see 01M45R3J here"), [id]);
        assert!(candidates(&seen, "see 01M45R3K here").is_empty());
        assert!(candidates(&seen, "see 01M45R3 here").is_empty(), "7 chars");
        assert!(
            candidates("k:x\n", "see 01M45R3J here").is_empty(),
            "never said"
        );
        assert!(candidates(&format!("{seen}^{id}\n"), id).is_empty(), "once");
    }

    #[test]
    fn same_ms_siblings_cite_only_the_named_full_id() {
        let (a, b) = (
            crate::core::ulid_at(1_789_000_000_000),
            crate::core::ulid_at(1_789_000_000_000),
        );
        assert_eq!(&a[..SHORT], &b[..SHORT], "one batch shares the prefix");
        let seen = format!("{a}\n{b}\n");
        let text = format!("see {a} here");
        let short = format!("see {} here", &a[..SHORT]);
        assert_eq!(candidates(&seen, &text), [a]);
        assert!(
            candidates(&seen, &short).is_empty(),
            "a shared prefix is ambiguous"
        );
    }

    fn issue_log(ids: &[&str], closed: &[&str]) -> crate::core::Log {
        let row = |id: &str| crate::core::Row {
            id: id.to_string(),
            ts: "2026-10-01T00:00:00Z".into(),
            by: "t-0000".into(),
            kind: "issue".into(),
            text: "an open issue".into(),
            files: vec!["src/a.rs".into()],
            ..crate::core::Row::default()
        };
        crate::core::Log {
            rows: ids.iter().map(|i| row(i)).collect(),
            closes: closed
                .iter()
                .map(|i| crate::core::Row::close("t-0000", i, "done"))
                .collect(),
            ..crate::core::Log::default()
        }
    }

    #[test]
    fn a_commit_names_open_issues_not_closed_ones_or_other_commands() {
        let open = "01M45R3J0A8Q39CQEH7YBCPA1N";
        let done = "01M45R3K0A8Q39CQEH7YBCPA1M";
        let log = issue_log(&[open, done], &[done]);
        let commit = |c: &str| commit_cites(&log, &json!({"command": c}));
        assert_eq!(commit(&format!("git commit -m 'fix per {open}'")), [open]);
        assert!(
            commit(&format!("git commit -m 're-states {done}'")).is_empty(),
            "closed is no cite"
        );
        assert!(
            commit(&format!("git show {open}")).is_empty(),
            "not a commit"
        );
        assert!(commit(&format!("echo {open}")).is_empty(), "not a commit");
        // a `fael close` of the id is a pull, never a commit cite
        assert!(commit(&format!("fael close {open} done")).is_empty());
        // env-prefixed and chained commits count
        assert_eq!(
            commit(&format!(
                "FAEL_STATE_DIR=y git commit -m '{open}' && git push"
            )),
            [open]
        );
        // a bare short prefix cites when no other open issue shares it
        assert_eq!(commit("git commit -m 'fix per 01M45R3J'"), [open]);
    }

    #[test]
    fn a_shared_short_prefix_is_no_commit_cite() {
        let (a, b) = (
            crate::core::ulid_at(1_789_000_000_000),
            crate::core::ulid_at(1_789_000_000_000),
        );
        assert_eq!(&a[..SHORT], &b[..SHORT], "one batch shares the prefix");
        let log = issue_log(&[&a, &b], &[]);
        let short = format!("git commit -m 'fix {}'", &a[..SHORT]);
        assert!(commit_cites(&log, &json!({"command": short})).is_empty());
        let full = format!("git commit -m 'fix {a}'");
        assert_eq!(commit_cites(&log, &json!({"command": full})), [a]);
    }

    #[test]
    fn one_commit_naming_two_open_issues_names_both() {
        let (a, b) = ("01M45R3J0A8Q39CQEH7YBCPA1N", "01M45R3K0A8Q39CQEH7YBCPA2N");
        let log = issue_log(&[a, b], &[]);
        let cmd = format!("git commit -m 'fix {a} and {b}'");
        assert_eq!(commit_cites(&log, &json!({"command": cmd})), [a, b]);
    }

    #[test]
    fn a_superseded_issue_is_no_commit_cite() {
        let (old, new) = ("01M45R3J0A8Q39CQEH7YBCPA1N", "01M45R3K0A8Q39CQEH7YBCPA2N");
        let mut log = issue_log(&[old, new], &[]);
        log.rows
            .iter_mut()
            .find(|r| r.id == new)
            .expect("new row")
            .supersedes = Some(old.to_string());
        let cmd = |id: &str| format!("git commit -m 'fix {id}'");
        assert!(
            commit_cites(&log, &json!({"command": cmd(old)})).is_empty(),
            "superseded stays silent"
        );
        assert_eq!(commit_cites(&log, &json!({"command": cmd(new)})), [new]);
    }
}
