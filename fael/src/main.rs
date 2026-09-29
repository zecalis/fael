//! fael CLI — add · close · find · keys · kickoff over fael-core.
//! Output for people and agents is one markdown line per row, cut to a token budget;
//! `--json` prints one JSON row per line, uncut, for programs. `fael mcp` serves the same
//! add/close/find over stdio (see mcp.rs).

mod aliases;
mod batch;
mod find;
mod help;
mod hook;
mod install;
mod journal;
mod maintain;
mod mcp;
mod refs;
mod restore;
mod schema;
mod selfheal;
mod session;
mod write;

use fael_core::{self as core, Config, Log};
use std::collections::HashMap;
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
        // chunk 6b: `fael add --json -` reads a JSON array of rows from stdin
        ("add", [dash]) if dash == "-" && a.has("json") => batch::batch_add(&a),
        ("close", rest) if rest.len() >= 2 => {
            batch::batch_close(&a, &rest[..rest.len() - 1], &rest[rest.len() - 1])
        }
        ("bump", [id]) => bump(&a, id).map(|()| ExitCode::SUCCESS),
        ("find", [] | [_]) => find::find(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("keys", [] | [_]) => find::keys(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("kickoff", [] | [_]) => find::kickoff(&a, rest.first()).map(|()| ExitCode::SUCCESS),
        ("mv", [old, new]) => mv(&a, old, new).map(|()| ExitCode::SUCCESS),
        ("restore", [] | [_]) => restore::restore(&repo()?, &a, rest.first().map(String::as_str))
            .map(|()| ExitCode::SUCCESS),
        ("hook", [event]) => Ok(hook::cmd(event, a.one("client"))),
        ("stats", []) => hook::stats(a.has("json"), a.has("rows")).map(|()| ExitCode::SUCCESS),
        ("doctor", []) => maintain::doctor(&a),
        ("compact", []) => maintain::compact(&a),
        ("import", [src]) => maintain::import(&a, src),
        ("mcp", []) => mcp::serve().map(|()| ExitCode::SUCCESS),
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

/// Positionals + `--flag value` / `--flag=value`; `--` ends flags.
pub(crate) struct Args {
    pos: Vec<String>,
    flags: HashMap<String, Vec<String>>,
}

impl Args {
    fn parse(argv: Vec<String>) -> Result<Args, String> {
        let mut a = Args {
            pos: vec![],
            flags: HashMap::new(),
        };
        let mut it = argv.into_iter().peekable();
        while let Some(s) = it.next() {
            if s == "--" {
                a.pos.extend(it.by_ref());
                break;
            }
            let Some(name) = s.strip_prefix("--") else {
                a.pos.push(s);
                continue;
            };
            let (name, inline) = match name.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (name.to_string(), None),
            };
            // `--revisit` takes an optional value: bare on `find` means "any
            // revisit" (`find --revisit`), with a value it narrows (`add` and
            // `find --revisit=<text>`); bare on `add` is rejected there.
            if name == "revisit" {
                let v = match inline {
                    Some(v) => Some(v),
                    None if it.peek().is_some_and(|n| !n.starts_with("--")) => it.next(),
                    None => None,
                };
                a.flags.entry(name).or_default().extend(v);
                continue;
            }
            match name.as_str() {
                "all" | "force" | "json" | "dry-run" | "yes" | "replace-fapony" | "fix"
                | "prune" | "urgent" | "not-urgent" | "full" | "rows" | "branches" | "fat" => {
                    a.flags.entry(name).or_default();
                }
                "files" | "key" | "supersedes" | "kind" | "since" | "by" | "client" | "writer"
                | "before" | "map" | "to" | "title" | "urgent-before" | "limit" | "offset"
                | "text" | "edge" => {
                    let v = inline
                        .or_else(|| it.next())
                        .ok_or(format!("--{name} needs a value"))?;
                    a.flags.entry(name).or_default().push(v);
                }
                _ => {
                    return Err(format!(
                        "rejected: unknown flag --{name} — try 'fael --help'"
                    ));
                }
            }
        }
        Ok(a)
    }

    pub(crate) fn has(&self, f: &str) -> bool {
        self.flags.contains_key(f)
    }

    pub(crate) fn one(&self, f: &str) -> Option<String> {
        self.flags.get(f).and_then(|v| v.last()).cloned()
    }

    /// `--map a=b --map c=d` → all values, in order.
    fn many(&self, f: &str) -> Vec<String> {
        self.flags.get(f).cloned().unwrap_or_default()
    }

    /// `--files a,b --files c` → [a, b, c]
    pub(crate) fn files(&self) -> Vec<String> {
        self.flags
            .get("files")
            .into_iter()
            .flatten()
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect()
    }

    /// `--limit N` / `--offset M` for pull paging (chunk 5): at most N ranked
    /// rows, skipping M first. Offset without limit pages budget cuts too.
    pub(crate) fn paging(&self) -> Result<(Option<usize>, usize), String> {
        let num = |f: &str| match self.one(f) {
            None => Ok(None),
            Some(v) => v
                .parse::<usize>()
                .map(Some)
                .map_err(|_| format!("rejected: --{f} needs a number — got {v:?}")),
        };
        let limit = num("limit")?;
        if limit == Some(0) {
            return Err("rejected: --limit 0 shows nothing — drop it or give 1 or more".into());
        }
        Ok((limit, num("offset")?.unwrap_or(0)))
    }
}

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
    let fael = root.join(".fael");
    let cfg = config(&fael.join("config.toml"))?;
    let journal = journal::root(&root);
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
    let name = git(&r.root, &["config", "user.name"]).unwrap_or_default();
    let email = git(&r.root, &["config", "user.email"]);
    let host = Command::new("hostname")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    if email.is_none() {
        eprintln!("fael: git user.email is not set — writer id hashes the hostname instead");
    }
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
        key: a.one("key"),
        to: a.one("to"),
        title: a.one("title"),
        revisit,
        urgent,
        supersedes: a.one("supersedes"),
        force: a.has("force"),
    };
    // `--dry-run` prints the Verdict the real add would act on and writes
    // nothing — same `prepare` as the real add, so the two can never disagree
    if a.has("dry-run") {
        let (p, _, _) = write::prepare(&r, kind, text, &a.files(), opts)?;
        p.warns.iter().for_each(|w| eprintln!("{w}"));
        println!(
            "{}",
            crate::selfheal::verdict_text(&p.evaluated, a.has("json"))
        );
        return Ok(());
    }
    let (row, path, warns) = write::add_row(&r, kind, text, &a.files(), opts)?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "add", Some(&r.root), &warns);
    batch::written(a, &r, &row, &path);
    Ok(())
}

/// `fael bump` — same text/files, new `to`/`urgent`/`revisit` (see write::bump).
fn bump(a: &Args, id: &str) -> Result<(), String> {
    let r = repo()?;
    let (row, path, warns) = write::bump(&r, a, id)?;
    warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "bump", Some(&r.root), &warns);
    batch::written(a, &r, &row, &path);
    Ok(())
}

/// Record that `old` moved to `new` — for what git can't see (anchors,
/// uncommitted rewrites, repos without git). Appends an alias row; the log
/// stays append-only, nothing is rewritten.
fn mv(a: &Args, old: &str, new: &str) -> Result<(), String> {
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
