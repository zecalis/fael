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
| 6 auto stages | with `push_policy` unset a repo runs its own `shadow → canary → ramp` machine, rolled back on evidence; `dup` writer | `fael tune` → `push gate` |

A repo that has not earned a stage sees exactly what it always saw: with `push_policy` unset
a repo starts in `shadow` (every push is `baseline@1`, `arm` is `all`, `would_drop` is recorded).

## The policies

A policy is `<id>@<version>`; a released version is never edited (a changed threshold is a
new version).

| Policy | Rule | State |
|---|---|---|
| `baseline@1` | no gate; cut only by the row cap, hub peek and token budget | what `shadow`, `baseline` and a baseline arm run |
| `touch@1` | drop a said row when none of its files was touched earlier in the session, unless it is an issue or a handoff | a repo's candidate under `auto`, or pinned (`push_policy`) |
| `touch-yield@1` | `touch@1`, but keep a row whose engaged share (cited, pulled by the agent, acted on) is ≥ 10% over at least 5 earlier sessions | replay only; cannot be a gate yet (needs the yield cache) |

Only a **gate** cut is a policy's decision. `cap`, `hub_peek` and `budget` are system
limits and are never used to judge a policy.

## Auto stages (chunk 6)

`push_policy` unset means `auto`. A value you set is a **pin** and always wins:
`baseline@1` opts out, `touch@1` is the chunk 5 experiment below.

Each repo (the same repo identity as `fael tune`: the journal all worktrees of a clone share)
has its own stage for `touch@1`:

| Stage | Sessions |
|---|---|
| `shadow` | everyone sees everything; `would_drop` is recorded (the default, nothing changes) |
| `canary` | 10% of sessions run the gate (candidate arm), 90% stay on `baseline@1` |
| `ramp` | 80% candidate, 20% `baseline@1` kept for a continuous comparison |
| `baseline` | rolled back; final for this policy version (it can only re-enter as a new `@version`) |

Moves are by verdict and nothing else: `validated` goes one stage up (`shadow → canary` on
the `touch@1` replay over the repo's shadow pushes — exposure down ≥ 40%, each outcome kept
≥ 85%, at the repo minimums; `canary → ramp` on the arm comparison, which now also reads
`dup`), `not_validated` from any stage rolls back, `insufficient_data` holds. The baseline
arm keeps the wire name `holdout` on usage lines; it is not the permanent holdout, which is
phase 2.

The evaluator runs at session start and Stop (never on the push path), and only once 100
new search pushes of the repo have come in since its last look. A stage change is filed
first as a `policy:push-gate` decision row (`policy`, `from`, `to`, `arm_split`, `reason`,
`validation`); only then does the state file beside the log (`cache/push-gate.json`) move.
Missing, torn or other-version state reads as `shadow`. `fael tune` lists each repo's stage.

`dup` (SPEC §B) is written when a row is filed over one the session was never shown
(the supersede proved the link). Lines from before the writer existed carry none.

## The experiment (chunk 5)

```toml
# .fael/config.toml — a human's pin, per repo
push_policy = "touch@1"   # default unset = auto (stages above)
push_holdout = 20         # percent of sessions kept on baseline@1 while touch@1 is pinned (default 20)
```

- Sessions are split by a hash of the session id, so a session never changes arm.
- **candidate** sessions: on a *search* push (Grep/Bash hit list) the gate removes the rows
  `touch@1` cuts before the row cap fills, and records them as `cut: gate`. A push the
  gate leaves silent is still recorded. Read and edit pushes are never gated.
- **holdout** sessions: `baseline@1`, everything shown, `would_drop` still recorded. They
  are the only place the cost of the gate can be read off, because those agents saw every row.
- Remove the line to hand the repo back to the stages, or set `"baseline@1"` to opt out; nothing else changes.

## Reading `fael tune`

`fael tune [--json] [--since d]` prints the replay tables, then, once any push carries an
`arm`, one **validation** block **per repo** (the repo is the journal all worktrees of a clone
share, else the folder itself — read off `.git`, no git spawn; data is never pooled across
repos, so one repo's verdict borrows nothing from another's):

- arm sizes per stratum (repo × client), with the trigger-mix guard: a gap above 10 points
  marks the stratum `unbalanced` and it is not compared; a stratum under 10 sessions or
  100 search pushes per arm is `insufficient` and reported only
- exposure: rows said per session, candidate against holdout
- retained: cited / pulled / acted kept when the candidate is replayed on holdout rows
  (an upper bound — a cut row may be said at a later push)
- `missed_push` over gate cuts, with its 95% upper bound
- sessions that went back for a cut row, candidate against holdout, and a warning when the
  session view and the per-push view disagree
- one verdict per repo: it needs ≥ 1 eligible stratum, and every eligible client stratum must
  clear exposure / retained / going-back on its own (a passing pool never hides a failing
  client); `missed_push` is judged on the repo pool only

| Verdict | Meaning |
|---|---|
| `validated` | enough data, balanced, and every bar met: exposure down ≥ 40%, each outcome retained ≥ 85%, `missed_push` upper bound ≤ 2% of gate cuts, candidate sessions going back for a cut row no more than 20% above the holdout |
| `not_validated` | enough data, a bar missed — keep `baseline@1`, the reasons are listed |
| `insufficient_data` | not enough sessions/pushes/gate cuts, no usable stratum in the repo, outside the coverage thresholds, or the candidate changed mid-window — this says nothing about the policy; keep collecting |

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

Chunk 6 built the repo-local activation and rollback for `touch@1`. What phase 2 still needs:
a rule interpreter for JSON definitions, the yield cache for `touch-yield`, and a permanent
holdout apart from the stages' baseline arm.

## What to do next

1. Ship a binary with the stages (chunk 6 code). Repos start in `shadow`; nothing changes for agents.
2. Run `fael tune` now and then: its `push gate` lines show each repo's stage, and the
   `policy:push-gate` rows are the history. While a stage says `insufficient_data`, it only collects.
3. To run the chunk 5 experiment by hand instead, pin `push_policy = "touch@1"` in one repo.
4. When it reaches `validated` or `not_validated`, file the decision row with the table
   (`fael add decision`, files `plan:fael-learn-loop`) and close chunk 5 with its id.
   On `validated` a human may open the policy for all sessions (re-bless the replay);
   on `not_validated` stay on `baseline@1` and record which bar missed.
5. Only after `validated`, plan phase 2.

Why a gate and a holdout rather than the replay alone: a replay shows what a rule would have
cut and what those rows earned, but not how agents behave when the rows are really missing.
Only the holdout comparison shows that.
