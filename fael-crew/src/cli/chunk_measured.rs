//! A `measured:` line in the `chunk done` hand-off (issue 01M6BKHZSP): the number a
//! measure-then-decide chunk found, in the observed form ledger rows use — `cmd:` the shell
//! line that prints it, `result:` what it printed, `date:` when. The shape is checked, the
//! date is stamped when left out (fael holds today); what the number means stays the
//! agent's. The follow-up chunk's brief prints the hand-off, so the number travels with it.

/// The hand-off to store: unchanged without a `measured:` line; with one, it must name `cmd:`
/// and `result:`, and gets today's `date:` if it has none.
pub(crate) fn check(handoff: &str) -> Result<String, String> {
    let today = fael_core::rfc3339(fael_core::now_ms())[..10].to_string();
    stamp(handoff, &today)
}

fn stamp(handoff: &str, today: &str) -> Result<String, String> {
    let Some(at) = handoff
        .lines()
        .position(|l| l.trim_start().starts_with("measured:"))
    else {
        return Ok(handoff.to_string());
    };
    let line = handoff.lines().nth(at).unwrap_or_default();
    let missing: Vec<&str> = ["cmd:", "result:"]
        .into_iter()
        .filter(|k| !line.contains(k))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "rejected: the `measured:` line needs {} — try `measured: <what> · cmd: <shell line that prints it> · result: <what it printed>` (fael adds date:)",
            missing.join(" and ")
        ));
    }
    if line.contains("date:") {
        return Ok(handoff.to_string());
    }
    Ok(handoff
        .lines()
        .enumerate()
        .map(|(i, l)| match i == at {
            true => format!("{} · date: {today}", l.trim_end()),
            false => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::stamp as check;

    #[test]
    fn a_measured_line_names_cmd_and_result_and_gets_a_date() {
        assert_eq!(
            check("did it\nleft: none", "2026-11-01").unwrap(),
            "did it\nleft: none"
        );
        let ok = check(
            "measured: sync hits · cmd: bun bench · result: 24/30\nleft: none",
            "2026-11-01",
        )
        .unwrap();
        assert_eq!(
            ok,
            "measured: sync hits · cmd: bun bench · result: 24/30 · date: 2026-11-01\nleft: none"
        );
        let dated = "measured: x · cmd: c · result: 1 · date: 2026-10-30";
        assert_eq!(check(dated, "2026-11-01").unwrap(), dated);
        let e = check("measured: 24/30", "2026-11-01").unwrap_err();
        assert!(
            e.contains("cmd: and result:") && e.starts_with("rejected:"),
            "{e}"
        );
    }
}
