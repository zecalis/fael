//! `fael chunk start|done|move` (PLAN-fael-board b2a): claim a chunk and print its brief,
//! finish it with a PR or an `--out` file copied under `<db dir>/out/<uid>/<R>/`, move it to
//! another plan of this db with its uid and history.

use crate::plan::db;
use crate::{Args, Repo, core};
use fael_crew::{Here, Start, Store};
use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;

/// `--as stack`: per `--with` partner, whether the caller's HEAD already contains the
/// partner's live branch tip (`merge-base --is-ancestor`). The store enforces the
/// result; this only reads git, never the clash gate.
fn stack_contains(
    store: &Store,
    r: &Repo,
    with: &[String],
    head: Option<&str>,
) -> Result<Vec<bool>, String> {
    let mut out = Vec::new();
    let expanded = with
        .iter()
        .flat_map(|w| w.split(','))
        .map(str::trim)
        .filter(|w| !w.is_empty());
    for key in expanded {
        let branch = store.live_branch(key)?;
        let tip = branch
            .as_deref()
            .and_then(|b| crate::git(&r.root, &["rev-parse", b]));
        let ok = match (tip, head) {
            (Some(t), Some(h)) => {
                crate::board::git_output(&r.root, &["merge-base", "--is-ancestor", &t, h])
                    .is_some_and(|(code, _)| code == Some(0))
            }
            _ => false,
        };
        out.push(ok);
    }
    Ok(out)
}

/// The joint handshake line plus the mode's seating rule (pair/lane/stack/follow).
fn say_joint(o: &Start, st: &fael_crew::Started, store: &Store, here: &Here) -> Result<(), String> {
    let (Some(j), false) = (o.joint, o.with.is_empty()) else {
        return Ok(());
    };
    let ws = o.with.join(",");
    println!("joint {} with {ws} (run {})", j.as_str(), st.run);
    if j == fael_crew::Joint::Pair {
        println!(
            "pair: work in {}'s worktree and branch (your seat {} freed)",
            o.with.first().map(String::as_str).unwrap_or("?"),
            here.worktree
        );
    }
    if j == fael_crew::Joint::Lane {
        // issue 01M5Y6H4: the lane rules said when a lane is seated, not at first start
        println!("{}", fael_core::work::LANE_LINE.replace("<R>", &st.run));
    }
    if j == fael_crew::Joint::Stack {
        println!("{}", fael_core::work::STACK_LINE.replace("<R>", &st.run));
        println!(
            "stack: your PR merges after {} — land it first, then rebase yours onto it",
            o.with.join(",")
        );
    }
    if j == fael_crew::Joint::Follow {
        for w in &o.with {
            print!("{}", store.follow_text(w)?);
        }
    }
    Ok(())
}

/// `fael chunk start <uid>`: claim it, bind the caller's session, print the brief.
pub(crate) fn start(a: &Args, r: &Repo, uid: &str, here: &Here) -> Result<(), String> {
    a.only(
        "chunk start",
        &[
            "client",
            "model",
            "run",
            "force",
            "because",
            "no-worktree",
            "session",
            "with",
            "as",
            "full",
        ],
    )?;
    // the agent's own start: an agent that ends with `chunk done` in the same turn never
    // reaches a Stop that could stamp it (issue 01M4NXDQ). Not a launcher (`--run R`): it
    // may sit in another agent's session, and the started agent's SessionStart stamps it
    // ponytail: resolved in `start_session` — explicit `--session` (MCP, hidden) first,
    // else the client env; native ids only (an RFC3339 hook session is never stored;
    // seen() writes last_seen-only).
    let no_run = a.one("run").is_none();
    let session =
        crate::start_session::resolve_session(a.one("session"), |k| std::env::var(k).ok(), no_run);
    let mut o = Start {
        run: a.one("run"),
        client: a.one("client"),
        model: a.one("model"),
        no_worktree: a.has("no-worktree"),
        force: a.has("force"),
        because: a.one("because"),
        session: session.clone(),
        base: crate::git(&r.root, &["rev-parse", "HEAD"]),
        stale: crate::board::clock().1,
        with: a.many("with"),
        joint: a
            .one("as")
            .map(|m| fael_crew::Joint::parse(&m))
            .transpose()?,
        stack_contains: Vec::new(),
    };
    let mut store = Store::open(&db(r))?;
    // `--as stack` is cut from the partner's branch: HEAD must already contain its tip
    if o.joint == Some(fael_crew::Joint::Stack) {
        o.stack_contains = stack_contains(&store, r, &o.with, o.base.as_deref())?;
    }
    if !o.force {
        crate::chunk_scope::gate(&store, r, uid)?;
    }
    // a session whose held mark exists already read the chunk rules at its first start
    let rules_seen = session
        .as_deref()
        .and_then(crate::chunk::held)
        .is_some_and(|m| m.exists());
    let mut st = store.start(uid, &o, here)?;
    if let Some(m) = session.as_deref().and_then(crate::chunk::held) {
        // the Stop hook finds this db from any cwd; a failed write only skips stamps
        let _ = m.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(m, db(r).to_string_lossy().as_bytes());
    }
    // ids before the rows replace them: start names only the unlinked open issues
    let linked: Vec<String> = st.chunks.iter().flat_map(|c| c.issues.clone()).collect();
    // each linked issue's id → its row, so the agent reads what to fix without a find
    resolve_issue_rows(r, &mut st);
    resolve_cited_rows(r, &mut st);
    for (u, t) in &st.orphaned {
        println!(
            "note: chunk {u} \"{t}\" was still running in this worktree — its run ended, the chunk stays running; if it shipped: fael chunk done {u} \"<handoff>\" --left none|<ids>"
        );
    }
    // c5: same-session retry returns the live R short — sections, answers, notes,
    // hand-offs and held rows stay in the first brief (session invariant); --full
    // reprints the whole brief from lifecycle truth (crew.db run + plan sections).
    if st.resumed && !a.has("full") {
        print!("{}", st.render_short());
        return Ok(());
    }
    print!("{}", st.render(a.has("full"), a.has("full") || !rules_seen));
    for f in &st.forced {
        println!("forced: {f}");
    }
    say_joint(&o, &st, &store, here)?;
    // plan fael-row-retire c3: contracts and open decisions and notes on files under the
    // chunk's scope, verdicts ready — the agent judges, start never blocks. `fael` words them
    // (its session state and said-once outbox), so this asks it.
    {
        let scope: Vec<String> = st
            .chunks
            .iter()
            .filter_map(|c| c.scope.as_deref())
            .flat_map(str::lines)
            .map(String::from)
            .collect();
        let out = crate::fael_bin::held("start", &scope, &linked);
        // the link call names the chunk it was started for
        print!("{}", out.replace("{uid}", uid));
    }
    // the declared scope may name a re-export while the work lives elsewhere
    // (th.ts spreads start.ts): warn now, not only at done's outside report.
    if let Ok(s) = Store::open(&db(r)) {
        for c in &st.chunks {
            crate::chunk_scope::warn(&s, r, &c.uid, true);
            print_drift(&s, r, &c.uid);
        }
    }
    // e3 scoped board default: this plan's open rows, plus colliding chunks.
    print!("{}", scoped_board_lines(&st));
    Ok(())
}

/// Start-time drift: what `origin/HEAD` did past this run's base, and closed rows
/// bearing on it — the same facts `chunk status` shows, fail-open and silent when
/// there is nothing (a fresh base usually means nothing moved yet; the agent
/// re-checks with `chunk status` before the first edit and mid-run).
fn print_drift(s: &Store, r: &Repo, uid: &str) {
    let line = crate::chunk_drift::of(s, r, uid)
        .and_then(|m| m.line())
        .unwrap_or_default()
        .replace("{uid}", uid);
    if !line.is_empty() {
        print!("{line}");
    }
}

/// e3 scoped board default: one plan's open rows plus the running chunks
/// colliding with the scope (the brief's avoid/live) — never whole-repo
/// `board --open`. `board --json` stays the app's contract, untouched.
fn scoped_board_lines(st: &fael_crew::Started) -> String {
    let mut o = String::new();
    for c in &st.chunks {
        let mut colliding: Vec<String> = c
            .avoid
            .iter()
            .map(|a| format!("{} [{}]", a.uid, a.class))
            .collect();
        for (who, _) in &c.live {
            if !who.trim().is_empty() && !colliding.iter().any(|x| x == who) {
                colliding.push(format!("{who} (live)"));
            }
        }
        // nothing running collides: say so and stay silent about the other chunks — the
        // board is one `fael board --open --plan` away, never pushed (it pulled agents into
        // reading chunks that do not block this one)
        if colliding.is_empty() {
            o.push_str("running overlaps: none");
        } else {
            let _ = write!(
                o,
                "board: `fael board --open --plan {}` · running overlaps: {}",
                c.plan,
                colliding.join(", ")
            );
        }
        o.push('\n');
    }
    o
}

/// Each linked issue's id → its row with its live state, so the agent reads what
/// to fix without a find: `<id> — open|closed: <first line>|superseded → <succ> — …`.
/// An id the ledger cannot resolve prints `<id> — not found in the ledger`.
/// The store never reads the ledger — this runs after `Store::start`.
fn resolve_issue_rows(r: &Repo, st: &mut fael_crew::Started) {
    if !st.chunks.iter().any(|c| !c.issues.is_empty()) {
        return;
    }
    let log = crate::read(r);
    let shut = core::closed(&log);
    let gone = core::superseded(&log);
    let succ = core::successors(&log);
    for id in st.chunks.iter_mut().flat_map(|c| &mut c.issues) {
        let bare = id.clone();
        match core::resolve_row(&log, &bare) {
            Err(_) => {
                *id = format!("{bare} — not found in the ledger");
            }
            Ok(row) => {
                let state = if gone.contains(row.id.as_str()) {
                    let to = succ.get(row.id.as_str()).copied().unwrap_or("?");
                    format!("superseded → {to}")
                } else if shut.contains(row.id.as_str()) {
                    let first = crate::chunk_scope::close_first_line(&log, &row.id);
                    match first.is_empty() {
                        true => "closed".to_string(),
                        false => format!("closed: {first}"),
                    }
                } else {
                    "open".to_string()
                };
                let title = row.title.as_deref().map(|t| format!("{t}\n"));
                let tag = row.from_report().then_some("(from report, unchecked)\n");
                *id = format!(
                    "{} — {} — {}{}{}",
                    row.id,
                    state,
                    tag.unwrap_or_default(),
                    title.unwrap_or_default(),
                    row.text
                );
            }
        }
    }
}

/// Each brief/title cite → its live line, so the agent reads what is true at start
/// without a find. The store never reads the ledger — this runs after `Store::start`.
fn resolve_cited_rows(r: &Repo, st: &mut fael_crew::Started) {
    let log = crate::read(r);
    for c in st.chunks.iter_mut() {
        c.cited = crate::chunk_scope_cited::cited_lines(&c.title, &c.brief, &log);
    }
}

/// issue 01M5H3FGB: a fence rejection that names `--run R` gets the whole retype appended,
/// built from the args passed. The run is never picked for the agent.
fn with_retype(
    e: String,
    uid: &str,
    handoff: &str,
    flag: impl Fn(&str) -> Option<String>,
) -> String {
    if let Some(run) = e
        .split("pass --run ")
        .nth(1)
        .and_then(|t| t.split_whitespace().next())
    {
        return format!("{e}\nretype: {} --run {run}", done_cmd(uid, handoff, flag));
    }
    // a linked issue still open: the way out is handing it to the next chunk — name it
    // in --left instead of closing it now
    if e.contains("linked issues still open") {
        // the handoff may already carry the `left:` line `resolve` appended: drop a
        // trailing one so the retry answers in `--left` instead of tripping the
        // `--left and a `left:` line both given` reject
        let mut lines: Vec<&str> = handoff.lines().collect();
        if lines
            .last()
            .is_some_and(|l| l.trim_start().starts_with("left:"))
        {
            lines.pop();
        }
        let bare = lines.join("\n");
        return format!(
            "{e}\nhanding the rest to the next chunk? name the still-open issue in --left: {}",
            done_cmd(uid, &bare, |f| match f {
                "left" => Some("<id>[,<ids>]".into()),
                _ => flag(f),
            })
        );
    }
    // a missing --left: the same line, the one answer left to the agent as a placeholder —
    // fael cannot know that nothing is left, so it never types `none` for it
    if e.starts_with("rejected: chunk done needs --left") {
        return format!(
            "{e}\ntry: {} --left none|<ids>",
            done_cmd(uid, handoff, flag)
        );
    }
    e
}

/// `fael chunk done <uid> "<handoff>"` with the flags the caller passed, in a fixed order.
fn done_cmd(uid: &str, handoff: &str, flag: impl Fn(&str) -> Option<String>) -> String {
    let q = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""));
    let mut cmd = format!("fael chunk done {uid} {}", q(handoff));
    for f in ["pr", "out", "review", "left", "no-change"] {
        match flag(f) {
            Some(v) if v.is_empty() => cmd += &format!(" --{f}"),
            Some(v) => cmd += &format!(" --{f} {}", if v.contains(' ') { q(&v) } else { v }),
            None => {}
        }
    }
    cmd
}

/// A flag's value for the retype line; a bare flag (`--no-change`) is the empty string.
fn flag_of(a: &Args, f: &str) -> Option<String> {
    match f {
        "no-change" => a.has(f).then(String::new),
        _ => a.one(f),
    }
}

/// e2 done gate (approval rule): refuse while linked ledger issues are open.
/// Crew holds only the ids (`chunk add --issue`, `issue::of`), so the ledger judges
/// through `fael held done` (stdin `---issues---` section, no new command or flag).
fn refuse_open_linked(a: &Args, uid: &str, handoff: &str, linked: &[String]) -> Result<(), String> {
    if let Err(e) = crate::fael_bin::held_done_checked(&[], &[], None, linked) {
        return Err(with_retype(
            format!("rejected: chunk {uid} done refuses — {e}"),
            uid,
            handoff,
            |f| flag_of(a, f),
        ));
    }
    Ok(())
}

/// Issue 01M63XC1: no branch diff and a PR not merged yet — the open PR's files, through gh.
fn pr_fallback(
    r: &Repo,
    diff: Option<Vec<String>>,
    trunk: Option<&str>,
    pr: Option<i64>,
) -> Option<Vec<String>> {
    match (trunk, pr) {
        (Some(t), Some(n)) => crate::chunk_pr_files::pr_files(&r.root, t, n, "gh").or(diff),
        _ => diff,
    }
}

/// A chunk is optional, but a chunk that pushed a PR must name it: done without
/// `--pr` takes it from the run's branch when `gh` finds the branch's open PR
/// (explicit `--pr N` — or `--pr N,M,...`, last is the board's — wins when given).
fn auto_pr(r: &Repo, branch: Option<&str>, here: &Here) -> Result<Option<i64>, String> {
    let probe = branch.or(here.branch);
    let found = crate::chunk_here::pr_of(&r.root, probe, "gh");
    if found.as_deref().is_some_and(|n| n.parse::<i64>().is_ok()) {
        println!(
            "note: --pr {} from the run's branch (push pr was the ok); pass --pr explicitly to correct it",
            found.as_deref().unwrap_or_default()
        );
    }
    Ok(found.and_then(|n| n.parse::<i64>().ok()))
}

/// The linked ids the done gate still holds: a still-open linked issue named in the
/// hand-off's `left:` line rides the next chunk (board `left_open` shows it), so only
/// the unnamed remainder refuses. Prefix-proof either way: `--left` takes a prefix.
fn gate_linked(linked: &[String], handoff: &str) -> Vec<String> {
    let left = crate::chunk_left::listed(handoff);
    linked
        .iter()
        .filter(|id| !left.iter().any(|l| covers(id, l)))
        .cloned()
        .collect()
}

/// A `--left` token names `id`: exact or a unique prefix, either direction.
fn covers(id: &str, l: &str) -> bool {
    id.starts_with(l) || l.starts_with(id)
}

/// e3 one-line done: the held detail over the run's diff (or its one-liner
/// when nothing is stale), printed above the done line — never a no-op status
/// block with no command after it. Carries the linked ids (e2) so the warn
/// lists unlinked issues only (linked already gated above).
fn held_hint(
    st: &Store,
    uid: &str,
    diff: Option<&Vec<String>>,
    started: Option<&str>,
    linked: &[String],
) -> String {
    // the declared scope rides along, read before `claim` swaps it for the diff: an open
    // issue under it warns at done even on a file the diff skipped
    let scope = st
        .outside(uid, &[], &[])
        .map(|(d, _)| d)
        .unwrap_or_default();
    let files = diff.map_or(&[][..], Vec::as_slice);
    crate::fael_bin::held_done_checked(files, &scope, started, linked).unwrap_or_default()
}

/// `chunk note`: the word is kept; a brief edited since the start is said too.
pub(crate) fn note(
    st: &mut Store,
    uid: &str,
    text: &str,
    now: &str,
    session: Option<String>,
) -> Result<(), String> {
    st.note(uid, text, now, session.as_deref())?;
    println!("chunk {uid} → noted for the next start");
    print!("{}", st.drift_line(uid));
    Ok(())
}

/// `--pr`'s number from `12`, `#12`, `owner/repo#12` or a pull URL.
pub(crate) fn pr_number(raw: Option<&str>) -> Result<Option<i64>, String> {
    raw.map(|n| {
        n.trim_end_matches('/')
            .rsplit(['#', '/'])
            .next()
            .unwrap_or("")
            .parse::<i64>()
    })
    .transpose()
    .map_err(|_| "rejected: --pr takes a PR number, owner/repo#N or its URL".into())
}

/// What `resolve_pr_out` returns: the PR number, earlier PRs riding the hand-off,
/// and the `--out` path.
type PrOut = (Option<i64>, Vec<i64>, Option<std::path::PathBuf>);

/// `12`, `#12`, `owner/repo#12` or a pull URL plus `--out`: the PR number, earlier
/// PRs riding the hand-off, and the `--out` path. Refuses another repo's PR and
/// `--pr` with `--out`.
fn resolve_pr_out(a: &Args, r: &Repo, pr_default: Option<String>) -> Result<PrOut, String> {
    let (raw, earlier) = crate::chunk_prs::split(a.one("pr").or(pr_default))?;
    let pr = pr_number(raw.as_deref())?;
    // issue 01M4QFJ6V: another repo's PR is refused, never dropped; no origin = no check
    if let Some((raw, want)) = raw.as_deref().and_then(|p| Some((p, pr_repo(p)?)))
        && let Some(have) = crate::board::remote(&r.root).filter(|h| !h.eq_ignore_ascii_case(&want))
    {
        return Err(format!("rejected: --pr {raw} is not this repo's ({have})"));
    }
    let src = a.one("out").map(|o| r.cwd.join(o));
    if pr.is_some() && src.is_some() {
        return Err("rejected: --pr or --out, not both".into());
    }
    if let Some(p) = src.as_ref().filter(|p| !p.exists()) {
        return Err(format!("rejected: --out {} does not exist", p.display()));
    }
    Ok((pr, earlier, src))
}

pub(crate) fn done(
    a: &Args,
    r: &Repo,
    uid: &str,
    handoff: &str,
    here: &Here,
    pr_default: Option<String>,
) -> Result<(), String> {
    a.only(
        "chunk done",
        &["pr", "out", "run", "review", "left", "no-change"],
    )?;
    // `-` reads the hand-off from stdin: a long one never rides a shell argument
    let handoff = &crate::chunk_here::handoff_text(handoff)?;
    let no_change = a.has("no-change");
    if no_change && (a.has("pr") || pr_default.is_some() || a.has("out")) {
        return Err(
            "rejected: --no-change says nothing was changed — drop --pr/--out, or drop --no-change"
                .into(),
        );
    }
    // `12`, `#12`, `owner/repo#12`, `https://github.com/owner/repo/pull/12`: the number
    let (pr, earlier, src) = resolve_pr_out(a, r, pr_default)?;
    let base = db(r).with_file_name("out").join(uid);
    let mut copy = |run: &str| -> Result<String, String> {
        let src = src.as_deref().unwrap_or(Path::new(""));
        let to = base.join(run).join(src.file_name().unwrap_or_default());
        copy_all(src, &to).map_err(|e| format!("--out: copy to {}: {e}", to.display()))?;
        Ok(to.to_string_lossy().into_owned())
    };
    let out = src
        .is_some()
        .then_some(&mut copy as &mut fael_crew::Copy<'_>);
    let mut st = Store::open(&db(r))?;
    // issue 01M5FMHQX: leftovers are rows; the statement rides in the hand-off the next brief prints
    let handoff = &crate::chunk_left::resolve(a.one("left"), handoff, &st, &crate::read(r))
        .map_err(|e| with_retype(e, uid, handoff, |f| flag_of(a, f)))?;
    // e2 done gate (approval rule): refuse while linked ledger issues are open.
    // This sits before the amend below, so the done and the amend paths refuse alike.
    let linked = st.linked_issues(uid).unwrap_or_default();
    refuse_open_linked(a, uid, handoff, &gate_linked(&linked, handoff))?;
    // issue 01M6BKHZSP: a `measured:` line is dated and checked before any prefix goes on
    let handoff = &crate::chunk_measured::check(handoff)?;
    // issue 01M62VY2: a review that changed nothing says so in the hand-off the next brief prints
    let handoff = &match no_change {
        true => format!("reviewed, no change: {handoff}"),
        false => crate::chunk_prs::with_earlier(handoff, &earlier),
    };
    // a done chunk's done again only amends: its hand-off — and, with `--pr`, the PR
    // it names (a wrong number, a reopened PR): no run or touched change either way
    if src.is_none() {
        if let Some(n) = pr {
            if st.amend_pr(uid, handoff, n, here.now)? {
                println!("pr corrected to #{n} and handoff amended for {uid}");
                print!("{}", st.drift_line(uid));
                return Ok(());
            }
        } else if st.amend(uid, handoff, here.now)? {
            println!("handoff amended for {uid}");
            print!("{}", st.drift_line(uid));
            return Ok(());
        }
    }
    // issue 01M4TFB5: the run's branch, not the cwd's — a lead's done per lane from the wave
    // branch diffs that lane only. Read before done: ending the run from its own worktree
    // stamps the cwd's branch on it, and a lead merges there on the wave branch
    // issue 01M4TPHM: a run started on the trunk, then switched, has no branch of its own
    // recorded — the cwd's is the work
    let head = crate::git(&r.root, &["rev-parse", "--abbrev-ref", "origin/HEAD"]);
    let on_trunk = |b: &String| head.as_deref().and_then(|h| h.strip_prefix("origin/")) == Some(b);
    let branch = st.run_branch(uid, here.run).filter(|b| !on_trunk(b));
    // a run `run end` ended kept its lane's diff then: the lane may be rebased since
    let kept = st.claimed_at_end(uid, here.run);
    // the run's start, read before done ends it: held rows `start` named after it are not repeated
    let started = st.run_started(uid, here.run);
    let drift = crate::chunk_drift::done_line(&st, r, uid);
    let mut pr = pr;
    if pr.is_none() && src.is_none() && !no_change {
        pr = auto_pr(r, branch.as_deref(), here)?;
    }
    st.done_review(uid, handoff, pr, out, a.one("review").as_deref(), here)
        .map_err(|e| with_retype(e, uid, handoff, |f| flag_of(a, f)))?;
    let trunk = base_of(&r.root, branch.as_deref().or(here.branch));
    // issue 01M4RRD1K: what the branch touched, not what it declared; a failed write only
    // keeps the declared scope
    let mut diff = kept.or_else(|| touched(&r.root, &st, uid, branch.as_deref().or(here.branch)));
    // issue 01M5QF7Q: no branch diff, a PR: the squash commit on the trunk is the work
    let named = trunk.clone().or(head.clone());
    let empty = |d: &Option<Vec<String>>| d.as_ref().is_none_or(|d| d.is_empty());
    if empty(&diff) {
        diff = pr_fallback(r, diff, named.as_deref(), pr);
    }
    let unclaimed = empty(&diff);
    let outside = diff.as_ref().map(|d| outside_text(r, &st, uid, d));
    let hint = held_hint(&st, uid, diff.as_ref(), started.as_deref(), &linked);
    if let Some(d) = &diff {
        let _ = st.claim(uid, d);
    }
    let to = if pr.is_some() { "done" } else { "review" };
    let explicit = a.has("pr") && earlier.is_empty();
    crate::chunk_prs::say_branch(explicit, (&r.root, branch.as_deref().or(here.branch)), pr);
    print!("{hint}");
    println!("chunk {uid} → {to}");
    print!("{}{drift}", st.drift_line(uid));
    let reviewed = a.one("review").is_some();
    print!(
        "{}",
        crate::chunk_here::done_hints(uid, pr.is_some(), reviewed)
    );
    if no_change {
        match unclaimed {
            true => println!("recorded: reviewed, no change — the next brief reads it as a result"),
            false => println!(
                "note: --no-change given, but the branch touches files — recorded as given"
            ),
        }
    } else if unclaimed && src.is_none() {
        let t = named.as_deref().unwrap_or("the trunk");
        print!("{}", crate::chunk_pr_files::none_claimed(t, pr, uid));
    }
    print!("{}", outside.unwrap_or_default());
    print!("{}", dodge_note(r, &st, uid, diff.as_ref(), pr, handoff));
    if let (Some(trunk), Some(_)) = (trunk, pr) {
        conflict_note(&r.root, &trunk, here.branch.unwrap_or_default());
    }
    Ok(())
}

/// l4: push-PR survival — `done --pr` against live partners' touched files
/// (board dodge data). Warn only, never blocks done.
fn dodge_note(
    r: &Repo,
    st: &Store,
    uid: &str,
    diff: Option<&Vec<String>>,
    pr: Option<i64>,
    handoff: &str,
) -> String {
    let (Some(d), Some(_)) = (diff, pr) else {
        return String::new();
    };
    let shared = fael_crew::shared(&r.root).unwrap_or_default();
    st.dodge(uid, d, &shared)
        .map(|hits| crate::chunk_prs::dodge_text(&hits, handoff))
        .unwrap_or_default()
}

/// What `uid`'s run on `branch` touched (None = the cwd's HEAD): its trunk diff since the
/// run's start; None = on the trunk or no `origin/HEAD`.
pub(crate) fn touched(
    dir: &Path,
    st: &Store,
    uid: &str,
    branch: Option<&str>,
) -> Option<Vec<String>> {
    let trunk = base_of(dir, branch)?;
    let tip = branch.unwrap_or("HEAD");
    let d = crate::git(dir, &["diff", "--name-only", &format!("{trunk}...{tip}")])?;
    Some(since(dir, st.run_base(uid), (&trunk, tip), d.lines()))
}

/// h4: each path of `diff` outside `uid`'s declared scope, unowned or overlapping the open chunk that
/// holds it — a fact, never a refusal; empty when all is inside. `done` and `run end` say it.
pub(crate) fn outside_text(r: &Repo, st: &Store, uid: &str, diff: &[String]) -> String {
    let shared = fael_crew::shared(&r.root).unwrap_or_default();
    let Some((declared, out)) = st
        .outside(uid, diff, &shared)
        .ok()
        .filter(|(_, o)| !o.is_empty())
    else {
        return String::new();
    };
    let mut s = format!("declared scope: {}\n", declared.join(", "));
    let mut free: Vec<String> = vec![];
    for (f, owner) in out {
        match owner {
            Some(o) => s += &format!("outside declared scope: {f} (overlaps open chunk {o})\n"),
            None => {
                s += &format!("outside declared scope: {f}\n");
                free.push(f);
            }
        }
    }
    // a widening keeps the owner's approval (chunk edit --scope), so the line is runnable as is
    // (`--scope` takes one value per flag: repeated, never space-joined)
    if !free.is_empty() {
        let plus: Vec<String> = free.iter().map(|p| format!("+{}", p.trim())).collect();
        s += &format!(
            "to cover them: fael chunk edit {uid} --scope {}\n",
            plus.join(" --scope "),
        );
    }
    s
}

/// Issue 01M4TVDGG: of the trunk diff, only the paths changed since the run's start commit —
/// a lane stacked on another lane's branch never claims that lane's files, and a rebase onto
/// the trunk since start never claims the trunk's (the trunk diff cuts them). No base
/// recorded: the trunk diff as is; nothing since the base: nothing claimed.
/// Issue 01M4W673: a lane rebased with the stack it started on (the trunk moved) has a base
/// that is no ancestor any more — the stack's copies keep author, date and subject of a commit
/// behind the base and drop (a patch-id match misses a copy whose context the trunk moved);
/// the trunk's own commits a rebase brought in are not the lane's either.
fn since<'a>(
    dir: &Path,
    base: Option<String>,
    (trunk, tip): (&str, &str),
    diff: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let Some(b) = base else {
        return diff.map(String::from).collect();
    };
    let (range, not) = (format!("{b}...{tip}"), format!("^{trunk}"));
    let log = |side: &str, more: &[&str]| {
        let head = ["log", side, "--format=%x01%ae %at %s"];
        let args = [&head[..], more, &[&range, &not]].concat();
        crate::git(dir, &args).unwrap_or_default()
    };
    let behind = log("--left-only", &[]);
    let behind: HashSet<&str> = behind.split('\x01').map(str::trim).collect();
    let ahead = log("--right-only", &["--name-only"]);
    let since: HashSet<&str> = ahead
        .split('\x01')
        .filter_map(|c| c.split_once('\n'))
        .filter(|(key, _)| !behind.contains(key.trim()))
        .flat_map(|(_, files)| files.lines())
        .collect();
    diff.filter(|f| since.contains(f))
        .map(String::from)
        .collect()
}

/// `owner/repo` of `owner/repo#12` or `…/owner/repo/pull/12`; None for `12` or `#12`.
fn pr_repo(pr: &str) -> Option<String> {
    let pr = pr.trim_end_matches('/');
    let head = match pr.split_once('#') {
        Some((h, _)) => h,
        None => pr.rsplit_once("/pull/")?.0,
    };
    let mut parts = head.rsplit('/').filter(|s| !s.is_empty());
    let name = parts.next()?;
    Some(format!("{}/{name}", parts.next()?))
}

/// The base (`origin/HEAD`, no fetch, as board does) when `branch` is another branch.
fn base_of(dir: &Path, branch: Option<&str>) -> Option<String> {
    let base = crate::git(dir, &["rev-parse", "--abbrev-ref", "origin/HEAD"])?;
    branch.filter(|b| Some(*b) != base.strip_prefix("origin/"))?;
    Some(base)
}

/// Decision 01M4QJPX: a PR whose branch no longer merges into the base is told which
/// files — never refused; a git error says nothing.
fn conflict_note(dir: &Path, base: &str, b: &str) {
    let files = crate::board::prs::merge_tree(dir, base, "HEAD").unwrap_or_default();
    if !files.is_empty() {
        let files = files.join(", ");
        println!("note: {b} conflicts with {base} in {files} — rebase before review");
    }
}

/// `fael chunk move <uid> --plan <p>`: the chunk goes to the end of `p`, uid, runs, notes
/// and edges kept.
pub(crate) fn move_to(a: &Args, s: &mut Store, uid: &str, now: &str) -> Result<(), String> {
    a.only("chunk move", &["plan"])?;
    let plan = a.one("plan").ok_or(format!(
        "rejected: chunk move needs --plan <p> — try `fael chunk move {uid} --plan <p>`"
    ))?;
    let from = s.move_to(uid, &plan, now)?;
    println!("chunk {uid} → moved from {from} to {plan}");
    Ok(())
}

/// A file or a whole dir. `fs::copy` clones on APFS, so a big video costs no space.
fn copy_all(src: &Path, to: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_all(&e.path(), &to.join(e.file_name()))?;
        }
        return Ok(());
    }
    if let Some(d) = to.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::copy(src, to).map(|_| ())
}

#[cfg(test)]
mod retype_tests {
    use super::with_retype;

    #[test]
    fn a_fence_reject_gets_a_copyable_retype_and_other_errors_stay() {
        let e = "rejected: chunk C1 was taken over by run R9 (codex) in /w — pass --run R9";
        let flag = |f: &str| match f {
            "pr" => Some("12".to_string()),
            "left" => Some("none".to_string()),
            _ => None,
        };
        let got = with_retype(e.into(), "C1", "said \"hi\"", flag);
        assert!(got.starts_with(e), "{got}");
        assert!(
            got.ends_with(
                "retype: fael chunk done C1 \"said \\\"hi\\\"\" --pr 12 --left none --run R9"
            ),
            "{got}"
        );
        assert_eq!(
            with_retype("rejected: x".into(), "C1", "h", flag),
            "rejected: x"
        );
    }

    /// A missing --left gets the whole command back with the answer as a placeholder.
    #[test]
    fn a_missing_left_gets_a_copyable_try_with_the_answer_left_open() {
        let e = "rejected: chunk done needs --left none or --left <id|uid>,… — how";
        let flag = |f: &str| (f == "pr").then(|| "12".to_string());
        let got = with_retype(e.into(), "C1", "did it", flag);
        assert!(got.starts_with(e), "{got}");
        assert!(
            got.ends_with("try: fael chunk done C1 \"did it\" --pr 12 --left none|<ids>"),
            "{got}"
        );
    }
}

#[cfg(test)]
mod retype_gate_tests {
    use super::with_retype;

    #[test]
    fn a_linked_gate_refuse_points_at_left_with_a_runnable_retype() {
        let e = "rejected: chunk C1 done refuses — linked issues still open — close each before done:\nopen linked issue 01AAA \"x\" — close it first: fael close 01AAA \"...\"";
        let flag = |f: &str| match f {
            "pr" => Some("9".to_string()),
            "left" => Some("none".to_string()),
            _ => None,
        };
        // the handoff resolve already appended to: the retry must not carry both
        let got = with_retype(e.into(), "C1", "part done\nleft: none", flag);
        assert!(got.contains("name the still-open issue in --left"), "{got}");
        assert!(got.contains("--left <id>[,<ids>]"), "{got}");
        assert!(!got.contains("left: none"), "{got}");
    }
}

#[cfg(test)]
mod gate_tests {
    use super::gate_linked;

    #[test]
    fn only_the_left_unnamed_linked_still_refuse() {
        let linked = ["01AAA".to_string(), "01BBB".to_string()];
        // nothing named: the gate holds both
        assert_eq!(
            gate_linked(&linked, "shipped\nleft: none"),
            linked,
            "none names nothing"
        );
        // the open issue handed to the next chunk rides it, the other still refuses
        assert_eq!(
            gate_linked(&linked, "part done\nleft: 01AAA"),
            ["01BBB".to_string()]
        );
        // a unique prefix names it too
        assert_eq!(
            gate_linked(&linked, "part done\nleft: 01A"),
            ["01BBB".to_string()]
        );
        // both named: the gate passes
        assert!(gate_linked(&linked, "part done\nleft: 01AAA,01BBB").is_empty());
        // no left line at all: the gate holds everything
        assert_eq!(gate_linked(&linked, "shipped"), linked);
    }
}
