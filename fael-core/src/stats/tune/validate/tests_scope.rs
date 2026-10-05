//! The per-repo verdict through `tune`, on generated usage: what the held-out
//! filter, the per-client bars and the per-repo policy read are only visible
//! with real observations, so these run the whole path.

use crate::Log;
use crate::stats::parse::parse;
use crate::stats::tune::{Tune, tune};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One arm of one client in one repo: `sessions` sessions of 10 search pushes.
struct Arm<'a> {
    /// The worktree root a usage line carries.
    repo: &'a str,
    client: &'a str,
    arm: &'a str,
    policy: &'a str,
    sessions: usize,
    /// First session number, so two arms never share a session id.
    from: usize,
    said: usize,
    touch: u32,
    cuts: usize,
    cite: bool,
}

impl<'a> Arm<'a> {
    fn new(repo: &'a str, client: &'a str, arm: &'a str, sessions: usize) -> Self {
        Arm {
            repo,
            client,
            arm,
            policy: "touch@1",
            sessions,
            from: if arm == "holdout" { 1000 } else { 0 },
            said: 0,
            touch: 1,
            cuts: 0,
            cite: false,
        }
    }

    fn lines(&self) -> String {
        let q = |p: &str, n: usize| {
            (0..n)
                .map(|i| format!("{p}{i}"))
                .map(|s| format!("\"{s}\""))
                .collect::<Vec<_>>()
        };
        let ids = q("R", self.said);
        let feat: Vec<String> = ids
            .iter()
            .map(|i| format!("{i}:{{\"tier\":0,\"hub\":false,\"kind\":\"decision\",\"age_d\":1,\"touch\":{}}}", self.touch))
            .collect();
        let cut: Vec<String> = q("C", self.cuts)
            .iter()
            .map(|i| format!("{{\"id\":{i},\"r\":\"gate\"}}"))
            .collect();
        let mut out = String::new();
        for n in self.from..self.from + self.sessions {
            let s = format!("{}-{}-{}-{n}", self.repo, self.client, self.arm);
            // four local days, evenly: the coverage guard passes
            let day = 5 + n % 4;
            for k in 0..10 {
                out += &format!(
                    "{{\"ts\":\"2026-10-{day:02}T00:{k:02}:00.000Z\",\"repo\":\"{}\",\"client\":\"{}\",\"session\":\"{s}\",\"event\":\"search\",\"trigger\":\"hitlist\",\"arm\":\"{}\",\"policy\":\"{}\",\"files\":[\"a.rs\"],\"ids\":[{}],\"feat\":{{{}}},\"cut\":[{}]}}\n",
                    self.repo,
                    self.client,
                    self.arm,
                    self.policy,
                    ids.join(","),
                    feat.join(","),
                    cut.join(",")
                );
            }
            if self.cite && self.said > 0 {
                out += &format!(
                    "{{\"ts\":\"2026-10-{day:02}T00:30:00.000Z\",\"repo\":\"{}\",\"client\":\"{}\",\"session\":\"{s}\",\"event\":\"outcome\",\"ids\":[],\"cited\":[\"R0\"]}}\n",
                    self.repo, self.client
                );
            }
        }
        out
    }
}

/// Every raw repo has the rows `R0..R4` in its own log.
fn run(arms: &[Arm], scope: &dyn Fn(&str) -> String) -> Tune {
    let text: String = arms.iter().map(Arm::lines).collect();
    let p = parse(
        &text,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let jsonl: String = (0..5)
        .map(|i| format!("{{\"v\":1,\"id\":\"R{i}\",\"ts\":\"2026-10-04T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]}}\n"))
        .collect();
    let logs: HashMap<String, Log> = arms
        .iter()
        .map(|a| {
            let log = Log {
                rows: crate::stats::said::tests::rows(&jsonl),
                ..Log::default()
            };
            (a.repo.to_string(), log)
        })
        .collect();
    tune(&p, &logs, 0, scope)
}

fn ident(r: &str) -> String {
    r.to_string()
}

/// `/w/a1`, `/w/a2` → `/w/a`: worktrees of one clone.
fn clone_of(r: &str) -> String {
    r.trim_end_matches(char::is_numeric).to_string()
}

fn of<'t>(t: &'t Tune, repo: &str) -> &'t super::Validation {
    t.validation
        .iter()
        .find(|v| v.repo == repo)
        .unwrap_or_else(|| panic!("{repo}: {:?}", t.validation))
}

#[test]
fn what_the_gate_forfeits_is_read_from_this_repos_holdout_only() {
    let hold = |repo: &'static str, touch: u32| Arm {
        said: 1,
        touch,
        cite: true,
        ..Arm::new(repo, "claude", "holdout", 5)
    };
    // repo a: two worktrees, its holdout rows are cut by touch@1 (touch 0);
    // repo b: its holdout rows are kept (touch 1) — each cites what it was told
    let arms = [
        hold("/w/a1", 0),
        Arm {
            from: 2000,
            ..hold("/w/a2", 0)
        },
        Arm::new("/w/a1", "claude", "candidate", 5),
        Arm::new("/w/a2", "claude", "candidate", 5),
        Arm {
            from: 3000,
            ..hold("/w/b", 1)
        },
        hold("/w/b", 1),
        Arm::new("/w/b", "claude", "candidate", 10),
    ];
    let t = run(&arms, &clone_of);
    let (a, b) = (of(&t, "/w/a"), of(&t, "/w/b"));
    assert_eq!(
        a.strata[0].holdout.sessions, 10,
        "both worktrees are one stratum"
    );
    let kept = |v: &super::Validation| (v.bars.retained.cited.x, v.bars.retained.cited.n);
    assert_eq!(
        kept(a),
        (0, 10),
        "all ten sessions' cited rows were cut: {a:?}"
    );
    assert_eq!(
        kept(b),
        (10, 10),
        "nothing of a leaks in, nothing of b is lost: {b:?}"
    );
}

#[test]
fn a_client_that_misses_a_bar_fails_the_repo_while_the_pool_passes() {
    let arms = |opencode_said: usize| {
        let (c, o) = ("claude", "opencode");
        vec![
            Arm {
                cuts: 8,
                ..Arm::new("/w/r", c, "candidate", 25)
            },
            Arm {
                said: 5,
                ..Arm::new("/w/r", c, "holdout", 25)
            },
            Arm {
                said: opencode_said,
                cuts: 8,
                ..Arm::new("/w/r", o, "candidate", 10)
            },
            Arm {
                said: 5,
                ..Arm::new("/w/r", o, "holdout", 10)
            },
        ]
    };
    // opencode's candidate still says 4 of the holdout's 5 rows: exposure down
    // 20% there, 77% pooled
    let t = run(&arms(4), &ident);
    let v = of(&t, "/w/r");
    assert!(
        v.bars.exposure_cut_pct.unwrap() >= 40.0,
        "the pool passes: {v:?}"
    );
    assert_eq!(v.verdict.result, "not_validated", "{v:?}");
    assert!(
        v.verdict.why[0].starts_with("client opencode: exposure"),
        "{v:?}"
    );
    // the same repo with opencode cutting too is validated
    let t = run(&arms(0), &ident);
    let v = of(&t, "/w/r");
    assert_eq!(v.verdict.result, "validated", "{v:?}");
}

#[test]
fn a_policy_change_in_one_repo_does_not_make_another_insufficient() {
    let repo = |r: &'static str, policy: &'static str| {
        vec![
            Arm {
                cuts: 8,
                policy,
                ..Arm::new(r, "claude", "candidate", 30)
            },
            Arm {
                said: 5,
                ..Arm::new(r, "claude", "holdout", 30)
            },
        ]
    };
    let mut arms = repo("/w/a", "touch@1");
    arms.extend(repo("/w/b", "touch@1"));
    // b's candidate ran touch@1 and then touch@2
    arms.push(Arm {
        from: 500,
        cuts: 8,
        policy: "touch@2",
        ..Arm::new("/w/b", "claude", "candidate", 5)
    });
    let t = run(&arms, &ident);
    assert_eq!(
        of(&t, "/w/a").verdict.result,
        "validated",
        "{:?}",
        of(&t, "/w/a")
    );
    let b = of(&t, "/w/b");
    assert_eq!(b.candidate, "touch@1,touch@2");
    assert_eq!(b.verdict.result, "insufficient_data", "{b:?}");
}
