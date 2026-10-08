//! The in-context mark of an edit (PLAN-fael-visible-secretary chunk 5), split
//! out of `say` to keep that file small: it reads and writes the `Outbox`'s
//! seen list directly.

use super::asks::{UsageMeta, hook_meta};
use super::protocol::Ctx;
use super::say::Outbox;
use super::usage::usage_row;
use crate::core;
use std::io::Write;

/// The usage event `fael-core::stats` reads as "in context at edit".
const IN_CONTEXT: &str = "in-context";

impl Outbox {
    /// PLAN-fael-visible-secretary chunk 5: decisions and issues about this
    /// very file (tier 0) already in the agent's context when it edited it.
    /// Its own 0-byte usage line under `in_context`, never `ids` (nothing was
    /// pushed). Notes ride the same line under `in_context_notes`
    /// (PLAN-fael-say-gate chunk 3), so `value` keeps counting decisions and
    /// issues only. The seen list also holds rows the agent filed or found
    /// itself, so stats counts only ids an earlier push of the session handed
    /// over. Each id once per session: an `@<id>` line in the seen list marks it.
    pub(crate) fn record_in_context(
        &mut self,
        c: &Ctx,
        tiered: &[(&core::Row, usize)],
        files: &[String],
    ) {
        let Some(f) = &mut self.file else { return };
        let seen = &self.lines;
        let (notes, ids): (Vec<&core::Row>, Vec<&core::Row>) = tiered
            .iter()
            .filter(|(r, tier)| {
                *tier == 0
                    && matches!(r.kind.as_str(), "decision" | "issue" | "note")
                    && seen.contains(r.id.as_str())
                    && !seen.contains(&format!("@{}", r.id))
            })
            .map(|(r, _)| *r)
            .partition(|r| r.kind == "note");
        if ids.is_empty() && notes.is_empty() {
            return;
        }
        let id = |rs: &[&core::Row]| rs.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        let marks: String = ids
            .iter()
            .chain(&notes)
            .map(|r| format!("@{}\n", r.id))
            .collect();
        let _ = f.write_all(marks.as_bytes());
        let meta = UsageMeta {
            files,
            ..hook_meta(c, None, false)
        };
        let mut row = usage_row(&c.client, IN_CONTEXT, &c.repo.root, "", &[], &meta);
        row["in_context"] = id(&ids).into();
        if !notes.is_empty() {
            row["in_context_notes"] = id(&notes).into();
        }
        super::asks::append_row(row);
    }
}
