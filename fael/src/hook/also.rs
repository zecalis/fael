//! The file list of a pushed row line, minus the file the agent just opened,
//! and the label of a row that came by its directory or key, not the file.

/// Mark a row line by how it reached the push: a tier-1 row is about a
/// neighbouring file, a tier-2 row shares a key with a row about this file.
/// Neither is about the opened file itself, so the agent should not read it
/// as one. Tier 0 (the file or its zone) stays unmarked.
pub(super) fn label(line: &str, tier: usize) -> String {
    let tag = match tier {
        1 => "(same dir) ",
        2 => "(same key) ",
        _ => return line.to_string(),
    };
    match line.strip_prefix("- [").and_then(|r| r.split_once("] ")) {
        Some((id, rest)) => format!("- [{id}] {tag}{rest}"),
        None => line.to_string(),
    }
}

/// A row line names every file its row is about; the agent just opened one of
/// them and knows it. Keep the others as `→ also: …` — a row about nothing else
/// loses the arrow. A row that does not name a touched file (it came by a shared
/// key or the Focus) keeps its list: those files are why it is here.
pub(super) fn drop_touched(line: &str, touched: &[String]) -> String {
    let Some((head, files)) = line
        .strip_prefix("- [")
        .and_then(|_| line.rsplit_once(" → "))
    else {
        return line.to_string();
    };
    let all: Vec<&str> = files.split(", ").collect();
    let rest: Vec<&str> = all
        .iter()
        .copied()
        .filter(|f| !touched.iter().any(|t| t == f))
        .collect();
    match rest.len() {
        n if n == all.len() => line.to_string(),
        0 => head.to_string(),
        n if n > ALSO_FILES => format!(
            "{head} → also: {} +{}",
            rest[..ALSO_FILES].join(", "),
            n - ALSO_FILES
        ),
        _ => format!("{head} → also: {}", rest.join(", ")),
    }
}

/// Other files a pushed row names before the rest fold into `+N` — the full
/// list is one `fael find <id>` away.
const ALSO_FILES: usize = 2;
