//! h4 (issue 01M4TFB5): `chunk done --run R` diffs R's branch, not the cwd's — a lead's done
//! per lane from the wave branch claims that lane only — and prints each file the lane
//! touched outside its declared scope: unowned, or in the open chunk whose scope holds it.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID").env_remove("FAEL_SESSION").env_remove("CODEX_THREAD_ID")
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

/// A lane in its own worktree on `branch` off `from`: start `uid` as run `run`, commit `files`.
fn lane(
    d: &Path,
    state: &Path,
    uid: &str,
    (branch, from): (&str, &str),
    run: &str,
    files: &[&str],
) {
    let w = d.with_file_name(branch);
    let wt = w.to_str().unwrap();
    git(d, &["worktree", "add", "-q", "-b", branch, wt, from]);
    fael(&w, state, &["chunk", "start", uid, "--run", run]);
    for f in files {
        let p = w.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, branch).unwrap();
        git(&w, &["add", f]);
    }
    git(&w, &["commit", "-qm", branch]);
}

/// A repo on `main` with `origin/HEAD` set and `Cargo.lock` shared; returns (repo, state dir).
fn init() -> (PathBuf, PathBuf) {
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
    (d, state)
}

#[test]
fn done_from_the_wave_branch_claims_the_lanes_diff_and_says_what_left_its_scope() {
    let (d, state) = init();
    let f = |args: &[&str]| fael(&d, &state, args);
    let add = |label: &str, scope: &str| {
        let args = ["--brief", "b", "--label", label, "--scope", scope];
        f(&[&["chunk", "add", label], &args[..]].concat())
    };
    let (l1, l2, _o) = (add("l1", "l1/"), add("l2", "l2/"), add("o", "o/"));
    let l1_files = ["l1/a.txt", "stray.txt", "o/x.txt", "Cargo.lock"];
    lane(&d, &state, &l1, ("lane1", "main"), "R1", &l1_files);
    lane(&d, &state, &l2, ("lane2", "main"), "R2", &["l2/b.txt"]);
    // the lead's wave branch merges both lanes
    git(&d, &["switch", "-q", "-c", "wave", "main"]);
    git(&d, &["merge", "-q", "--no-edit", "lane1", "lane2"]);

    // l1/a.txt is inside, Cargo.lock is shared: nobody's, never said
    let want = "declared scope: l1/\noutside, in inbox/o: o/x.txt\noutside, unowned: stray.txt";
    // the lead's run end at the lane's exit says it first, before the lane lands
    let out = f(&["run", "end", "R1"]);
    assert_eq!(out, format!("run R1: {l1} {want}"));
    let out = f(&["chunk", "done", &l1, "h", "--run", "R1"]);
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &l1, "--json"])).unwrap();
    let scope = serde_json::json!(["Cargo.lock", "l1/a.txt", "o/x.txt", "stray.txt"]);
    assert_eq!(c["scope"], scope, "lane1's diff, not the wave's l2/b.txt");
    // run end's claim is the scope now: said once, done after it is silent
    assert_eq!(out.lines().count(), 1, "{out}");
    // a run nobody ended: done says it
    let l3 = add("l3", "l3/");
    lane(
        &d,
        &state,
        &l3,
        ("lane3", "main"),
        "R3",
        &["l3/a.txt", "stray3.txt"],
    );
    let out = f(&["chunk", "done", &l3, "h", "--run", "R3"]);
    let block = out.lines().skip(1).collect::<Vec<_>>().join("\n");
    assert_eq!(block, "declared scope: l3/\noutside, unowned: stray3.txt");

    // the lead merges in the lane's own worktree, now on a wave branch: ending the run there
    // must not swap its branch for the cwd's before done reads it; all inside: no block
    let w2 = d.with_file_name("lane2");
    git(&w2, &["switch", "-q", "-c", "wave2", "wave"]);
    let out = fael(&w2, &state, &["chunk", "done", &l2, "h", "--run", "R2"]);
    assert!(!out.contains("declared scope"), "{out}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &l2, "--json"])).unwrap();
    assert_eq!(c["scope"], serde_json::json!(["l2/b.txt"]));

    // issue 01M4TPHM: a solo agent starts on the trunk, then branches: the cwd's branch is
    // the work, not the trunk the run recorded
    let s = add("s", "s/");
    git(&d, &["switch", "-q", "main"]);
    f(&["chunk", "start", &s]);
    git(&d, &["switch", "-q", "-c", "solo"]);
    for p in ["s/a.txt", "solo.txt"] {
        std::fs::create_dir_all(d.join(p).parent().unwrap()).unwrap();
        std::fs::write(d.join(p), "solo").unwrap();
        git(&d, &["add", p]);
    }
    git(&d, &["commit", "-qm", "solo"]);
    let out = f(&["chunk", "done", &s, "h"]);
    assert!(out.ends_with("outside, unowned: solo.txt"), "{out}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &s, "--json"])).unwrap();
    assert_eq!(c["scope"], serde_json::json!(["s/a.txt", "solo.txt"]));
}

#[test]
fn a_pair_with_an_after_starts_as_one_run_and_a_stacked_lane_claims_only_its_own_diff() {
    let (d, state) = init();
    let f = |args: &[&str]| fael(&d, &state, args);
    let add = |label: &str, more: &[&str]| {
        let scope = format!("{label}/");
        let args = [
            "chunk", "add", label, "--brief", "b", "--label", label, "--scope", &scope,
        ];
        f(&[&args[..], more].concat())
    };
    // two small chunks in order: paired, one start claims both (no `--force` past the
    // after), one worktree, and each done lands under the same run
    let p1 = add("p1", &[]);
    let p2 = add("p2", &["--after", &p1]);
    let pair = format!(r#"[{{"op":"pair","chunks":["{p1}","{p2}"]}}]"#);
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["chunk", "apply", "--json", "-"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", &state)
        .env_remove("FAEL_DIR")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut c.stdin.take().unwrap(), pair.as_bytes()).unwrap();
    assert!(c.wait().unwrap().success());
    let both = ["p1/a.txt", "p2/b.txt"];
    lane(&d, &state, &p2, ("lanep", "main"), "RP", &both);
    // each done of the pair: the partner's files are the pair's, never "outside"
    for u in [&p1, &p2] {
        let out = f(&["chunk", "done", u, "h", "--run", "RP"]);
        assert!(!out.contains("declared scope"), "{out}");
        let c: Value = serde_json::from_str(&f(&["chunk", "show", u, "--json"])).unwrap();
        assert_eq!(c["scope"], serde_json::json!(both));
    }

    // issue 01M4TVDGG: lane b starts on lane a's unmerged branch — done claims b's files only
    let (a, b) = (add("a", &[]), add("b", &[]));
    lane(&d, &state, &a, ("lanea", "main"), "RA", &["a/x.txt"]);
    lane(&d, &state, &b, ("laneb", "lanea"), "RB", &["b/y.txt"]);
    let out = f(&["chunk", "done", &b, "h", "--run", "RB"]);
    assert!(!out.contains("declared scope"), "{out}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &b, "--json"])).unwrap();
    assert_eq!(c["scope"], serde_json::json!(["b/y.txt"]));

    // a lead ends lane f's run, then rebases it onto lane e landed before it: done keeps f's own
    let (e, g) = (add("e", &[]), add("f", &[]));
    lane(&d, &state, &e, ("lanee", "main"), "RE", &["e/x.txt"]);
    lane(&d, &state, &g, ("lanef", "main"), "RF", &["f/y.txt"]);
    assert_eq!(
        f(&["run", "end", "RF"]),
        "",
        "all inside: run end says nothing"
    );
    git(&d.with_file_name("lanef"), &["rebase", "-q", "lanee"]);
    let out = f(&["chunk", "done", &g, "h", "--run", "RF"]);
    assert!(!out.contains("declared scope"), "{out}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &g, "--json"])).unwrap();
    assert_eq!(c["scope"], serde_json::json!(["f/y.txt"]));

    // issue 01M4WZ4H: lane k's worker switches to a branch of its own and commits there — run
    // end claims that branch and says so, never an empty claim of the recorded one
    let k = add("k", &[]);
    let wk = d.with_file_name("lanek");
    git(
        &d,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lanek",
            wk.to_str().unwrap(),
            "main",
        ],
    );
    fael(&wk, &state, &["chunk", "start", &k, "--run", "RK"]);
    git(&wk, &["switch", "-q", "-c", "lanek-own", "main"]);
    std::fs::create_dir_all(wk.join("k")).unwrap();
    std::fs::write(wk.join("k/z.txt"), "k").unwrap();
    git(&wk, &["add", "k/z.txt"]);
    git(&wk, &["commit", "-qm", "k"]);
    let out = f(&["run", "end", "RK"]);
    assert!(out.contains("claimed lanek-own"), "{out}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &k, "--json"])).unwrap();
    assert_eq!(c["touched"], serde_json::json!(["k/z.txt"]));

    // issue 01M4W673: lane h starts on a stack whose commit appends to README, then the trunk
    // edits README's first line and the lead rebases the lane, stack and all: its start commit
    // is no ancestor any more, and the stack's copy has a new patch (its context moved)
    let h = add("h", &[]);
    let readme = |s: &str| std::fs::write(d.join("README"), s).unwrap();
    readme("1\n2\n3\n");
    git(&d, &["commit", "-qam", "readme"]);
    git(&d, &["switch", "-q", "-c", "stack"]);
    readme("1\n2\n3\nstack\n");
    git(&d, &["commit", "-qam", "stack"]);
    git(&d, &["switch", "-q", "main"]);
    lane(&d, &state, &h, ("laneh", "stack"), "RH", &["h/z.txt"]);
    readme("one\n2\n3\n");
    git(&d, &["commit", "-qam", "trunk moves"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(&d.with_file_name("laneh"), &["rebase", "-q", "main"]);
    f(&["chunk", "done", &h, "h", "--run", "RH"]);
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &h, "--json"])).unwrap();
    assert_eq!(
        c["scope"],
        serde_json::json!(["h/z.txt"]),
        "not the stack's README"
    );
}

/// plan fael-row-retire w2: done names the open decisions and notes on the files the chunk
/// touched, verdicts ready, at most five then where the rest are; never an issue, never a
/// row on a file the chunk left alone.
#[test]
fn done_lists_the_open_rows_on_touched_files_with_their_verdicts() {
    let (d, state) = init();
    let f = |args: &[&str]| fael(&d, &state, args);
    let add = |label: &str| f(&["chunk", "add", label, "--brief", "b", "--scope", "w/"]);
    std::fs::create_dir_all(d.join("w")).unwrap();
    for p in ["w/a.txt", "w/b.txt"] {
        std::fs::write(d.join(p), "v1").unwrap();
    }
    git(&d, &["add", "w"]);
    git(&d, &["commit", "-qm", "w"]);
    let row = |kind: &str, file: &str| {
        let out = f(&["add", kind, &format!("{kind} on {file}"), "--files", file]);
        let id = out
            .split_whitespace()
            .find(|w| w.starts_with("01"))
            .unwrap();
        id.trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_string()
    };
    let ids: Vec<String> = (0..6).map(|_| row("decision", "w/a.txt")).collect();
    let (issue, other) = (row("issue", "w/a.txt"), row("note", "w/b.txt"));
    // a chunk whose diff touches w/a.txt
    let (u, v) = (add("u"), add("v"));
    f(&["chunk", "start", &u]);
    git(&d, &["switch", "-q", "-c", "touch"]);
    std::fs::write(d.join("w/a.txt"), "v2").unwrap();
    git(&d, &["commit", "-qam", "touch"]);
    let out = f(&["chunk", "done", &u, "h"]);
    let id = said_id(&out, &ids[0]).unwrap_or_default();
    for want in [
        format!("fael bump {id}"),
        format!("fael close {id} \"now in `w/a.txt`\""),
        format!("--supersedes {id}"),
        "+1 more: fael find --files w/a.txt".into(),
        "chain several in one shell call with &&".into(),
    ] {
        assert!(out.contains(&want), "{want}: {out}");
    }
    for id in [&ids[5], &issue, &other] {
        assert!(said_id(&out, id).is_none(), "{id}: {out}");
    }
    // a chunk whose diff touches nothing with rows: nothing extra
    git(&d, &["switch", "-q", "main"]);
    f(&["chunk", "start", &v]);
    git(&d, &["switch", "-q", "-c", "quiet"]);
    std::fs::write(d.join("w/c.txt"), "v1").unwrap();
    git(&d, &["add", "w/c.txt"]);
    git(&d, &["commit", "-qm", "quiet"]);
    assert_eq!(
        f(&["chunk", "done", &v, "h"]),
        format!("chunk {v} → review")
    );
}

/// The short id `out` names `id` by — fael abbreviates, and no other row's
/// short id is a prefix of this one's. `None` when `out` never names it.
fn said_id(out: &str, id: &str) -> Option<String> {
    out.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|w| w.len() >= 8 && id.starts_with(w))
        .map(String::from)
}
