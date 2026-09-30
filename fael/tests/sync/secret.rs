//! A leaked row never travels: sync runs the same secret check as `add` on
//! both sides — push drops it from the writer's own ref, ingest skips one an
//! older fael already pushed — and names it by id + label, never the token.

use super::*;
use std::io::Write;
use std::process::Stdio;

#[test]
fn a_secret_row_is_never_pushed_nor_ingested_and_never_echoed() {
    let remote = bare("secret");
    let src = repo("secret-src", "Seed", "seed@example.com");
    let a = clone(&src, "secret-a", "Alice", "alice@example.com");
    let b = clone(&src, "secret-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let clean = add(&a, "a row that is fine");

    // a leaked row already in alice's journal (say, from before the check):
    // `add` would refuse it, so it is appended by hand.
    let token = format!("ghp_{}", "a".repeat(24));
    let leaked = "01M3ZZZZZZZZZZZZZZZZZZZZZZ";
    let by = writer("Alice", "alice@example.com");
    let line = format!(
        r#"{{"v":1,"id":"{leaked}","ts":"2026-09-30T06:00:00.000Z","by":"{by}","kind":"note","text":"key is {token}","files":["doc:sync"]}}"#
    );
    let common = git_out(
        &a,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    let dir = Path::new(&common).join("fael").join("log").join(&by);
    let month = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| !p.to_string_lossy().ends_with(".close.jsonl"))
        .unwrap();
    let mut body = std::fs::read_to_string(&month).unwrap();
    body.push_str(&line);
    body.push('\n');
    std::fs::write(&month, body).unwrap();

    // push side: the clean row travels, the leaked one stays home
    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("pushed 1"), "only the clean row: {out}");
    assert!(err.contains(&format!("not pushing row {leaked}")), "{err}");
    assert!(err.contains("GitHub token"), "label named: {err}");
    assert!(!format!("{out}{err}").contains(&token), "token echoed");
    let own = fael_refs(&remote).remove(0);
    let pushed = ref_body(&remote, &own);
    assert!(
        pushed.contains(&clean) && !pushed.contains(&token),
        "{pushed}"
    );

    // ingest side: an older fael, with no push-side check, left it on the ref
    plant(&remote, &own, &line);
    let (ok, out, err) = sync(&b);
    assert!(ok, "{err}");
    assert!(out.contains("ingested 1"), "only the clean row: {out}");
    assert!(err.contains(&format!("skipped row {leaked}")), "{err}");
    assert!(err.contains("GitHub token"), "label named: {err}");
    assert!(!format!("{out}{err}").contains(&token), "token echoed");
    assert!(err.contains(&by), "writer named: {err}");

    let seen = unique_ids(&b);
    assert!(
        seen.contains(&clean) && !seen.contains(&leaked.to_string()),
        "{seen:?}"
    );
    let common = git_out(
        &b,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    );
    assert!(
        !dir_holds(&Path::new(&common).join("fael"), &token),
        "the token reached bob's journal"
    );

    // the nag repeats while the ref still carries it…
    let (_, _, err) = sync(&b);
    assert!(err.contains(&format!("skipped row {leaked}")), "{err}");
    // …and the owner's next sync drops it from the tip
    assert!(sync(&a).0);
    assert!(
        !ref_body(&remote, &own).contains(&token),
        "still on the tip"
    );
}

/// What an older fael left on `rname`: one extra month file holding `line`,
/// committed on top of the tip straight in the bare remote.
fn plant(remote: &Path, rname: &str, line: &str) {
    let tip = tip(remote, rname);
    let blob = git_in(
        remote,
        &["hash-object", "-w", "--stdin"],
        &format!("{line}\n"),
    );
    let listing = git_out(remote, &["ls-tree", &tip]);
    let entries = format!("{listing}\n100644 blob {blob}\t2000-01.jsonl\n");
    let tree = git_in(remote, &["mktree"], &entries);
    let id = ["-c", "user.name=old", "-c", "user.email=old@example.com"];
    let commit = git_out(
        remote,
        &[
            &id[..],
            &["commit-tree", &tree, "-p", &tip, "-m", "older fael"],
        ]
        .concat(),
    );
    git(remote, &["update-ref", rname, &commit]);
}

/// `git <args>` with `input` on stdin → trimmed stdout.
fn git_in(d: &Path, args: &[&str], input: &str) -> String {
    let mut c = Command::new("git")
        .args(args)
        .current_dir(d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// Whether any file under `dir` contains `needle`.
fn dir_holds(dir: &Path, needle: &str) -> bool {
    std::fs::read_dir(dir).unwrap().any(|e| {
        let p = e.unwrap().path();
        if p.is_dir() {
            dir_holds(&p, needle)
        } else {
            std::fs::read_to_string(&p).is_ok_and(|s| s.contains(needle))
        }
    })
}
