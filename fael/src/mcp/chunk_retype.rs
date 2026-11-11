//! CLI retype → MCP keys (issue mcp-left): `--left` → `left`, `--pr` → `pr`,
//! `--because` → `because`, `--force` → `force`. One translation serves every
//! reject that prints `--force --because` (`start.rs`: `skipped`,
//! `held_takeover`, `need_because`, `check_lonely_because`, `seat_taken`) plus
//! the missing-`--left` retype. Appended as an `MCP:` line — the CLI text stays,
//! so forwarding rejects (`--left X is no open row`, `--pr takes …`) still read.

/// Apply every MCP retype hint that fires.
pub(crate) fn retype(mut e: String) -> String {
    if let Some(hint) = left_hint(&e) {
        e.push('\n');
        e.push_str(&hint);
    }
    if let Some(hint) = force_hint(&e) {
        e.push('\n');
        e.push_str(&hint);
    }
    e
}

/// Missing `--left`: a line with `left: "none"` and `left: "<id|uid>"`, no `--left`.
fn left_hint(e: &str) -> Option<String> {
    if !e.contains("chunk done needs --left") {
        return None;
    }
    match done_uid(e) {
        Some(uid) => Some(format!(
            "MCP: re-run chunk {{verb: \"done\", uid: \"{uid}\"}} with left: \"none\" or left: \"<id|uid>\""
        )),
        None => Some("MCP: re-run this call with left: \"none\" or left: \"<id|uid>\"".to_string()),
    }
}

/// Any `--force --because` start retype: `force: true, because: "…"` in MCP form,
/// with the real uid — never a placeholder.
fn force_hint(e: &str) -> Option<String> {
    if !(e.contains("fael chunk start ") && (e.contains("--force") || e.contains("--because"))) {
        return None;
    }
    let uid = start_uid(e)?;
    let because = because_of(e).unwrap_or_else(|| "<what you will avoid>".to_string());
    Some(format!(
        "MCP: re-run chunk {{verb: \"start\", uid: \"{uid}\", force: true, because: \"{because}\"}}"
    ))
}

/// The uid after the first `fael chunk done ` — up to whitespace, backtick, quote or newline.
fn done_uid(e: &str) -> Option<String> {
    let rest = e.split("fael chunk done ").nth(1)?;
    let uid = rest
        .split(|c: char| c.is_whitespace() || c == '`' || c == '"' || c == '\'')
        .next()?
        .trim();
    (!uid.is_empty()).then(|| uid.to_string())
}

/// The uid of the `fael chunk start` retype that carries `--force` when several are named
/// (`check_lonely_because` names the bare call too); else the first one.
fn start_uid(e: &str) -> Option<String> {
    let mut fallback: Option<String> = None;
    for part in e.split("fael chunk start ").skip(1) {
        let uid = part
            .split(|c: char| c.is_whitespace() || c == '`' || c == '"' || c == '\'')
            .next()
            .unwrap_or("")
            .trim();
        if uid.is_empty() {
            continue;
        }
        // the retype window ends at the closing backtick — `--force` inside it only
        let window = part.split('`').next().unwrap_or("");
        if window.contains("--force") {
            return Some(uid.to_string());
        }
        fallback.get_or_insert_with(|| uid.to_string());
    }
    fallback
}

/// The quoted value after the first `--because "` — the placeholder the CLI names.
fn because_of(e: &str) -> Option<String> {
    let rest = e.split("--because \"").nth(1)?;
    rest.split('"').next().map(String::from)
}
