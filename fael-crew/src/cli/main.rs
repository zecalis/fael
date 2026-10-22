//! fael-crew — plan · chunk · board · sweep · run: the owner's layer. `fael` execs
//! this binary for those verbs with the same argv (`fael/src/crew.rs`), so `fael chunk start`
//! and `fael-crew chunk start` are one command. The ledger's push/write machinery stays in
//! `fael`: this binary calls back into it (`FAEL_BIN`) for the three things only it can do.

mod board;
mod chunk;
mod chunk_ask;
mod chunk_elsewhere;
mod chunk_left;
mod chunk_run;
mod chunk_scope;
mod chunk_scope_gone;
mod chunk_scope_report;
mod chunk_scope_skip;
mod chunk_show;
mod chunk_verbs;
mod fael_bin;
mod plan;
mod sweep;

use fael_core::help;
use fael_core::{self as core};
pub(crate) use fael_core::{Repo, args::Args, journal, read_repo as read, repo, repo_at};
use std::path::Path;
use std::process::ExitCode;

/// The crew verbs; anything else is not this binary's.
const VERBS: [&str; 5] = ["plan", "chunk", "board", "sweep", "run"];

fn main() -> ExitCode {
    #[cfg(unix)]
    reset_sigpipe();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut res = run(argv.clone());
    // the arity reject runs nothing: retry once with `--scope a b` as `--scope a,b`
    let mut shown = argv.clone();
    if let Err(e) = &res
        && e.starts_with("rejected: wrong arguments")
        && let Some(joined) = help::reject::join_lists(&argv)
    {
        shown = joined;
        res = run(shown.clone());
    }
    match res.map_err(|e| help::reject::reject(&shown, e)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Rust ignores SIGPIPE, so `fael board --json | head` panicked in `println!` once
/// head closed the pipe. The default action ends the process quietly, like any unix tool.
#[cfg(unix)]
fn reset_sigpipe() {
    unsafe extern "C" {
        fn signal(sig: i32, handler: usize) -> usize;
    }
    const SIGPIPE: i32 = 13; // same on Linux and macOS
    const SIG_DFL: usize = 0;
    unsafe {
        signal(SIGPIPE, SIG_DFL);
    }
}

fn run(argv: Vec<String>) -> Result<ExitCode, String> {
    if matches!(argv.as_slice(), [v] if v == "--version" || v == "-v" || v == "-V") {
        println!("fael-crew {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    // `fael-crew <cmd> --help` — the same section `fael <cmd> --help` prints; no command, the verbs
    if help::is_request(&argv) {
        let named = argv
            .iter()
            .take_while(|x| *x != "--")
            .any(|x| !x.starts_with('-') && x != "help");
        println!(
            "{}",
            if named {
                help::for_argv(&argv)
            } else {
                usage()
            }
        );
        return Ok(ExitCode::SUCCESS);
    }
    let a = Args::parse(argv)?;
    let cmd = a.pos.first().map(String::as_str).unwrap_or("");
    let rest = a.pos.get(1..).unwrap_or_default();
    match (cmd, rest) {
        ("plan", rest) => plan::cmd(&a, rest),
        ("board", []) => board::cmd(&a),
        ("sweep", []) => sweep::cmd(&a),
        ("chunk", rest) => chunk::cmd(&a, rest),
        ("run", [end, r]) if end == "end" => chunk::run_end(&a, r),
        ("run", [beat, r]) if beat == "beat" => chunk::run_beat(&a, r),
        ("", []) => {
            println!("{}", usage());
            Ok(ExitCode::SUCCESS)
        }
        _ if VERBS.contains(&cmd) => Err(format!(
            "rejected: wrong arguments for {cmd:?} — try 'fael {cmd} --help'"
        )),
        _ => Err(format!(
            "rejected: unknown command {cmd:?} — try 'fael-crew --help'"
        )),
    }
}

/// `fael-crew` / `fael-crew --help`: the verbs `fael` hands over, one line each.
fn usage() -> String {
    let line = |name: &str| {
        let summary = help::summary(name).unwrap_or_default();
        format!("  {name:8} {summary}\n")
    };
    let list: String = VERBS.iter().map(|v| line(v)).collect();
    format!(
        "fael-crew — the owner's layer: plan, chunk, board (run as `fael <verb>`, which hands over)

usage: fael-crew <command> [options]

commands:
{list}
run 'fael-crew <command> --help' for the flags of one command."
    )
}

/// `git <args>` in `dir`: trimmed stdout when it exits 0 and prints something. Answered from
/// the --watch cache while the board lives (`board::git_output`).
pub(crate) fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let (code, out) = board::git_output(dir, args)?;
    let s = out.trim().to_string();
    (code == Some(0) && !s.is_empty()).then_some(s)
}
