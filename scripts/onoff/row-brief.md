# Row brief — PLAN-fael-capture-yield (read fully before starting)

The one set of criteria every capture-yield sheet is screened with: baseline and post-ship
rows are judged by this text, frozen before ship. Change it only through a new plan decision.
Its cuts a/b are `screen-brief.md`'s, moved from a later commit's parent to the row itself.

You screen ledger rows (closed issues, decisions) of one repo. Your output is a recommendation;
the repo owner makes the final call. READ-ONLY: never modify, checkout, stash or commit anything.

## The question
Would a new agent reading only the code, tests and docs of the repo, as it stood when this row
was closed (issue) or written (decision), know what the row says? If yes, the repo already
carries it. If no, and it is concrete enough to check on a diff, only the ledger carries it.

## Inputs
- Sheet: `<out-dir>/sheet.md` — rows are the `## N.` headings: kind, the ref `at-<n>` it is
  judged at, files, title, text, and the close text when it has one.
- Repo state: `<out-dir>/repo.git` (bare). Use only `git -C <out-dir>/repo.git show at-<n>:<path>`,
  `grep -n <pattern> at-<n> [-- <path>]` and `ls-tree -r --name-only at-<n> [<path>]`.
  Never run `log`, `rev-parse`, `cat-file`, `describe` or `for-each-ref`, never open `key.json`
  or another repo: the screen is blind to when a row was written.

## Decide per row: ONE verdict
- **g** (held by guard) — a test or check that the row's own close or text names as its guard
  exists at `at-<n>` and fails if the lesson is broken, the next instance included (see a).
  The repo carries it, through the guard.
- **a** — otherwise, at `at-<n>` code, a test, a lint/check script, or a doc in the repo
  (CLAUDE.md, AGENTS.md, DESIGN.md, docs/, in-repo PLAN/SPEC) already states it plainly: a
  reader of those files would learn the rule or the why, not just see a mechanism with no
  reason. A code comment that cites a fael id (e.g. "— fael 01M42RK2") counts as stated.
  Judge the lesson, not the instance: it is stated only where the next change that could
  repeat it would meet it (the next field added, index created, enum value, caller, path).
  A comment or test on the one spot that was fixed does not state the rule for the next one.
- **b** — no checkable rubric: too vague to judge from a diff/behaviour/test, OR no plausible
  future change to its files could repeat that failure / contradict that decision. Also b:
  rows about another repo/path layout, or pure process/tooling notes unrelated to code behaviour.
- **fael** — none of the above: only the ledger carries it.
When unsure between fael and a cut, cut and say why. Between g and a, g only when the row names it.

For **fael**, tag the kind of knowledge, `type` = the one that fits best, `also` = any others:
1. a thing exists already, reuse it (a helper, a registry, a contract) instead of a new one
2. changing here means changing there too (a coupled file or field the edit does not show)
3. A was chosen over B, and why (or: tried B, it failed because)
4. the cause lives outside the file it shows up in

## Output
One JSON object per line, per row in your range, in order, to the file named in your task:
{"n": <number>, "verdict": "fael"|"g"|"a"|"b", "type": 1|2|3|4|null, "also": [<types>],
 "why": "<1-2 sentences of evidence, citing path:line at at-<n> where relevant>"}
Your final message must be ONLY one line: `<range>: <fael> fael, <g> g, <a> a, <b> b → <file>`.
