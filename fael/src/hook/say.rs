//! The one door to the agent's context (PLAN-fael-say-gate). Every line a hook
//! hands the agent is a `Line` said through an `Outbox`: `policy` decides, in
//! one exhaustive match, whether the line's keys are spent in a seen list and
//! whether it must carry a command; `reply` builds the `Reply`. `context` is
//! private to this module, so no other path can put words in front of the agent.
//!
//! The session's seen list (per session + sub-agent + worktree, gone on
//! compact) holds a row id per row said, `~<id>` / `~*` per edit-hint ask and
//! `@<id>` per in-context mark. The prompt's pointer list is its own file of
//! bare keys. No session = no file: every line is said, nothing remembered.

use super::asks::hook_meta;
use super::protocol::Ctx;
use super::usage::usage_row;
use crate::core;
use serde::Serialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};

/// The usage event `fael-core::stats` reads as "in context at edit".
const IN_CONTEXT: &str = "in-context";

/// Neutral Reply (SPEC §9).
#[derive(Debug, Default, Serialize)]
pub(crate) struct Reply {
    // ponytail: always false since the Stop-block mode went; kept on the wire
    // so an integration that reads `block` keeps parsing
    pub(crate) block: bool,
    /// Only an `Outbox` fills it.
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
    /// One line for the user, never the agent (PLAN-fael-visible-secretary
    /// chunk 4): Claude's `systemMessage`, an OpenCode toast. A client with
    /// no such channel drops it — it must never land in `context`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) notice: Option<String>,
}

impl Reply {
    pub(crate) fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// Two replies as one, in order (a shell call's edit side, then its read
    /// side): the contexts joined, the first notice kept.
    pub(crate) fn and(self, next: Reply) -> Reply {
        let context = match (self.context, next.context) {
            (Some(a), Some(b)) => Some(a + &b),
            (a, b) => a.or(b),
        };
        Reply {
            block: false,
            context,
            notice: self.notice.or(next.notice),
        }
    }
}

/// What a line is, with the evidence fael holds for it.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    /// The rows of a read/edit/search push (the body under `fael mem for …`).
    Row { ids: Vec<String> },
    /// The session-start rows: said once per new or compacted context, never
    /// spent — the next read of a briefed file may push its rows again.
    Brief,
    /// The edit hint: re-check rows already in context (`*` = the generic clause).
    Ask { ids: Vec<String> },
    /// The prompt hint: open keys the prompt names.
    Pointer { keys: Vec<String> },
    /// A push's count lines (`… +N more — fael find …`): each once per file
    /// set and what it counts (this file, a dir, a key), so a new kind of cut
    /// on a later push is still said.
    Count { keys: Vec<String> },
    /// `bodies: fael find <id> …` under a push whose rows have bodies.
    Bodies,
    /// A line fael raises on its own: a stashed risk or capture reject, a
    /// session-start rule or warning.
    Notice,
}

/// How a kind is said only once.
#[derive(Debug, PartialEq)]
pub(crate) enum Once {
    /// Its keys are spent in the seen list; a line whose keys are all spent is dropped.
    Key,
    /// Its event fires once: session start re-briefs only a new or compacted
    /// context, a stashed notice is taken off disk when said.
    Event,
}

#[derive(Debug)]
pub(crate) struct Policy {
    pub(crate) once: Once,
    /// The command the line must offer, found in its text, or it is dropped.
    /// Set here, never by the caller that writes the text.
    pub(crate) command: Option<&'static str>,
}

/// The noise policy, in one place. A new `Kind` does not compile until it
/// has an arm here; `say_contract::every_kind_has_a_fixture` fails until it
/// has a fixture there.
pub(crate) fn policy(k: &Kind) -> Policy {
    let (once, command) = match k {
        Kind::Row { .. } => (Once::Key, None),
        Kind::Ask { .. } => (Once::Key, Some("fael close")),
        Kind::Pointer { .. } => (Once::Key, Some("fael find --key")),
        Kind::Count { .. } => (Once::Key, Some("fael find --")),
        Kind::Bodies => (Once::Key, Some("fael find <id>")),
        Kind::Brief | Kind::Notice => (Once::Event, None),
    };
    Policy { once, command }
}

impl Kind {
    /// The seen-list lines this kind names — spent only under `Once::Key`.
    pub(crate) fn keys(&self) -> Vec<String> {
        match self {
            Kind::Row { ids } => ids.clone(),
            Kind::Ask { ids } => ids.iter().map(|i| format!("~{i}")).collect(),
            Kind::Pointer { keys } => keys.clone(),
            Kind::Count { keys } => keys.iter().map(|k| format!("~count:{k}")).collect(),
            Kind::Bodies => vec!["~bodies".into()],
            Kind::Brief | Kind::Notice => vec![],
        }
    }
}

/// One thing to say. `text` goes into the context verbatim (it carries its
/// own line breaks).
#[derive(Clone)]
pub(crate) struct Line {
    pub(crate) kind: Kind,
    pub(crate) text: String,
}

impl Line {
    /// A line fael raises on its own, with no command required.
    pub(crate) fn notice(text: String) -> Line {
        Line {
            kind: Kind::Notice,
            text,
        }
    }
}

/// The lines of one reply and the seen list they are checked against. The
/// seen file stays locked from `open` to `reply`, so parallel pushes queue up
/// behind each other instead of each saying the same row.
pub(crate) struct Outbox {
    file: Option<File>,
    seen: String,
    /// `seen`'s lines, parsed once.
    lines: HashSet<String>,
    spent: Vec<String>,
    text: String,
}

impl Outbox {
    /// `file`: the locked seen list, `None` = no session (say everything,
    /// remember nothing).
    pub(crate) fn open(mut file: Option<File>) -> Self {
        let mut seen = String::new();
        if let Some(f) = &mut file {
            let _ = f.read_to_string(&mut seen);
        }
        Outbox {
            file,
            lines: seen.lines().map(String::from).collect(),
            seen,
            spent: vec![],
            text: String::new(),
        }
    }

    /// The seen list as it was at `open`.
    pub(crate) fn seen(&self) -> &str {
        &self.seen
    }

    /// True when `line` was in the seen list at `open`.
    pub(crate) fn has(&self, line: &str) -> bool {
        self.lines.contains(line)
    }

    fn is_spent(&self, key: &str) -> bool {
        self.has(key) || self.spent.iter().any(|k| k == key)
    }

    /// The keys `kind` would spend now; `None` when a line of it would be
    /// dropped because every one is already spent.
    fn fresh_keys(&self, kind: &Kind) -> Option<Vec<String>> {
        if policy(kind).once != Once::Key || self.file.is_none() {
            return Some(vec![]);
        }
        let keys = kind.keys();
        let new: Vec<String> = keys.iter().filter(|k| !self.is_spent(k)).cloned().collect();
        (keys.is_empty() || !new.is_empty()).then_some(new)
    }

    /// False when a line of `kind` would be dropped for its spent keys.
    pub(crate) fn fresh(&self, kind: &Kind) -> bool {
        self.fresh_keys(kind).is_some()
    }

    /// False for a line `say` would drop: empty, without the command its kind
    /// needs, or with every key it would spend already spent.
    fn sayable(&self, l: &Line) -> bool {
        let acted = policy(&l.kind).command.is_none_or(|c| l.text.contains(c));
        !l.text.is_empty() && acted && self.fresh(&l.kind)
    }

    /// Add `l`, unless it is empty, lacks the command its kind needs, or every
    /// key it would spend is already spent.
    pub(crate) fn say(&mut self, l: Line) {
        if !self.sayable(&l) {
            return;
        }
        self.spent
            .extend(self.fresh_keys(&l.kind).unwrap_or_default());
        self.text.push_str(&l.text);
    }

    /// Say `lines` in order within `budget` tokens. What is said is charged,
    /// and rows, bodies and counts are never cut (a count is the pointer to
    /// what the budget cut): over the budget the stashed notice goes first,
    /// then the edit hint, each whole. A cut line keeps its keys, so a later
    /// push may say it. True when a notice was said, so the caller takes it
    /// off disk.
    pub(crate) fn say_within(&mut self, budget: usize, lines: Vec<Line>) -> bool {
        let mut keep: Vec<Line> = lines.into_iter().filter(|l| self.sayable(l)).collect();
        let cost = |ls: &[Line]| ls.iter().map(|l| core::est_tokens(&l.text)).sum::<usize>();
        while cost(&keep) > budget {
            let cut = keep
                .iter()
                .position(|l| matches!(l.kind, Kind::Notice))
                .or_else(|| keep.iter().position(|l| matches!(l.kind, Kind::Ask { .. })));
            match cut {
                Some(i) => keep.remove(i),
                None => break,
            };
        }
        let notice = keep.iter().any(|l| matches!(l.kind, Kind::Notice));
        for l in keep {
            self.say(l);
        }
        notice
    }

    /// Spend the keys and build the reply — no context when nothing was said.
    pub(crate) fn reply(mut self) -> Reply {
        if let Some(f) = &mut self.file
            && !self.spent.is_empty()
        {
            let out: String = self.spent.iter().map(|k| format!("{k}\n")).collect();
            let _ = f.write_all(out.as_bytes());
        }
        Reply {
            block: false,
            context: (!self.text.is_empty()).then_some(self.text),
            notice: None,
        }
    }

    /// PLAN-fael-visible-secretary chunk 5: decisions and issues about this
    /// very file (tier 0) already in the agent's context when it edited it.
    /// Its own 0-byte usage line under `in_context`, never `ids` (nothing was
    /// pushed). The seen list also holds rows the agent filed or found itself,
    /// so stats counts only ids an earlier push of the session handed over.
    /// Each id once per session: an `@<id>` line in the seen list marks it.
    pub(crate) fn record_in_context(&mut self, c: &Ctx, tiered: &[(&core::Row, usize)]) {
        let Some(f) = &mut self.file else { return };
        let seen = &self.lines;
        let ids: Vec<&str> = tiered
            .iter()
            .filter(|(r, tier)| {
                *tier == 0
                    && matches!(r.kind.as_str(), "decision" | "issue")
                    && seen.contains(r.id.as_str())
                    && !seen.contains(&format!("@{}", r.id))
            })
            .map(|(r, _)| r.id.as_str())
            .collect();
        if ids.is_empty() {
            return;
        }
        let marks: String = ids.iter().map(|id| format!("@{id}\n")).collect();
        let _ = f.write_all(marks.as_bytes());
        let meta = hook_meta(c, None, false);
        let mut row = usage_row(&c.client, IN_CONTEXT, &c.repo.root, "", &[], &meta);
        row["in_context"] = ids.into();
        super::asks::append_row(row);
    }
}
