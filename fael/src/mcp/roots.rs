//! The client's roots (MCP `roots/list`): where the client says the session works. An MCP
//! server is launched in the app's dir, so a worktree session's `chunk start` could not say
//! its seat; a root that differs from the launch dir is news the client gave, a root equal to
//! it adds nothing (issue 01M6BKHZP). Asked only of a client that declared `roots`, again when
//! it says they changed; an answer that never comes just leaves `cwd` required, as before.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// The id of our `roots/list` request — its response is the one reply this server reads.
pub(super) const ASK_ID: &str = "fael-roots";

static DECLARED: AtomicBool = AtomicBool::new(false);
static ROOTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `initialize` params: did the client declare the `roots` capability?
pub(super) fn declare(params: &Value) {
    DECLARED.store(
        params["capabilities"]["roots"].is_object(),
        Ordering::Relaxed,
    );
}

/// The request to send for a notification, if the client can answer it.
pub(super) fn ask_for(method: &str) -> Option<Value> {
    let asked = matches!(
        method,
        "notifications/initialized" | "notifications/roots/list_changed"
    );
    (asked && DECLARED.load(Ordering::Relaxed))
        .then(|| json!({"jsonrpc": "2.0", "id": ASK_ID, "method": "roots/list"}))
}

/// The client's answer: keep its `file://` roots.
pub(super) fn take(msg: &Value) {
    let roots: Vec<PathBuf> = msg["result"]["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["uri"].as_str()?.strip_prefix("file://"))
        .map(PathBuf::from)
        .collect();
    if let Ok(mut g) = ROOTS.lock() {
        *g = roots;
    }
}

/// The one root that is not `launch` (the dir this server started in), if exactly one is.
pub(crate) fn seat(launch: &Path) -> Option<PathBuf> {
    let g = ROOTS.lock().ok()?;
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let launch = canon(launch);
    let other: Vec<&PathBuf> = g.iter().filter(|r| canon(r) != launch).collect();
    match (other.as_slice(), g.len()) {
        ([one], 1) => Some((*one).clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_other_than_the_launch_dir_is_the_seat_and_a_launch_dir_root_is_not() {
        // one test owns the statics
        assert!(
            ask_for("notifications/initialized").is_none(),
            "not declared yet"
        );
        declare(&json!({"capabilities": {"roots": {}}}));
        assert_eq!(
            ask_for("notifications/initialized").unwrap()["method"],
            "roots/list"
        );
        assert!(ask_for("tools/call").is_none());
        let msg = |u: &[&str]| json!({"id": ASK_ID, "result": {"roots": u.iter().map(|u| json!({"uri": u})).collect::<Vec<_>>()}});
        take(&msg(&["file:///w/wt-a"]));
        assert_eq!(seat(Path::new("/w/launch")), Some("/w/wt-a".into()));
        assert_eq!(seat(Path::new("/w/wt-a")), None, "same as launch: no news");
        take(&msg(&["file:///w/wt-a", "file:///w/wt-b"]));
        assert_eq!(
            seat(Path::new("/w/launch")),
            None,
            "two roots: never a guess"
        );
        take(&msg(&[]));
        assert_eq!(seat(Path::new("/w/launch")), None);
    }
}
