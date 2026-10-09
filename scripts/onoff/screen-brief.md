# Screen brief — PLAN-fael-onoff chunk 2 (read fully before starting)

The one set of criteria every cohort is screened with. Change it only through a new
plan decision: a cohort screened under different criteria cannot join the task set.

You are pre-screening candidate tasks for a controlled experiment. Your output is a recommendation; the repo owner makes the final call. READ-ONLY on every repo: never modify, checkout, switch, stash or commit. Use only `git -C <repo> show/log/grep/diff/cat-file/merge-base` and `gh pr view` / `gh issue view`.

## The experiment
We test whether a new coding agent that receives a repo's recorded experience (closed issues: cause → fix; active decisions) at the moment it edits a file makes fewer repeat mistakes than a new agent without it. Each task replays a real historical commit C: the agent gets the repo at C's parent (C^) plus a written task prompt, and must implement the change. The "experience row" is a ledger row that existed before C^ and names a file C touches. The agent's diff is graded with a rubric: did it repeat the closed issue's failure (`repeat_failure`) or contradict the active decision (`decision_violation`)?

## Inputs
- Sheet: `<out-dir>/cohort-<n>/screen.md` — candidates are the `## N.` headings. Each has PR number, subject, `C^ → C` short shas, and rows (`<full row id> <closed_issue|active_decision> — <title> (<overlapping files>)`).
- Full row text: `<out-dir>/cohort-<n>/candidates.jsonl` (one JSON per candidate; match on `sha` prefix; `rows[].text`).
- Repo: the repo the cohort was mined from (vela: /Users/delamind/Project/zecalis/zecalis, GitHub zecalis/workspace).
- Useful: `git -C <repo> show C --stat`, `git -C <repo> show C -- <file>`, `git -C <repo> show C^:<path>`, `git -C <repo> grep -n <pattern> C^ -- <path>`, `gh pr view <N> --repo zecalis/workspace --json title,body`.

## Decide per candidate: pass, or cut with ONE reason
Pick the single best row and judge it:
- **cut a** — at C^ the code, a test, a lint/check script, or a doc in the repo (CLAUDE.md, AGENTS.md, DESIGN.md, docs/, in-repo PLAN/SPEC) already states this experience plainly. Both arms would see it, so fael adds nothing. Check at C^, not C. A code comment that cites a fael id (e.g. "— fael 01M42RK2") counts as stated.
- **cut b** — no checkable rubric: the row is too vague to judge from a diff/behaviour/test, OR task C gives no real opportunity to repeat that failure / violate that decision (the file overlap is incidental). Also cut as b rows about another repo/path layout, or pure process/tooling notes unrelated to code behaviour. Be strict: the row must be something an agent implementing C could plausibly get wrong.
- **cut c** — you cannot write a task prompt that states C's goal (from evidence existing before the work: PR title/body, linked issue) without revealing the cause, the fix, or the decision under test. Typical case: C is itself the fix for that row.
- **pass** — none of the above.
When unsure between pass and cut, cut and say why.

Also check: if a closed issue's close claims a fix at some sha, verify with `git merge-base --is-ancestor <sha> C^`. If that fix never reached C^, note it in `why` (the bug may still be live at C^, which can make it a pass).

## Output
Write one JSON object per line, one line per candidate in your range, in order, to `<out-dir>/cohort-<n>/prescreen/<first>-<last>.jsonl` or the file named in your task. Keep any helper script under a name unique to your range: parallel screeners share one scratch dir. Schema:
{"n": <number>, "pr": <pr>, "verdict": "pass"|"cut", "reason": "a"|"b"|"c"|null, "row": "<full row id or null>", "label": "repeat_failure"|"decision_violation"|null, "why": "<1-2 sentences of evidence, citing file:line at C^ where relevant>", "rubric": "<pass only: concrete check on the agent's diff that marks a violation; say whether deterministic (grep/test) or judgment>", "prompt": "<pass only: task prompt for the agent, goal only, no cause/fix/decision names, no hints from C's diff>", "hidden_test": "<pass only: tests added/changed in C usable as hidden evaluation, or 'none'>"}
Your final message must be ONLY one line: `<range>: <passes> pass, <a> a, <b> b, <c> c → <output file>` plus at most one extra line for anything the owner must know (e.g. a fix that never reached main).
