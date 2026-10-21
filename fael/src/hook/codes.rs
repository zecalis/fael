//! The closed vocabulary usage.jsonl stores for what went wrong: a reject's
//! `reason`, a warning's `code`, a shape gate's `rules`. Read off the message
//! at the one write choke point, so no reject site carries its own code and
//! the free text itself is never stored. First match wins; a message no
//! needle knows is `other` — `fael stats` shows it, and the test below fails
//! on any `rejected:`/`warning:` literal in the source that reads as `other`,
//! so a new message gets its code in the same change.

use crate::core;

/// Reject needles. A message names values (ids, keys, flags), so the
/// specific families come before the generic ones they could also match.
const REJECTS: &[(&str, &str)] = &[
    // the gate quotes other faults in its reasons: first, before any of them
    ("nothing written", core::stats::SHAPE_GATE),
    ("unknown command", "unknown_command"),
    ("unknown tool", "unknown_command"),
    ("unknown chunk verb", "unknown_command"),
    ("unknown flag", "unknown_flag"),
    (" takes no -", "flag_not_taken"),
    ("is one argument", "flag_joined"),
    ("open rows on", "row_state"),
    ("pick one", "pick_one"),
    ("not both", "pick_one"),
    ("takes at most one", "pick_one"),
    ("a text was given too", "pick_one"),
    ("looks like a secret", "secret"),
    ("bytes, limit is", "too_big"),
    ("MiB", "too_big"),
    ("--replace", "amend"),
    ("in the body of", "amend"),
    ("is a path, not an id", "id_is_path"),
    ("is not an id", "id_not_shaped"),
    ("ids not shown", "id_partial"),
    ("no row with id", "id_missing"),
    (" rows (", "id_ambiguous"),
    ("is a ledger", "not_a_chunk"),
    ("lists its roots", "no_root"),
    ("no chunk", "chunk_missing"),
    ("is not on the board", "chunk_missing"),
    ("lives in", "chunk_missing"),
    ("uid prefix", "chunk_prefix"),
    ("more than one chunk", "chunk_ambiguous"),
    ("is in another plan", "chunk_other_plan"),
    ("is in plan", "chunk_other_plan"),
    ("no section", "no_section"),
    (".fael/sources.json", "no_source"),
    ("no plan", "no_plan"),
    ("plans — use", "no_plan"),
    ("no crew.db yet", "no_plan"),
    ("names one plan", "usage"),
    ("try 'fael chunk --help'", "usage"),
    ("try 'fael plan --help'", "usage"),
    ("wrong arguments", "usage"),
    ("pinned to one workspace", "usage"),
    ("takes one row", "usage"),
    ("lists a plan's chunks", "usage"),
    ("rows rejected", "partial"),
    ("closes rejected", "partial"),
    ("chunks rejected", "partial"),
    ("already closed", "row_state"),
    ("superseded", "row_state"),
    ("held @", "row_state"),
    ("no ready issue", "row_state"),
    ("no open row", "row_state"),
    ("is a close event", "row_state"),
    ("supersedes nothing", "row_state"),
    ("still point at", "row_state"),
    ("lives in immutable", "row_state"),
    ("already itself", "row_state"),
    ("not an issue", "row_state"),
    ("takes an open issue", "row_state"),
    ("needs an urgent row", "row_state"),
    ("queue holds issues", "row_state"),
    ("is for issues", "row_state"),
    ("is taken by chunk", "label_taken"),
    ("on no branch", "no_branch"),
    ("was already used", "chunk_state"),
    ("cannot become", "chunk_state"),
    ("cannot wait on itself", "chunk_state"),
    ("closes a loop", "chunk_state"),
    ("running chunk", "chunk_state"),
    ("row carries no", "invalid_row"),
    ("row closes", "invalid_row"),
    ("alias", "invalid_row"),
    ("needs a number", "bad_format"),
    ("plan name", "bad_format"),
    ("section key", "bad_format"),
    (" needs ", "missing_value"),
    ("is required", "missing_value"),
    ("is empty", "missing_value"),
    ("nothing to edit", "missing_value"),
    ("rejected: chunk ", "chunk_state"),
    ("rejected: plan ", "plan_state"),
    ("stdin", "bad_input"),
    ("JSON", "bad_input"),
    ("bill", "bad_input"),
    ("stale rev", "bad_input"),
    ("nothing to apply", "bad_input"),
    ("nothing to add", "bad_input"),
    ("twice", "bad_input"),
    ("takes at least", "bad_input"),
    ("split makes", "bad_input"),
    ("merge joins", "bad_input"),
    ("an op touches", "bad_input"),
    ("drop:true", "bad_input"),
    ("repo-relative", "bad_path"),
    ("outside the repo", "bad_path"),
    ("matches nothing", "bad_path"),
    ("is not a file or directory", "bad_path"),
    ("no .jsonl", "bad_path"),
    ("does not exist", "bad_path"),
    ("rejected: scope ", "bad_path"),
    ("has no ref", "bad_format"),
    ("— use lowercase", "bad_format"),
    ("kind must be", "bad_format"),
    ("rejected: area ", "bad_format"),
    ("rejected: from ", "bad_format"),
    ("rejected: reads ", "bad_format"),
    ("finite number", "bad_format"),
    ("RFC 3339", "bad_format"),
    ("folder name", "bad_format"),
    ("is gone", "bad_format"),
    ("ref path", "bad_format"),
    ("repo_id", "bad_format"),
    ("format_version", "bad_format"),
    ("is a percent", "bad_format"),
    ("— want", "bad_format"),
    ("shows nothing", "bad_format"),
    ("is not this repo's", "bad_format"),
    ("takes a PR number", "bad_format"),
    ("rejected: --", "bad_format"),
];

/// Warning needles — also the gate's rules: the gate names the same faults.
const WARNINGS: &[(&str, &str)] = &[
    ("with no title", "untitled"),
    ("topic separators", "topic_list"),
    ("files derived", "derived_files"),
    ("no --key", "no_key"),
    ("WHEN:", "when_no_revisit"),
    ("*.md docs", "docs_only_files"),
    ("(warn at", "long_text"),
    ("title is", "long_title"),
    ("key domain", "key_domain"),
    ("similar keys", "similar_key"),
    ("matches nothing on disk", "files_missing"),
    ("is not under", "path_resolved"),
    ("the user's call", "supersedes_user"),
    ("was held @", "claim_taken"),
    ("warning: scope", "chunk_scope"),
    ("fael.remote is set", "purge_remote"),
    ("warning: brief names", "chunk_brief_path"),
    ("warning: brief cites", "chunk_closed_ref"),
    ("warning: commit ", "unmerged"),
    // self-heal wraps another warning; that one's needle wins above
    ("— key ", "key_moved"),
];

/// Only the first line: what follows (a usage synopsis, a list to pick
/// from) quotes flags and words of other families.
fn code(table: &[(&str, &'static str)], msg: &str) -> &'static str {
    let msg = msg.lines().next().unwrap_or("");
    table
        .iter()
        .find(|(n, _)| msg.contains(n))
        .map_or("other", |(_, c)| c)
}

pub(crate) fn reject(msg: &str) -> &'static str {
    code(REJECTS, msg)
}

pub(crate) fn warning(msg: &str) -> &'static str {
    code(WARNINGS, msg)
}

/// The faults a shape gate named (`nothing written — a · b (or add --force …)`),
/// each as its warning code — empty for the batch summary, which names none.
pub(crate) fn gate_rules(msg: &str) -> Vec<&'static str> {
    let Some((_, why)) = msg.split_once("nothing written — ") else {
        return vec![];
    };
    let why = why.split(" (or add --force").next().unwrap_or(why);
    let mut r: Vec<&str> = why.split(" · ").map(warning).collect();
    r.dedup();
    r
}

#[cfg(test)]
mod tests;
