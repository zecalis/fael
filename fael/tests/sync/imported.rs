//! An imported row keeps its legacy `by` (`claude`, `legacy`), so no writer id
//! owns it. The clone that ran `import` carries it under its own ref; clones
//! that receive it ingest it and never push it again.

use super::*;

#[test]
fn imported_rows_travel_once_under_the_importing_clones_ref() {
    let remote = bare("imported");
    let src = repo("imported-src", "Seed", "seed@example.com");
    let a = clone(&src, "imported-a", "Alice", "alice@example.com");
    let b = clone(&src, "imported-b", "Bob", "bob@example.com");
    point(&a, &remote);
    point(&b, &remote);

    let legacy = temp("imported-legacy");
    let old = "01J8ZQ3K400000000000000001";
    std::fs::write(
        legacy.join("2026-09.jsonl"),
        format!(
            r#"{{"id":"{old}","ts":"2026-09-01T10:00:00.000Z","agent":"claude","kind":"decision","text":"legacy decision","files":["a.rs"]}}"#
        ) + "\n",
    )
    .unwrap();
    let (ok, _, err) = fael(&a, &["import", legacy.to_str().unwrap()]);
    assert!(ok, "{err}");
    let own = add(&a, "alice's own row");

    let (ok, out, err) = sync(&a);
    assert!(ok, "{err}");
    assert!(out.contains("pushed 2"), "own + imported: {out}");

    let (ok, out, err) = sync(&b);
    assert!(ok, "{err}");
    assert!(out.contains("ingested 2"), "{out}");
    let seen = unique_ids(&b);
    assert!(
        seen.contains(&old.to_string()) && seen.contains(&own),
        "{seen:?}"
    );

    // bob holds the imported row now but it is not his to push: no ref of his
    assert_eq!(fael_refs(&remote).len(), 1, "{:?}", fael_refs(&remote));
    let (ok, _, err) = sync(&b);
    assert!(ok, "{err}");
    assert_eq!(fael_refs(&remote).len(), 1, "bob must not re-push it");
}
