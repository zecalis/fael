//! The copyable `chunk done` retype every done reject appends (issue 01M891M06): the whole
//! next call with the uid and the args already passed, the one answer fael cannot know
//! left as a placeholder. fael never types `--left none` or picks a uid for the agent.

/// `fael chunk done <uid> <handoff>` with the flags the caller passed, in a fixed order.
/// A hand-off the shell would expand or split (a newline, a backtick, `$`) rides a quoted
/// heredoc, so pasting the line back cannot run part of it.
pub(crate) fn done_cmd(uid: &str, handoff: &str, flag: impl Fn(&str) -> Option<String>) -> String {
    let q = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
    let heredoc = handoff.contains(['\n', '`', '$']);
    let mut cmd = match heredoc {
        true => format!("fael chunk done {uid} -"),
        false => format!("fael chunk done {uid} {}", q(handoff)),
    };
    for f in ["pr", "out", "review", "left", "no-change"] {
        match flag(f) {
            Some(v) if v.is_empty() => cmd += &format!(" --{f}"),
            Some(v) => cmd += &format!(" --{f} {}", if v.contains(' ') { q(&v) } else { v }),
            None => {}
        }
    }
    if heredoc {
        let end = match handoff.lines().any(|l| l == "EOF") {
            true => "HANDOFF_EOF",
            false => "EOF",
        };
        cmd += &format!(" <<'{end}'\n{handoff}\n{end}");
    }
    cmd
}

/// A `chunk done "<handoff>"` with no uid found no one run to close: the reject names the
/// call per running chunk of this worktree, or the call with `<uid>` open when none runs.
/// Which chunk it closes is the agent's to say.
pub(crate) fn uidless(
    e: String,
    held: &[String],
    handoff: &str,
    flag: impl Fn(&str) -> Option<String>,
) -> String {
    let flag = |f: &str| match f {
        "left" => flag(f).or_else(|| Some("none|<ids>".into())),
        _ => flag(f),
    };
    match held {
        [] => format!(
            "{e}\ntry: {}",
            done_cmd("<uid>", handoff, flag).replacen("<uid>", "<uid of the chunk you started>", 1)
        ),
        many => {
            let lines: Vec<String> = many
                .iter()
                .map(|u| format!("try: {}", done_cmd(u, handoff, flag)))
                .collect();
            format!("{e}\n{}", lines.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(f: &str) -> Option<String> {
        (f == "pr").then(|| "12".to_string())
    }

    #[test]
    fn a_plain_handoff_stays_on_one_quoted_line() {
        assert_eq!(
            done_cmd("C1", "said \"hi\"", flags),
            "fael chunk done C1 \"said \\\"hi\\\"\" --pr 12"
        );
    }

    #[test]
    fn a_handoff_the_shell_would_expand_rides_a_quoted_heredoc() {
        let got = done_cmd("C1", "ran `cargo test`\nall $green", flags);
        assert_eq!(
            got,
            "fael chunk done C1 - --pr 12 <<'EOF'\nran `cargo test`\nall $green\nEOF"
        );
        let got = done_cmd("C1", "a\nEOF\nb", flags);
        assert!(got.ends_with("\nHANDOFF_EOF"), "{got}");
    }

    #[test]
    fn no_uid_names_each_running_chunk_or_leaves_the_uid_open() {
        let e = "rejected: more than one chunk runs".to_string();
        let got = uidless(e.clone(), &["A1".into(), "B2".into()], "did it", flags);
        assert!(
            got.contains("try: fael chunk done A1 \"did it\" --pr 12 --left none|<ids>"),
            "{got}"
        );
        assert!(got.contains("try: fael chunk done B2 \"did it\""), "{got}");
        let got = uidless("rejected: no chunk run".into(), &[], "did it", flags);
        assert!(
            got.ends_with(
                "try: fael chunk done <uid of the chunk you started> \"did it\" --pr 12 --left none|<ids>"
            ),
            "{got}"
        );
    }
}

#[cfg(test)]
mod left_tests {
    /// A missing --left on a multi-line hand-off with a backtick retypes as a heredoc.
    #[test]
    fn a_missing_left_on_a_long_handoff_is_a_pasteable_heredoc() {
        let e = "rejected: chunk done needs --left none or --left <id|uid>,… — how";
        let got = crate::chunk_run::with_retype(e.into(), "C1", "fixed `x`\nmore", |_| None);
        assert!(
            got.ends_with(
                "try: fael chunk done C1 - --left none|<ids> <<'EOF'\nfixed `x`\nmore\nEOF"
            ),
            "{got}"
        );
    }
}
