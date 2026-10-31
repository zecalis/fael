//! The name of every push line a hook says, shared by both ends of the wire:
//! the `fael` crate writes `said` entries from it (`say_card::policy`), and
//! `stats` reads them back by it. One enum, so a kind the hook can say is a
//! kind stats can tally — a name that is in one list and not the other
//! (`briefdrift`, `retired`, issue 01M5ZH6V) does not compile.

use serde::{Serialize, Serializer};

/// A `said` entry's kind: the wire name in `usage.jsonl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SayKind {
    /// The rows of an edit push. Stats splits it into `row` and `note` by the row's kind.
    Row,
    /// The session-start hand-offs and assigned rows. Names no key: stats reads the line's `ids`.
    Brief,
    /// The edit hint: re-check an open issue. Said at an `edit` only; old usage
    /// holds it at `shell-edit` too.
    Ask,
    /// Legacy: no hook says it now (the prompt pointer went), old usage still holds it.
    Pointer,
    /// Legacy: no hook says it now — the `bodies: fael find <id>` pointer under a
    /// push was cut at 26 of 282 pulled (9%); old usage still holds it.
    Bodies,
    /// A line fael raises on its own: a stashed risk, a session-start warning.
    Notice,
    /// A `git commit` that named open issues.
    Cited,
    /// A review finding, once per file and line.
    Finding,
    /// The agent said it fixed a bug and filed nothing after.
    Fixed,
    /// Legacy: no hook says it now — the `fix:` commit line was cut at 2 of 65
    /// (3%); old usage still holds it.
    FixCommit,
    /// An edit outside the held chunk's scope, inside another open chunk's.
    Overlap,
    /// A step commit in a worktree holding a chunk: ask for the checkpoint note.
    Checkpoint,
    /// Legacy: no hook says it now (the held-chunk line went), old usage still holds it.
    Held,
    /// The held chunk's brief, scope or title was edited after its run started.
    BriefDrift,
    /// A note said in its last session: file it as a decision, or let it go.
    Retired,
    /// Session start's pointer to the chunk run this worktree holds. Split from
    /// `Brief`: it names no row id, so no row rate could ever score it.
    ChunkBrief,
}

/// What an entry of a kind must be followed by, in the same session, to count
/// as acted on. An upper bound: the agent may have done it anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Earned {
    /// The entry's id was in context at an edit, or retired within a day.
    InContext,
    /// The entry's id was closed, superseded or bumped within a day.
    Retired,
    /// The session filed an issue naming the entry's key (a file).
    FiledIssueOnFile,
    /// The session filed an issue.
    FiledIssue,
    /// The session filed a decision.
    FiledDecision,
    /// The session filed a row of any kind.
    FiledAny,
    /// The session ran `fael chunk note`.
    ChunkNote,
    /// The session ran any accepted `fael chunk` call (show, note, done …).
    ChunkCall,
    /// A later pull by the entry's key.
    Pulled,
    /// A later pull by an id.
    PulledId,
}

impl SayKind {
    /// Every kind, in `fael stats` order (`buckets` adds `note` after `row`).
    pub const ALL: [SayKind; 16] = [
        SayKind::Row,
        SayKind::Brief,
        SayKind::Ask,
        SayKind::Pointer,
        SayKind::Bodies,
        SayKind::Notice,
        SayKind::Cited,
        SayKind::Finding,
        SayKind::Fixed,
        SayKind::FixCommit,
        SayKind::Overlap,
        SayKind::Checkpoint,
        SayKind::Held,
        SayKind::BriefDrift,
        SayKind::Retired,
        SayKind::ChunkBrief,
    ];

    /// The name on the wire.
    pub const fn name(self) -> &'static str {
        match self {
            SayKind::Row => "row",
            SayKind::Brief => "brief",
            SayKind::Ask => "ask",
            SayKind::Pointer => "pointer",
            SayKind::Bodies => "bodies",
            SayKind::Notice => "notice",
            SayKind::Cited => "cited",
            SayKind::Finding => "finding",
            SayKind::Fixed => "fixed",
            SayKind::FixCommit => "fixcommit",
            SayKind::Overlap => "overlap",
            SayKind::Checkpoint => "checkpoint",
            SayKind::Held => "held",
            SayKind::BriefDrift => "briefdrift",
            SayKind::Retired => "retired",
            SayKind::ChunkBrief => "chunkbrief",
        }
    }

    /// The kind a wire name names. `None` for a name no hook says any more
    /// (`count`, `merge`, `carry`, `promote`): stats skips those.
    pub fn parse(name: &str) -> Option<SayKind> {
        SayKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The stats buckets, in order: every kind's name, plus `note` (the notes
    /// among a push's rows) after `row`.
    pub fn buckets() -> impl Iterator<Item = &'static str> {
        SayKind::ALL
            .into_iter()
            .flat_map(|k| std::iter::once(k.name()).chain((k == SayKind::Row).then_some("note")))
    }

    /// How an entry of this kind earns.
    pub const fn earned(self) -> Earned {
        match self {
            SayKind::Row | SayKind::Brief => Earned::InContext,
            SayKind::Ask | SayKind::Cited | SayKind::Held => Earned::Retired,
            SayKind::Finding => Earned::FiledIssueOnFile,
            SayKind::Fixed | SayKind::FixCommit => Earned::FiledIssue,
            // it offers `fael add decision`: the session filed one after it
            SayKind::Retired => Earned::FiledDecision,
            SayKind::Notice => Earned::FiledAny,
            // a chunk note is no ledger row: its `call` usage line is the act.
            // `briefdrift` offers `fael chunk show`, but no usage line of a
            // `show` is indexed — the nearest recorded act on that chunk is
            // the note an agent writes once it has read the changed brief
            SayKind::Overlap | SayKind::Checkpoint | SayKind::BriefDrift => Earned::ChunkNote,
            // it offers `fael chunk show`: any accepted chunk call after it
            SayKind::ChunkBrief => Earned::ChunkCall,
            SayKind::Pointer => Earned::Pulled,
            SayKind::Bodies => Earned::PulledId,
        }
    }
}

/// On the wire as its name, so a `said` entry's JSON reads as it always did.
impl Serialize for SayKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ALL` lists every variant once: this match has no `_` arm, so a new
    /// variant fails to compile here until it is given a position, and the
    /// positions must read back from `ALL`.
    fn position(k: SayKind) -> usize {
        match k {
            SayKind::Row => 0,
            SayKind::Brief => 1,
            SayKind::Ask => 2,
            SayKind::Pointer => 3,
            SayKind::Bodies => 4,
            SayKind::Notice => 5,
            SayKind::Cited => 6,
            SayKind::Finding => 7,
            SayKind::Fixed => 8,
            SayKind::FixCommit => 9,
            SayKind::Overlap => 10,
            SayKind::Checkpoint => 11,
            SayKind::Held => 12,
            SayKind::BriefDrift => 13,
            SayKind::Retired => 14,
            SayKind::ChunkBrief => 15,
        }
    }

    #[test]
    fn all_lists_every_kind_once_and_names_round_trip() {
        for (i, k) in SayKind::ALL.into_iter().enumerate() {
            assert_eq!(position(k), i, "{k:?}");
            assert_eq!(SayKind::parse(k.name()), Some(k));
        }
    }

    #[test]
    fn a_name_no_hook_says_does_not_parse() {
        for n in ["count", "merge", "carry", "promote", "note", ""] {
            assert_eq!(SayKind::parse(n), None, "{n}");
        }
    }

    #[test]
    fn buckets_keep_the_old_order_and_add_the_new_kinds() {
        let b: Vec<&str> = SayKind::buckets().collect();
        assert_eq!(
            b,
            [
                "row",
                "note",
                "brief",
                "ask",
                "pointer",
                "bodies",
                "notice",
                "cited",
                "finding",
                "fixed",
                "fixcommit",
                "overlap",
                "checkpoint",
                "held",
                "briefdrift",
                "retired",
                "chunkbrief"
            ]
        );
    }

    #[test]
    fn it_serialises_as_its_name() {
        assert_eq!(
            serde_json::to_string(&SayKind::FixCommit).unwrap(),
            "\"fixcommit\""
        );
    }
}
