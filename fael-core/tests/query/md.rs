//! `[Phantom]` in markdown (issue `ids:doctor-plans`): prose is scanned like a
//! row's text, fenced code blocks are examples (never citations), and only
//! `*.md` under the repo root is read — `.git`/`target`/`node_modules` never.

use super::row;
use fael_core::*;
use std::path::{Path, PathBuf};

const DEAD: &str = "01ZZZZ99999999999999999999";
const ALIVE: &str = "01AAAA00000000000000000001";
const CLOSED: &str = "01BBBB11111111111111111111";

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-md-{name}-{}", ulid()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// An open row plus a close row — citing either id is a live citation.
fn log() -> Log {
    Log {
        rows: vec![row(ALIVE, "note", &["src/a.rs"], None)],
        closes: vec![Row {
            id: CLOSED.into(),
            ts: "2026-09-20T00:00:00Z".into(),
            kind: "note".into(),
            text: "fixed".into(),
            reference: Some(ALIVE.into()),
            files: vec!["src/a.rs".into()],
            ..Row::default()
        }],
        warnings: vec![],
    }
}

fn put(root: &Path, path: &str, text: &str) {
    let p = root.join(path);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn prose_is_scanned_and_fenced_blocks_are_not() {
    let r = tmp("fences");
    let body = format!(
        "# plan\n\ncites {ALIVE} in prose, and {CLOSED} too — both resolve\n\n\
         dead cite {DEAD} in prose\n\n```json\n{{\"id\":\"{DEAD}\"}}\n```\n\n\
         ~~~\ndead cite {DEAD} inside a tilde fence\n~~~\n"
    );
    put(&r, "PLAN-x.md", &body);
    // line 5 is the prose citation; the fenced ones (8 and 13) never count
    assert_eq!(
        phantom_md_refs(&log(), &r),
        vec![("PLAN-x.md".to_string(), 5, DEAD.to_string())]
    );
}

#[test]
fn only_markdown_counts_and_build_dirs_are_skipped() {
    let r = tmp("scope");
    for path in [
        "README.md",
        "docs/README.MD",
        ".fapony/plan/PLAN-x.md",
        "target/gen.md",
        "node_modules/pkg/README.md",
        ".git/HOOKS.md",
    ] {
        put(&r, path, &format!("prose citing {DEAD}\n"));
    }
    // prose that is not markdown is never read
    put(&r, "notes.txt", &format!("prose citing {DEAD}\n"));
    put(&r, "src/lib.rs", &format!("// prose citing {DEAD}\n"));
    let found = phantom_md_refs(&log(), &r);
    let paths: Vec<&str> = found.iter().map(|(p, _, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [".fapony/plan/PLAN-x.md", "README.md", "docs/README.MD"]
    );
}

#[test]
fn an_unclosed_fence_swallows_the_rest_and_a_token_counts_once() {
    let r = tmp("unclosed");
    put(
        &r,
        "a.md",
        &format!("first {DEAD} here\nand again {DEAD}\n```\nnever closed {DEAD}\n"),
    );
    let found = phantom_md_refs(&log(), &r);
    assert_eq!(found, vec![("a.md".to_string(), 1, DEAD.to_string())]);
}
