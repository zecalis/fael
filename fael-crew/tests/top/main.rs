//! Consolidated crew integration suites: one binary instead of one per file.
//!
//! Thin entry only — each module is a former `tests/*.rs` file. `common` is
//! shared (one instance, via `crate::common`) instead of one `mod common`
//! per file.

#[path = "../common/mod.rs"]
mod common;

mod board;
mod board_branches;
mod board_lean;
mod board_left_stale;
mod board_merged;
mod board_prs;
mod board_read;
mod board_shape;
mod board_watch;
mod chunk;
mod chunk_amend;
mod chunk_apply_plan;
mod chunk_brief_paths;
mod chunk_contract;
mod chunk_issue;
mod chunk_model;
mod chunk_show_forms;
mod chunk_verbs;
mod dispatch;
mod done_merged_pr;
mod done_pr;
mod done_scope;
mod help_reach;
mod plan;
mod start_gates;
mod start_scope;
mod usage_flags;
