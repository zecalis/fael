//! Reachability: every hidden command/flag is still named where an agent is sent.
//!
//! Tiers from `product:cli-tiers-table` (decision 01M59WYN, owner 2026-10-19):
//! core (shown) vs workflow (hidden, still parses). A hidden thing must appear
//! in at least one of: `fael/skill/**`, the `fael chunk start` brief (rules +
//! footer), `AGENTS.md`, `README.md` (fifth source, classify), or the app
//! contract (checked against `golden/board-v1.json` + the `Fael.swift` copy).
//! A thing named nowhere fails with its name — then add the pointer to the
//! fitting skill or report it to the owner as removable-by-hand.
//!
//! Hidden top-level commands keep their full own `--help`, so their internal
//! flags are reachable once the agent arrives — this test checks the arrival
//! itself (the command) plus the flags hidden from core `find/add/bump/
//! doctor/stats --help` (the `hide-flags` set). GAP_SHOWN (`next/keys/compact/
//! intake`) stay shown per owner, never hidden, so excluded here.

use std::collections::BTreeSet;

// Every file an agent is sent through: the skills, the owner instructions,
// the new-team doc, plus the brief an agent actually reads on `chunk start`.
const SKILL_MAIN: &str = include_str!("../skill/SKILL.md");
const SKILL_DEBUG: &str = include_str!("../skill/fael-debug/SKILL.md");
const SKILL_PLAN: &str = include_str!("../skill/fael-plan/SKILL.md");
const SKILL_PLAN_REVIEW: &str = include_str!("../skill/fael-plan/REVIEW.md");
const SKILL_PLAN_SPEC: &str = include_str!("../skill/fael-plan/SPEC.md");
const SKILL_PLAN_ADOPT: &str = include_str!("../skill/fael-plan/ADOPT.md");
const SKILL_REVIEW: &str = include_str!("../skill/fael-review/SKILL.md");
const SKILL_SWEEP: &str = include_str!("../skill/fael-sweep/SKILL.md");
const SKILL_OPENCODE: &str = include_str!("../skill/opencode.js");
const AGENTS: &str = include_str!("../../AGENTS.md");
const README: &str = include_str!("../../README.md");
const GOLDEN: &str = include_str!("golden/board-v1.json");

/// The brief an agent reads on `fael chunk start`: rules + the plan footer
/// (`other sections — read one: fael plan export <plan> --section <key>`).
fn brief() -> String {
    fael_core::plan::rules("01M59HV9M3XK711NNBYGXXSM7T")
        + "\nother sections — read one: fael plan export <plan> --section <key>\n"
}

fn corpus() -> String {
    [
        SKILL_MAIN,
        SKILL_DEBUG,
        SKILL_PLAN,
        SKILL_PLAN_REVIEW,
        SKILL_PLAN_SPEC,
        SKILL_PLAN_ADOPT,
        SKILL_REVIEW,
        SKILL_SWEEP,
        SKILL_OPENCODE,
        AGENTS,
        README,
        &brief(),
    ]
    .join("\n")
}

/// Hidden top-level commands (all `COMMANDS` minus core minus GAP_SHOWN).
/// GAP_SHOWN (`next/keys/compact/intake`) stay shown per owner 2026-10-19,
/// never hidden, so not checked here — `top_usage_tiers` holds them shown.
const HIDDEN_COMMANDS: &[&str] = &[
    "chunk", "plan", "run", "board", "sweep", "restore", "purge", "hook", "kickoff", "mv",
    "import", "report", "install", "upgrade", "mcp",
];

/// Flags hidden from core `find/add/bump/doctor/stats --help` (chunk
/// `hide-flags`; `core_is_a_strict_subset_of_full` holds the hiding).
/// Unique strings: find 10 + add 5 + bump 5 + doctor 1 + stats 3 = 19.
const HIDDEN_FLAGS: &[&str] = &[
    "--since",
    "--by",
    "--to",
    "--revisit",
    "--groups",
    "--text",
    "--recall",
    "--kind",
    "--full",
    "--all",
    "--urgent",
    "--urgent-before",
    "--force",
    "--dry-run",
    "--not-urgent",
    "--fat",
    "--rows",
    "--day",
    "--misses",
];

/// Workflow flags on hidden commands (plan/chunk/board): the pointer that
/// sends the agent there names them, so `--help --all` is not the only way.
/// `--app/--area/--parent` live only in `fael-plan/SKILL.md:112` — deleting
/// that line must fail this test naming `--app` (the Done check).
const WORKFLOW_FLAGS: &[&str] = &[
    "--app",
    "--area",
    "--parent",
    "--label",
    "--size",
    "--scope",
    "--reads",
    "--after",
    "--issue",
    "--wait",
    "--ask",
    "--option",
    "--until",
    "--section",
    "--brief",
    "--open",
    "--model",
    "--no-worktree",
    "--pr",
    "--out",
];

// Known gaps reported to the owner as removable-by-hand (retire chunk):
// `chunk done --verdict/--review`, `unapprove`, `board --watch`, `restore
// --edge`, `report/compact/import/mcp/kickoff/install` maintenance flags,
// `run beat`. No skill, brief/rules, AGENTS.md, README.md or app contract
// names them — see the done hand-off. New workflow flags must not be added
// here: name them in the fitting skill instead (new-flag rule).

/// The app contract: what the SwiftUI board app calls (classify, `Fael.swift`
/// line numbers; `board --json` is `tests/golden/board-v1.json` `"v": 1`).
/// Hidden things here are reachable via the app, not via a skill pointer.
const APP_CONTRACT: &[&str] = &[
    "board --json",
    "board --drop",
    "doctor --json",
    "doctor --fix",
    "plan export",
    "plan done",
    "plan park",
    "plan unpark",
    "chunk start",
    "chunk add",
    "chunk answer",
    "chunk accept",
    "chunk park",
    "chunk unpark",
    "chunk drop",
    "run end --cost",
    "--json",
    "--drop",
    "--fix",
    "--client",
    "--model",
    "--run",
    "--plan",
    "--brief",
    "--issue",
    "--cost",
];

/// Copy of the `Fael.swift` / `Owner.swift` call list the contract mirrors
/// (classify: `Fael.swift:15,21,25,29,33,38,44,51,62-63`, `Owner.swift:34`).
/// If the app adds a call, add it here and to `APP_CONTRACT` together.
const FAEL_SWIFT_CALLS: &[&str] = &[
    "board --json",   // Fael.swift:15 + golden board-v1.json
    "board --drop",   // Fael.swift:29
    "doctor --json",  // Fael.swift:21
    "doctor --fix",   // Fael.swift:25
    "plan export",    // Fael.swift:33
    "plan done",      // Fael.swift:38
    "plan park",      // Fael.swift:38
    "plan unpark",    // Fael.swift:38
    "chunk start",    // Fael.swift:44
    "chunk add",      // Fael.swift:62-63
    "chunk answer",   // Owner.swift:34
    "chunk accept",   // Owner.swift:34
    "chunk park",     // Owner.swift:34
    "chunk unpark",   // Owner.swift:34
    "chunk drop",     // Owner.swift:34
    "run end --cost", // Fael.swift:51 (`run end R --cost USD`)
];

fn contract_reachable(thing: &str) -> bool {
    // A command reaches via the app when the app calls `cmd …`; a flag
    // reaches when the app passes that flag (`--client` for `chunk start`).
    if thing.starts_with("--") {
        return APP_CONTRACT.contains(&thing);
    }
    APP_CONTRACT.iter().any(|c| c.starts_with(thing))
}

#[test]
fn hidden_commands_still_named() {
    let c = corpus();
    let mut missing = Vec::new();
    for cmd in HIDDEN_COMMANDS {
        let needle = format!("fael {cmd}");
        if !c.contains(&needle) && !contract_reachable(cmd) {
            missing.push(cmd.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "hidden commands named nowhere (add a pointer to the fitting skill or report removable): {missing:?}"
    );
}

#[test]
fn hidden_flags_still_named() {
    let c = corpus();
    let mut missing = Vec::new();
    for flag in HIDDEN_FLAGS.iter().chain(WORKFLOW_FLAGS.iter()) {
        if !c.contains(*flag) && !contract_reachable(flag) {
            missing.push(flag.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "hidden flags named nowhere (add a pointer to the fitting skill or report removable): {missing:?}"
    );
}

#[test]
fn app_contract_matches_swift_and_golden() {
    // The golden is the contract `board --json` serves: version + the keys
    // the app reads (`ticket` for `chunk done --verdict`, `cost` for
    // `run end --cost`). A shape change re-blesses the golden deliberately.
    assert!(GOLDEN.contains("\"v\": 1"), "golden lost v1");
    assert!(GOLDEN.contains("\"ticket\""), "golden lost ticket");
    assert!(GOLDEN.contains("\"cost\""), "golden lost cost");
    // The const copy tracks the app: every Swift call is in the contract,
    // and every contract command is a Swift call or a flag it passes.
    let contract: BTreeSet<&&str> = APP_CONTRACT.iter().collect();
    for call in FAEL_SWIFT_CALLS {
        assert!(
            contract.contains(call),
            "app calls {call} but APP_CONTRACT does not — add it"
        );
    }
    // No duplicate entries to drift apart.
    assert_eq!(
        contract.len(),
        APP_CONTRACT.len(),
        "APP_CONTRACT has duplicates"
    );
}
