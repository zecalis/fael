//! Push policy identity (SPEC-fael-learn-loop §D).

/// A push policy's identity (SPEC-fael-learn-loop §D): `<id>@<version>`. A
/// released version's definition never changes — a different rule or
/// threshold is a new version, and the golden test below breaks if one drifts.
#[derive(Debug, PartialEq, Eq)]
pub struct PolicyDef {
    pub id: &'static str,
    pub version: u32,
    pub rule: &'static str,
}

impl PolicyDef {
    /// `baseline@1` — what a usage line's `policy` carries.
    pub fn name(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

/// Today's push, named: nothing is cut by evidence, only by the row cap, the
/// hub peek (`PUSH_HUB_ROWS` / `PUSH_HUB_PEEK`) and the token budget.
pub const BASELINE: PolicyDef = PolicyDef {
    id: "baseline",
    version: 1,
    rule: "no gate; cut by row cap, hub peek and token budget only",
};

/// Shadow candidate (chunk 3): a said row whose files the session has not
/// touched yet is the low-yield one (23% in-context at edit, against 43–49%
/// once touched). Never applied — a usage line's `would_drop` records what it
/// would have cut, so chunk 4 can replay it against the outcomes.
pub const TOUCH: PolicyDef = PolicyDef {
    id: "touch",
    version: 1,
    rule: "drop a said row when none of its files was touched earlier in the session, unless it is an issue or a handoff",
};

/// Would `touch@1` drop `r` when `touch` of its files were already in the
/// session's working set? Issues and handoffs stay: no yield data backs
/// cutting them yet.
pub fn touch_drops(r: &crate::Row, touch: usize) -> bool {
    touch == 0 && r.kind != "issue" && !r.key.as_deref().is_some_and(|k| k.ends_with(":handoff"))
}

/// Shadow candidate (chunk 4): `touch@1` held back by what the row earned
/// before. A said row is engaged when the agent cited it, pulled it itself or
/// acted on it; its history is the first of (row, file, trigger), (row, file),
/// (row), (class) with enough earlier sessions. The decay window (how many
/// earlier sessions count) is a parameter `tune` sweeps: releasing the policy
/// pins it as a new version.
pub const TOUCH_YIELD: PolicyDef = PolicyDef {
    id: "touch-yield",
    version: 1,
    rule: "drop a said row when touch@1 would, and the engaged share of the first of (row,file,trigger), (row,file), (row), (class) with at least 5 earlier sessions is under 10%; keep it when no level has 5",
};

/// Earlier sessions a history level needs before it speaks.
pub const YIELD_MIN_N: usize = 5;
/// The engaged share (percent) under which a history level calls a row low-yield.
pub const YIELD_FLOOR_PCT: usize = 10;

/// Would `touch-yield@1` drop `r`? `hist` = (engaged, sessions) of the first
/// history level with enough sessions; `None` = no level had enough, and a row
/// with no evidence is kept, never guessed at.
pub fn touch_yield_drops(r: &crate::Row, touch: usize, hist: Option<(usize, usize)>) -> bool {
    touch_drops(r, touch)
        && hist.is_some_and(|(x, n)| n >= YIELD_MIN_N && x * 100 < YIELD_FLOOR_PCT * n)
}

/// A session's arm in the validation experiment (SPEC §E): `holdout` keeps
/// `baseline@1`, `candidate` has the configured gate applied on search pushes,
/// `all` = no experiment (no gate configured, or no session to assign).
pub const ARM_ALL: &str = "all";
pub const ARM_CANDIDATE: &str = "candidate";
pub const ARM_HOLDOUT: &str = "holdout";

/// `push_policy` unset: the repo runs the stage machine (`Stage`). A set value
/// is a human's pin and always wins — `baseline@1` is the opt-out.
pub const AUTO: &str = "auto";

/// The values `push_policy` takes: `auto`, or a policy whose rule is a pure
/// call on one said row and its working-set count, so the push can apply it
/// and `tune` can replay it. `touch-yield@1` waits for the yield cache (§D).
pub const GATES: [&str; 3] = [AUTO, "baseline@1", "touch@1"];

fn session_pct(session: &str) -> u64 {
    let h = session.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    h % 100
}

/// A session's arm and the gate it runs under a pinned `push_policy`. The
/// holdout is by session (a per-push draw would mix inside one seen list):
/// FNV-1a of the session id, mod 100, under `holdout_pct`. Pinned by a test —
/// a session never changes arm.
pub fn arm_of(push_policy: &str, holdout_pct: usize, session: &str) -> (&'static str, bool) {
    if push_policy != TOUCH.name() || session.is_empty() {
        return (ARM_ALL, false);
    }
    if session_pct(session) < holdout_pct as u64 {
        (ARM_HOLDOUT, false)
    } else {
        (ARM_CANDIDATE, true)
    }
}

/// Where a repo's `touch@1` stands under `push_policy = auto` (PLAN-fael-learn-loop
/// chunk 6): `shadow` — every session sees everything, `would_drop` records;
/// `canary` — 10% of sessions run the gate; `ramp` — 80%, the rest stay on
/// `baseline@1` for a continuous comparison; `baseline` — rolled back, and
/// sticky for that policy version (it re-enters only as a new `@version`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Shadow,
    Canary,
    Ramp,
    Baseline,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Stage::Shadow => "shadow",
            Stage::Canary => "canary",
            Stage::Ramp => "ramp",
            Stage::Baseline => "baseline",
        }
    }

    pub fn parse(s: &str) -> Option<Stage> {
        [Stage::Shadow, Stage::Canary, Stage::Ramp, Stage::Baseline]
            .into_iter()
            .find(|g| g.name() == s)
    }

    /// Percent of sessions on the candidate arm; the rest are the baseline arm.
    /// `None` = no experiment, everyone sees the baseline push.
    pub fn candidate_pct(self) -> Option<usize> {
        match self {
            Stage::Canary => Some(10),
            Stage::Ramp => Some(80),
            Stage::Shadow | Stage::Baseline => None,
        }
    }
}

/// A session's arm and gate in a stage. The candidate arm is the sessions
/// whose hash is under the stage's share, so canary's sessions stay candidates
/// when the repo ramps — a session never changes arm by a promotion. The
/// baseline arm keeps the wire name `holdout` (`tune` reads it); it is not the
/// permanent holdout, which is phase 2 (SPEC §E).
pub fn arm_in(stage: Stage, session: &str) -> (&'static str, bool) {
    match stage.candidate_pct() {
        Some(pct) if !session.is_empty() => {
            if session_pct(session) < pct as u64 {
                (ARM_CANDIDATE, true)
            } else {
                (ARM_HOLDOUT, false)
            }
        }
        _ => (ARM_ALL, false),
    }
}

/// The arm and gate under the repo's `push_policy`: a pin wins, `auto` follows
/// the resolved stage.
pub fn arm_for(
    push_policy: &str,
    holdout_pct: usize,
    stage: Stage,
    session: &str,
) -> (&'static str, bool) {
    if push_policy == AUTO {
        arm_in(stage, session)
    } else {
        arm_of(push_policy, holdout_pct, session)
    }
}

/// The stage after a verdict (`validated` / `not_validated` / `insufficient_data`
/// — the shadow replay's in `shadow`, the arms' in `canary` and `ramp`):
/// `validated` moves one stage up, `not_validated` from anywhere is a rollback,
/// `insufficient_data` holds, and `baseline` never moves.
pub fn next_stage(from: Stage, verdict: &str) -> Stage {
    match (from, verdict) {
        (Stage::Baseline, _) => Stage::Baseline,
        (_, "not_validated") => Stage::Baseline,
        (Stage::Shadow, "validated") => Stage::Canary,
        (Stage::Canary, "validated") => Stage::Ramp,
        (s, _) => s,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{PUSH_HUB_PEEK, PUSH_HUB_ROWS};
    use super::*;

    #[test]
    fn baseline_1_is_pinned() {
        assert_eq!(BASELINE.name(), "baseline@1");
        assert_eq!(
            BASELINE.rule,
            "no gate; cut by row cap, hub peek and token budget only"
        );
        // the rule rests on these two: change either and baseline@1 is a lie —
        // release baseline@2 instead
        assert_eq!((PUSH_HUB_ROWS, PUSH_HUB_PEEK), (8, 3));
    }

    #[test]
    fn touch_1_is_pinned() {
        assert_eq!(TOUCH.name(), "touch@1");
        assert_eq!(
            TOUCH.rule,
            "drop a said row when none of its files was touched earlier in the session, unless it is an issue or a handoff"
        );
        let row = |kind: &str, key: Option<&str>| crate::Row {
            kind: kind.into(),
            key: key.map(Into::into),
            ..crate::Row::default()
        };
        assert!(touch_drops(&row("decision", None), 0));
        assert!(!touch_drops(&row("decision", None), 1));
        assert!(!touch_drops(&row("issue", None), 0));
        assert!(!touch_drops(&row("note", Some("plan:x:handoff")), 0));
        assert!(touch_drops(&row("note", Some("plan:x:scope")), 0));
    }

    #[test]
    fn touch_yield_1_is_pinned() {
        assert_eq!(TOUCH_YIELD.name(), "touch-yield@1");
        assert_eq!(
            TOUCH_YIELD.rule,
            "drop a said row when touch@1 would, and the engaged share of the first of (row,file,trigger), (row,file), (row), (class) with at least 5 earlier sessions is under 10%; keep it when no level has 5"
        );
        assert_eq!((YIELD_MIN_N, YIELD_FLOOR_PCT), (5, 10));
        let d = crate::Row {
            kind: "decision".into(),
            ..crate::Row::default()
        };
        assert!(touch_yield_drops(&d, 0, Some((0, 5))));
        assert!(!touch_yield_drops(&d, 0, Some((1, 10)))); // 10% is not under 10%
        assert!(touch_yield_drops(&d, 0, Some((0, 100))));
        assert!(!touch_yield_drops(&d, 0, Some((0, 4))), "too few sessions");
        assert!(!touch_yield_drops(&d, 0, None), "no evidence keeps the row");
        assert!(!touch_yield_drops(&d, 1, Some((0, 50))), "touched stays");
    }

    #[test]
    fn stages_walk_up_on_validated_and_fall_to_baseline_for_good() {
        use Stage::*;
        let n = next_stage;
        assert_eq!(n(Shadow, "validated"), Canary);
        assert_eq!(n(Canary, "validated"), Ramp);
        assert_eq!(n(Ramp, "validated"), Ramp);
        for s in [Shadow, Canary, Ramp] {
            assert_eq!(n(s, "insufficient_data"), s, "holds");
            assert_eq!(n(s, "not_validated"), Baseline, "rolls back from {s:?}");
        }
        for v in ["validated", "not_validated", "insufficient_data"] {
            assert_eq!(n(Baseline, v), Baseline, "rollback is sticky");
        }
        assert_eq!(Stage::parse("ramp"), Some(Ramp));
        assert_eq!(Stage::parse("auto"), None);
    }

    #[test]
    fn a_stage_splits_sessions_and_a_promotion_moves_none_to_baseline() {
        use Stage::*;
        for s in [Shadow, Baseline] {
            assert_eq!(arm_in(s, "s1"), (ARM_ALL, false), "{s:?} gates nothing");
        }
        assert_eq!(
            arm_in(Canary, ""),
            (ARM_ALL, false),
            "no session, no experiment"
        );
        let cand = |st, n: usize| {
            (0..n)
                .filter(|i| arm_in(st, &format!("sess-{i}")).0 == ARM_CANDIDATE)
                .count()
        };
        assert!((140..260).contains(&cand(Canary, 2000)), "~10% of 2000");
        assert!((1500..1700).contains(&cand(Ramp, 2000)), "~80% of 2000");
        // canary's candidates are still candidates at ramp
        for i in 0..500 {
            let s = format!("sess-{i}");
            if arm_in(Canary, &s).1 {
                assert!(arm_in(Ramp, &s).1, "{s}");
            }
        }
        // FNV-1a("s1") % 100 = 29: out of canary, inside ramp
        assert_eq!(arm_in(Canary, "s1"), (ARM_HOLDOUT, false));
        assert_eq!(arm_in(Ramp, "s1"), (ARM_CANDIDATE, true));
    }

    #[test]
    fn a_human_pin_wins_over_the_stage() {
        use Stage::Ramp;
        // baseline@1 opts out, whatever the stage says
        assert_eq!(arm_for("baseline@1", 20, Ramp, "s1"), (ARM_ALL, false));
        // touch@1 is the chunk 5 experiment, its own holdout share
        assert_eq!(
            arm_for("touch@1", 20, Stage::Shadow, "s1"),
            (ARM_CANDIDATE, true)
        );
        assert_eq!(arm_for(AUTO, 20, Ramp, "s1"), arm_in(Ramp, "s1"));
    }

    #[test]
    fn the_holdout_is_by_session_and_never_moves() {
        let arm = arm_of;
        // no gate configured, or no session: no experiment
        assert_eq!(arm("baseline@1", 20, "s1"), (ARM_ALL, false));
        assert_eq!(arm("touch@1", 20, ""), (ARM_ALL, false));
        // FNV-1a("s1") % 100 = 29, FNV-1a("s2") % 100 = 96: pinned
        assert_eq!(arm("touch@1", 29, "s1"), (ARM_CANDIDATE, true));
        assert_eq!(arm("touch@1", 30, "s1"), (ARM_HOLDOUT, false));
        assert_eq!(arm("touch@1", 96, "s2"), (ARM_CANDIDATE, true));
        assert_eq!(arm("touch@1", 97, "s2"), (ARM_HOLDOUT, false));
        assert_eq!(arm("touch@1", 0, "s1"), (ARM_CANDIDATE, true));
        assert_eq!(arm("touch@1", 100, "s1"), (ARM_HOLDOUT, false));
        // about the asked share over many sessions
        let held = (0..2000)
            .map(|i| format!("sess-{i}"))
            .filter(|s| arm("touch@1", 20, s).0 == ARM_HOLDOUT)
            .count();
        assert!((300..500).contains(&held), "{held} of 2000 at 20%");
    }
}
