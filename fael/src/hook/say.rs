//! The one door to the agent's context (PLAN-fael-say-gate). Every line a hook
//! hands the agent is a `Line` said through an `Outbox`: `policy` decides, in
//! one exhaustive match, whether the line's keys are spent in a seen list and
//! whether it must carry a command; `reply` builds the `Reply`. `context` is
//! private to this module, so no other path can put words in front of the agent.
//!
//! The session's seen list (per session + sub-agent + worktree, gone on
//! compact) holds a row id per row said, `~<id>` per edit-hint ask (`~*`, the old generic clause, in older lists) and
//! `@<id>` per in-context mark and `^<id>` per cited id (`cited`). The prompt's pointer list is its own file of
//! bare keys. No session = no file: every line is said, nothing remembered.

use crate::core;
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

    /// Two replies as one, in order (a shell call's edit side, then its read
    /// side): the contexts joined, the first notice kept.
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
    /// The session-start rows: said once per new or compacted context, never
    /// spent — the next read of a briefed file may push its rows again.
    Brief,
    /// The edit hint: re-check rows already in context, each named. `issue`:
    /// open issues on the edited file, free of the per-turn limit (a vela
    /// edit's issue went unasked behind a package.json ask in the same turn).
    Ask { ids: Vec<String>, issue: bool },
    /// The prompt hint: open keys the prompt names.
    Pointer { keys: Vec<String> },
    /// `bodies: fael find <id> …` under a push whose rows have bodies.
    Bodies,
    /// A `git commit` that named open issues (PLAN-fael-agent-ergonomics
    /// chunk 5): each named once per session with its ready `fael close`.
    Cited { ids: Vec<String> },
    /// The consolidate ask (PLAN-fael-context-loop chunk 3): a file whose
    /// open decision/note rows crowd the agent's context, asked once per file.
    Merge { file: String, ids: Vec<String> },
    /// A review finding (PLAN-fael-experience-loop chunk 2): the ready
    /// `fael add issue` for a ReportFindings entry, once per file and line.
    Finding { file: String, line: i64 },
    /// A closed issue whose close named a path that is gone (PLAN-fael-
    /// experience-loop chunk 3): asked once per issue and session.
    Check { id: String },
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
    /// At most one line of it per user turn (prompt to prompt): a task that
    /// edits ten files would otherwise ask after ten edits. A row not asked
    /// keeps its key, so a later turn's edit may ask it.
    pub(crate) per_turn: bool,
}

/// The noise policy, in one place. A new `Kind` does not compile until it
/// has an arm here; `say_contract::every_kind_has_a_fixture` fails until it
/// has a fixture there.
pub(crate) fn policy(k: &Kind) -> Policy {
    let (once, command, per_turn) = match k {
        Kind::Row { .. } => (Once::Key, None, false),
        Kind::Ask { issue, .. } => (Once::Key, Some("fael close"), !issue),
        Kind::Pointer { .. } => (Once::Key, Some("fael find --key"), false),
        Kind::Bodies => (Once::Key, Some("fael find <id>"), false),
        Kind::Cited { .. } => (Once::Key, Some("fael close"), false),
        Kind::Merge { .. } => (Once::Key, Some("fael add"), true),
        Kind::Finding { .. } => (Once::Key, Some("fael add issue"), false),
        Kind::Check { .. } => (Once::Key, Some("fael add issue"), true),
        Kind::Brief | Kind::Notice => (Once::Event, None, false),
    };
    Policy {
        once,
        command,
        per_turn,
    }
}

impl Kind {
    /// The seen-list lines this kind names — spent only under `Once::Key`.
    pub(crate) fn keys(&self) -> Vec<String> {
        match self {
            Kind::Row { ids } => ids.clone(),
            Kind::Ask { ids, .. } => ids.iter().map(|i| format!("~{i}")).collect(),
            Kind::Pointer { keys } => keys.clone(),
            Kind::Bodies => vec!["~bodies".into()],
            Kind::Cited { ids } => ids.iter().map(|i| format!("~cited:{i}")).collect(),
            Kind::Merge { file, .. } => vec![format!("~merge:{file}")],
            Kind::Finding { file, line } => vec![format!("~finding:{file}:{line}")],
            Kind::Check { id } => vec![format!("~check:{id}")],
            Kind::Brief | Kind::Notice => vec![],
        }
    }
}

/// One `said` entry: the kind's name and the id, key or count key it names.
/// `Brief` names none (stats reads the line's `ids`), nor do `Bodies` and
/// `Notice`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Said {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<String>,
}

impl Kind {
    fn said(&self) -> Vec<Said> {
        let (kind, keys) = match self {
            Kind::Row { ids } => ("row", ids.clone()),
            Kind::Brief => ("brief", vec![]),
            Kind::Ask { ids, .. } => ("ask", ids.clone()),
            Kind::Pointer { keys } => ("pointer", keys.clone()),
            Kind::Bodies => ("bodies", vec![]),
            Kind::Cited { ids } => ("cited", ids.clone()),
            Kind::Merge { ids, .. } => ("merge", ids.clone()),
            Kind::Finding { file, .. } => ("finding", vec![file.clone()]),
            Kind::Check { id } => ("check", vec![id.clone()]),
            Kind::Notice => ("notice", vec![]),
        };
        match keys.is_empty() {
            true => vec![Said { kind, key: None }],
            false => keys
                .into_iter()
                .map(|k| Said { kind, key: Some(k) })
                .collect(),
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
    pub(super) file: Option<File>,
    seen: String,
    /// `seen`'s lines, parsed once.
    pub(super) lines: HashSet<String>,
    spent: Vec<String>,
    text: String,
    said: Vec<Said>,
    /// `~turn:<id>`, the once-mark of a `per_turn` kind for the user's turn
    /// (`turn`); `None` when no prompt hook marked one — no per-turn limit.
    turn: Option<String>,
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
            said: vec![],
            turn: None,
        }
    }

    /// The user turn this push runs in (`state::read_turn`): a `per_turn`
    /// kind is said once in it.
    pub(crate) fn in_turn(mut self, turn: Option<String>) -> Self {
        self.turn = turn.map(|t| format!("~turn:{t}"));
        self
    }

    /// The turn mark `kind` would spend: `None` when it has no per-turn limit.
    fn turn_key(&self, kind: &Kind) -> Option<&String> {
        self.turn.as_ref().filter(|_| policy(kind).per_turn)
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
        self.fresh_keys(kind).is_some() && self.turn_key(kind).is_none_or(|t| !self.is_spent(t))
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
        if let Some(t) = self.turn_key(&l.kind).cloned() {
            self.spent.push(t);
        }
        self.text.push_str(&l.text);
        self.said.extend(l.kind.said());
    }

    /// Say `lines` in order within `budget` tokens; rows and bodies are never cut.
    /// Over budget the stashed notice goes first, then the consolidate ask, then the
    /// gone-check ask, then the
    /// edit hints (the open-issue one, said first, goes last), then the commit-cite
    /// hint, each whole. A cut line
    /// keeps its keys for a later push. True when a notice was said (caller unstashes).
    pub(crate) fn say_within(&mut self, budget: usize, lines: Vec<Line>) -> bool {
        let mut keep: Vec<Line> = lines.into_iter().filter(|l| self.sayable(l)).collect();
        let cost = |ls: &[Line]| ls.iter().map(|l| core::est_tokens(&l.text)).sum::<usize>();
        while cost(&keep) > budget {
            let cut = keep
                .iter()
                .position(|l| matches!(l.kind, Kind::Notice))
                .or_else(|| {
                    keep.iter()
                        .position(|l| matches!(l.kind, Kind::Merge { .. }))
                })
                .or_else(|| {
                    keep.iter()
                        .position(|l| matches!(l.kind, Kind::Check { .. }))
                })
                .or_else(|| {
                    keep.iter()
                        .rposition(|l| matches!(l.kind, Kind::Ask { .. }))
                })
                .or_else(|| {
                    keep.iter()
                        .position(|l| matches!(l.kind, Kind::Cited { .. }))
                });
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
