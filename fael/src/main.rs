//! fael CLI — add · close · find · keys · kickoff over fael-core.
//! Output for people and agents is one markdown line per row, cut to a token budget;
//! `--json` prints one JSON row per line, uncut, for programs. `fael mcp` serves the same
//! add/close/find over stdio (see mcp.rs).

mod aliases;
mod amend;
mod args;
mod batch;
mod claim;
mod close_key;
mod find;
mod help;
mod hook;
mod install;
mod journal;
mod maintain;
mod mcp;
mod migrate;
mod purge;
mod refs;
mod report;
mod restore;
mod schema;
mod selfheal;
mod session;
mod sync;
mod write;

use fael_core::{self as core, Config, Log};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // usage event for rejects, so they join their command's warnings
    let event = argv
        .first()
        .filter(|c| c.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-'))
        .map(String::as_str)
        .unwrap_or("cli")
        .to_string();
    match run(argv) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            // every reject costs the agent a round — one choke point, so no
            // command records its own rejects and nothing double-counts
            hook::record_cli_reject(&event, &e);
            ExitCode::FAILURE
        }
    }
}

fn run(argv: Vec<String>) -> Result<ExitCode, String> {
    if matches!(argv.as_slice(), [v] if v == "--version" || v == "-v" || v == "-V") {
        println!("fael {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    // `fael help`, `fael --help`, `fael -h`, `fael <cmd> --help` — usage on
    // stdout, exit 0; with a command it shows only that command's section
    // after `--` it is text, not a flag: `fael add note --files a.rs -- -h`
    if argv.first().is_some_and(|c| c == "help")
        || argv
            .iter()
            .take_while(|x| *x != "--")
            .any(|x| x == "--help" || x == "-h")
    {
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
        ("find", [] | [_]) => find::find(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("find", ids) => find::many::find_many(&a, ids).map(|()| ExitCode::SUCCESS),
        ("keys", [] | [_]) => find::keys(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("kickoff", [] | [_]) => find::kickoff(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("mv", [old, new]) => mv(&a, old, new).map(|()| ExitCode::SUCCESS),
        ("restore", [] | [_]) => restore::restore(&repo()?, &a, rest.first().map(String::as_str))
            .map(|()| ExitCode::SUCCESS),
        ("purge", [id]) => purge::purge(&repo()?, &a, id).map(|()| ExitCode::SUCCESS),
        ("migrate", [to]) if to == "local" => migrate::local(&repo()?).map(|()| ExitCode::SUCCESS),
        ("hook", [event]) => Ok(hook::cmd(event, a.one("client"))),
        ("stats", []) if a.has("misses") => {
            find::misses::print_recent(20);
            Ok(ExitCode::SUCCESS)
        }
        ("stats", []) => hook::stats(
            a.has("json"),
            a.has("rows"),
            a.has("day"),
            report::since(&a)?,
        )
        .map(|()| ExitCode::SUCCESS),
        ("report", []) => report::report(&a).map(|()| ExitCode::SUCCESS),
        ("doctor", []) => maintain::doctor(&a),
        ("compact", []) => maintain::compact(&a),
        ("import", [src]) => maintain::import(&a, src),
        ("sync", []) => sync::sync(&repo()?, &a).map(|()| ExitCode::SUCCESS),
        ("mcp", []) => mcp::serve(a.has("pin")).map(|()| ExitCode::SUCCESS),
        ("install" | "upgrade" | "update", []) => {
            // `install` keeps writing without a question; upgrade/update ask first
            let ask = cmd != "install" && !a.has("yes");
            install::cmd(
                a.one("client"),
                a.has("dry-run"),
                a.has("replace-fapony"),
                ask,
            )
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

pub(crate) use args::Args;

pub(crate) struct Repo {
    pub(crate) root: PathBuf,
    pub(crate) cwd: PathBuf,
    pub(crate) fael: PathBuf,
    pub(crate) cfg: Config,
    /// Clone-shared journal (`<git-common-dir>/fael`); `None` without git.
    pub(crate) journal: Option<PathBuf>,
}

pub(crate) fn repo() -> Result<Repo, String> {
    let cwd = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| format!("cwd: {e}"))?;
    repo_at(&cwd)
}

/// The same resolution from an explicit directory — what hooks pass.
pub(crate) fn repo_at(cwd: &Path) -> Result<Repo, String> {
    // normalize_files is lexical, so root must be symlink-resolved like cwd
    let cwd = cwd.canonicalize().map_err(|e| format!("cwd: {e}"))?;
    // ponytail: walk up for `.git` (dir, or file for worktree/submodule) instead
    // of spawning `git rev-parse` — that spawn was ~13 of a hook's ~15 ms.
    // Ignores GIT_DIR/GIT_WORK_TREE/core.worktree; spawn git if those matter.
    let find = |name: &str| {
        cwd.ancestors()
            .find(|d| d.join(name).exists())
            .map(Path::to_path_buf)
    };
    let root = find(".git")
        .or_else(|| find(".fael"))
        .unwrap_or_else(|| cwd.clone());
    // FAEL_DIR: scratch log (tree only, no journal) — see CONTRIBUTING.
    let scratch = std::env::var_os("FAEL_DIR").filter(|d| !d.is_empty());
    let journal = scratch.is_none().then(|| journal::root(&root)).flatten();
    let fael = scratch.map_or_else(|| root.join(".fael"), PathBuf::from);
    let mut cfg = config(&fael.join("config.toml"))?;
    // unset store is `local` everywhere: a tree log from before stays read
    // (the union, tree wins on ids) as frozen history, never written again —
    // only an explicit `store = "tracked"` asks for rows to commit (01M3YQT4)
    if !cfg.store_set {
        cfg.store = core::Store::Local;
    }
    Ok(Repo {
        root,
        cwd,
        fael,
        cfg,
        journal,
    })
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (o.status.success() && !s.is_empty()).then_some(s)
}

/// `.fael/config.toml` — missing file = defaults, a broken file = error.
/// The user's home: `HOME` first (git and Git Bash on Windows honour it too,
/// and tests set it), else the OS answer (`USERPROFILE` on Windows).
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(std::env::home_dir)
}

fn config(path: &Path) -> Result<Config, String> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return Ok(Config::default());
    };
    Config::from_toml(&s).map_err(|e| format!("{}: {e}", path.display()))
}

/// Read the log — tree + journal union, tree wins on duplicate ids; skipped
/// lines go to stderr as one summary, never fail the command.
pub(crate) fn read(r: &Repo) -> Log {
    journal::merged(r)
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
fn stamp(r: &Repo) -> core::Stamp {
    core::Stamp {
        by: writer(r),
        branch: git(&r.root, &["symbolic-ref", "--short", "-q", "HEAD"]),
        sha: git(&r.root, &["rev-parse", "--short", "HEAD"]),
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
    let revisit = write::parse_revisit(a.has("revisit"), a.one("revisit"))?;
    let opts = write::AddOpts {
        key: a.one("key").or(base.as_ref().and_then(|b| b.key.clone())),
        to: a.one("to"),
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
        let (p, _, _) = write::prepare(&r, kind, text, &files, opts)?;
        p.warns.iter().for_each(|w| eprintln!("{w}"));
        println!(
            "{}",
            crate::selfheal::verdict_text(&p.evaluated, a.has("json"))
        );
        return Ok(());
    }
    let (row, path, warns) = write::add_row(&r, kind, text, &files, opts)?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_row_asks("cli", "add", &r.root, &row, &warns);
    batch::written(a, &r, &row, &path);
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
    batch::written(a, &r, &row, &path);
    Ok(())
}

/// `fael claim <id>` — this branch holds the issue (see claim::claim).
fn claim(a: &Args, id: &str) -> Result<(), String> {
    a.only("claim", &["json", "force"])?;
    let r = repo()?;
    let (row, path, warns) = claim::claim(&r, id, a.has("force"))?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "claim", Some(&r.root), &warns);
    batch::written(a, &r, &row, &path);
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
    batch::written(a, &r, &row, &path);
    if !a.has("json") {
        println!(
            "{}",
            row.text.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    Ok(())
}

/// Record that `old` moved to `new` — for what git can't see (anchors,
/// uncommitted rewrites, repos without git). Appends an alias row; the log
/// stays append-only, nothing is rewritten.
fn mv(a: &Args, old: &str, new: &str) -> Result<(), String> {
    a.only("mv", &["json"])?;
    let r = repo()?;
    let norm = core::normalize_files(&[old.to_string(), new.to_string()], &r.cwd, &r.root)?;
    let (from, to) = (&norm[0], &norm[1]);
    if from == to {
        return Err(format!(
            "rejected: {from:?} is already itself — `fael mv` needs two different paths"
        ));
    }
    let log = read(&r);
    if core::Aliases::from_log(&log).forward(from).contains(to) {
        return Err(format!(
            "rejected: {from} → {to} is already recorded — `fael find --files {to}` shows the rows"
        ));
    }
    let (row, _, warns) =
        core::mv_row(&r.fael, r.journal.as_deref(), &r.cfg, &stamp(&r), from, to)?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "mv", Some(&r.root), &warns);
    if a.has("json") {
        println!("{}", row.to_line());
    } else {
        println!("{} → {from} → {to}", row.id);
    }
    Ok(())
}
