//! The one door to the agent's context (PLAN-fael-say-gate). Every line a hook
//! hands the agent is a `Line` said through an `Outbox` opened for one hook
//! event (`At`): the kind's card (`say_card::policy`, one exhaustive match)
//! decides its stats name, whether its keys are spent in a seen list, the
//! command it must carry, the events it may be said at and whether a push over
//! budget cuts it; `reply` builds the `Reply`. `context` is private to this
//! module, so no other path can put words in front of the agent.
//!
//! The session's seen list (per session + sub-agent + worktree, gone on
//! compact) holds a row id per row said, `~<id>` per edit-hint ask (`~*`, the old generic clause, in older lists) and
//! `@<id>` per in-context mark and `^<id>` per cited id (`cited`), `~start:<id>` per row session start said.
//! No session = no file: every line is said, nothing remembered.
//!
//! Crash boundary (c3): a push counts as said when its reply persists the
//! spent keys. A process restart inside the same session reopens the same
//! seen file, so an already-pushed row stays silent. A crash before the
//! reply persists nothing and the next push says the rows again —
//! at-most-once per completed push, at-least-once across crashes.
//!
//! Lines cut for low yield have no `Kind` any more (the `fix:` commit line and
//! the `bodies:` pointer; the edit ask is said at an edit only) — their
//! `SayKind` names stay in `fael-core` so old usage rows still tally
//! (decision `product:push-noise-bar`).

use super::say_card::{At, Once, next_cut, policy};
use crate::core::{self, SayKind};
use serde::Serialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Write};

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
    /// What `context` holds, line by line — usage only, never on the wire.
    #[serde(skip)]
    said: Vec<Said>,
}

impl Reply {
    pub(crate) fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// The usage `said` list (PLAN-fael-say-gate chunk 3): one entry per key
    /// of each line said, read back by `fael-core` stats as yield per kind.
    pub(crate) fn said(&self) -> &[Said] {
        &self.said
    }

    /// Two replies as one, in order (a shell call's edit side, then its
    /// commit side): the contexts joined, the first notice kept.
    pub(crate) fn and(self, next: Reply) -> Reply {
        let context = match (self.context, next.context) {
            (Some(a), Some(b)) => Some(a + &b),
            (a, b) => a.or(b),
        };
        let mut said = self.said;
        said.extend(next.said);
        Reply {
            block: false,
            context,
            notice: self.notice.or(next.notice),
            said,
        }
    }
}

/// What a line is, with the evidence fael holds for it.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    /// The rows of a read/edit/search push (the body under `fael mem for …`).
    Row { ids: Vec<String> },
    /// The session-start hand-offs and assigned rows: said once per new,
    /// compacted or cleared context. Its keys are `~start:<id>`, never the row
    /// id — the next read of a briefed file may push its rows again.
    Brief { keys: Vec<String> },
    /// Session start's pointer to the chunk run this worktree holds: split
    /// from `Brief` so row rates never carry a line that names no row.
    ChunkBrief { uid: String },
    /// The edit hint: re-check open issues already in context, each named.
    /// It names only open issues on the edited
    /// file (a vela edit's issue went unasked behind another ask in the turn).
    Ask { ids: Vec<String> },
    /// A `git commit` that named open issues (PLAN-fael-agent-ergonomics
    /// chunk 5): each named once per session with its ready `fael close`.
    Cited { ids: Vec<String> },
    /// A review finding (PLAN-fael-experience-loop chunk 2): the ready
    /// `fael add issue` for a ReportFindings entry, once per file and line.
    Finding { file: String, line: i64 },
    /// The agent said it fixed a bug and filed nothing after (PLAN-fael-
    /// experience-loop chunk 5a): stashed by stop, said once per session.
    Fixed,
    /// An edit outside the held chunk's scope, inside another open chunk's
    /// (issues 01M5D9GB, 01M5QF1Q): said once per owner chunk and session.
    Overlap { owner: String },
    /// The held chunk's brief, scope or title was edited after its run started; the key
    /// names the edit event, so a later edit says again.
    BriefDrift { key: String },
    /// A step commit in a worktree holding a chunk: ask for the checkpoint
    /// note a resume reads. The key
    /// names the last note's event id, so it is asked once per gap between notes.
    Checkpoint { key: String },
    /// A note said in its last session (`spent_notes`): file it as a decision, or let it go.
    Retired { id: String },
    /// A line fael raises on its own: a stashed risk or capture reject, a
    /// session-start rule or warning.
    Notice,
}

/// The seen-list line of the session's one fix line — stop reads it too, so
/// a session already told stashes no second one.
pub(crate) const FIXED_KEY: &str = "~fixed";

impl Kind {
    /// The seen-list lines this kind names — spent only under `Once::Key`.
    pub(crate) fn keys(&self) -> Vec<String> {
        match self {
            Kind::Row { ids } => ids.clone(),
            Kind::Ask { ids, .. } => ids.iter().map(|i| format!("~{i}")).collect(),
            Kind::Cited { ids } => ids.iter().map(|i| format!("~cited:{i}")).collect(),
            Kind::Finding { file, line } => vec![format!("~finding:{file}:{line}")],
            Kind::Fixed => vec![FIXED_KEY.into()],
            Kind::Overlap { owner } => vec![format!("~overlap:{owner}")],
            Kind::Checkpoint { key } | Kind::BriefDrift { key } => vec![key.clone()],
            Kind::Retired { id } => vec![format!("~retired:{id}")],
            Kind::Brief { keys } => keys.clone(),
            Kind::ChunkBrief { uid } => vec![format!("~start:chunk:{uid}")],
            Kind::Notice => vec![],
        }
    }
}

/// One `said` entry: the kind's name and the id, key or count key it names.
/// `Brief` names none (stats reads the line's `ids`), nor does `Notice`. `t` is the entry's share of its line's `est_tokens`: the entries
/// of one line sum to exactly that line's tokens. Usage lines from before it
/// existed have none, and stats counts them as 0.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Said {
    kind: SayKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    #[serde(rename = "t", skip_serializing_if = "is_zero")]
    tokens: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Said {
    /// The issue id of a said edit ask, `None` for any other line.
    pub(crate) fn ask_id(&self) -> Option<&str> {
        self.key.as_deref().filter(|_| self.kind == SayKind::Ask)
    }
}

impl Kind {
    /// What a `said` entry of this kind names, one entry per key.
    fn said_keys(&self) -> Vec<String> {
        match self {
            Kind::Row { ids } | Kind::Ask { ids } | Kind::Cited { ids } => ids.clone(),
            Kind::Finding { file, .. } => vec![file.clone()],
            Kind::Overlap { owner } => vec![owner.clone()],
            Kind::Checkpoint { key } | Kind::BriefDrift { key } => vec![key.clone()],
            Kind::Retired { id } => vec![id.clone()],
            Kind::ChunkBrief { uid } => vec![uid.clone()],
            Kind::Brief { .. } | Kind::Fixed | Kind::Notice => vec![],
        }
    }

    /// The entries of a line costing `tokens`, split evenly; the remainder
    /// rides on the first so the entries sum to the line's tokens.
    fn entries(&self, tokens: usize) -> Vec<Said> {
        let kind = policy(self).said;
        let keys: Vec<Option<String>> = match self.said_keys() {
            keys if keys.is_empty() => vec![None],
            keys => keys.into_iter().map(Some).collect(),
        };
        let n = keys.len();
        keys.into_iter()
            .enumerate()
            .map(|(i, key)| Said {
                kind,
                key,
                tokens: tokens / n + if i == 0 { tokens % n } else { 0 },
            })
            .collect()
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
    /// The hook event this outbox speaks for: a kind not allowed there is dropped.
    at: At,
    pub(super) file: Option<File>,
    seen: String,
    /// `seen`'s lines, parsed once.
    pub(super) lines: HashSet<String>,
    spent: Vec<String>,
    text: String,
    said: Vec<Said>,
}

impl Outbox {
    /// `file`: the locked seen list, `None` = no session (say everything,
    /// remember nothing). `at`: the hook event the lines are said at.
    pub(crate) fn open(at: At, mut file: Option<File>) -> Self {
        let mut seen = String::new();
        if let Some(f) = &mut file {
            let _ = f.read_to_string(&mut seen);
        }
        Outbox {
            at,
            file,
            lines: seen.lines().map(String::from).collect(),
            seen,
            spent: vec![],
            text: String::new(),
            said: vec![],
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
    /// needs, not allowed at this event, or with every key it would spend
    /// already spent.
    pub(super) fn sayable(&self, l: &Line) -> bool {
        let card = policy(&l.kind);
        let acted = card.command.is_none_or(|c| l.text.contains(c));
        let here = card.at.contains(&self.at);
        !l.text.is_empty() && acted && here && self.fresh(&l.kind)
    }

    /// `sayable`, after checking the line is one this event may say at all: a
    /// kind built for another event is a bug in the caller, caught in a debug
    /// build and dropped in a release one.
    fn admit(&self, l: &Line) -> bool {
        debug_assert!(
            policy(&l.kind).at.contains(&self.at),
            "{:?} is not said at {:?}",
            l.kind,
            self.at
        );
        self.sayable(l)
    }

    /// Add `l`, unless it is empty, lacks the command its kind needs, is not
    /// allowed at this event, or every key it would spend is already spent.
    pub(crate) fn say(&mut self, l: Line) {
        if !self.admit(&l) {
            return;
        }
        self.spent
            .extend(self.fresh_keys(&l.kind).unwrap_or_default());
        self.text.push_str(&l.text);
        self.said.extend(l.kind.entries(core::est_tokens(&l.text)));
    }

    /// Say `lines` in order within `budget` tokens; a kind whose card says
    /// `Cut::Never` (rows) is never cut. Over budget the lines go
    /// whole, lowest cut rank first (the stashed notice, then the stashed fix,
    /// then the edit hint, then the commit-cite hint). A cut line keeps its
    /// keys for a later push. True when a stashed line was said (caller unstashes).
    pub(crate) fn say_within(&mut self, budget: usize, lines: Vec<Line>) -> bool {
        let mut keep: Vec<Line> = lines.into_iter().filter(|l| self.admit(l)).collect();
        let cost = |ls: &[Line]| ls.iter().map(|l| core::est_tokens(&l.text)).sum::<usize>();
        while cost(&keep) > budget {
            match next_cut(keep.iter().map(|l| policy(&l.kind).cut)) {
                Some(i) => keep.remove(i),
                None => break,
            };
        }
        let stashed = keep.iter().any(|l| policy(&l.kind).stash);
        for l in keep {
            self.say(l);
        }
        stashed
    }

    /// Spend `key` with no line said: a once-mark nothing reads aloud (the
    /// hub peek, once per file per session).
    pub(crate) fn spend(&mut self, key: String) {
        self.spent.push(key);
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
            said: self.said,
        }
    }
}
