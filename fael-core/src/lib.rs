//! fael-core — the log format (docs/format.md): row v1, validate, read, append under lock.
//! Knows the row format, never a client. The CLI, MCP and hooks sit on top of this.
//!
//! Thin entry only: module wiring, the public surface (every name the old flat
//! `lib.rs` exported still resolves here), the size/key consts, and `anchor`
//! — the one primitive every matcher shares. The row shape lives in `row`,
//! repo settings in `config`, write-time checks in `validate`.

mod aliases;
mod blob;
mod compact;
mod config;
mod doctor;
mod hook;
mod id;
mod import;
pub mod lang;
mod log;
mod query;
mod row;
pub mod stats;
pub mod sync;
mod validate;

pub use aliases::{Aliases, is_alias_row, is_carrier_row};
pub use blob::blob_id_stream;
pub use compact::{Opts as CompactOpts, Report as CompactReport, WriterReport, compact};
pub use config::{Config, CrossKey, Store};
pub use import::{Opts as ImportOpts, Report as ImportReport, import};

pub use doctor::{
    Kind as ProblemKind, Problem, Report as DoctorReport, Severity, current_month,
    fix as doctor_fix, precision as doctor_precision, scan as doctor_scan,
};

pub use hook::last_row_ms;

pub use id::{
    looks_like_id, now_ms, rfc3339, to_matches, ts_ms, ulid, ulid_at, ulid_ms, writer_id,
};
pub use lang::{Hit, Lang, by_name, marker_hit, row_language_check};
pub use log::{
    Adopted, BumpOpts, Log, MONTH_MAX, Purged, Restored, add, add_row, adopt_tree, append,
    bump_row, close, close_row, decode_text, fold_bumps, is_month, mv_row, needs_seal, parse,
    purge_row, read, read_imported, restore_row,
};
pub use query::{
    Abbrev, BASELINE, Background, Bucket, CUT_BUDGET, CUT_CAP, CUT_HUB_PEEK, Cut, EXPAND_MAX,
    Filter, Focus, Hidden, KeyUse, MdRef, PUSH_BACKGROUND, PUSH_HUB_PEEK, PUSH_HUB_ROWS, PolicyDef,
    PushPolicy, Ref, Selection, Urgent, UrgentChange, abbrev, backtick_paths, brief, bucket,
    closed, cmp_rows, due, est_tokens, expands, fat_reasons, find, fresh_ts, freshness, glob, gone,
    gone_files, groups, id_tokens, is_date, json_note, key_hints, keys, kickoff, levenshtein,
    on_work, page, phantom_md_refs, phantom_refs, push, push_tiered, query, ranked, ref_state,
    render, render_full, render_full_page, render_groups, render_page, resolve, resolve_row,
    resolve_urgent, restored, reverted, row_due, row_waiting, select, stale_refs, successors,
    superseded, today, waiting, waiting_line, warnings, why_empty, with_due,
};
pub use row::{Row, Stamp};
pub use validate::{
    normalize_files, secret, valid_key, validate, validate_alias, validate_bump, validate_close,
    validate_restore,
};

/// Core kinds with fixed meaning; a repo adds more through `Config::kinds`.
pub const CORE_KINDS: [&str; 3] = ["decision", "issue", "note"];
/// Hard cap on one serialised row, in bytes (Thai is 3 bytes/char).
pub const ROW_BYTES_MAX: usize = 10 * 1024;
pub const KEY_MAX: usize = 64;

/// A file glob (`*`, `?`, `[...]`) is a pattern, not a path — `find` matches
/// it the same way, so it always passes the write check.
pub fn is_glob(f: &str) -> bool {
    f.contains(['*', '?', '['])
}

/// `scheme:ref` anchor (`doc:pricing`, `issue:#12`): a scheme of ≥ 2 chars `[a-z0-9+.-]`, starting
/// with a letter, before the first `:` and before any `/`. Two chars minimum so `C:` stays a drive.
/// Returns the ref — opaque to fael (`/` in it is not a path separator); it must be non-empty.
// ponytail: a root-level file named like `notes:v2.md` reads as an anchor — rare, rename the file
pub(crate) fn anchor(f: &str) -> Option<&str> {
    f.split_once(':')
        .filter(|(s, _)| {
            s.len() >= 2
                && s.as_bytes()[0].is_ascii_lowercase()
                && s.bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'+' | b'.' | b'-'))
        })
        .map(|(_, r)| r)
}
