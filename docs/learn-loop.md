# The learn loop — how fael checks its own push decisions

fael decides, on every push, which rows the agent sees and which it does not. The
learn loop records those decisions, measures what happened to the rows, and tests a
stricter rule against evidence before it is switched on. It never guesses and never
removes a row from `fael find`. Source plan: `PLAN-fael-learn-loop` (chunks 1–5).

Claims here are limited to what `fael stats` and `fael tune` print. Nothing says
"saves tokens", "saves time" or "saves rounds": fael cannot see the counterfactual.

## What exists today

| Chunk | What it adds | Where you see it |
|---|---|---|
| 1 decision record | each push line in `usage.jsonl` carries `trigger`, `files`, `cut: [{id, r}]`, `feat`, `policy` | `usage.jsonl` |
| 2 outcomes | per row: `cited`, `pulled` (agent-initiated vs fael-induced), `acted`, `retrieved_after_cut`, `missed_push` (`outcomes_v: 1`) | `fael stats --rows` |
| 3 working set | `feat.touch` (files of the row the session already touched) and `would_drop` (rows `touch@1` would cut, recorded only) | `usage.jsonl` |
| 4 `fael tune` | read-only replay of candidate rules beside `baseline@1`; names no winner | `fael tune` |
| 5 holdout + gate | `push_policy` opt-in, session-level holdout, real `gate` cuts, `arm` on every push line, a verdict in `fael tune` | `fael tune` → `validation` |

The default is unchanged: with no `push_policy`, every push is `baseline@1`, `arm` is
`all`, and the agent sees exactly what it saw before.

## The policies

A policy is `<id>@<version>`; a released version is never edited (a changed threshold is a
new version).

| Policy | Rule | State |
|---|---|---|
| `baseline@1` | no gate; cut only by the row cap, hub peek and token budget | the default |
| `touch@1` | drop a said row when none of its files was touched earlier in the session, unless it is an issue or a handoff | can be switched on (`push_policy`) |
| `touch-yield@1` | `touch@1`, but keep a row whose engaged share (cited, pulled by the agent, acted on) is ≥ 10% over at least 5 earlier sessions | replay only; cannot be a gate yet (needs the yield cache) |

Only a **gate** cut is a policy's decision. `cap`, `hub_peek` and `budget` are system
limits and are never used to judge a policy.

## The experiment (chunk 5)

```toml
# .fael/config.toml — a human's opt-in, per repo
push_policy = "touch@1"   # default "baseline@1"
push_holdout = 20         # percent of sessions kept on baseline@1 (default 20)
```

- Sessions are split by a hash of the session id, so a session never changes arm.
- **candidate** sessions: on a *search* push (Grep/Bash hit list) the gate removes the rows
  `touch@1` cuts before the row cap fills, and records them as `cut: gate`. A push the
  gate leaves silent is still recorded. Read and edit pushes are never gated.
- **holdout** sessions: `baseline@1`, everything shown, `would_drop` still recorded. They
  are the only place the cost of the gate can be read off, because those agents saw every row.
- Remove the line (or set `"baseline@1"`) to stop; nothing else changes.

## Reading `fael tune`

`fael tune [--json] [--since d]` prints the replay tables, then, once any push carries an
`arm`, a **validation** block:

- arm sizes per stratum (repo × client), with the trigger-mix guard: a gap above 10 points
  marks the stratum `unbalanced` and it is not compared; a stratum under 10 sessions or
  100 search pushes per arm is `insufficient` and reported only
- exposure: rows said per session, candidate against holdout
- retained: cited / pulled / acted kept when the candidate is replayed on holdout rows
  (an upper bound — a cut row may be said at a later push)
- `missed_push` over gate cuts, with its 95% upper bound
- sessions that went back for a cut row, candidate against holdout, and a warning when the
  session view and the per-push view disagree
- one verdict

| Verdict | Meaning |
|---|---|
| `validated` | enough data, balanced, and every bar met: exposure down ≥ 40%, each outcome retained ≥ 85%, `missed_push` upper bound ≤ 2% of gate cuts, candidate sessions going back for a cut row no more than 20% above the holdout |
| `not_validated` | enough data, a bar missed — keep `baseline@1`, the reasons are listed |
| `insufficient_data` | not enough sessions/pushes/gate cuts, fewer than 2 usable strata, outside the coverage thresholds, or the candidate changed mid-window — this says nothing about the policy; keep collecting |

The thresholds were frozen before any data (decision `01M45DKB4`). `tune` applies them, never
picks them, and writes nothing.

Not measured yet: `dup` (a self-heal link to a row that was cut). The session-level safety
gate rests on `retrieved_after_cut` alone, and `fael tune` says so in a note.

## Can an agent create a policy?

**Not today.** A policy is code: a pinned definition, a pure rule function (`touch_drops`),
a replay branch in `tune`, and an entry in `GATES`. An agent can *propose* one the way any
change is proposed — a PR a human reviews — but fael does not generate, load or activate a
policy at runtime, and `tune` never turns one on.

That is deliberate for this phase. The point of chunk 5 is to test the learning *procedure*
(replay, holdout, verdict) once, with a human opening the policy, before anything is allowed
to open itself.

**Phase 2 (designed, not built; starts only after a `validated` verdict)** lets fael propose
and activate a policy itself, inside fixed bounds:

- cut-only (never adds a row), search push only, a threshold within a declared range
- each policy stored as a full definition (rule + params as JSON) in a decision row
  `policy:push-gate` with `from` → `to` and the `tune` table that justified it, so it can
  always be rebuilt from the log — no policy named `learned` with no definition
- the holdout stays on permanently and is compared with the candidate continuously
- a candidate that does worse than the holdout by the declared bar is rolled back by
  superseding the row to the previous version; `push_policy = "baseline@1"` is the manual
  escape hatch

What phase 2 still needs: a rule interpreter for JSON definitions, the yield cache for
`touch-yield`, a `dup` writer, and the activation / rollback logic. None exists yet.

## What to do next

1. Merge the chunk 5 PR and ship a binary that writes `feat.touch` and `arm`.
2. Set `push_policy = "touch@1"` in at least two strata (for example fael and vela).
   Workload differs between repos; `validated` needs two usable strata.
3. Run `fael tune` now and then. While it says `insufficient_data`, only collect.
4. When it reaches `validated` or `not_validated`, file the decision row with the table
   (`fael add decision`, files `plan:fael-learn-loop`) and close chunk 5 with its id.
   On `validated` a human may open the policy for all sessions (re-bless the replay);
   on `not_validated` stay on `baseline@1` and record which bar missed.
5. Only after `validated`, plan phase 2.

Why a gate and a holdout rather than the replay alone: a replay shows what a rule would have
cut and what those rows earned, but not how agents behave when the rows are really missing.
Only the holdout comparison shows that.
