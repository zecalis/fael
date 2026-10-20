//! fael CLI — add · close · find · keys · kickoff over fael-core.
//! Output for people and agents is one markdown line per row, cut to a token budget;
//! `--json` prints one JSON row per line, uncut, for programs. `fael mcp` serves the same
//! add/close/find over stdio (see mcp.rs).

mod aliases;
mod amend;
mod batch;
mod board;
mod chunk;
mod chunk_ask;
mod chunk_elsewhere;
mod chunk_run;
mod chunk_scope;
mod chunk_show;
mod chunk_verbs;
mod claim;
mod close_key;
mod filehash;
mod find;
mod help;
mod hook;
mod install;
mod intake;
mod maintain;
mod mcp;
mod mv;
mod plan;
mod purge;
mod refs;
mod report;
mod restore;
mod schema;
mod selfheal;
mod session;
mod sweep;
mod synonyms;
mod unmerged;
mod write;

use fael_core::{self as core};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    #[cfg(unix)]
    reset_sigpipe();
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut argv = synonyms::rewrite(raw.clone());
    let mut res = run(argv.clone());
    // the arity reject runs nothing: retry once with `--files a b` as `--files a,b`
    if let Err(e) = &res
        && e.starts_with("rejected: wrong arguments")
        && let Some(joined) = synonyms::join_lists(&argv)
    {
        argv = joined;
        res = run(argv.clone());
    }
    let res = res.map_err(|e| write::arity::explain(&argv, synonyms::explain(&raw, &argv, e)));
    hook::record_cli(&argv, &res);
    match res {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// Rust ignores SIGPIPE, so `fael board --json | head` panicked in `println!` once
/// head closed the pipe. The default action ends fael quietly, like any unix tool.
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
        let git = fael_core::GIT.map(|g| format!("+{g}")).unwrap_or_default();
        println!("fael {}{git}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    // `fael help`, `fael --help`, `fael -h`, `fael <cmd> --help` — usage on
    // stdout, exit 0; with a command it shows only that command's section
    // after `--` it is text, not a flag: `fael add note --files a.rs -- -h`
    if help::is_request(&argv) {
        println!("{}", help::for_argv(&argv));
        return Ok(ExitCode::SUCCESS);
    }
    let a = Args::parse(argv)?;
    let cmd = a.pos.first().map(String::as_str).unwrap_or("");
    let rest = a.pos.get(1..).unwrap_or_default();
    match (cmd, rest) {
        ("add", [kind, text]) => add(&a, kind, text).map(|()| ExitCode::SUCCESS),
        // `--replace` re-files a row with one passage changed — no text to type
        ("add", [kind]) if a.has("replace") => add(&a, kind, "").map(|()| ExitCode::SUCCESS),
        // chunk 6b: `fael add --json -` reads a JSON array of rows from stdin
        ("add", [dash]) if dash == "-" && a.has("json") => batch::batch_add(&a),
        ("close", rest) if a.has("key") => {
            close_key::cli(&a, &a.one("key").unwrap_or_default(), rest)
        }
        ("close", rest) if rest.len() >= 2 => {
            batch::batch_close(&a, &rest[..rest.len() - 1], &rest[rest.len() - 1])
        }
        ("bump", [id]) => bump(&a, id).map(|()| ExitCode::SUCCESS),
        ("claim", [id]) => claim(&a, id).map(|()| ExitCode::SUCCESS),
        ("next", []) => next(&a).map(|()| ExitCode::SUCCESS),
        ("find", _) => find::extra::dispatch(&a, rest),
        ("keys", [] | [_]) => find::keys(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("kickoff", [] | [_]) => find::kickoff(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("mv", [old, new]) => mv::mv(&a, old, new).map(|()| ExitCode::SUCCESS),
        ("plan", rest) => plan::cmd(&a, rest),
        ("board", []) => board::cmd(&a),
        ("sweep", []) => sweep::cmd(&a),
        ("intake", [] | [_]) => intake::cmd(&a, rest.first()),
        ("chunk", rest) => chunk::cmd(&a, rest),
        ("run", [end, r]) if end == "end" => chunk::run_end(&a, r),
        ("run", [beat, r]) if beat == "beat" => chunk::run_beat(&a, r),
        ("restore", [] | [_]) => restore::restore(&repo()?, &a, rest.first().map(String::as_str))
            .map(|()| ExitCode::SUCCESS),
        ("purge", [id]) => purge::purge(&repo()?, &a, id).map(|()| ExitCode::SUCCESS),
        ("hook", [event]) => Ok(hook::cmd(event, a.one("client"))),
        ("stats", []) if a.has("misses") => {
            find::misses::print_recent(20);
            Ok(ExitCode::SUCCESS)
        }
        ("stats", []) => hook::stats(&a).map(|()| ExitCode::SUCCESS),
        ("report", []) => report::report(&a).map(|()| ExitCode::SUCCESS),
        ("doctor", []) => maintain::doctor(&a),
        ("compact", []) => maintain::compact(&a),
        ("import", [src]) => maintain::import(&a, src),
        ("mcp", []) => mcp::serve(a.has("pin")).map(|()| ExitCode::SUCCESS),
        ("install" | "upgrade" | "update", []) => {
            let (client, dry) = (a.one("client"), a.has("dry-run"));
            // install writes unasked; upgrade/update update the binary, look first and
            // ask (`--wiring` = the new binary's half)
            if cmd == "install" {
                install::cmd(client, dry, false)
            } else if a.has("auto") {
                install::auto::run(a.has("dry-run"), client.is_some())
            } else {
                install::upgrade(client, dry, a.has("yes"), a.has("wiring"))
            }
            .map(|()| ExitCode::SUCCESS)
        }
        // bare `fael` is a probe, not an error — the usage, on stdout, exit 0
        ("", []) => {
            println!("{}", help::usage());
            Ok(ExitCode::SUCCESS)
        }
        // a real command with the wrong arity names the command and points
        // at its own help; anything else names no command at all
        _ if help::for_command(cmd).is_some() => Err(format!(
            "rejected: wrong arguments for {cmd:?} — try 'fael {cmd} --help'"
        )),
        _ => Err(format!(
            "rejected: unknown command {cmd:?} — try 'fael --help'"
        )),
    }
}

pub(crate) use fael_core::args::Args;
pub(crate) use fael_core::journal;
pub(crate) use fael_core::{Repo, read_repo as read, repo, repo_at};

// answered from the --watch cache while the board lives here (moves with it)
pub(crate) fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let (code, out) = board::git_output(dir, args)?;
    let s = out.trim().to_string();
    (code == Some(0) && !s.is_empty()).then_some(s)
}

/// The user's home: `HOME` first (git and Git Bash on Windows honour it too,
/// and tests set it), else the OS answer (`USERPROFILE` on Windows).
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(std::env::home_dir)
}

/// Writer id from git identity; no email → hostname hash, with a warning.
/// `pub(crate)` — the session-start hook matches `--to` against it.
pub(crate) fn writer(r: &Repo) -> String {
    // one spawn for both keys (last value wins, as `git config <key>`); `hostname` only as the seed
    let cfg = git(
        &r.root,
        &["config", "--get-regexp", r"^user\.(name|email)$"],
    )
    .unwrap_or_default();
    let (mut name, mut email) = (String::new(), None);
    for line in cfg.lines() {
        match line.split_once(' ') {
            Some(("user.name", v)) => name = v.to_string(),
            Some(("user.email", v)) if !v.is_empty() => email = Some(v.to_string()),
            _ => {}
        }
    }
    let host = if email.is_some() {
        String::new()
    } else {
        eprintln!("fael: git user.email is not set — writer id hashes the hostname instead");
        Command::new("hostname")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    core::writer_id(&name, email.as_deref(), &host)
}

/// Writer, branch and sha from git — never asked of the agent.
pub(crate) fn stamp(r: &Repo) -> core::Stamp {
    core::Stamp {
        by: writer(r),
        branch: git(&r.root, &["symbolic-ref", "--short", "-q", "HEAD"]),
        sha: git(&r.root, &["rev-parse", "--short", "HEAD"]),
        session: None,
        shown: None,
    }
}

fn add(a: &Args, kind: &str, text: &str) -> Result<(), String> {
    let r = repo()?;
    let base = match a.has("replace") {
        true if !text.is_empty() => {
            return Err("rejected: --replace takes the text from the row it supersedes — drop the \"<text>\"".into());
        }
        true => Some(amend::base(&r, a, kind)?),
        false => None,
    };
    let text = base.as_ref().map_or(text, |b| b.text.as_str());
    let files = match (a.files(), &base) {
        (f, Some(b)) if f.is_empty() => b.files.clone(),
        (f, _) => f,
    };
    let urgent = match (a.has("urgent"), a.one("urgent-before")) {
        (false, None) => core::Urgent::Unset,
        (true, None) => core::Urgent::End,
        (false, Some(t)) => core::Urgent::Before(t),
        (true, Some(_)) => {
            return Err("rejected: --urgent and --urgent-before pick one — the queue takes a single position".into());
        }
    };
    // bare `--revisit` names no date or text — that only filters on `find`
    let revisit = a.revisit_value()?;
    let opts = write::AddOpts {
        key: a.one("key").or(base.as_ref().and_then(|b| b.key.clone())),
        to: a.one("to"),
        from: a.one("from").or(base.as_ref().and_then(|b| b.from.clone())),
        title: a
            .one("title")
            .or(base.as_ref().and_then(|b| b.title.clone())),
        revisit,
        urgent,
        supersedes: a.one("supersedes"),
        force: a.has("force"),
    };
    // `--dry-run` prints the Verdict the real add would act on and writes
    // nothing — same `prepare` as the real add, so the two can never disagree
    if a.has("dry-run") {
        let (p, _, log) = write::prepare(&r, kind, text, &files, opts)?;
        p.warns.iter().for_each(|w| eprintln!("{w}"));
        println!(
            "{}",
            crate::selfheal::verdict_text(&p.evaluated, a.has("json"), (&log, &p.row))
        );
        return Ok(());
    }
    let (row, path, warns) = write::add_row(&r, kind, text, &files, opts)?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_row_asks("cli", "add", &r.root, &row, &warns);
    batch::written(a, &r, &row, &path);
    // c2: open held rows on the new row's files, verdicts ready — human
    // output after the add result, never a hook line; itself excluded. Under
    // `--json` stdout stays one JSON row per line, so the held text is a
    // warning on stderr instead.
    let held = hook::held_add_text(&r, &row.files, &row.id);
    if a.has("json") {
        eprint!("{held}");
    } else {
        print!("{held}");
    }
    Ok(())
}

/// `fael bump` — same text/files, new `to`/`urgent`/`revisit` (see write::bump).
fn bump(a: &Args, id: &str) -> Result<(), String> {
    a.only(
        "bump",
        &[
            "to",
            "urgent",
            "urgent-before",
            "not-urgent",
            "revisit",
            "json",
        ],
    )?;
    let r = repo()?;
    let (row, path, warns) = write::bump(&r, a, id)?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "bump", Some(&r.root), &warns);
    batch::moved(a, &r, &row, &path, a.has("to"));
    Ok(())
}

/// `fael claim <id>` — this branch holds the issue (see claim::claim).
fn claim(a: &Args, id: &str) -> Result<(), String> {
    a.only("claim", &["json", "force"])?;
    let r = repo()?;
    let (row, path, warns) = claim::claim(&r, id, a.has("force"))?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "claim", Some(&r.root), &warns);
    batch::moved(a, &r, &row, &path, false);
    Ok(())
}

/// `fael next` — claim the best free issue and print it, so an agent starts
/// without a second call (see claim::next).
fn next(a: &Args) -> Result<(), String> {
    a.only("next", &["json"])?;
    let r = repo()?;
    let (row, path, warns) = claim::next(&r, &writer(&r))?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "next", Some(&r.root), &warns);
    batch::moved(a, &r, &row, &path, false);
    if !a.has("json") {
        println!(
            "{}",
            row.text.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    Ok(())
}
