//! The table, pair by pair — a guess and the real thing must run the same.

use super::*;

fn rw(s: &str) -> String {
    rewrite(s.split(' ').map(String::from).collect()).join(" ")
}

/// Every pair in the table: the guess and the real thing run the same.
#[test]
fn flags_become_the_real_flag() {
    for (guess, real) in [
        ("find --query x", "find --text x"),
        ("find --query=x", "find --text=x"),
        ("find -q x", "find --text x"),
        ("find --search x", "find --text x"),
        ("find --type issue", "find --kind issue"),
        ("find --file a.rs", "find --files a.rs"),
        ("find --path a.rs", "find --files a.rs"),
        ("find --paths a.rs", "find --files a.rs"),
        ("add note x --file a.rs", "add note x --files a.rs"),
        ("find --tag k", "find --key k"),
        ("add note x --tag k", "add note x --key k"),
        ("find -n 3 --type issue", "find --limit 3 --kind issue"),
        ("find -a", "find --all"),
        ("find -j", "find --json"),
        ("add note x -j", "add note x --json"),
        ("find --id 01ABC", "find 01ABC"),
        ("find --id=01ABC", "find 01ABC"),
    ] {
        assert_eq!(rw(guess), real, "{guess}");
    }
}

#[test]
fn the_text_goes_last() {
    for why in ["--why", "--reason", "--note", "--body", "-m"] {
        assert_eq!(rw(&format!("close 01ABC {why} done")), "close 01ABC done");
        assert_eq!(rw(&format!("close {why} done 01ABC")), "close 01ABC done");
        assert_eq!(
            rw(&format!("close --key k {why} done")),
            "close --key k done"
        );
    }
    for body in ["--body", "--message", "-m"] {
        assert_eq!(
            rw(&format!("add note {body} x --files a.rs")),
            "add note --files a.rs x"
        );
    }
    // nothing after the flag: the parser rejects it, not us
    assert_eq!(rw("close 01ABC --why"), "close 01ABC --why");
}

#[test]
fn commands_become_the_real_command() {
    for (guess, real) in [
        ("decision x --file a.rs", "add decision x --files a.rs"),
        ("issue x", "add issue x"),
        ("note x", "add note x"),
        ("show 01ABC", "find 01ABC"),
        ("get 01ABC", "find 01ABC"),
        ("search x", "find x"),
        ("list --type issue", "find --kind issue"),
        ("ls", "find"),
        ("done 01ABC x", "close 01ABC x"),
        ("resolve 01ABC x", "close 01ABC x"),
        ("new note x", "add note x"),
        ("create note x", "add note x"),
        ("version", "--version"),
    ] {
        assert_eq!(rw(guess), real, "{guess}");
    }
}

#[test]
fn real_names_and_text_are_left_alone() {
    for same in [
        "find --text x --files a.rs",
        "add note x --files a.rs -- --file",
        "find --files a.rs -- -n",
        "add --json -",
        // per command: `--why` means nothing to find, `-n` nothing to add
        "find --why x",
        "add note x -n",
    ] {
        assert_eq!(rw(same), same);
    }
}

#[test]
fn properties_become_the_real_property() {
    let a = |tool, v: Value| mcp(tool, &v);
    assert_eq!(a("find", json!({"query": "x"})), json!({"text": "x"}));
    assert_eq!(
        a("close", json!({"id": "i", "why": "w"})),
        json!({"id": "i", "text": "w"})
    );
    assert_eq!(a("close", json!({"note": "w"})), json!({"text": "w"}));
    assert_eq!(
        a("find", json!({"file": "a.rs"})),
        json!({"files": ["a.rs"]})
    );
    assert_eq!(
        a("add", json!({"kind": "note", "body": "x"})),
        json!({"kind": "note", "text": "x"})
    );
    // the real property wins, the guess is dropped nowhere it matters
    assert_eq!(
        a("find", json!({"text": "t", "query": "q"})),
        json!({"text": "t", "query": "q"})
    );
    // per tool: `why` means nothing to find
    assert_eq!(a("find", json!({"why": "x"})), json!({"why": "x"}));
}

#[test]
fn near_names_the_closest_and_never_picks() {
    assert_eq!(near("find", "-k"), "did you mean --key or --kind?");
    assert_eq!(near("find", "--group"), "did you mean --groups?");
    assert_eq!(
        near("find", "--stale"),
        "open issues: fael find --kind issue"
    );
    assert_eq!(near("find", "--why"), "try 'fael find --help'");
    assert!(near("find", "--status").contains("--all"));
}

/// The table is never advertised: no help section and no schema property names
/// a guess (advertised, it would be one more thing to learn).
#[test]
fn guesses_are_not_advertised() {
    let flags = |s: &str| -> Vec<String> {
        s.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .filter(|w| w.starts_with('-') && w.len() > 1)
            .map(String::from)
            .collect()
    };
    for (cmds, guess, _) in FLAGS {
        for cmd in cmds.split(' ') {
            // neither the core help nor the full `--help --all` advertises it
            for help in [
                crate::help::for_command(cmd).unwrap(),
                crate::help::for_command_full(cmd).unwrap(),
            ] {
                assert!(
                    !flags(help).iter().any(|f| f == guess),
                    "{cmd} help names {guess}"
                );
            }
        }
    }
    let usage = crate::help::usage();
    for (from, _) in COMMANDS {
        assert!(
            crate::help::for_command(from).is_none(),
            "{from} is a command"
        );
        assert!(
            !usage.contains(&format!("fael {from} ")),
            "usage names fael {from}"
        );
    }
    for k in KINDS {
        assert!(
            !usage.contains(&format!("fael {k} ")),
            "usage names fael {k}"
        );
    }
    let schema = crate::schema::tools();
    for tool in schema.as_array().unwrap() {
        let props = tool["inputSchema"]["properties"].as_object().unwrap();
        for (tools, from, _) in PROPS {
            if listed(tools, tool["name"].as_str().unwrap()) {
                assert!(
                    !props.contains_key(*from),
                    "{} schema has {from}",
                    tool["name"]
                );
            }
        }
    }
}

fn argv(s: &str) -> Vec<String> {
    s.split(' ').map(String::from).collect()
}

/// A reject that is not ours, or has no command to read usage from, goes out
/// as it came — never cut, never crashing.
#[test]
fn reject_leaves_what_it_cannot_improve() {
    let flag_first = "rejected: unknown flag --stale — try 'fael --help'";
    assert_eq!(
        reject(&argv("--json find --stale"), flag_first.into()),
        flag_first
    );
    // another module's own "takes no" line, which does not end in our tail
    let auto = "rejected: `upgrade --auto` takes no --dry-run, --client or --replace-fapony";
    assert_eq!(reject(&argv("upgrade --auto --dry-run"), auto.into()), auto);
    // a command with no help section, an empty argv, an error of another kind
    assert_eq!(reject(&argv("zzz --x"), flag_first.into()), flag_first);
    assert_eq!(reject(&[], flag_first.into()), flag_first);
    let other = "rejected: --limit 0 shows nothing";
    assert_eq!(reject(&argv("find --limit 0"), other.into()), other);
    // an unknown command far from every name has no "did you mean"
    let far = "rejected: unknown command \"qqqqqqq\" — try 'fael --help'";
    assert_eq!(reject(&argv("qqqqqqq"), far.into()), far);
    // same input, same answer
    let a = reject(&argv("find --group"), flag_first.into());
    assert_eq!(a, reject(&argv("find --group"), flag_first.into()));
}
