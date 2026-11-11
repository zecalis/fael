//! Calls back into `fael` for what only the ledger's own binary can do: the held-rows text
//! (session state, the said-once outbox) and closing a row. `fael` sets `FAEL_BIN` to itself
//! when it hands a verb over; run bare, `fael` on PATH is the one.

use std::io::Write;
use std::process::{Command, Stdio};

/// `fael <args>` with `stdin` fed in; its stdout on success, its stderr as the error.
/// `FAEL_FROM_CREW` keeps the child out of the usage log: the verb the agent typed is
/// the call that counts, recorded by the `fael` that handed over.
pub(crate) fn run(args: &[&str], stdin: &str) -> Result<String, String> {
    let bin = std::env::var_os("FAEL_BIN")
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "fael".into());
    let mut child = Command::new(&bin)
        .args(args)
        .env("FAEL_FROM_CREW", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("fael-crew: cannot run {}: {e}", bin.to_string_lossy()))?;
    // ignore a write error: the child may exit without reading
    let _ = child
        .stdin
        .take()
        .map(|mut i| i.write_all(stdin.as_bytes()));
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let (so, se) = (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).trim().to_string(),
    );
    match out.status.success() {
        true => {
            if !se.is_empty() {
                eprintln!("{se}");
            }
            Ok(so)
        }
        false => Err(se),
    }
}

/// The held-rows text for a chunk `start` (`"start"`, scope paths) or `done` (`"done"`, the
/// paths the branch touched): contracts and open decisions on those files, exactly as `fael`
/// words them. Empty when there is nothing, or when `fael` cannot be run (a hint, not a gate).
///
/// `start` also takes the chunk's linked issue ids (same `---issues---` section as done):
/// open issues under the scope that are not linked get named, with the link call.
pub(crate) fn held(kind: &str, files: &[String], linked: &[String]) -> String {
    let mut stdin = files.join("\n");
    if !linked.is_empty() {
        stdin.push_str("\n---issues---\n");
        stdin.push_str(&linked.join("\n"));
    }
    run(&["held", kind], &stdin).unwrap_or_default()
}

/// The held-rows text for a chunk `done` (the paths the branch touched), leaving out
/// rows named at or after `since` (the run's start, as stored): this chunk's `start`
/// already showed them. Carries the chunk's linked ledger issue ids to the ledger
/// (e2 done gate): stdin gains a `---issues---` section (see
/// `fael/src/hook/held_done_gate.rs`), and an `Err` means linked issues are still
/// open — refuse `chunk done` with it. The `Ok` text is the hint plus the
/// unlinked-issue warn, printed as before.
pub(crate) fn held_done_checked(
    files: &[String],
    scope: &[String],
    since: Option<&str>,
    linked: &[String],
) -> Result<String, String> {
    let mut stdin = files.join("\n");
    for (marker, ids) in [("---scope---", scope), ("---issues---", linked)] {
        if !ids.is_empty() {
            stdin.push_str(&format!("\n{marker}\n{}", ids.join("\n")));
        }
    }
    let mut args = vec!["held", "done"];
    args.extend(since.into_iter().flat_map(|s| ["--since", s]));
    run(&args, &stdin)
}

/// `fael close <id> "<why>" --force` — the row, closed by the ledger's own
/// write path. `--force` because this is a machine close (a drop's one-word
/// verdict, no cause → fix); the shape gate is for an agent's own close.
pub(crate) fn close(id: &str, why: &str) -> Result<(), String> {
    run(&["close", id, why, "--force"], "").map(|_| ())
}
