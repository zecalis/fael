//! h4 (issue 01M4TFB5): `chunk done --run R` diffs R's branch, not the cwd's — a lead's done
//! per lane from the wave branch claims that lane only — and prints each file the lane
//! touched outside its declared scope: unowned, or in the open chunk whose scope holds it.

use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{args:?}: {err}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let ok = Command::new("git")
        .args(id)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A lane in its own worktree on `branch`: start `uid` as run `run`, commit `files`.
fn lane(d: &Path, state: &Path, uid: &str, branch: &str, run: &str, files: &[&str]) {
    let w = d.with_file_name(branch);
    let wt = w.to_str().unwrap();
    git(d, &["worktree", "add", "-q", "-b", branch, wt, "main"]);
    fael(&w, state, &["chunk", "start", uid, "--run", run]);
    for f in files {
        let p = w.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, branch).unwrap();
        git(&w, &["add", f]);
    }
    git(&w, &["commit", "-qm", branch]);
}

#[test]
fn done_from_the_wave_branch_claims_the_lanes_diff_and_says_what_left_its_scope() {
    let tmp = std::env::temp_dir().join(format!("fael-done-scope-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    // no canonicalize: Windows' `\\?\C:\…` breaks `git worktree add`, and no path is compared
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("README"), "base").unwrap();
    git(&d, &["add", "README"]);
    git(&d, &["commit", "-qm", "init"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    let head = "refs/remotes/origin/HEAD";
    git(&d, &["symbolic-ref", head, "refs/remotes/origin/main"]);
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/shared"), "Cargo.lock\n").unwrap();
    let f = |args: &[&str]| fael(&d, &state, args);
    let add = |label: &str, scope: &str| {
        let args = ["--brief", "b", "--label", label, "--scope", scope];
        f(&[&["chunk", "add", label], &args[..]].concat())
    };
    let (l1, l2, _o) = (add("l1", "l1/"), add("l2", "l2/"), add("o", "o/"));
    let l1_files = ["l1/a.txt", "stray.txt", "o/x.txt", "Cargo.lock"];
    lane(&d, &state, &l1, "lane1", "R1", &l1_files);
    lane(&d, &state, &l2, "lane2", "R2", &["l2/b.txt"]);
    // the lead's wave branch merges both lanes
    git(&d, &["switch", "-q", "-c", "wave", "main"]);
    git(&d, &["merge", "-q", "--no-edit", "lane1", "lane2"]);

    let out = f(&["chunk", "done", &l1, "h", "--run", "R1"]);
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &l1, "--json"])).unwrap();
    let scope = serde_json::json!(["Cargo.lock", "l1/a.txt", "o/x.txt", "stray.txt"]);
    assert_eq!(c["scope"], scope, "lane1's diff, not the wave's l2/b.txt");
    // l1/a.txt is inside, Cargo.lock is shared: nobody's, never said
    let block = out.lines().skip(1).collect::<Vec<_>>().join("\n");
    let want = "declared scope: l1/\noutside, in inbox/o: o/x.txt\noutside, unowned: stray.txt";
    assert_eq!(block, want);

    // the lead merges in the lane's own worktree, now on a wave branch: ending the run there
    // must not swap its branch for the cwd's before done reads it; all inside: no block
    let w2 = d.with_file_name("lane2");
    git(&w2, &["switch", "-q", "-c", "wave2", "wave"]);
    let out = fael(&w2, &state, &["chunk", "done", &l2, "h", "--run", "R2"]);
    assert!(!out.contains("declared scope"), "{out}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &l2, "--json"])).unwrap();
    assert_eq!(c["scope"], serde_json::json!(["l2/b.txt"]));
}
