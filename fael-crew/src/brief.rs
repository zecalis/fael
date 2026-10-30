//! What `fael chunk start` prints — the agent's first message (SPEC §8). The first line
//! names R; then the plan's Goal and Done sections, each section the chunk reads, and an
//! index of the rest (no reads: Goal, Scope, Done, Constraints).

use super::export::one;
use super::section::Row;
use super::start::{Brief, Prev, Started};
use std::fmt::Write;

/// How a chunk's state moves — one copy, in fael-core (kickoff prints it too).
pub use fael_core::work::{rules, rules_full};

/// A heading a brief carries by default: Goal, Scope, Done, Constraints (any numbering);
/// `few` = Goal and Done only, for a chunk that names what else it reads.
fn base(heading: &str, few: bool) -> bool {
    let h = heading.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ');
    let words: &[&str] = if few {
        &["Goal", "Done"]
    } else {
        &["Goal", "Scope", "Done", "Constraints"]
    };
    words.iter().any(|w| h.starts_with(w))
}

/// Where the run this start took over stopped. git owns what changed: its commands, never
/// their output.
fn resume(o: &mut String, p: &Prev) {
    let _ = writeln!(
        o,
        "\nresume — run {} ({}) started {}, last seen {}",
        p.start,
        p.client.as_deref().unwrap_or("agent"),
        p.started,
        p.last_seen.as_deref().unwrap_or("never")
    );
    if let Some(s) = &p.session {
        let _ = writeln!(o, "session {s}");
    }
    if let Some(b) = &p.branch {
        let wt = p.worktree.as_deref().unwrap_or("?");
        let _ = writeln!(
            o,
            "branch {b} in {wt} — what it did: `git log origin/main..{b}` · `git status`"
        );
    }
}

/// By-file hand-offs printed in full; the rest get one line.
const FULL: usize = 2;

/// Each chunk's hand-off (its `left:` line is part of it) and the files its done claimed.
fn handoffs(o: &mut String, how: &str, all: &[(String, String, String)], none: bool) {
    for (who, text, scope) in all {
        let _ = writeln!(o, "\n{who}, {how}:");
        let _ = writeln!(o, "> {}", text.replace('\n', "\n> "));
        let files: Vec<&str> = scope.lines().filter(|l| !l.is_empty()).collect();
        if !files.is_empty() {
            let _ = writeln!(o, "files it touched: {}", files.join(", "));
        } else if none {
            // issue 01M5QF7Q: the paths in this brief are the planner's, not the code's
            let _ = writeln!(o, "files it touched: none claimed");
        }
    }
}

/// Live touches (`chunk touch`, issue 01M5PB7Z): running chunks, newest run first —
/// no hand-off text yet, so a section of its own, never merged with the done-claimed
/// one above. Only what the agent reported is shown.
fn live(o: &mut String, all: &[(String, String)], more: usize) {
    let (full, rest) = all.split_at(all.len().min(FULL));
    for (who, files) in full {
        let _ = writeln!(o, "\n{who}, live touching files in this scope:");
        let f: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();
        if !f.is_empty() {
            let _ = writeln!(o, "files it is touching now: {}", f.join(", "));
        }
    }
    for (who, files) in rest {
        let f: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();
        let tail = if f.len() > 5 { ", …" } else { "" };
        let _ = writeln!(
            o,
            "- {who} (live) — {}{tail}",
            f[..f.len().min(5)].join(", ")
        );
    }
    if more > 0 {
        let _ = writeln!(o, "({more} more live chunks not shown)");
    }
}

/// The chunk rules whole, or one pointer line when the session already read them
/// (issue 01M5Y6H4).
fn rules_text(o: &mut String, uid: &str, first: Option<&Brief>, whole: bool, full: bool) {
    o.push('\n');
    let model = first.and_then(|c| c.model.as_deref());
    if full {
        // `--full`: the core plus the situational rules the verbs otherwise say
        o.push_str(&rules_full(uid, model));
    } else if whole {
        o.push_str(&rules(uid, model));
    } else {
        let _ = writeln!(
            o,
            "## chunk rules — printed at this session's first start; `fael chunk start {uid} --full` prints them again\n\
             - finish: `fael chunk done {uid} \"<result>\" --left none|<ids>`; one chunk = one branch = one PR, never merge"
        );
    }
}

impl Started {
    /// The whole brief (`chunk start --full`): plan sections and the full chunk rules.
    pub fn text(&self) -> String {
        self.render(true, true)
    }

    /// `full` = plan sections §1–4 and the index of the rest; `rules_full` = the chunk rules
    /// whole, else one pointer line (the session already read them). Lean puts the
    /// hand-off of the chunk(s) this one is after before the TLDR.
    pub fn render(&self, full: bool, rules_full: bool) -> String {
        let mut o = String::new();
        let n = self.chunks.len();
        let what = if n == 1 {
            "1 chunk".to_string()
        } else {
            format!("{n} paired chunks")
        };
        let _ = writeln!(o, "fael run {} · {what}", self.run);
        for c in &self.chunks {
            // an imported title already starts with its label (`c1 — …`)
            let label = c
                .label
                .as_deref()
                .filter(|l| !c.title.strip_prefix(l).is_some_and(|r| r.starts_with(" ")))
                .map(|l| format!("{l} — "))
                .unwrap_or_default();
            let _ = writeln!(
                o,
                "\n# {label}{} ({})\nplan {} — {}",
                c.title, c.uid, c.plan, c.plan_title
            );
            let fields: Vec<String> = [("size", &c.size), ("model", &c.model)]
                .iter()
                .filter_map(|(k, v)| v.as_ref().map(|v| format!("{k} {v}")))
                .chain(
                    c.scope
                        .as_ref()
                        .map(|s| format!("scope {}", s.replace('\n', ", "))),
                )
                .collect();
            if !fields.is_empty() {
                let _ = writeln!(o, "{}", fields.join(" · "));
            }
            if !full {
                handoffs(&mut o, "before this, handed off", &c.after, true);
            }
            // zero-knowledge first line: what to do, then the exact close verb
            let task = if c.brief == c.title || c.brief.trim().is_empty() {
                c.title.clone()
            } else {
                c.brief
                    .lines()
                    .next()
                    .unwrap_or(&c.title)
                    .trim()
                    .to_string()
            };
            let task = match task.char_indices().nth(100) {
                Some((i, _)) => format!("{}…", &task[..i]),
                None => task,
            };
            let _ = writeln!(
                o,
                "TLDR: do \"{task}\" then `fael chunk done {} \"<handoff>\" --pr N`",
                c.uid
            );
            if let Some(p) = &c.resume {
                resume(&mut o, p);
            }
            if c.brief != c.title {
                let _ = writeln!(o, "\n{}", c.brief.trim_end());
            }
            let quote = |o: &mut String, head: &str, all: &[String]| {
                if !all.is_empty() {
                    let _ = writeln!(o, "\n{head}:");
                    for s in all {
                        let _ = writeln!(o, "> {}", s.replace('\n', "\n> "));
                    }
                }
            };
            quote(
                &mut o,
                "ledger issues this chunk fixes — once the fix holds, close each with \
                 `fael close <id> \"<cause> → <fix>; guard \\`<test path>\\`\"` (done never closes them)",
                &c.issues,
            );
            quote(&mut o, "owner said", &c.said);
            quote(&mut o, "notes left for this chunk", &c.notes);
            if full {
                handoffs(&mut o, "before this, handed off", &c.after, true);
            }
            // one line per chunk (issue 01M5SWWY): the hand-off is one `chunk show` away
            for (who, _, _) in &c.by_file {
                let (uid, title) = who.split_once(' ').unwrap_or((who, ""));
                let _ = writeln!(o, "- {uid} {title} — fael chunk show {uid}");
            }
            if c.by_file_more > 0 {
                let _ = writeln!(o, "({} more such chunks not shown)", c.by_file_more);
            }
            // live touches (`chunk touch`, issue 01M5PB7Z): a section of its own, never
            // merged with the done-claimed one above.
            live(&mut o, &c.live, c.live_more);
        }
        let first = self.chunks.first();
        let uid = first.map_or("<uid>", |c| &c.uid);
        if full {
            self.plan_context(&mut o);
        } else {
            let _ = writeln!(
                o,
                "\nplan sections (goal, scope, done, constraints): `fael chunk start {uid} --full`"
            );
        }
        rules_text(&mut o, uid, first, rules_full, full);
        o
    }

    /// Same-session retry (`reuse_live`): the live R again, short — resume + TLDR
    /// only. Full returns with `--full`. Short is deterministic rendering, never a
    /// rank: sections, answers, notes and hand-offs stay in the first brief.
    pub fn render_short(&self) -> String {
        let mut o = String::new();
        let n = self.chunks.len();
        let what = if n == 1 {
            "1 chunk".to_string()
        } else {
            format!("{n} paired chunks")
        };
        let _ = writeln!(o, "fael run {} · {what} (resumed)", self.run);
        for c in &self.chunks {
            let label = c
                .label
                .as_deref()
                .filter(|l| !c.title.strip_prefix(l).is_some_and(|r| r.starts_with(" ")))
                .map(|l| format!("{l} — "))
                .unwrap_or_default();
            let _ = writeln!(o, "\n# {label}{} ({})", c.title, c.uid);
            let task = if c.brief == c.title || c.brief.trim().is_empty() {
                c.title.clone()
            } else {
                c.brief
                    .lines()
                    .next()
                    .unwrap_or(&c.title)
                    .trim()
                    .to_string()
            };
            let task = match task.char_indices().nth(100) {
                Some((i, _)) => format!("{}…", &task[..i]),
                None => task,
            };
            let _ = writeln!(
                o,
                "TLDR: do \"{task}\" then `fael chunk done {} \"<handoff>\" --pr N`",
                c.uid
            );
            if let Some(p) = &c.resume {
                resume(&mut o, p);
            }
        }
        let uid = self.chunks.first().map_or("<uid>", |c| &c.uid);
        let _ = writeln!(
            o,
            "\nresumed {0} — full brief: `fael chunk start {uid} --full`",
            self.run
        );
        o
    }

    /// Plan context once per plan, after the chunks.
    fn plan_context(&self, o: &mut String) {
        let mut seen = Vec::new();
        for c in &self.chunks {
            if seen.contains(&&c.plan) {
                continue;
            }
            seen.push(&c.plan);
            // what every chunk of this plan in the start reads, once each
            let mut reads: Vec<&(String, Option<Row>)> = Vec::new();
            for r in self
                .chunks
                .iter()
                .filter(|d| d.plan == c.plan)
                .flat_map(|d| &d.reads)
            {
                if !reads.iter().any(|x| x.0 == r.0) {
                    reads.push(r);
                }
            }
            // a section still as `plan new` left it says nothing: neither printed nor indexed
            let mut shown: Vec<&str> = c
                .sections
                .iter()
                .filter(|s| super::section::blank(s))
                .map(|s| s.key.as_str())
                .collect();
            let heads: Vec<&Row> = c
                .sections
                .iter()
                .filter(|s| !shown.contains(&s.key.as_str()))
                .filter(|s| base(&s.title, !reads.is_empty()))
                .collect();
            // a blank Goal/Done names its fill command instead of printing nothing
            let missing: Vec<&Row> = c
                .sections
                .iter()
                .filter(|s| shown.contains(&s.key.as_str()))
                .filter(|s| base(&s.title, true))
                .collect();
            if !missing.is_empty() || !heads.is_empty() {
                let _ = writeln!(o, "\n<!-- plan {} -->", c.plan);
                for s in heads {
                    let _ = write!(o, "\n{}", one(s));
                    shown.push(&s.key);
                }
                for s in missing {
                    let _ = writeln!(
                        o,
                        "\nsection {} ({}) is still blank — fill it: `fael plan export {} --json`, edit `sections`, `fael plan apply`",
                        s.key, s.title, c.plan
                    );
                }
            }
            for (r, s) in &reads {
                match s {
                    Some(s) if shown.contains(&s.key.as_str()) => {}
                    Some(s) => {
                        let _ = write!(o, "\n{}", one(s));
                        shown.push(&s.key);
                    }
                    None => {
                        let _ = writeln!(o, "\nsection {r} not found");
                    }
                }
            }
            let rest: Vec<&Row> = c
                .sections
                .iter()
                .filter(|s| !shown.contains(&s.key.as_str()))
                .collect();
            if !rest.is_empty() {
                let _ = writeln!(
                    o,
                    "\nother sections of {0} — read one: `fael plan export {0} --section <key>`",
                    c.plan
                );
                for s in rest {
                    let day = s.updated.get(..10).unwrap_or(&s.updated);
                    let _ = writeln!(o, "- {} · {} · {day}", s.key, s.title);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "brief_tests.rs"]
mod tests;
