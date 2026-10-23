//! `fael plan|chunk|board|sweep|run` are fael crew's: `fael-crew` runs them, `fael`
//! only hands the argv over. The child inherits stdin and stdout; its stderr is copied through
//! as it arrives and kept, so a reject it printed is still counted in the usage log here.

use crate::{Args, hook};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

/// The verbs that live in `fael-crew`.
const VERBS: [&str; 5] = ["plan", "chunk", "board", "sweep", "run"];

/// Not found: say how to get it (one line, exit 1).
const MISSING: &str =
    "fael: plan/chunk/board are fael crew — install it: `cargo install --path fael-crew`";

/// The binary: next to this one (a `cargo install`, a test build), else on PATH.
fn locate() -> Option<PathBuf> {
    let name = format!("fael-crew{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(&name)))
        .filter(|p| p.is_file());
    beside.or_else(|| {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|d| d.join(&name))
            .find(|p| p.is_file())
    })
}

/// `Some(exit code)` when `argv` is a crew verb — run, counted, and its exit code returned —
/// else `None` and `fael` carries on.
pub(crate) fn dispatch(argv: &[String]) -> Option<ExitCode> {
    let a = Args::parse(argv.to_vec()).ok()?;
    let cmd = a.pos.first()?.as_str();
    if !VERBS.contains(&cmd) {
        return None;
    }
    let Some(bin) = locate() else {
        // the help text is fael-core's: it reads the same without the binary to run the verb
        if crate::help::is_request(argv) {
            println!("{}", crate::help::for_argv(argv));
            return Some(ExitCode::SUCCESS);
        }
        eprintln!("{MISSING}");
        return Some(ExitCode::FAILURE);
    };
    let (code, err) = match exec(&bin, argv) {
        Ok(done) => done,
        Err(e) => {
            eprintln!("fael: cannot run {}: {e}", bin.display());
            return Some(ExitCode::FAILURE);
        }
    };
    let res = match code {
        0 => Ok(ExitCode::SUCCESS),
        _ => Err(reject_block(&err)),
    };
    hook::record_cli(argv, &res);
    Some(ExitCode::from(code))
}

/// Run it: the exit code (a signal is 1) and everything it wrote to stderr.
fn exec(bin: &PathBuf, argv: &[String]) -> std::io::Result<(u8, String)> {
    let mut cmd = Command::new(bin);
    cmd.args(argv).stderr(Stdio::piped());
    // fael-crew calls back into this binary for the ledger's own writes
    if let Ok(me) = std::env::current_exe() {
        cmd.env("FAEL_BIN", me);
    }
    let mut child = cmd.spawn()?;
    let mut pipe = child.stderr.take().expect("stderr is piped");
    let tee = std::thread::spawn(move || {
        let (mut kept, mut buf) = (Vec::new(), [0u8; 4096]);
        let mut out = std::io::stderr();
        while let Ok(n) = pipe.read(&mut buf) {
            if n == 0 {
                break;
            }
            let _ = out.write_all(&buf[..n]).and_then(|()| out.flush());
            kept.extend_from_slice(&buf[..n]);
        }
        String::from_utf8_lossy(&kept).into_owned()
    });
    let status = child.wait()?;
    let err = tee.join().unwrap_or_default();
    let code = status.code().map_or(1, |c| u8::try_from(c).unwrap_or(1));
    Ok((code, err))
}

/// The error the child ended on: its last `rejected:` line to the end, else all it said.
fn reject_block(err: &str) -> String {
    let err = err.trim_end();
    let at = err
        .match_indices("rejected:")
        .map(|(i, _)| i)
        .filter(|&i| i == 0 || err.as_bytes()[i - 1] == b'\n')
        .last();
    at.map_or(err, |i| &err[i..]).to_string()
}

/// Run a crew verb in `dir` with its output captured: stdout trimmed, or its stderr as the
/// error — what the MCP `chunk` tool returns, so an agent sees what the CLI would print.
pub(crate) fn capture(args: &[String], dir: &std::path::Path) -> Result<String, String> {
    let bin = locate().ok_or(MISSING)?;
    let mut cmd = Command::new(&bin);
    cmd.args(args).current_dir(dir);
    if let Ok(me) = std::env::current_exe() {
        cmd.env("FAEL_BIN", me);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("fael: cannot run {}: {e}", bin.display()))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
}
