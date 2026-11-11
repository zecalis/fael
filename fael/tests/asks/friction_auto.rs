//! `FAEL_FROM` poller/autosync calls: `who: "auto"`, in `by_flag`, never in
//! `friction`. Split from `friction.rs` (file-size cap).

use super::{all_usage, repo, stats_json};
use std::path::Path;

/// A call with `FAEL_FROM` set is a poller/autosync call: `who: "auto"`,
/// counted in `by_flag` and never in `friction`.
#[test]
fn a_poller_call_is_auto_never_friction() {
    use std::io::Write;
    use std::process::Stdio;
    let d = repo();
    let root = d.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let run = |args: &[&str], from: Option<&str>| {
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_fael"));
        c.args(args)
            .current_dir(&d)
            .env("FAEL_STATE_DIR", root.join("state"))
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("FAEL_SESSION")
            .env_remove("CODEX_THREAD_ID")
            .env_remove("FAEL_FROM_CREW");
        if let Some(f) = from {
            c.env("FAEL_FROM", f);
        } else {
            c.env_remove("FAEL_FROM");
        }
        // inside an agent session, so without the marker this would be friction
        c.env("FAEL_SESSION", "s1");
        let mut child = c
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(b"").unwrap();
        let o = child.wait_with_output().unwrap();
        assert!(
            o.status.success(),
            "{:?}",
            String::from_utf8_lossy(&o.stderr)
        );
    };
    // `doctor --json` is always human without the marker; with it, auto.
    // `autosync` is the value the autosync spawn sets.
    run(&["doctor", "--json"], Some("board"));
    run(&["find", "--files", "src/a.rs"], Some("autosync"));
    let c = calls(&d);
    assert_eq!(c.len(), 2, "{c:?}");
    assert!(c.iter().all(|v| v["who"] == "auto"), "{c:?}");
    let s = stats_json(&d);
    assert_eq!(s["friction"]["calls"], 0, "{s}");
    let flags = s["usage"]["by_flag"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        flags
            .iter()
            .map(|f| f["auto"].as_u64().unwrap_or(0))
            .sum::<u64>()
            >= 2,
        "{flags:?}"
    );
}

fn calls(d: &Path) -> Vec<serde_json::Value> {
    let mut u = all_usage(d);
    u.retain(|v| v["event"] == "call");
    u
}
