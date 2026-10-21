//! `fael compact` is authoritative for this writer's own rows: a pruned row and
//! a folded close is not re-appended to the local log by the next sync. The
//! raw close stays on the own ref so a teammate who held the row first gets it.

use super::*;

const PRUNED: &str = "A0000000000000000000000001";
const FOLDED: &str = "A0000000000000000000000002";
const CLOSE_P: &str = "C0000000000000000000000001";
const CLOSE_F: &str = "C0000000000000000000000002";

fn row(id: &str, by: &str, files: &str) -> String {
    format!(
        r#"{{"v":1,"id":"{id}","ts":"2000-01-01T00:00:00.000Z","by":"{by}","kind":"decision","text":"old {id}","files":["{files}"]}}"#
    )
}

fn close(id: &str, by: &str, target: &str) -> String {
    format!(
        r#"{{"v":1,"id":"{id}","ts":"2000-01-02T00:00:00.000Z","by":"{by}","ref":"{target}","text":"done"}}"#
    )
}

#[test]
fn compact_prune_and_folded_closes_survive_a_sync() {
    let remote = bare("compact");
    let src = repo("compact-src", "Seed", "seed@example.com");
    let a = clone(&src, "compact-a", "Alice", "alice@example.com");
    let b = clone(&src, "compact-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let by = writer("Alice", "alice@example.com");
    let keep = add(&a, "current row");

    // a past month by hand: one closed row whose file is gone, one closed row
    // behind an opaque anchor (folds, never prunes)
    let dir = a.join(".git/fael/log").join(&by);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("2000-01.jsonl"),
        format!(
            "{}\n{}\n",
            row(PRUNED, &by, "old.rs"),
            row(FOLDED, &by, "doc:old")
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("2000-01.close.jsonl"),
        format!(
            "{}\n{}\n",
            close(CLOSE_P, &by, PRUNED),
            close(CLOSE_F, &by, FOLDED)
        ),
    )
    .unwrap();
    assert!(sync(&a).0);
    let own = fael_refs(&remote).remove(0);
    assert!(
        ref_body(&remote, &own).contains(PRUNED),
        "setup: on the ref"
    );

    let (ok, out, err) = fael(&a, &["compact", "--prune"]);
    assert!(ok, "{err}");
    assert!(out.contains("1 pruned"), "{out}");
    let (ok, _, err) = sync(&a);
    assert!(ok, "{err}");

    let seen = ids(&a);
    assert!(!seen.contains(&PRUNED.to_string()), "pruned row came back");
    assert!(!seen.contains(&CLOSE_P.to_string()), "its close came back");
    assert!(
        !seen.contains(&CLOSE_F.to_string()),
        "folded close came back"
    );
    assert!(seen.contains(&FOLDED.to_string()) && seen.contains(&keep));

    let body = ref_body(&remote, &own);
    assert!(!body.contains(&format!("\"id\":\"{PRUNED}\"")), "{body}");
    assert!(body.contains(FOLDED) && body.contains(&keep));

    // other writers still ingest everything the ref carries now
    assert!(sync(&b).0);
    let seen = ids(&b);
    assert!(seen.contains(&FOLDED.to_string()) && seen.contains(&keep));
    assert!(!seen.contains(&PRUNED.to_string()));
}

#[test]
fn teammate_holding_the_row_still_gets_a_folded_close() {
    let remote = bare("compact-mate");
    let src = repo("compact-mate-src", "Seed", "seed@example.com");
    let a = clone(&src, "compact-mate-a", "Alice", "alice@example.com");
    let b = clone(&src, "compact-mate-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);
    let by = writer("Alice", "alice@example.com");

    // the row alone reaches Bob first
    let dir = a.join(".git/fael/log").join(&by);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("2000-01.jsonl"),
        format!("{}\n", row(FOLDED, &by, "doc:old")),
    )
    .unwrap();
    assert!(sync(&a).0);
    assert!(sync(&b).0);
    assert!(ids(&b).contains(&FOLDED.to_string()));

    // then the close lands and is pushed, Alice compacts and syncs before Bob does
    std::fs::write(
        dir.join("2000-01.close.jsonl"),
        format!("{}\n", close(CLOSE_F, &by, FOLDED)),
    )
    .unwrap();
    assert!(sync(&a).0);
    let (ok, _, err) = fael(&a, &["compact", "--prune"]);
    assert!(ok, "{err}");
    assert!(sync(&a).0);
    assert!(!ids(&a).contains(&CLOSE_F.to_string()));

    assert!(sync(&b).0);
    assert!(
        ids(&b).contains(&CLOSE_F.to_string()),
        "teammate never got the close"
    );
}
