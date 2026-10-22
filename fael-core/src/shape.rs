//! Text-shape rules for rows and closes: the one module that reads the form
//! of a text, never its meaning. Add's gate (`add_gate`) and close's gate
//! (`close_gate`) both live here; the write paths call them before appending,
//! `--force` skips them. Stats (`stats/experience.rs`) and warnings
//! (`query/lookup.rs`) reuse the same predicates, so what is taught, warned
//! and rejected cannot drift apart.

use crate::{Config, Log, Row, backtick_paths, code_spans};

/// Chars per clause below which `;`-separated chunks read as a topic list
/// rather than prose sentences: three-plus separators warn only while the
/// row stays short (`chars < seps * CHARS_PER_CLAUSE`). Calibrated against
/// real ledger rows (604–949 chars, 3–5 seps, all single-topic, all silent).
/// Char-based so Thai/CJK (no spaces) judge by the same ruler as English.
const CHARS_PER_CLAUSE: usize = 100;

/// The topic-list check over raw text, shared by rows and closes.
pub(crate) fn topic_list_text(text: &str) -> Option<String> {
    let chars = text.chars().count();
    let mid = text.chars().filter(|&c| c == '·').count();
    let seps = mid + text.chars().filter(|&c| c == ';').count();
    (mid >= 2 || (seps >= 3 && chars < seps * CHARS_PER_CLAUSE)).then(|| {
        format!(
            "text has {seps} topic separators (; / ·) — one topic per row: split it, \
each with --key area:topic, so one can be superseded alone"
        )
    })
}

/// `·`/`;` joining topics. `·` joins topics — two of them is a list of
/// topics. `;` is also plain English clause punctuation inside one topic:
/// three separators read as a topic list only while the clauses stay short —
/// prose clauses run a sentence long, so the check is a density and a long
/// row needs more `;` per char before it stops being prose.
pub(crate) fn topic_list(row: &Row) -> Option<String> {
    topic_list_text(&row.text)
}

/// A long text with no title. Lists show the title, bodies are pulled by id —
/// a long untitled row costs its full text on every push. Thai and CJK have
/// no spaces between words, so chars count too.
pub(crate) fn untitled(row: &Row) -> Option<String> {
    let words = row.text.split_whitespace().count();
    let chars = row.text.chars().count();
    ((words > 60 || chars > 400) && row.title.as_deref().is_none_or(|t| t.trim().is_empty()))
        .then(|| {
            let n = if words > 60 { format!("{words} words") } else { format!("{chars} chars") };
            format!(
                "text is {n} with no title — add --title \"<≤15-word headline>\" so lists stay skimmable"
            )
        })
}

/// The shape faults `add` rejects before writing (unless `--force`): a topic
/// list and a long untitled text — the two the caller fixes in the same call
/// (split it, add --title). A warning after the write came too late: the row
/// was already filed and nobody went back. Length, a missing key and a
/// docs-only list stay warnings. What `old` (the row this one re-files) already
/// had is not new, so it never blocks: `--supersedes <id> --replace` on a
/// fat row must still go through.
pub fn shape_rejects(row: &Row, old: Option<&Row>) -> Vec<String> {
    let mut r = vec![];
    if topic_list(row).is_some() && old.is_none_or(|o| topic_list(o).is_none()) {
        r.extend(topic_list(row));
    }
    if untitled(row).is_some() && old.is_none_or(|o| untitled(o).is_none()) {
        r.extend(untitled(row));
    }
    r
}

/// `shape_rejects` as the add path's reject: nothing is written, and the
/// message names every other fat reason too, so one re-run fixes them all.
/// `old` is the id the row re-files, if any.
pub fn add_gate(row: &Row, log: &Log, cfg: &Config, old: Option<&str>) -> Result<(), String> {
    let old = old.and_then(|s| crate::resolve_row(log, s).ok());
    let mut why = shape_rejects(row, old);
    if why.is_empty() {
        return Ok(());
    }
    for f in crate::fat_reasons(row, cfg) {
        if !why.contains(&f) {
            why.push(f);
        }
    }
    Err(format!(
        "rejected: nothing written — {} (or add --force to file it as is)",
        why.join(" · ")
    ))
}

/// The close text a fixed bug is taught to take — one const for every channel
/// (skill, MCP `close`, fix-net, fix-commit line) so what is taught and what
/// `close_shape` reads cannot drift. The core `<cause> → <fix>` is
/// `close_shape`'s; the branch right after it (a test to `guard`, or what not
/// to do when no test can say it) comes before `tried`, so carry's 160-char
/// clip cuts what failed, never the lesson. It asks for no sha — a squash or
/// a rebase rewrites it (fael:01M4HTZ4): the fix commit cites `(fael:<id>)`,
/// and the close ends on an optional `(#N)`.
pub const CLOSE_TEMPLATE: &str =
    "<cause> → <fix>; guard `<test path>` or don't <X> because <Y>; tried <what failed>[; (#N)]";

/// A close text's label shape (`format:label`, `docs/format.md` § Label).
/// Pure form, never meaning — the text itself is kept as written either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// `cause → fix`: the first `→` or `->` outside a code span, with text on
    /// both sides once code spans are cut — blanks and stray backticks are none.
    pub core: bool,
    /// `guard`: a backticked path holding a `/`.
    pub guard: bool,
}

pub fn close_shape(text: &str) -> Shape {
    let prose: Vec<(bool, &str)> = code_spans(text);
    let prose: String = prose
        .into_iter()
        .map(|(code, t)| if code { " " } else { t }) // a space, so no arrow is glued
        .collect();
    let arrow = [
        prose.find('→').map(|i| (i, '→'.len_utf8())),
        prose.find("->").map(|i| (i, 2)),
    ];
    let said = |s: &str| s.chars().any(|c| !c.is_whitespace() && c != '`');
    let core = arrow
        .into_iter()
        .flatten()
        .min()
        .is_some_and(|(i, n)| said(&prose[..i]) && said(&prose[i + n..]));
    Shape {
        core,
        guard: guard(text),
    }
}

/// A backticked path holding a `/`, read off the whole text, code spans
/// included — the check `closed_with_check` counts and `Shape::guard` reads.
pub(crate) fn guard(text: &str) -> bool {
    backtick_paths(text).iter().any(|p| p.contains('/'))
}

/// `don't <X> because <Y>` — the template's branch when no test can guard the
/// fix. Case-insensitive; both the curly and the straight apostrophe count.
fn dont_because(text: &str) -> bool {
    let lower = text.to_lowercase();
    (lower.contains("don't") || lower.contains("don\u{2019}t") || lower.contains("do not"))
        && lower.contains("because")
}

/// The shape faults a close rejects before writing (unless `--force`): an
/// issue closed with no `<cause> → <fix>`, or with neither a guard nor a
/// don't-because. A bad close is permanent (closing twice is rejected as
/// already-closed), so like `add` it rejects before the write, never warns
/// after it. Notes and decisions owe nothing: a handoff done (`done`), a
/// verdict the code now holds (`now in \`<file>\``) are complete as written.
/// No topic-list check: the template itself joins its clauses with `;`, so
/// the row-calibrated density misfires on short closes. `kind` is the target
/// row's kind.
pub fn close_rejects(text: &str, kind: &str) -> Vec<String> {
    if kind != "issue" {
        return vec![];
    }
    let mut r = vec![];
    let shape = close_shape(text);
    if !shape.core {
        r.push(
            "close names no fix — write <cause> → <fix> so the next reader sees what changed"
                .into(),
        );
    }
    if kind == "issue" && !shape.guard && !dont_because(text) {
        r.push("close names no check — guard `<test path>` or don't <X> because <Y>".into());
    }
    r
}

/// `close_rejects` as the close path's reject: nothing is written. Only
/// issues are gated (a fixed bug owes its evidence); other kinds pass. The
/// `--force` escape mirrors `add_gate`.
pub fn close_gate(text: &str, kind: &str) -> Result<(), String> {
    let why = close_rejects(text, kind);
    if why.is_empty() {
        return Ok(());
    }
    Err(format!(
        "rejected: nothing written — {} (or close with --force to file it as is)",
        why.join(" · ")
    ))
}
