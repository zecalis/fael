//! Help unit tests: requests, week-one list, docs parity, core surface.

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

/// A misspelled or renamed CORE entry would silently drop that command
/// from the "commands:" list.
#[test]
fn core_names_are_commands() {
    for c in super::CORE {
        assert!(
            super::COMMANDS.iter().any(|(n, ..)| n == c),
            "CORE names {c}, which is not in COMMANDS"
        );
    }
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
            [
                "--since",
                "--by",
                "--to",
                "--revisit",
                "--branches",
                "--groups",
            ]
            .as_slice(),
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
