//! Claude client: MCP through the CLI, never `~/.claude.json` by hand.

use super::{Ctx, on_path};
use std::process::Command;

/// The path after `Command:` in `claude mcp get`. Compared whole: a bare `fael`
/// is a substring of every stale npm path, which then read as already set.
fn mcp_command(out: &str) -> Option<&str> {
    out.lines()
        .find_map(|l| l.trim().strip_prefix("Command:"))
        .map(str::trim)
}

pub(crate) fn claude_mcp(c: &Ctx) {
    let add = format!("claude mcp add fael -s user -- {} mcp", c.exe);
    if !on_path("claude") {
        println!("  ! claude CLI not on PATH — add the MCP server yourself: {add}");
        return;
    }
    let get = |name: &str| {
        Command::new("claude")
            .args(["mcp", "get", name])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| {
                String::from_utf8_lossy(&o.stdout).into_owned()
                    + &String::from_utf8_lossy(&o.stderr)
            })
    };
    let remove = "claude mcp remove fael -s user";
    match get("fael") {
        Some(out) if mcp_command(&out) == Some(&c.exe) => println!("  mcp fael already set"),
        // a fael binary there is ours to repoint
        Some(out) if !mcp_command(&out).is_some_and(|v| v.contains("fael")) => println!(
            "  ! an MCP server named fael points elsewhere — left alone; `{remove}` and rerun"
        ),
        Some(_) if c.dry => {
            c.changed.set(c.changed.get() + 1);
            println!("  would run: {remove} && {add}");
        }
        Some(_) => {
            c.changed.set(c.changed.get() + 1);
            let ok = [
                &["mcp", "remove", "fael", "-s", "user"][..],
                &["mcp", "add", "fael", "-s", "user", "--", &c.exe, "mcp"],
            ]
            .iter()
            .all(|a| {
                Command::new("claude")
                    .args(*a)
                    .status()
                    .is_ok_and(|s| s.success())
            });
            println!(
                "  {}",
                if ok {
                    format!("ran: {remove} && {add} (repointed)")
                } else {
                    format!("! failed: {remove} && {add}")
                }
            );
        }
        None if c.dry => {
            c.changed.set(c.changed.get() + 1);
            println!("  would run: {add}");
        }
        None => {
            c.changed.set(c.changed.get() + 1);
            let ok = Command::new("claude")
                .args(["mcp", "add", "fael", "-s", "user", "--", &c.exe, "mcp"])
                .status()
                .is_ok_and(|s| s.success());
            println!(
                "  {}",
                if ok {
                    format!("ran: {add}")
                } else {
                    format!("! failed: {add}")
                }
            );
        }
    }
    if get("fapony").is_some() {
        if !c.replace {
            println!("  ! MCP fapony is still set — --replace-fapony removes it");
        } else if c.dry {
            c.changed.set(c.changed.get() + 1);
            println!("  would run: claude mcp remove fapony -s user");
        } else {
            c.changed.set(c.changed.get() + 1);
            let ok = Command::new("claude")
                .args(["mcp", "remove", "fapony", "-s", "user"])
                .status()
                .is_ok_and(|s| s.success());
            println!(
                "  {}",
                if ok {
                    "ran: claude mcp remove fapony -s user"
                } else {
                    "! failed: claude mcp remove fapony -s user"
                }
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stale_npm_path_is_not_bare_fael() {
        let out = "fael:\n  Command: /x/node_modules/.bin_real/fael\n  Args: mcp\n";
        assert_eq!(
            super::mcp_command(out),
            Some("/x/node_modules/.bin_real/fael")
        );
        assert_ne!(super::mcp_command(out), Some("fael"));
    }
}
