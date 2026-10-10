//! PLAN-fael-experience-loop chunk 6, the carry-back: an edit of a file whose
//! closed issue's fix reached main puts "what broke → how it was fixed" in
//! front of the agent once per session; a fix not found there, or a read, is
//! silent. A close written now is judged by PLAN-fael-fix-evidence: a commit
//! on main citing `(fael:<prefix>)`, or the close's `(#N)` there — never a
//! sha. An old close (`backdate`) keeps the sha + subject rule.

use super::{fael, fael_env, git, json, repo};
use std::path::Path;

const SAID: &str = "broke before";

fn hook(d: &Path, event: &str, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael(d, &["hook", event, "--client", "claude"], &input);
    assert!(ok, "{err}");
    out
}

/// An issue on `src/a.rs`, closed with `why`.
fn closed_with(d: &Path, why: &str) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let add = [
        "add",
        "issue",
        "retry loops",
        "--files",
        "src/a.rs",
        "--key",
        "a:retry",
    ];
    let (ok, _, err) = fael(d, &add, "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(d, &["close", "--key", "a:retry", why], "");
    assert!(ok, "{err}");
}

/// The short id `fael find` prints for the issue, as an agent cites it.
fn short_id(d: &Path) -> String {
    let (_, out, _) = fael(d, &["find", "--key", "a:retry", "--all"], "");
    out.split(['[', ']']).nth(1).expect(&out).to_string()
}

fn commit_msg(d: &Path, msg: &str) {
    git(d, &["commit", "-q", "--allow-empty", "-m", msg]);
}

/// Moves every close row to before the fix-evidence cutoff: an old close.
fn backdate(d: &Path) {
    for who in std::fs::read_dir(d.join(".fael/log")).unwrap() {
        for f in std::fs::read_dir(who.unwrap().path()).unwrap() {
            let f = f.unwrap().path();
            if !f.to_string_lossy().ends_with(".close.jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&f).unwrap();
            let rows: String = text
                .lines()
                .map(|l| {
                    let mut v: serde_json::Value = serde_json::from_str(l).unwrap();
                    v["ts"] = "2026-10-02T00:00:00.000Z".into();
                    format!("{v}\n")
                })
                .collect();
            std::fs::write(&f, rows).unwrap();
        }
    }
}

fn carry(d: &Path) -> (u64, u64) {
    let (_, out, _) = fael(d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let n = |k: &str| v["said"]["carry"][k].as_u64().unwrap();
    (n("said"), n("earned"))
}

#[test]
fn a_fixed_bug_is_carried_back_once_per_session_and_earns_on_a_find() {
    let d = repo();
    std::fs::create_dir_all(d.join("tests")).unwrap();
    std::fs::write(d.join("tests/retry.rs"), "\n").unwrap();
    closed_with(&d, "no cap → cap at 3; guard `tests/retry.rs`");
    assert!(!hook(&d, "edit", "s0").contains(SAID), "no commit cites it");
    commit_msg(
        &d,
        &format!("fix: cap retries at 3 (fael:{})", short_id(&d)),
    );
    let first = hook(&d, "edit", "s1");
    assert!(
        first.contains("src/a.rs broke before")
            && first.contains("\\\"retry loops\\\" was fixed: no cap → cap at 3")
            && first.contains("fael find "),
        "{first}"
    );
    assert!(!hook(&d, "edit", "s1").contains(SAID));
    assert_eq!(carry(&d), (1, 0));
    // the agent pulls the whole story by the short id the line named
    let id = first.split("`fael find ").nth(1).expect(&first);
    let id = id.split('`').next().unwrap();
    let env = [("CLAUDE_CODE_SESSION_ID", "s1")];
    let (ok, _, err) = fael_env(&d, &["find", id], "", &env);
    assert!(ok, "{err}");
    assert_eq!(carry(&d), (1, 1));
    // another session is told again
    assert!(hook(&d, "edit", "s2").contains(SAID));
}

#[test]
fn no_fix_named_or_a_read_is_silent() {
    let d = repo();
    closed_with(&d, "not a bug, works as meant");
    assert!(!hook(&d, "edit", "s1").contains(SAID));
    let d = repo();
    closed_with(&d, "fixed in (#12)");
    assert!(!hook(&d, "edit", "s0").contains(SAID), "(#12) not on main");
    commit_msg(&d, "cap retries (#12)");
    assert!(!hook(&d, "read", "s1").contains(SAID));
    // the read spent nothing: the edit that follows is told
    assert!(hook(&d, "edit", "s1").contains(SAID));
}

/// A new close's sha is no evidence (fael:01M4HTZ4): not when it is on main,
/// not when this clone lacks it (the old rule read that as reached,
/// fael 01M3MJKS), not when its subject lands in a squash.
#[test]
fn a_new_close_naming_only_a_sha_is_not_carried() {
    let d = repo();
    commit_msg(&d, "fix: cap retries at 3");
    let sha = git(&d, &["rev-parse", "HEAD"]);
    closed_with(&d, &format!("no cap → cap at 3; {sha}"));
    assert!(!hook(&d, "edit", "s1").contains(SAID), "sha on main");
    let d = repo();
    closed_with(&d, "no cap → cap at 3; fixed in 01defaced01");
    assert!(!hook(&d, "edit", "s1").contains(SAID), "sha not in clone");
}

/// A squash keeps each commit's `(fael:<prefix>)` in its body; a
/// cherry-pick keeps the message whole. A bare id is no citation.
#[test]
fn a_cite_in_a_squash_body_or_a_cherry_pick_is_carried_a_bare_id_is_not() {
    let d = repo();
    closed_with(&d, "no cap → cap at 3");
    let id = short_id(&d);
    commit_msg(&d, &format!("fixes {id} in passing"));
    assert!(!hook(&d, "edit", "s1").contains(SAID), "bare id counted");
    commit_msg(
        &d,
        &format!("feat: retry (#9)\n\n* fix: cap at 3 (fael:{id})"),
    );
    assert!(hook(&d, "edit", "s2").contains(SAID), "squash body missed");

    let d = repo();
    closed_with(&d, "no cap → cap at 3");
    let home = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(&d, &["switch", "-q", "-c", "side"]);
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    git(&d, &["add", "src/b.rs"]);
    commit_msg(&d, &format!("fix: cap at 3 (fael:{})", short_id(&d)));
    let pick = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["switch", "-q", &home]);
    assert!(
        !hook(&d, "edit", "s1").contains(SAID),
        "unmerged cite carried"
    );
    git(&d, &["cherry-pick", "-x", &pick]);
    assert!(hook(&d, "edit", "s2").contains(SAID), "cherry-pick missed");
}

/// Main is HEAD and `origin/HEAD` as the local refs stand: a fix merged
/// upstream but not fetched is not reached, and is once `origin/HEAD` moves.
/// No main ref at all is unknown, and a new close fails closed.
#[test]
fn main_is_the_local_refs_and_no_ref_is_unknown() {
    let d = repo();
    closed_with(&d, "no cap → cap at 3");
    let home = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    let old = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", &old]);
    let origin = [
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ];
    git(&d, &origin);
    git(&d, &["switch", "-q", "-c", "upstream"]);
    commit_msg(&d, &format!("fix: cap at 3 (fael:{})", short_id(&d)));
    let fix = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["switch", "-q", &home]);
    assert!(!hook(&d, "edit", "s1").contains(SAID), "stale origin/HEAD");
    git(&d, &["update-ref", "refs/remotes/origin/main", &fix]);
    assert!(hook(&d, "edit", "s2").contains(SAID), "fetched fix missed");

    // a repo with no commit: neither HEAD nor origin/HEAD reads
    let d = repo();
    git(&d, &["update-ref", "-d", "HEAD"]);
    closed_with(&d, "no cap → cap at 3; fixed in (#9)");
    assert!(!hook(&d, "edit", "s1").contains(SAID));
}

/// An old close keeps its rule (fael:01M4GQVP): a branch sha that never
/// reached HEAD or the default branch is not carried; once a commit there
/// keeps that sha's subject (a squash does), it is.
#[test]
fn an_old_close_on_an_unmerged_branch_is_not_carried_until_its_subject_lands() {
    let d = repo();
    let home = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(&d, &["switch", "-q", "-c", "side"]);
    commit_msg(&d, "fix: cap retries at 3");
    // the full sha: a short one is all digits ~4% of the time, and a close
    // naming only digits names no fix (`sha_like` needs a letter too)
    let sha = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["switch", "-q", home.trim()]);
    closed_with(&d, &format!("no cap → cap at 3; {}", sha.trim()));
    backdate(&d);
    assert!(
        !hook(&d, "edit", "s1").contains(SAID),
        "unmerged fix carried"
    );
    commit_msg(&d, "squash (#9)\n\n* fix: cap retries at 3");
    assert!(
        hook(&d, "edit", "s2").contains(SAID),
        "merged fix not carried"
    );
    // a sha not in this clone stays no evidence either way: carried
    let d = repo();
    closed_with(&d, "no cap → cap at 3; fixed in 01defaced01");
    backdate(&d);
    assert!(hook(&d, "edit", "s1").contains(SAID));
}

/// The old rule's other evidence a squash keeps: a commit naming the id.
#[test]
fn an_old_unmerged_sha_is_carried_once_a_commit_names_the_issue() {
    let d = repo();
    let home = git(&d, &["rev-parse", "--abbrev-ref", "HEAD"]);
    git(&d, &["switch", "-q", "-c", "side"]);
    commit_msg(&d, "wip");
    let sha = git(&d, &["rev-parse", "HEAD"]);
    git(&d, &["switch", "-q", home.trim()]);
    closed_with(&d, &format!("no cap → cap at 3; {}", sha.trim()));
    backdate(&d);
    assert!(!hook(&d, "edit", "s1").contains(SAID));
    commit_msg(&d, &format!("cap retries ({})", short_id(&d)));
    assert!(hook(&d, "edit", "s2").contains(SAID));
}

/// fael:01M4HW99: two rows sharing 8 chars, as vela's `01M4G1N1…` do. The
/// hook resolves the cite against the log on disk: the shared 8-char prefix
/// names neither row, a longer one names the closed issue alone.
#[test]
fn a_cite_two_rows_share_is_not_carried_a_longer_one_is() {
    let d = repo();
    let row = |id: &str, kind: &str, file: &str| {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-10-11T00:00:00.000Z\",\"by\":\"t\",\"kind\":\"{kind}\",\"text\":\"retry loops\",\"files\":[\"{file}\"]}}\n"
        )
    };
    let (issue, twin) = ("01AAAA0000000000000000000A", "01AAAA0000000000000000000B");
    std::fs::create_dir_all(d.join(".fael/log/t")).unwrap();
    std::fs::write(
        d.join(".fael/log/t/2026-10.jsonl"),
        row(issue, "issue", "src/a.rs") + &row(twin, "note", "src/b.rs"),
    )
    .unwrap();
    std::fs::write(
        d.join(".fael/log/t/2026-10.close.jsonl"),
        format!(
            "{{\"v\":1,\"id\":\"01AAAA0000000000000000000C\",\"ts\":\"2026-10-11T00:00:01.000Z\",\"by\":\"t\",\"text\":\"no cap → cap at 3\",\"ref\":\"{issue}\"}}\n"
        ),
    )
    .unwrap();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    commit_msg(&d, "fix: cap at 3 (fael:01AAAA00)");
    assert!(
        !hook(&d, "edit", "s1").contains(SAID),
        "ambiguous cite carried"
    );
    commit_msg(&d, "fix: cap at 3 (fael:01AAAA0000000000000000000A)");
    assert!(hook(&d, "edit", "s2").contains(SAID), "unique cite missed");
}

/// The push budget for new closes: each closed issue on the file is a
/// candidate (its evidence is in git), yet one edit looks up `REACHED` (3)
/// of them, two spawns each at most (`origin/HEAD` unset here). A `git` shim
/// on PATH logs every call; only the fix lookups are counted.
#[cfg(unix)]
#[test]
fn new_closes_stay_inside_the_reached_spawn_budget() {
    use std::os::unix::fs::PermissionsExt;
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    for i in 0..8 {
        let key = format!("a:retry{i}");
        let add = [
            "add",
            "issue",
            "retry loops",
            "--files",
            "src/a.rs",
            "--key",
            &key,
        ];
        let (ok, _, err) = fael(&d, &add, "");
        assert!(ok, "{err}");
        let (ok, _, err) = fael(&d, &["close", "--key", &key, "no cap → cap at 3"], "");
        assert!(ok, "{err}");
    }
    let real = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let (bin, calls) = (d.join("shim"), d.join("git-calls"));
    std::fs::create_dir_all(&bin).unwrap();
    let shim = format!(
        "#!/bin/sh\necho \"$*\" >> '{}'\nexec '{}' \"$@\"\n",
        calls.display(),
        real.trim()
    );
    std::fs::write(bin.join("git"), shim).unwrap();
    std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&d.join("src/a.rs"))
    );
    let args = ["hook", "edit", "--client", "claude"];
    let (ok, out, err) = fael_env(&d, &args, &input, &[("PATH", &path)]);
    assert!(ok, "{err}");
    assert!(!out.contains(SAID), "{out}");
    let log = std::fs::read_to_string(&calls).unwrap_or_default();
    let lookups = log.lines().filter(|l| l.contains("--grep=(fael:")).count();
    assert_eq!(
        lookups, 6,
        "3 closes × (HEAD origin/HEAD, then HEAD):\n{log}"
    );
}
