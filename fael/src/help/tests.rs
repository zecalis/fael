//! Help unit tests: requests, core list (cli-tiers), docs parity, core surface.

/// `--files`, `--revisit` … named anywhere in `s`.
fn flags(s: &str) -> std::collections::BTreeSet<&str> {
    s.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .filter(|w| w.starts_with("--") && w.len() > 2)
        .collect()
}

#[test]
fn help_is_a_request_before_dashdash_only() {
    let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    for yes in ["help", "help add", "--help", "find --help", "add -h"] {
        assert!(super::is_request(&argv(yes)), "{yes}");
    }
    for no in [
        "find x",
        "add note x -- -h",
        "add note x -- --help",
        "find --files a.rs",
    ] {
        assert!(!super::is_request(&argv(no)), "{no}");
    }
}

/// A misspelled or renamed CORE/GAP entry would silently drop that command
/// from the default list.
#[test]
fn core_names_are_commands() {
    for c in super::top::CORE.iter().chain(super::top::GAP_SHOWN) {
        assert!(
            super::COMMANDS.iter().any(|(n, ..)| n == c),
            "CORE names {c}, which is not in COMMANDS"
        );
    }
}

/// `usage()` lists the cli-tiers core plus the unguided GAP; `usage_all()` lists everything.
#[test]
fn top_usage_tiers() {
    let core = super::usage();
    for present in ["add", "find", "close", "bump", "claim", "doctor", "stats"] {
        assert!(core.contains(&format!("  {present}")), "{present} missing");
    }
    // tier GAP stays shown in default help (owner 2026-10-19) — never hidden
    for present in ["next", "keys", "compact", "intake"] {
        assert!(core.contains(&format!("  {present}")), "{present} missing");
    }
    for hidden in ["chunk", "plan", "board", "kickoff", "install"] {
        assert!(
            !core.lines().any(|l| l.trim_start().starts_with(hidden)),
            "{hidden} leaks into core usage"
        );
    }
    assert!(
        !core.contains("more commands:"),
        "core usage has a rest section"
    );
    assert!(
        core.contains("fael --help --all"),
        "core usage lacks the --all line"
    );
    // core examples never name a hidden command
    for hidden in ["kickoff", "chunk", "plan", "board", "install", "mv "] {
        assert!(
            !core
                .split_once("examples:")
                .unwrap_or_default()
                .1
                .contains(hidden),
            "core examples name {hidden}"
        );
    }
    let all = super::usage_all();
    for present in ["chunk", "plan", "board"] {
        assert!(all.contains(&format!("  {present}")), "{present} missing");
    }
    assert!(
        all.contains("more commands:"),
        "full usage lost its rest section"
    );
    // --all with no command named returns the full usage, not the core
    let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    assert_eq!(super::for_argv(&argv("--help --all")), all);
    assert_eq!(super::for_argv(&argv("--help")), core);
}

/// docs/architecture.md's CLI table is prose around the same synopses —
/// each command's row must name exactly the flags its full `--help`
/// (i.e. `--help --all`) names. The core surface hides flags from agents,
/// never from the docs.
#[test]
fn docs_match_flags() {
    let docs = include_str!("../../../docs/architecture.md");
    for (name, _, _) in super::COMMANDS {
        let row = docs
            .lines()
            .find(|l| l.starts_with(&format!("| `fael {name}")))
            .unwrap_or_else(|| panic!("docs/architecture.md has no row for fael {name}"));
        let synopsis = row.split("` |").next().unwrap_or_default();
        let help = super::for_command_full(name)
            .unwrap()
            .lines()
            .next()
            .unwrap_or_default();
        assert_eq!(flags(synopsis), flags(help), "fael {name}: docs vs --help");
    }
}

/// Chunk 3: the core is a strict subset of the full surface — every flag the
/// core names the full names too, and the hidden ones name nothing in core.
#[test]
fn core_is_a_strict_subset_of_full() {
    for (cmd, hidden) in [
        (
            "find",
            ["--since", "--by", "--to", "--revisit", "--groups"].as_slice(),
        ),
        (
            "add",
            ["--urgent-before", "--force", "--dry-run"].as_slice(),
        ),
    ] {
        let core = super::for_command(cmd).unwrap();
        let full = super::for_command_full(cmd).unwrap();
        assert_ne!(core, full, "{cmd}: core must differ from full");
        let (cf, ff) = (flags(core), flags(full));
        for f in &cf {
            assert!(ff.contains(f), "{cmd}: core names {f}, full does not");
        }
        for h in hidden {
            assert!(!cf.contains(h), "{cmd}: core still names {h}");
            assert!(ff.contains(h), "{cmd}: full lost {h}");
        }
        // the synopsis line is what rejects quote — it must stay the core
        let core_line = core.lines().next().unwrap_or_default();
        for h in hidden {
            assert!(
                !core_line.split(' ').any(|w| w == *h),
                "{cmd}: synopsis still names {h}"
            );
        }
    }
    // commands without a core override show their full section as-is
    for cmd in ["close", "kickoff"] {
        assert_eq!(
            super::for_command(cmd),
            super::for_command_full(cmd),
            "{cmd}"
        );
    }
}

/// `--all` rides along with the help request: `fael <cmd> --help --all`
/// (or `fael help <cmd> --all`) prints the full section, plain `--help`
/// the core.
#[test]
fn all_flag_shows_the_full_section() {
    let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    for cmd in ["find", "add"] {
        let core = super::for_argv(&argv(&format!("{cmd} --help")));
        assert_eq!(
            core.lines().next(),
            super::for_command(cmd).unwrap().lines().next()
        );
        for args in [format!("{cmd} --help --all"), format!("help {cmd} --all")] {
            let full = super::for_argv(&argv(&args));
            assert!(
                full.starts_with(
                    super::for_command_full(cmd)
                        .unwrap()
                        .lines()
                        .next()
                        .unwrap()
                ),
                "{args}"
            );
        }
    }
}

/// verb-help: `fael chunk approve --help` names only approve in <=6 lines.
#[test]
fn verb_help_shows_only_that_verb() {
    let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    let out = super::for_argv(&argv("chunk approve --help"));
    assert!(out.lines().count() <= 6, "got:\n{out}");
    assert!(out.contains("approve"), "got:\n{out}");
    assert!(out.contains("eg:"), "got:\n{out}");
    assert!(
        out.lines().last().unwrap_or_default() == "all verbs: fael chunk --help",
        "got:\n{out}"
    );
    // the shared-line neighbour stays out: approve help is not unapprove help
    assert!(
        !super::for_argv(&argv("chunk approve --help"))
            .lines()
            .next()
            .unwrap_or_default()
            .contains("unapprove"),
        "got:\n{out}"
    );
}

/// `fael chunk --help` is unchanged by verb help.
#[test]
fn command_help_is_unchanged() {
    let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    let out = super::for_argv(&argv("chunk --help"));
    assert!(!out.contains("all verbs:"), "got:\n{out}");
    assert!(out.contains("fael chunk add"), "got:\n{out}");
    assert!(out.contains("fael chunk approve"), "got:\n{out}");
}

/// An unknown verb falls back to the full command section.
#[test]
fn unknown_verb_falls_back_to_full() {
    let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    assert_eq!(
        super::for_argv(&argv("chunk frobnicate --help")),
        super::for_argv(&argv("chunk --help")),
    );
}

/// `--all` means the full section, even with a verb present: verb help must
/// not swallow it.
#[test]
fn all_flag_beats_verb_help() {
    let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    let full = super::for_argv(&argv("chunk --help"));
    for args in ["chunk approve --help --all", "help chunk approve --all"] {
        assert_eq!(super::for_argv(&argv(args)), full, "{args}");
    }
}
