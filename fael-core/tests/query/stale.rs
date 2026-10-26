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
    // #293: a route with a query, a placeholder, a glob and dotted code
    // identifiers (`process.env`, `np.sum`, `.clone()`) name no file; a bare
    // file name with a file's extension does, and an anchor cites its file
    assert_eq!(
        stale_refs(
            &r,
            &text_row(
                "A0000000000000000000000026",
                "`dev/ui?s=marks` `e2e/<flow>.ts` `src/*.rs` `createDocumentService.copy` \
                 `document.type` `money.read` `process.env` `np.sum` `.clone()` `README.MD` \
                 `src/keep.rs#usage` `src/gone.rs#L3`"
            ),
            &al
        ),
        ["README.MD", "src/gone.rs"]
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
fn backtick_paths_skips_an_unpaired_last_backtick() {
    assert!(backtick_paths("cause → fix `src/a.rs").is_empty());
    assert_eq!(
        backtick_paths("`src/a.rs` then `oops b/c"),
        vec!["src/a.rs"]
    );
    assert_eq!(backtick_paths("use `` a`b/c.rs `` here"), vec!["a`b/c.rs"]);
}

#[test]
fn code_spans_pair_runs_of_equal_length() {
    assert_eq!(
        code_spans("a `x` b ``y`z`` c"),
        [
            (false, "a "),
            (true, "x"),
            (false, " b "),
            (true, "y`z"),
            (false, " c")
        ]
    );
    // an unclosed run is plain text, and a later span still pairs
    assert_eq!(
        code_spans("`a ``b`` c"),
        [(false, "`a "), (true, "b"), (false, " c")]
    );
}
