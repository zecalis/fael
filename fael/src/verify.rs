//! `fael verify <id>` (hidden): re-run the `cmd:` a row recorded beside a number and print
//! the recorded `result:` next to what it prints now. Reads only — the row is never edited,
//! and fael never says which of the two is right. The command is a shell line, so it runs
//! only for a row this machine's writer filed (a row that came by `sync`/`import` is a
//! teammate's text), for at most `LIMIT`, in the repo root, with no stdin.

use crate::{core, read, repo, writer};
use std::io::Read;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(60);
const CAP: usize = 4000;

/// `key: value` from the row's own lines; the first one wins.
fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|l| {
        l.trim()
            .strip_prefix(key)
            .and_then(|v| v.strip_prefix(':'))
            .map(str::trim)
            .filter(|v| !v.is_empty())
    })
}

/// stdout of `sh -c cmd`, trimmed; `Err` for a command that ran too long or would not start.
fn measure(cmd: &str, root: &std::path::Path) -> Result<(String, i32), String> {
    let mut child = Command::new("sh")
        .args(["-c", cmd])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("verify: cannot run {cmd:?}: {e}"))?;
    let mut out = child.stdout.take().ok_or("verify: no stdout")?;
    // drain on a thread so a chatty command cannot fill the pipe and stall past the limit
    let drain = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.by_ref().take(CAP as u64).read_to_end(&mut buf);
        let _ = std::io::copy(&mut out, &mut std::io::sink());
        buf
    });
    let start = Instant::now();
    let code = loop {
        if let Some(s) = child.try_wait().map_err(|e| format!("verify: {e}"))? {
            break s.code().unwrap_or(-1);
        }
        if start.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "verify: {cmd:?} ran past {}s — killed",
                LIMIT.as_secs()
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let buf = drain.join().unwrap_or_default();
    Ok((String::from_utf8_lossy(&buf).trim().to_string(), code))
}

pub(crate) fn cmd(id: &str) -> Result<ExitCode, String> {
    let r = repo()?;
    let log = read(&r);
    let row = core::resolve_row(&log, id)?;
    let (Some(cmd), Some(was)) = (field(&row.text, "cmd"), field(&row.text, "result")) else {
        return Err(format!(
            "rejected: {} has no 'cmd:' and 'result:' lines — write the command that prints the number and what it printed, one per line",
            row.id
        ));
    };
    if row.by != writer(&r) {
        return Err(format!(
            "rejected: {} was filed by {}, not this machine's writer — verify runs only your own rows; run the command by hand if you trust it",
            row.id, row.by
        ));
    }
    let (now, code) = measure(cmd, &r.root)?;
    println!("{}", row.id);
    println!("cmd:      {cmd}");
    if let Some(d) = field(&row.text, "date") {
        println!("date:     {d}");
    }
    println!("recorded: {was}");
    println!("now:      {now} (exit {code})");
    let same = now == was;
    println!("{}", if same { "same" } else { "differs" });
    Ok(if same {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn field_reads_its_own_line() {
        let t = "223 hits\ncmd: rg -c x | wc -l\ndate: 2026-10-27\nresult: 210";
        assert_eq!(super::field(t, "cmd"), Some("rg -c x | wc -l"));
        assert_eq!(super::field(t, "result"), Some("210"));
        assert_eq!(super::field(t, "date"), Some("2026-10-27"));
        assert_eq!(super::field("cmdx: 1", "cmd"), None);
    }
}
