//! fael-core — the log format (docs/format.md): row v1, validate, read, append under lock.
//! Knows the row format, never a client. The CLI, MCP and hooks sit on top of this.
//!
//! Thin entry only: module wiring, the public surface (every name the old flat
//! `lib.rs` exported still resolves here), the size/key consts, and `anchor`
//! — the one primitive every matcher shares. The row shape lives in `row`,
//! repo settings in `config`, write-time checks in `validate`.

mod aliases;
pub mod args;
mod blob;
mod compact;
mod config;
mod doctor;
pub mod drift;
mod gitcmd;
pub mod help;
mod hook;
mod id;
mod import;
pub mod journal;
pub mod lang;
mod log;
mod overlap;
mod query;
mod repo;
mod row;
mod shape;
pub mod stats;
pub mod sync;
mod validate;
pub mod work;

/// The commit a dev build came from (`abc1234`, `abc1234.dirty`); None on a release.
pub const GIT: Option<&str> = option_env!("FAEL_GIT");

pub use aliases::{Aliases, is_alias_row, is_carrier_row};
pub use blob::blob_id_stream;
pub use compact::{Opts as CompactOpts, Report as CompactReport, WriterReport, compact};
pub use config::{Config, CrossKey, auto_update_on};
pub use import::{Opts as ImportOpts, Report as ImportReport, import};

pub use doctor::{
    Kind as ProblemKind, Problem, Report as DoctorReport, Severity, current_month,
    fix as doctor_fix, precision as doctor_precision, scan as doctor_scan,
};

pub use hook::last_row_ms;

pub use gitcmd::git_cmd;
pub use id::{
    looks_like_id, now_ms, rfc3339, to_matches, ts_ms, ulid, ulid_at, ulid_ms, writer_id,
};
pub use lang::{Hit, Lang, by_name, fixed_hit, marker_hit, row_language_check};
pub use log::{
    BumpOpts, Log, MONTH_MAX, NOOP_MARK, Purged, Restored, add, add_close, add_row, append,
    append_many, bump_row, close, close_row, decode_text, fold_bumps, is_month, mv_row, needs_seal,
    parse, purge_row, read, read_imported, restore_row,
};
pub use overlap::{OverlapClass, overlap, overlap_class, overlap_how, under, within};
pub use query::{
    Abbrev, Background, Bucket, CUT_BUDGET, CUT_CAP, CUT_HUB_PEEK, Cut, Filter, Focus, Hidden,
    KeyUse, MdRef, NO_ROW_WITH_ID, PLAN_KICKOFF_ROWS, PUSH_BACKGROUND, PUSH_HUB_PEEK,
    PUSH_HUB_ROWS, Prior, PushPolicy, Ref, Selection, Urgent, UrgentChange, abbrev, add_gate,
    backtick_paths, brief, bucket, chunk_stem, closed, closed_fallback, cmp_rows, code_spans, due,
    est_tokens, fat_reasons, find, fix_close, fresh_ts, freshness, glob, gone, gone_files,
    handoff_first, has_regex_chars, id_tokens, is_date, json_note, key_hints, keys, kickoff,
    levenshtein, names_key, nearest, on_work, page, parse_recall, phantom_md_refs, phantom_refs,
    plan_anchor, push, push_tiered, query, ranked, ref_state, regex_hint, render, render_full,
    render_full_page, render_page, resolve, resolve_identity, resolve_row, resolve_urgent,
    restored, reverted, row_due, row_waiting, select, select_with, stale_refs, successors,
    superseded, today, waiting, waiting_line, warnings, why_empty, with_due,
};
pub use repo::{Repo, git, git_output, read as read_repo, repo, repo_at, sibling_suggest};
pub use row::{Row, Stamp};
pub use shape::{close_gate, close_rejects};
pub use stats::SayKind;
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
pub fn anchor(f: &str) -> Option<&str> {
    f.split_once(':')
        .filter(|(s, _)| {
            s.len() >= 2
                && s.as_bytes()[0].is_ascii_lowercase()
                && s.bytes()
                    .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'+' | b'.' | b'-'))
        })
        .map(|(_, r)| r)
}

/// A `scheme:ref` anchor (opaque, never a filesystem path): a scheme of ≥2 lower chars
/// before the first `:`, and not a `://` url.
pub fn is_anchor(f: &str) -> bool {
    if f.contains("://") || !f.contains(':') {
        return false;
    }
    let scheme = f.split(':').next().unwrap_or("");
    scheme.len() >= 2
        && scheme.as_bytes()[0].is_ascii_lowercase()
        && scheme
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'+' | b'.' | b'-'))
}
