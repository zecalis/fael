//! `[Stale]` — backticked paths in row text with no file behind them.

use super::row;
use fael_core::*;

fn text_row(id: &str, text: &str) -> Row {
    let mut r = row(id, "note", &[], None);
    r.text = text.into();
    r
}

#[test]
fn backtick_paths_picks_paths_not_talk() {
    assert_eq!(
        backtick_paths("catalog ใน `pnpm-workspace.yaml` หายไป"),
        ["pnpm-workspace.yaml"]
    );
    assert_eq!(
        backtick_paths("see `src/a.rs` and `docs/guide/` but run `git log -M`"),
        ["src/a.rs", "docs/guide/"]
    );
    // versions and bare words are not paths
    assert!(backtick_paths("drizzle `1.x` broke, pin `0.45`").is_empty());
    assert!(backtick_paths("run `merge=union` after").is_empty());
    // fenced blocks are commands and output, not pointers
    assert!(backtick_paths("```\nfael doctor --fix\nsrc/a.rs\n```").is_empty());
    // a URL is a link, not a repo path
    assert!(backtick_paths("see `https://github.com/zecalis/fael`").is_empty());
    assert!(backtick_paths("no backticks at all").is_empty());
}

#[test]
fn stale_refs_flags_only_what_is_gone() {
    let r = std::env::temp_dir().join(format!("fael-stale-{}", ulid()));
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(r.join("src/keep.rs"), "x").unwrap();
    let al = Aliases::default();
    // present on disk: not stale
    assert!(
        stale_refs(
            &r,
            &text_row("A0000000000000000000000020", "see `src/keep.rs`"),
            &al
        )
        .is_empty()
    );
    // deleted: stale
    assert_eq!(
        stale_refs(
            &r,
            &text_row(
                "A0000000000000000000000021",
                "catalog ใน `pnpm-workspace.yaml`"
            ),
            &al
        ),
        ["pnpm-workspace.yaml"]
    );
    // a command naming a dir is not a path that must exist
    assert!(
        stale_refs(
            &r,
            &text_row(
                "A0000000000000000000000022",
                "point at `fael find --files <dir>/`"
            ),
            &al
        )
        .is_empty()
    );
    // renamed through the resolver: not stale (chunk 2 rule)
    let moved = Aliases::from_pairs(vec![("src/old.rs".to_string(), "src/keep.rs".to_string())]);
    assert!(
        stale_refs(
            &r,
            &text_row("A0000000000000000000000022", "see `src/old.rs`"),
            &moved
        )
        .is_empty()
    );
    // already in files[]: PartGone's job, not Stale's
    let mut filed = text_row("A0000000000000000000000023", "see `src/gone.rs`");
    filed.files = vec!["src/gone.rs".into()];
    assert!(stale_refs(&r, &filed, &al).is_empty());
    // one row, two dead pointers, deduped
    assert_eq!(
        stale_refs(
            &r,
            &text_row(
                "A0000000000000000000000024",
                "`a/b.rs` broke `a/b.rs` and `c.toml`",
            ),
            &al
        ),
        ["a/b.rs", "c.toml"]
    );
    // no file name and a first dir this repo never had: a name from
    // elsewhere (a docker image), not a path — a dir under `src/` still is
    assert_eq!(
        stale_refs(
            &r,
            &text_row(
                "A0000000000000000000000025",
                "run `verapdf/cli` on `src/gone/`"
            ),
            &al
        ),
        ["src/gone/"]
    );
}

#[test]
fn stale_refs_reads_file_line_only_and_skips_urls() {
    let r = std::env::temp_dir().join(format!("fael-stale-{}", ulid()));
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(r.join("src/a.rs"), "x").unwrap();
    let al = Aliases::default();
    // `file.rs:88[:col]` cites a line: the file is what must exist, so not stale
    assert!(
        stale_refs(
            &r,
            &text_row(
                "A0000000000000000000000025",
                "see `src/a.rs:88` and `src/a.rs:88:5`"
            ),
            &al
        )
        .is_empty()
    );
    // gone file with a location suffix: the path is flagged, without the suffix
    assert_eq!(
        stale_refs(
            &r,
            &text_row("A0000000000000000000000026", "was `src/gone.rs:12`"),
            &al
        ),
        ["src/gone.rs"]
    );
    // a URL is a link, not a repo path
    assert!(
        stale_refs(
            &r,
            &text_row(
                "A0000000000000000000000027",
                "see `https://github.com/zecalis/fael/src/a.rs`"
            ),
            &al
        )
        .is_empty()
    );
}

#[test]
fn stale_close_refs_reads_the_close_text_not_the_row() {
    let r = std::env::temp_dir().join(format!("fael-stale-{}", ulid()));
    std::fs::create_dir_all(r.join("t")).unwrap();
    std::fs::write(r.join("t/kept.sh"), "x").unwrap();
    let al = Aliases::default();
    let issue = text_row("A0000000000000000000000030", "see `t/gone.sh` in the row");
    let close = |id: &str, ts: &str, text: &str| {
        let mut c = text_row(id, text);
        c.ts = ts.into();
        c.reference = Some("A0000000000000000000000030".into());
        c
    };
    let log = |closes: Vec<Row>| Log {
        rows: vec![issue.clone()],
        closes,
        ..Log::default()
    };
    // no close record: nothing to judge (the row's own text is `stale_refs`' job)
    assert!(stale_close_refs(&r, &log(vec![]), &issue, &al).is_empty());
    // the close points at a check that is there, then at one that is gone
    let kept = close(
        "C1",
        "2026-10-01T00:00:00Z",
        "fixed, guarded by `t/kept.sh`",
    );
    assert!(stale_close_refs(&r, &log(vec![kept.clone()]), &issue, &al).is_empty());
    let gone = close("C2", "2026-10-02T00:00:00Z", "moved to `t/gone.sh:9`");
    assert_eq!(
        stale_close_refs(&r, &log(vec![kept.clone(), gone.clone()]), &issue, &al),
        ["t/gone.sh"]
    );
    // the newest close wins, whatever the file order
    let newer = close("C3", "2026-10-03T00:00:00Z", "now `t/kept.sh`");
    assert!(stale_close_refs(&r, &log(vec![newer, gone]), &issue, &al).is_empty());
    // a check that moved through the resolver is not gone
    let moved = Aliases::from_pairs(vec![("t/gone.sh".to_string(), "t/kept.sh".to_string())]);
    let gone = close("C4", "2026-10-04T00:00:00Z", "moved to `t/gone.sh`");
    assert!(stale_close_refs(&r, &log(vec![gone]), &issue, &moved).is_empty());
    let _ = std::fs::remove_dir_all(&r);
}
