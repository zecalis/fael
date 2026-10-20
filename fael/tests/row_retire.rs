//! Row lifecycle D1 (decision 01M5528X): a note tied to a chunk or a branch retires when
//! `fael chunk done` ends that chunk or git proves the branch merged; an unmerged branch's
//! note stays. Retired rows are hidden like closed and shown `(retired)` under `--all`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(d: &Path, state: &Path, session: Option<&str>, args: &[&str]) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("CLAUDE_CODE_SESSION_ID");
    if let Some(s) = session {
        c.env("CLAUDE_CODE_SESSION_ID", s);
    }
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(d: &Path, args: &[&str]) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let ok = Command::new("git")
        .args(id)
        .args(args)
        .current_dir(d)
        .status()
        .unwrap();
    assert!(ok.success(), "git {args:?}");
}

fn repo() -> (PathBuf, PathBuf) {
    let t = std::env::temp_dir().join(format!("fael-retire-{}", fael_core::ulid()));
    let d = t.join("r");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    (d.canonicalize().unwrap(), t.join("state"))
}

#[test]
fn a_note_retires_when_its_chunk_is_done() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    git(&d, &["switch", "-qc", "feat/x"]);
    f(&["add", "note", "stopped at step two", "--files", "plan:demo"]);
    let a = f(&["chunk", "add", "a", "--brief", "b"]);
    f(&["chunk", "start", &a, "--run", "R1"]);
    assert!(f(&["find", "stopped"]).contains("stopped at step two"));
    f(&["chunk", "done", &a, "h", "--run", "R1"]);
    assert!(f(&["find", "stopped"]).starts_with("no open row matches"));
    let all = f(&["find", "stopped", "--all"]);
    assert!(all.contains("(retired)"), "{all}");
}

#[test]
fn a_handoff_note_with_no_stamped_session_stays() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, Some("s1"), a);
    f(&["plan", "new", "demo", "Demo"]);
    f(&[
        "add",
        "note",
        "where it stopped",
        "--files",
        "plan:demo",
        "--key",
        "plan:demo:handoff",
    ]);
    let a = f(&["chunk", "add", "a", "--plan", "demo", "--brief", "b"]);
    f(&["chunk", "start", &a, "--run", "R1", "--no-worktree"]);
    f(&["chunk", "done", &a, "h", "--run", "R1"]);
    // the run's session is stamped by the Stop hook, not by start; here: unproven, stays
    assert!(f(&["find", "where it stopped"]).contains("where it stopped"));
}

#[test]
fn a_note_retires_when_its_branch_is_merged_and_not_before() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    git(&d, &["switch", "-qc", "feat/x"]);
    f(&[
        "add",
        "note",
        "uncommitted on feat/x",
        "--files",
        "plan:demo",
    ]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    git(&d, &["switch", "-q", "main"]);
    assert!(f(&["find", "uncommitted"]).contains("uncommitted on feat/x"));
    git(&d, &["merge", "-q", "--squash", "feat/x"]);
    git(&d, &["commit", "-qm", "squash"]);
    assert!(f(&["find", "uncommitted"]).starts_with("no open row matches"));
    assert!(f(&["find", "uncommitted", "--all"]).contains("(retired)"));
}

#[test]
fn notes_retire_when_their_branches_merge_regularly() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    for (branch, text, file) in [
        ("feat/a", "note on feat/a", "a.txt"),
        ("feat/b", "note on feat/b", "b.txt"),
    ] {
        git(&d, &["switch", "-qc", branch]);
        f(&["add", "note", text, "--files", "plan:demo"]);
        std::fs::write(d.join(file), "work\n").unwrap();
        git(&d, &["add", file]);
        git(&d, &["commit", "-qm", "work"]);
    }
    git(&d, &["switch", "-q", "main"]);
    assert!(f(&["find", "note on feat"]).contains("note on feat/a"));
    git(&d, &["merge", "-q", "--no-ff", "feat/a"]);
    git(&d, &["merge", "-q", "--no-ff", "feat/b"]);
    assert!(f(&["find", "note on feat"]).starts_with("no open row matches"));
    let all = f(&["find", "note on feat", "--all"]);
    assert!(all.contains("(retired)"), "{all}");
}

#[test]
fn a_note_on_a_branch_with_no_commits_stays() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    // cut from main, nothing committed: the tip sits on HEAD's first-parent
    // line, so there is no merged work to prove — unproven, stays live
    git(&d, &["switch", "-qc", "feat/empty"]);
    f(&["add", "note", "just branched", "--files", "plan:demo"]);
    git(&d, &["switch", "-q", "main"]);
    assert!(f(&["find", "just branched"]).contains("just branched"));
}

#[test]
fn a_note_stays_when_only_its_remote_is_merged() {
    // 01M555VZ: origin/feat/a merged, local feat/a moved on — the batch must
    // follow the proof's order (local first) and leave the note live.
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    git(&d, &["switch", "-qc", "feat/a"]);
    f(&["add", "note", "remote only merged", "--files", "plan:demo"]);
    std::fs::write(d.join("a.txt"), "work\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    let tip = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&d)
        .output()
        .unwrap();
    let tip = String::from_utf8_lossy(&tip.stdout).trim().to_string();
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["merge", "-q", "--no-ff", "feat/a"]);
    git(&d, &["update-ref", "refs/remotes/origin/feat/a", &tip]);
    git(&d, &["switch", "-q", "feat/a"]);
    std::fs::write(d.join("a.txt"), "work\nmore\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "more"]);
    git(&d, &["switch", "-q", "main"]);
    assert!(
        f(&["find", "remote only merged"]).contains("remote only merged"),
        "local feat/a has unmerged commits"
    );
}

/// A branch with one commit, a note filed on it at its tip, then back to main.
fn noted_branch(d: &Path, s: &Path, text: &str) {
    git(d, &["switch", "-qc", "feat/gone"]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(d, &["add", "a.txt"]);
    git(d, &["commit", "-qm", "work"]);
    fael(d, s, None, &["add", "note", text, "--files", "plan:demo"]);
    git(d, &["switch", "-q", "main"]);
}

#[test]
fn a_note_retires_when_its_squash_merged_branch_is_deleted() {
    let (d, s) = repo();
    noted_branch(&d, &s, "gone after squash");
    git(&d, &["merge", "-q", "--squash", "feat/gone"]);
    git(&d, &["commit", "-qm", "squash"]);
    git(&d, &["branch", "-qD", "feat/gone"]);
    let out = fael(&d, &s, None, &["find", "gone after squash"]);
    assert!(out.starts_with("no open row matches"), "{out}");
}

#[test]
fn a_note_stays_when_its_branch_is_deleted_unmerged() {
    let (d, s) = repo();
    noted_branch(&d, &s, "gone unmerged");
    git(&d, &["branch", "-qD", "feat/gone"]);
    assert!(fael(&d, &s, None, &["find", "gone unmerged"]).contains("gone unmerged"));
}

#[test]
fn a_row_with_no_stamped_tip_stays_when_its_branch_is_deleted() {
    let (d, s) = repo();
    noted_branch(&d, &s, "gone no tip");
    git(&d, &["merge", "-q", "--squash", "feat/gone"]);
    git(&d, &["commit", "-qm", "squash"]);
    git(&d, &["branch", "-qD", "feat/gone"]);
    // an old row: drop its `sha` stamp from the log
    let mut cutn = 0;
    for e in [d.join(".fael"), d.join(".git"), s.clone()]
        .iter()
        .flat_map(|p| walk(p))
    {
        if let Ok(t) = std::fs::read_to_string(&e) {
            let cut = regex_free_cut(&t);
            if cut != t {
                std::fs::write(&e, cut).unwrap();
                cutn += 1;
            }
        }
    }
    assert!(cutn > 0, "no sha stamp found to strip");
    assert!(fael(&d, &s, None, &["find", "gone no tip"]).contains("gone no tip"));
}

fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
        let q = e.path();
        if q.is_dir() {
            v.extend(walk(&q));
        } else {
            v.push(q);
        }
    }
    v
}

/// Remove every `"sha":"<hex>",` / `,"sha":"<hex>"` pair from jsonl text.
fn regex_free_cut(t: &str) -> String {
    let mut out = String::new();
    let mut rest = t;
    while let Some(i) = rest.find("\"sha\":\"") {
        let (head, tail) = rest.split_at(i);
        let end = tail[7..].find('"').map_or(tail.len(), |j| 7 + j + 1);
        let head = head.strip_suffix(',').unwrap_or(head);
        out.push_str(head);
        let tail = &tail[end..];
        rest = if head.len() == rest[..i].len() {
            tail.strip_prefix(',').unwrap_or(tail)
        } else {
            tail
        };
    }
    out.push_str(rest);
    out
}

#[test]
fn stats_counts_rows_by_state_per_kind() {
    let (d, s) = repo();
    let f = |a: &[&str]| fael(&d, &s, None, a);
    std::fs::write(d.join("a.rs"), "1\n").unwrap();
    std::fs::write(d.join("b.rs"), "1\n").unwrap();
    f(&[
        "add",
        "decision",
        "all changed",
        "--title",
        "t one",
        "--files",
        "a.rs",
    ]);
    f(&[
        "add",
        "decision",
        "some changed",
        "--title",
        "t two",
        "--files",
        "a.rs,b.rs",
    ]);
    f(&[
        "add",
        "decision",
        "none changed",
        "--title",
        "t three",
        "--files",
        "b.rs",
    ]);
    f(&[
        "add", "issue", "gone", "--title", "t four", "--files", "b.rs",
    ]);
    let listed = f(&["find", "--kind", "issue"]);
    let id = listed.split(['[', ']']).nth(1).unwrap().to_string();
    f(&["close", &id, "done"]);
    std::fs::write(d.join("a.rs"), "2\n").unwrap();
    let out = f(&["stats", "--json"]);
    let l: serde_json::Value = serde_json::from_str(&out).unwrap();
    let l = &l["lifecycle"];
    assert_eq!(l["decision"]["dormant"], 1, "{l}");
    assert_eq!(l["decision"]["held"], 1, "{l}");
    assert_eq!(l["decision"]["live"], 1, "{l}");
    assert_eq!(l["issue"]["retired"], 1, "{l}");
    assert!(f(&["stats"]).starts_with("rows (this repo):\n  decision: live 1 · held 1"));
}

/// `fael hook edit` on `a.txt` as session `session`: what the push says.
fn edit_push(d: &Path, s: &Path, session: &str) -> String {
    use std::io::Write as _;
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["hook", "edit"])
        .current_dir(d)
        .env("FAEL_STATE_DIR", s)
        .env_remove("FAEL_DIR")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let e = serde_json::json!({"cwd": d, "session": session, "files": ["a.txt"]});
    c.stdin
        .take()
        .unwrap()
        .write_all(e.to_string().as_bytes())
        .unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A note on `a.txt` filed on feat/x, whose branch is then merged into main.
fn merged_note(d: &Path, s: &Path) {
    git(d, &["switch", "-qc", "feat/x"]);
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(d, &["add", "a.txt"]);
    git(d, &["commit", "-qm", "work"]);
    fael(
        d,
        s,
        None,
        &["add", "note", "merged work note", "--files", "a.txt"],
    );
    git(d, &["switch", "-q", "main"]);
    git(d, &["merge", "-q", "--no-ff", "feat/x"]);
}

#[test]
fn the_edit_push_hides_a_note_a_pull_proved_merged() {
    let (d, s) = repo();
    merged_note(&d, &s);
    // no pull yet: the push owns no git, so the merged note still rides it
    assert!(edit_push(&d, &s, "s1").contains("merged work note"));
    fael(&d, &s, None, &["find", "anything"]);
    assert!(!edit_push(&d, &s, "s2").contains("merged work note"));
}

#[test]
fn the_edit_push_ignores_the_cache_once_head_moved() {
    let (d, s) = repo();
    merged_note(&d, &s);
    fael(&d, &s, None, &["find", "anything"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "later"]);
    assert!(edit_push(&d, &s, "s1").contains("merged work note"));
}

#[test]
fn the_edit_push_ignores_the_cache_once_the_branch_changed() {
    let (d, s) = repo();
    merged_note(&d, &s);
    fael(&d, &s, None, &["find", "anything"]);
    // same sha, other branch: the note's own branch is "being worked on", not merged work
    git(&d, &["switch", "-q", "feat/x"]);
    assert!(edit_push(&d, &s, "s1").contains("merged work note"));
}
