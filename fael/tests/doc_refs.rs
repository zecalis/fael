//! Docs name code that must still exist: every `fael/…/x.rs` or `fael-core/…/x.rs`
//! path and every `Type::item` / `module::item` (first segment defined in this
//! workspace) cited in a committed Markdown file. A rename that forgets the
//! docs fails here instead of leaving a dead pointer for the next reader.

use std::path::{Path, PathBuf};

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

/// Backticked spans of one Markdown file.
fn spans(md: &str) -> impl Iterator<Item = &str> {
    md.split('`').skip(1).step_by(2)
}

#[test]
fn docs_cite_only_code_that_exists() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut src = vec![];
    for krate in ["fael", "fael-core"] {
        walk(&root.join(krate).join("src"), "rs", &mut src);
    }
    let code: String = src
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap())
        .collect();
    let defined = |name: &str| {
        ["struct", "enum", "trait", "mod"].iter().any(|k| {
            code.contains(&format!("{k} {name} ")) || code.contains(&format!("{k} {name};"))
        }) || src.iter().any(|p| p.file_stem().is_some_and(|s| s == name))
    };
    // ponytail: top-level and docs/ only; .fapony/ plans are local and gitignored
    let mut docs = vec![];
    for dir in [root.to_path_buf(), root.join("docs")] {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            if e.path().extension().is_some_and(|x| x == "md") {
                docs.push(e.path());
            }
        }
    }
    assert!(docs.len() > 3, "found no docs under {}", root.display());
    let mut dead = vec![];
    for doc in &docs {
        let md = std::fs::read_to_string(doc).unwrap();
        for s in spans(&md) {
            let s = s.trim_end_matches("()");
            let path_ref = (s.starts_with("fael/") || s.starts_with("fael-core/"))
                && s.ends_with(".rs")
                && !s.contains('<');
            if path_ref && !root.join(s).is_file() {
                dead.push(format!("{}: `{s}`", doc.display()));
            }
            if let Some((head, item)) = s.split_once("::") {
                let ident =
                    |x: &str| !x.is_empty() && x.chars().all(|c| c.is_alphanumeric() || c == '_');
                if ident(head)
                    && ident(item)
                    && defined(head)
                    && !code.contains(&format!("fn {item}"))
                {
                    dead.push(format!("{}: `{s}`", doc.display()));
                }
            }
        }
    }
    assert!(
        dead.is_empty(),
        "docs cite code that is gone:\n{}",
        dead.join("\n")
    );
}
