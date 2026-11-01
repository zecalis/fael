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
        .filter_map(|r| path_of(r["uri"].as_str()?))
        .collect();
    if let Ok(mut g) = ROOTS.lock() {
        *g = roots;
    }
}

/// `file:///a/My%20Project` → `/a/My Project`; `file:///C:/x` → `C:/x`. Not a `file://` uri,
/// or no directory there: None, so a root fael cannot use never replaces the `cwd` ask.
fn path_of(uri: &str) -> Option<PathBuf> {
    let raw = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(raw.len());
    let mut it = raw.bytes();
    while let Some(b) = it.next() {
        if b != b'%' {
            bytes.push(b);
            continue;
        }
        let hex: Vec<u8> = it.by_ref().take(2).collect();
        bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).ok()?, 16).ok()?);
    }
    let path = String::from_utf8(bytes).ok()?;
    // a Windows drive path keeps no leading slash
    let path = match path.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => path[1..].to_string(),
        _ => path,
    };
    let p = PathBuf::from(path);
    p.is_dir().then_some(p)
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
        let t = std::env::temp_dir().join(format!("fael-roots-{}", std::process::id()));
        // a space in the path arrives percent-encoded
        let (a, b, launch) = (t.join("wt a"), t.join("wt-b"), t.join("launch"));
        for d in [&a, &b, &launch] {
            std::fs::create_dir_all(d).unwrap();
        }
        let uri = |p: &Path| p.to_str().unwrap().replace(' ', "%20");
        let msg = |u: &[String]| json!({"id": ASK_ID, "result": {"roots": u.iter().map(|u| json!({"uri": format!("file://{u}")})).collect::<Vec<_>>()}});
        take(&msg(&[uri(&a)]));
        assert_eq!(
            seat(&launch).map(|p| p.canonicalize().unwrap()),
            Some(a.canonicalize().unwrap())
        );
        assert_eq!(seat(&a), None, "same as launch: no news");
        take(&msg(&[uri(&a), uri(&b)]));
        assert_eq!(seat(&launch), None, "two roots: never a guess");
        take(&msg(&[uri(&t.join("gone"))]));
        assert_eq!(
            seat(&launch),
            None,
            "a root that is no directory is no seat"
        );
        take(&msg(&[]));
        assert_eq!(seat(&launch), None);
        let _ = std::fs::remove_dir_all(&t);
    }
}
