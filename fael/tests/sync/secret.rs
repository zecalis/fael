//! A leaked row in one writer's ref never enters another clone's journal (and
//! so never travels under that clone's ref): sync ingest runs the same secret
//! check as `add`, skips the row, and names it by id + label — never the token.

use super::*;

#[test]
fn ingest_skips_a_secret_row_and_never_echoes_it() {
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
    body.push_str(&format!(
        r#"{{"v":1,"id":"{leaked}","ts":"2026-09-30T06:00:00.000Z","by":"{by}","kind":"note","text":"key is {token}","files":["doc:sync"]}}"#
    ));
    body.push('\n');
    std::fs::write(&month, body).unwrap();
    assert!(sync(&a).0, "alice pushes her journal");

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

    // the nag repeats until the owner purges at the source
    let (_, _, err) = sync(&b);
    assert!(err.contains(&format!("skipped row {leaked}")), "{err}");
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
