//! What `fael chunk start` prints — the agent's first message (SPEC §8). The first line
//! names R; then the plan's Goal and Done sections, each section the chunk reads, and an
//! index of the rest (no reads: Goal, Scope, Done, Constraints) — a plan with no sections
//! yet reads those from its md by heading. A ref that no longer exists is listed as missing.

use super::export::one;
use super::section::Row;
use super::start::{Prev, Started};
use std::fmt::Write;

/// How a chunk's state moves, said once at the end of every brief and kickoff.
pub fn rules(uid: &str) -> String {
    format!(
        "## chunk rules\n\
         - state moves only through fael, never by editing a plan file:\n  \
         finished → `fael chunk done {uid} \"<handoff>\" --pr N` (or `--out <path>` for a file, image or video): done with a PR, review for a file the owner accepts\n  \
         need the owner → `fael chunk wait {uid} --on owner \"<question>\"` · need data → `--on data \"<what>\" --until YYYY-MM-DD`\n  \
         needs another chunk first → `fael chunk after {uid} <other-uid> \"<why>\"`\n\
         - one chunk = one branch = one PR; never merge, the owner does\n\
         - you triggered Windows CI (`gh workflow run ci.yml --ref <branch>`)? wait for it (`gh run watch <id> --exit-status`) and write pass, or fail with the failing test, in the done text — never just \"triggered\"\n\
         - the done text is the hand-off: first sentence = the outcome (what works or exists now), then what the owner checks; the brief of every chunk after this one prints it\n\
         - a wait --on owner is one question the owner answers in one line: options inline (`A or B?`), first line <= 100 chars, the action asked for — not preconditions that may be stale when it is read\n\
         - after each step that passes: a WIP commit + `fael chunk note {uid} \"<done; next>\"` — the checkpoint a resume reads\n\
         - a section you read that your change makes untrue: update it in this chunk through the bill (`fael plan export <plan> --json`, edit its `sections`, `fael plan apply`) before done\n\
         - a word for a chunk not yet run → `fael chunk note <uid> \"<text>\"` · work left over → `fael chunk add \"<title>\" --plan <plan> --brief \"…\"` · a fact the next session needs → `fael add note … --files <f>`\n"
    )
}

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

/// The md sections a brief carries: `## Goal`, `## 2. Scope`, `## Done criteria`,
/// `## Constraints` (any numbering), each up to the next `## `.
pub fn sections(md: &str) -> String {
    let mut out = String::new();
    let mut keep = false;
    for l in md.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            keep = base(h, false);
        }
        if keep {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
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

impl Started {
    /// `md(source)` = the plan file's text, `exists(path)` = whether a ref is still there.
    pub fn text(
        &self,
        md: &dyn Fn(&str) -> Option<String>,
        exists: &dyn Fn(&str) -> bool,
    ) -> String {
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
            if let Some(repo) = &c.repo {
                let _ = writeln!(o, "code in {repo} — work, scope and PR there");
            }
            let fields: Vec<String> = [
                ("size", &c.size),
                ("client", &c.client_hint),
                ("model", &c.model_hint),
            ]
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
                 `fael close <id> \"<why>\"` (done never closes them)",
                &c.issues,
            );
            quote(&mut o, "owner said", &c.said);
            quote(&mut o, "notes left for this chunk", &c.notes);
            for (who, text) in &c.after {
                quote(
                    &mut o,
                    &format!("{who}, before this, handed off"),
                    std::slice::from_ref(text),
                );
            }
        }
        self.plan_context(&mut o, md, exists);
        o.push('\n');
        o.push_str(&rules(self.chunks.first().map_or("<uid>", |c| &c.uid)));
        o
    }

    /// Plan context once per plan, after the chunks.
    fn plan_context(
        &self,
        o: &mut String,
        md: &dyn Fn(&str) -> Option<String>,
        exists: &dyn Fn(&str) -> bool,
    ) {
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
            let mut shown: Vec<&str> = Vec::new();
            if c.sections.is_empty() {
                // ponytail: md fallback until `fael plan migrate` gives every plan sections
                let text = (!c.source.is_empty()).then(|| md(&c.source)).flatten();
                if let Some(s) = text.map(|t| sections(&t)).filter(|s| !s.is_empty()) {
                    let _ = write!(o, "\n<!-- {} -->\n{s}", c.source);
                }
            } else {
                let _ = writeln!(o, "\n<!-- plan {} -->", c.plan);
                for s in c
                    .sections
                    .iter()
                    .filter(|s| base(&s.title, !reads.is_empty()))
                {
                    let _ = write!(o, "\n{}", one(s));
                    shown.push(&s.key);
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
            if !c.refs.is_empty() {
                let refs: Vec<String> = c
                    .refs
                    .iter()
                    .map(|r| {
                        let path = r.split('#').next().unwrap_or(r);
                        if exists(path) {
                            r.clone()
                        } else {
                            format!("{r} (missing)")
                        }
                    })
                    .collect();
                let _ = writeln!(o, "\nrefs: {}", refs.join(" · "));
            }
        }
    }
}

#[cfg(test)]
#[path = "brief_tests.rs"]
mod tests;
