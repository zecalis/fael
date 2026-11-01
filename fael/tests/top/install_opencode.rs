//! The generated OpenCode plugin keeps reporting session cost (01M6B4CX):
//! split out of `install.rs` (file-size ratchet) — install's own shape tests
//! stay there, the cost-path pin lives here.

use std::path::Path;
use std::process::Command;

fn install(home: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("install")
        .args(args)
        .env("HOME", home)
        .env("FAEL_STATE_DIR", home.join("state"))
        // fael on PATH (install refuses without it), no claude CLI
        .env(
            "PATH",
            std::env::join_paths([
                Path::new(env!("CARGO_BIN_EXE_fael")).parent().unwrap(),
                Path::new("/usr/bin"),
                Path::new("/bin"),
            ])
            .unwrap(),
        )
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

// Cost rides the messages the stop hook already fetches: a plugin without
// `cost_of_messages` reports nothing and the board stays cost-unknown.
// Pinned here so an edit to `opencode.js` cannot drop it silently.
#[test]
fn installed_opencode_plugin_reports_session_cost() {
    let home = std::env::temp_dir().join(format!("fael-install-cost-{}", fael_core::ulid()));
    install(&home, &["--client", "opencode"]);
    let p = std::fs::read_to_string(home.join(".config/opencode/plugins/fael.js")).unwrap();
    assert!(
        p.contains("cost_of_messages"),
        "plugin reports no session cost: {p}"
    );
}
