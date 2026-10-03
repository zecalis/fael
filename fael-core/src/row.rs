use crate::{now_ms, rfc3339, ulid_at};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One line of a log file — an add row, or a close row (`ref` set, no kind/files).
/// Reading is lenient: every field defaults, and fields fael doesn't know are kept in `extra`
/// and written back untouched (forward-compat, format.md §Readers).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Row {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub v: Option<u64>,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub ts: String,
    #[serde(default)]
    pub by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Who has to answer (PLAN-fael-direction chunk 2): an `issue --to <who>`
    /// routes a question to who must answer. Optional, stored lowercase.
    /// A top-level field (not `extra`) so select/render read it without
    /// parsing — old readers keep it in `extra` and stay compatible, no `v` bump.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Where the issue sits in the urgent queue (chunk 3): absent = not
    /// urgent, present = urgent with lower more urgent. Fractional indexing —
    /// `--urgent` files at the end, `--urgent-before <id>` just above that
    /// row, `fael bump` moves it later. Top-level like `to`, same compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgent: Option<f64>,
    /// The ≤ ~15-word headline lists show (chunk 4): agents skim titles,
    /// then pull the body by id (`find <id>`, `--full`). Optional — rows
    /// without one render the first sentence of `text` instead. Top-level
    /// like `to`, same compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// When to look at this row again (row-hygiene chunk 5): a date
    /// `YYYY-MM` or `YYYY-MM-DD`, or free text like `mdl lands`. A date ≤
    /// today surfaces the row at the top of `kickoff` whatever its files;
    /// free text only counts (`fael find --revisit` lists it). Top-level
    /// like `to`, same compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revisit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    /// Which self-heal rule filed this row's `supersedes`
    /// (PLAN-fael-selfheal-verdict chunk 3): `explicit:text`, `identity:key`,
    /// `heuristic:files`, each with `:cross-key` when the key moved, or
    /// `caller:flag` for a resolving `--supersedes` that passed through.
    /// Absent reads as `unknown` — rows filed before chunk 3 are never
    /// backfilled, and readers must not error on its absence. Top-level like
    /// `to`, same compat: old readers keep it in `extra`, no `v` bump.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_source: Option<String>,
    /// The supersede edge this row reverts (PLAN-fael-selfheal-restore chunk
    /// 1): the superseder's id — one row supersedes at most one row, so the
    /// superseder names the edge. Set only on restore event rows (no kind, no
    /// files): readers subtract these edges in `superseded()`. Absent reads
    /// as "reverts nothing". Top-level like `to`, same compat: old readers
    /// keep it in `extra`, no `v` bump.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restores: Option<String>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Row {
    /// Who this row routes to: the `to` field. Top-level since chunk 2, so
    /// a deserialized row never carries it in `extra` — there is no
    /// fallback to look for.
    pub fn to_who(&self) -> Option<&str> {
        self.to.as_deref()
    }

    /// The branch this row was filed on: `extra.branch`, stamped by the
    /// adapter from git (servers leave it absent). Read like `to_who` — the
    /// branch a row's work belongs to, for the `[Orphan]` doctor check.
    pub fn branch(&self) -> Option<&str> {
        self.extra.get("branch").and_then(|v| v.as_str())
    }

    /// The branch working on this issue: `extra.held`, set by `fael claim`.
    /// Informational only — nothing refuses a second claim, it warns.
    pub fn held(&self) -> Option<&str> {
        self.extra.get("held").and_then(|v| v.as_str())
    }

    /// The agent session that filed this row: `extra.session`, stamped by the
    /// adapter from the hook session (absent outside one). With a push's session
    /// it tells "written by A, used by B" from "written and used by A".
    pub fn session(&self) -> Option<&str> {
        self.extra.get("session").and_then(|v| v.as_str())
    }

    /// The file hashes this row carries: `extra.fh`, a map from each real
    /// file in `files` to its 12-hex git blob id at write time
    /// (PLAN-fael-file-hash chunk 1). `None` on rows written before `fh`,
    /// or whose files were all anchors, globs or unreadable.
    pub fn file_hashes(&self) -> Option<&Map<String, Value>> {
        self.extra.get("fh").and_then(|v| v.as_object())
    }

    /// When to look at this row again: the `revisit` field, falling back to
    /// a hand-written `revisit` in `extra` (forward-compat read).
    pub fn revisit(&self) -> Option<&str> {
        self.revisit
            .as_deref()
            .or_else(|| self.extra.get("revisit").and_then(|v| v.as_str()))
    }

    /// The row's urgent number, if any: the `urgent` field. Top-level since
    /// chunk 3, so a deserialized row never carries it in `extra` — the old
    /// number/numeric-string fallback could never run (and a hand-written
    /// `"urgent":"1"` fails Row deserialization outright, skipping the row
    /// as unreadable). Non-finite values read as absent — ranking must stay
    /// deterministic.
    pub fn urgent_value(&self) -> Option<f64> {
        self.urgent.filter(|u| u.is_finite())
    }

    /// What lists show: the `title` when set, else the first line's first
    /// sentence of `text` cut at ~20 words or ~80 chars + `…` (old rows never get a
    /// title, no backfill). `;`/`·`/`—` join topics the way `.` joins
    /// sentences, so the auto title is the first topic only — Thai and CJK
    /// have neither `.` nor spaces to cut on, which is why the char cap
    /// exists next to the word cap. A short single-topic text renders
    /// unchanged — no `…` when nothing was dropped.
    pub fn display_title(&self) -> String {
        if let Some(t) = self.title.as_deref() {
            let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
            if !t.is_empty() {
                return t;
            }
        }
        let mut lines = self.text.lines();
        let first = lines.next().unwrap_or("");
        let more_lines = lines.next().is_some();
        let one = first.split_whitespace().collect::<Vec<_>>().join(" ");
        let end = first_sentence_end(&one);
        let (head, rest) = (&one[..end], one[end..].trim());
        let first = head.split([';', '·', '—']).next().unwrap_or(head).trim();
        let first = if first.is_empty() { head.trim() } else { first };
        let words: Vec<&str> = first.split_whitespace().collect();
        let mut title = if words.len() > 20 {
            format!("{} …", words[..20].join(" "))
        } else if rest.is_empty() && !more_lines && first == head.trim() {
            first.to_string()
        } else {
            format!("{first} …")
        };
        if title.chars().count() > 80 {
            let cut: String = title.chars().take(80).collect();
            title = format!("{} …", cut.trim_end_matches([' ', '…']));
        }
        title
    }

    /// A fresh v1 add row stamped with a ULID and the current UTC time.
    pub fn new(by: &str, kind: &str, text: &str, files: Vec<String>) -> Row {
        let ms = now_ms();
        Row {
            v: Some(1),
            id: ulid_at(ms),
            ts: rfc3339(ms),
            by: by.into(),
            kind: kind.into(),
            text: text.into(),
            files,
            ..Row::default()
        }
    }

    /// A fresh v1 close row pointing at `reference`.
    pub fn close(by: &str, reference: &str, text: &str) -> Row {
        let ms = now_ms();
        Row {
            v: Some(1),
            id: ulid_at(ms),
            ts: rfc3339(ms),
            by: by.into(),
            text: text.into(),
            reference: Some(reference.into()),
            ..Row::default()
        }
    }

    /// A fresh v1 alias row recording `from → to` (`fael mv`). Carries no
    /// kind and no files — it only says where a path lives now. Readers that
    /// don't know `moved` skip the row; `text` is human-readable and ignored.
    pub fn moved(by: &str, from: &str, to: &str) -> Row {
        let ms = now_ms();
        Row {
            v: Some(1),
            id: ulid_at(ms),
            ts: rfc3339(ms),
            by: by.into(),
            text: format!("{from} → {to}"),
            extra: Map::from_iter([(
                "moved".to_string(),
                serde_json::json!({"from": from, "to": to}),
            )]),
            ..Row::default()
        }
    }

    /// A fresh v1 restore row reverting edge `edge → target` (`fael restore`).
    /// Carries no kind and no files — a carrier, never a result (format.md
    /// §Readers). Readers that predate the carrier rule still list it, so
    /// `text` names both ends in full: informative degrade, not garbage.
    pub fn restored(by: &str, edge: &str, target: &str) -> Row {
        let ms = now_ms();
        Row {
            v: Some(1),
            id: ulid_at(ms),
            ts: rfc3339(ms),
            by: by.into(),
            text: format!("{target} restored — supersede by {edge} reverted"),
            restores: Some(edge.into()),
            ..Row::default()
        }
    }

    /// The row as one JSON line, without the trailing `\n`.
    pub fn to_line(&self) -> String {
        serde_json::to_string(self).expect("Row always serialises")
    }
}

/// Byte index just past the first sentence end (`.`/`!`/`?` followed by
/// whitespace or end), or the whole string when there is none — so `src/a.rs`
/// mid-text never splits a title, only a real sentence break does.
fn first_sentence_end(s: &str) -> usize {
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if matches!(c, b'.' | b'!' | b'?') && b.get(i + 1).is_none_or(|n| n.is_ascii_whitespace()) {
            return i + 1;
        }
    }
    s.len()
}

/// Who wrote a row and where the tree stood — the adapter fills it: git on a dev box,
/// the signed-in user (no branch/sha) on a server. Core never asks git itself.
#[derive(Debug, Clone, Default)]
pub struct Stamp {
    pub by: String,
    pub branch: Option<String>,
    pub sha: Option<String>,
}

impl Stamp {
    pub(crate) fn apply(&self, row: &mut Row) {
        if let Some(b) = &self.branch {
            row.extra.insert("branch".into(), b.clone().into());
        }
        if let Some(s) = &self.sha {
            row.extra.insert("sha".into(), s.clone().into());
        }
    }
}
