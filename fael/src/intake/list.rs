//! `.fael/sources.json` and the JSONL a source's `list` prints. Strict on purpose: exactly five
//! fields per line, so no customer text comes in (decision 01M4V3AD).

use serde::Deserialize;
use std::collections::BTreeMap;

/// One entry of `.fael/sources.json`. `list` feeds the ledger; `read` and `open` are the app's
/// commands, copied into the row so the agent knows where the ticket is. The rest of the
/// app's adapter (`draft`, `claim`) is its own — tolerated, never read.
#[derive(Debug, Deserialize)]
pub(super) struct Source {
    #[serde(skip)]
    pub name: String,
    pub list: String,
    #[serde(default)]
    pub read: Option<String>,
    #[serde(default)]
    pub open: Option<String>,
}

/// One line of `list`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Listed {
    pub r#ref: String,
    pub kind: String,
    pub title: String,
    pub reopen: u32,
    pub closed: Option<String>,
}

/// `.fael/sources.json`, by name.
pub(super) fn sources(json: &str) -> Result<Vec<Source>, String> {
    let m: BTreeMap<String, Source> =
        serde_json::from_str(json).map_err(|e| format!(".fael/sources.json: {e}"))?;
    Ok(m.into_iter()
        .map(|(name, s)| Source { name, ..s })
        .collect())
}

/// Every line of `list`'s stdout, all checked before anything is written. Err names the line.
pub(super) fn listed(out: &str) -> Result<Vec<Listed>, String> {
    let mut v = Vec::new();
    for (i, line) in out.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        v.push(one(line).map_err(|e| format!("list line {}: {e}", i + 1))?);
    }
    Ok(v)
}

fn one(line: &str) -> Result<Listed, String> {
    let serde_json::Value::Object(m) =
        serde_json::from_str(line).map_err(|e| format!("not JSON ({e})"))?
    else {
        return Err("not a JSON object".into());
    };
    const KEYS: [&str; 5] = ["ref", "kind", "title", "reopen", "closed"];
    if let Some(k) = m.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!(
            "field {k:?} is not in the contract ({})",
            KEYS.join(", ")
        ));
    }
    if let Some(k) = KEYS.iter().find(|k| !m.contains_key(**k)) {
        return Err(format!("field {k:?} is missing"));
    }
    let word = |k: &str| match m[k].as_str() {
        Some(s) if !s.is_empty() && !s.contains(char::is_whitespace) => Ok(s.to_string()),
        _ => Err(format!("{k} must be one word")),
    };
    let title: String = match m["title"].as_str().map(str::trim) {
        Some(t) if !t.is_empty() => t.chars().take(100).collect(),
        _ => return Err("title must be a non-empty string".into()),
    };
    let reopen = m["reopen"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("reopen must be an integer >= 0")?;
    let closed = match &m["closed"] {
        serde_json::Value::Null => None,
        serde_json::Value::String(c) if c == "closed" || c.len() > 4 && c.starts_with("dup:") => {
            Some(c.clone())
        }
        _ => return Err("closed must be null, \"closed\" or \"dup:<ref>\"".into()),
    };
    Ok(Listed {
        r#ref: word("ref")?,
        kind: word("kind")?,
        title,
        reopen,
        closed,
    })
}
