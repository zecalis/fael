use super::*;
use crate::parse;

fn row(id: &str, ts: &str, reference: Option<&str>) -> Row {
    Row {
        v: Some(1),
        id: id.into(),
        ts: ts.into(),
        by: "alice-3f9a".into(),
        kind: if reference.is_some() {
            String::new()
        } else {
            "note".into()
        },
        text: format!("row {id}"),
        files: if reference.is_some() {
            vec![]
        } else {
            vec!["a.rs".into()]
        },
        reference: reference.map(str::to_string),
        ..Row::default()
    }
}

fn meta() -> Meta {
    Meta::new("abc123", "https://example.test/r", "r")
}

#[test]
fn round_trip_rows_through_tree_files() {
    let rows = vec![
        row(
            "01J8ZQ3K400000000000000001",
            "2026-09-15T10:00:00.000Z",
            None,
        ),
        row(
            "01J8ZQ3K400000000000000002",
            "2026-09-16T10:00:00.000Z",
            None,
        ),
        row(
            "01J8ZQ3K400000000000000004",
            "2026-10-01T10:00:00.000Z",
            None,
        ),
    ];
    let closes = vec![row(
        "01J8ZQ3K400000000000000003",
        "2026-09-17T10:00:00.000Z",
        Some("01J8ZQ3K400000000000000001"),
    )];
    let files = tree_files(&meta(), &rows, &closes);
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "meta.json",
            "2026-09.jsonl",
            "2026-09.close.jsonl",
            "2026-10.jsonl"
        ]
    );
    assert_eq!(Meta::from_json(&files[0].body), Ok(meta()));
    let mut back = vec![];
    let mut warns = vec![];
    for f in &files[1..] {
        parse(f.body.as_bytes(), &f.path, &mut back, &mut warns);
    }
    assert!(warns.is_empty(), "{warns:?}");
    let mut want: Vec<String> = rows.iter().chain(&closes).map(|r| r.to_line()).collect();
    let mut got: Vec<String> = back.iter().map(|r| r.to_line()).collect();
    want.sort();
    got.sort();
    assert_eq!(got, want);
}

#[test]
fn stream_split_follows_the_reader_not_the_ref_field() {
    // a ref-less close (an import can carry one) still rides .close.jsonl
    let mut no_ref = row(
        "01J8ZQ3K400000000000000021",
        "2026-09-15T10:00:00.000Z",
        None,
    );
    no_ref.kind.clear();
    no_ref.files.clear();
    // an add row with a stray `ref` stays in the month file
    let mut stray = row(
        "01J8ZQ3K400000000000000022",
        "2026-09-15T10:00:00.000Z",
        None,
    );
    stray.reference = Some("01J8ZQ3K400000000000000021".into());
    let files = tree_files(&meta(), &[stray], &[no_ref]);
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["meta.json", "2026-09.jsonl", "2026-09.close.jsonl"]);
    assert!(files[1].body.contains("01J8ZQ3K400000000000000022"));
    assert!(files[2].body.contains("01J8ZQ3K400000000000000021"));
}

#[test]
fn carriers_ride_the_main_stream() {
    let mut moved = row(
        "01J8ZQ3K400000000000000011",
        "2026-09-15T10:00:00.000Z",
        None,
    );
    moved.kind.clear();
    moved.files.clear();
    moved.extra.insert(
        "moved".into(),
        serde_json::json!({"from": "a.rs", "to": "b.rs"}),
    );
    let mut restored = row(
        "01J8ZQ3K400000000000000012",
        "2026-09-15T10:00:00.000Z",
        None,
    );
    restored.kind.clear();
    restored.files.clear();
    restored.restores = Some("01J8ZQ3K400000000000000011".into());
    let files = tree_files(&meta(), &[moved, restored], &[]);
    assert_eq!(files.len(), 2); // meta.json + one month file, no .close file
    assert_eq!(files[1].path, "2026-09.jsonl");
}

#[test]
fn ref_name_ok_and_rejected() {
    assert_eq!(
        ref_name("abc123", "alice-3f9a"),
        Ok("refs/fael/abc123/alice-3f9a".into())
    );
    for bad in [
        "", "a/b", ".a", "a..b", "..", "a b", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b",
        "a@{b", "a.lock", "a/", "a.",
    ] {
        assert!(ref_name("abc123", bad).is_err(), "{bad:?}");
        assert!(ref_name(bad, "alice-3f9a").is_err(), "repo {bad:?}");
    }
}

#[test]
fn union_dedupes_by_id_first_wins() {
    let a = row(
        "01J8ZQ3K400000000000000001",
        "2026-09-15T10:00:00.000Z",
        None,
    );
    let mut a2 = a.clone();
    a2.text = "forked copy".into();
    let b = row(
        "01J8ZQ3K400000000000000002",
        "2026-09-16T10:00:00.000Z",
        None,
    );
    let local = vec![a.clone()];
    let fetched = vec![a2, b.clone(), b.clone()];
    let miss = missing(&fetched, &local);
    assert_eq!(miss, vec![b.clone()]);
    assert_eq!(union(&fetched, &local), vec![a, b]);
}

#[test]
fn meta_origin_never_carries_credentials() {
    for (raw, want) in [
        (
            "https://alice:ghp_x@github.com/acme/app.git",
            "https://github.com/acme/app.git",
        ),
        (
            "ssh://git@nas.local:22/team/m.git",
            "ssh://nas.local:22/team/m.git",
        ),
        ("https://github.com/acme/app", "https://github.com/acme/app"),
        ("git@github.com:acme/app.git", "git@github.com:acme/app.git"),
        ("/srv/git/app.git", "/srv/git/app.git"),
        ("", ""),
    ] {
        assert_eq!(Meta::new("abc123", raw, "app").origin, want, "{raw}");
    }
}

#[test]
fn validate_meta_rejects_version_and_repo_mismatch() {
    assert!(validate(&meta(), "abc123").is_ok());
    let mut v2 = meta();
    v2.format_version = 2;
    assert!(validate(&v2, "abc123").is_err());
    assert!(validate(&meta(), "other").is_err());
}

#[test]
fn a_git_error_loses_every_url_password() {
    let raw = "fatal: unable to access 'https://u:ghp_x@host/m.git/': 403\nvia http://a:b@c d";
    assert_eq!(
        strip_userinfo(raw),
        "fatal: unable to access 'https://host/m.git/': 403\nvia http://c d"
    );
}
