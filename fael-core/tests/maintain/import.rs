use crate::common::*;
use fael_core::*;
use std::fs;

fn line(kind: &str, id: &str, files: &str) -> String {
    format!(
        r#"{{"v":1,"ts":"2026-01-01T00:00:00Z","by":"delamind","id":"{id}","kind":"{kind}","text":"row {id}","files":[{files}]}}"#
    )
}

fn close(id: &str, target: &str) -> String {
    format!(
        r#"{{"v":1,"ts":"2026-01-02T00:00:00Z","by":"delamind","id":"{id}","ref":"{target}","text":"done","files":[]}}"#
    )
}

#[test]
fn import_folds_closes_and_skips_what_is_not_a_fael_row() {
    let r = root();
    let fael = fael_of(&r);
    let src = r.join("old-log");
    fs::create_dir_all(&src).unwrap();
    let lines = [
        line("decision", "m0001", r#""src/a.rs""#),
        line("issue", "m0002", r#""src/b.rs""#),
        close("m0004", "m0001"),
        close("m0005", "missing-id"),
        r#"{"v":1,"ts":"2026-01-02T00:00:00Z","by":"delamind","kind":"note","text":"no id here","files":["src/d.rs"]}"#.to_string(),
        r#"{"v":1,"ts":"2026-01-03T00:00:00Z","by":"delamind","id":"m0007","kind":"note","text":"no files here"}"#.to_string(),
        line("note", "m0008", r#""doc:pricing""#),
        // an old row with no `by` is not read
        r#"{"ts":"2026-01-04T00:00:00Z","agent":"delamind","id":"muft0009","kind":"bug","text":"old","v":2}"#.to_string(),
    ];
    fs::write(src.join("log.delamind.jsonl"), lines.join("\n") + "\n").unwrap();
    let rep = import(&fael, None, &src, &ImportOpts::default()).unwrap();
    assert_eq!(
        (rep.adds, rep.folded, rep.carried, rep.skipped),
        (5, 1, 1, 1),
        "{rep:?}"
    );
    assert!(rep.warnings[0].contains("no `by`"), "{:?}", rep.warnings);
    assert_eq!(rep.paths.len(), 2); // .jsonl + .close.jsonl companion
    let log = read(&fael);
    let by_id = |id: &str| log.rows.iter().find(|x| x.id == id).unwrap().clone();
    assert_eq!(by_id("m0002").kind, "issue");
    assert!(closed(&log).contains("m0001")); // the close folded in
    assert!(by_id("m0001").extra.contains_key("closed"));
    assert_eq!(log.closes.len(), 1); // the unresolvable close rides the companion
    let noid = log
        .rows
        .iter()
        .find(|x| x.id.starts_with("legacy-"))
        .unwrap();
    assert_eq!(noid.text, "no id here");
    assert!(by_id("m0007").files.is_empty()); // kept as-is, read-valid
    assert_eq!(by_id("m0008").files, vec!["doc:pricing"]);
    // importing twice is safe — dedupe by id
    let rep2 = import(&fael, None, &src, &ImportOpts::default()).unwrap();
    assert_eq!(rep2.adds, 5);
    assert_eq!(read(&fael).rows.len(), log.rows.len());
}

#[test]
fn import_map_rewrites_prefixes_not_anchors() {
    let r = root();
    let fael = fael_of(&r);
    let src = r.join("old");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("log.jsonl"),
        line("decision", "m0001", r#""old/svc/a.rs","doc:pricing""#) + "\n",
    )
    .unwrap();
    let rep = import(
        &fael,
        None,
        &src,
        &ImportOpts {
            maps: vec![("old/".into(), "new/".into())],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rep.adds, 1);
    let row = &read(&fael).rows[0];
    assert_eq!(row.files, vec!["new/svc/a.rs", "doc:pricing"]);
}

#[test]
fn import_native_fael_log() {
    let r = root();
    let fael = fael_of(&r);
    let other = tmp().join("other").join(".fael").join("log");
    fs::create_dir_all(other.join("w-0000")).unwrap();
    let line = row("A0000000000000000000000001", "decision", &["x.rs"]).to_line();
    fs::write(
        other.join("w-0000").join("2026-01.jsonl"),
        line.clone() + "\n",
    )
    .unwrap();
    let rep = import(&fael, None, &other, &ImportOpts::default()).unwrap();
    assert_eq!((rep.adds, rep.skipped), (1, 0));
    assert_eq!(read(&fael).rows.len(), 1);
}

#[test]
fn import_350_rows_drops_nothing() {
    let r = root();
    let fael = fael_of(&r);
    let src = r.join("big");
    fs::create_dir_all(&src).unwrap();
    let mut lines = vec![];
    for i in 0..350 {
        let kind = if i % 10 == 0 { "issue" } else { "note" };
        if i % 70 == 0 {
            lines.push(format!(
                r#"{{"v":1,"ts":"2026-01-01T00:00:00Z","by":"w","kind":"{kind}","text":"row {i}","files":["f{i}.rs"]}}"#
            )); // no id → legacy-…
        } else {
            lines.push(line(kind, &format!("id{i:04}"), &format!(r#""f{i}.rs""#)));
        }
    }
    for (c, t) in [("c0001", "id0001"), ("c0002", "id0002"), ("c0003", "nope")] {
        lines.push(close(c, t));
    }
    fs::write(src.join("log.jsonl"), lines.join("\n") + "\n").unwrap();
    let rep = import(&fael, None, &src, &ImportOpts::default()).unwrap();
    assert_eq!(rep.skipped, 0, "{:?}", rep.warnings);
    assert_eq!(rep.adds + rep.folded + rep.carried, lines.len(), "{rep:?}");
    assert_eq!(rep.folded, 2);
    assert_eq!(rep.carried, 1);
}

#[test]
fn import_skips_a_secret_row_and_never_echoes_it() {
    let r = root();
    let fael = fael_of(&r);
    let src = tmp().join("leaky").join(".fael").join("log");
    fs::create_dir_all(src.join("w-0000")).unwrap();
    let token = format!("ghp_{}", "a".repeat(24));
    let clean = row("A0000000000000000000000001", "decision", &["x.rs"]).to_line();
    let mut leaked = row("A0000000000000000000000002", "note", &["x.rs"]);
    leaked.text = format!("token is {token}");
    fs::write(
        src.join("w-0000").join("2026-01.jsonl"),
        format!("{clean}\n{}\n", leaked.to_line()),
    )
    .unwrap();
    let rep = import(&fael, None, &src, &ImportOpts::default()).unwrap();
    assert_eq!((rep.adds, rep.skipped), (1, 1));
    assert_eq!(read(&fael).rows.len(), 1);
    let w = rep.warnings.join("\n");
    assert!(
        w.contains("2026-01.jsonl:2") && w.contains("GitHub token"),
        "{w}"
    );
    assert!(!w.contains(&token), "the warning must not echo the secret");
}
