//! Docs name code that must still exist: every `fael/…/x.rs`, `fael-core/…/x.rs` or `fael-crew/…/x.rs`
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
    for krate in ["fael", "fael-core", "fael-crew"] {
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
    // ponytail: top-level and docs/ only; plans live in crew.db, not in files
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
            let path_ref = (s.starts_with("fael/")
                || s.starts_with("fael-core/")
                || s.starts_with("fael-crew/"))
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

/// Docs cite fields, not figures (plan proof chunk `provenance`): a number
/// followed by `%` or `×` in README.md or docs/ must sit in a block that
/// names a `fael stats` field, cites an external page (a link in the same
/// block), or is marked `one-off measurement, <date>`.
#[test]
fn docs_numbers_cite_a_field_or_a_dated_one_off() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = vec![root.join("README.md")];
    for e in std::fs::read_dir(root.join("docs")).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "md") {
            files.push(e.path());
        }
    }
    let mut bad = vec![];
    for doc in &files {
        let md = std::fs::read_to_string(doc).unwrap();
        // fenced code is an example, never a claim
        let mut prose = String::new();
        for (i, part) in md.split("```").enumerate() {
            if i % 2 == 0 {
                prose.push_str(part);
                prose.push('\n');
            }
        }
        for block in prose.split("\n\n") {
            if block.trim().is_empty() {
                continue;
            }
            // inline code is a name, never a claim — scan the rest
            let mut clean = String::new();
            for (i, part) in block.split('`').enumerate() {
                if i % 2 == 0 {
                    clean.push_str(part);
                }
            }
            // a block that names the numbers' source, links it, or dates it
            if block.contains("fael stats")
                || block.contains("outcome.")
                || block.contains("experience.")
                || block.contains("context_loop.")
                || block.contains("provenance")
                || block.contains("https://")
                || block.contains("http://")
                || dated_one_off(block)
            {
                continue;
            }
            for hit in flagged_numbers(&clean) {
                bad.push(format!(
                    "{}: `{hit}` in {:?}",
                    doc.display(),
                    block.lines().next().unwrap_or("").trim()
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "docs state numbers with no field behind them:\n{}",
        bad.join("\n")
    );
}

/// `one-off measurement, <YYYY-MM-DD>` in the block.
fn dated_one_off(block: &str) -> bool {
    let mark = "one-off measurement,";
    let Some(at) = block.find(mark) else {
        return false;
    };
    block[at + mark.len()..]
        .chars()
        .filter(|c| !c.is_whitespace())
        .take(10)
        .collect::<String>()
        .chars()
        .enumerate()
        .all(|(i, c)| match i {
            4 | 7 => c == '-',
            _ => c.is_ascii_digit(),
        })
}

/// Number snippets followed by `%` or `×`: `18.5%`, `±20%`, `2.4×`.
fn flagged_numbers(text: &str) -> Vec<String> {
    let mut out = vec![];
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '%' || chars[j] == '×') {
                out.push(chars[start..=j].iter().collect());
                i = j + 1;
            }
        } else {
            i += 1;
        }
    }
    out
}
