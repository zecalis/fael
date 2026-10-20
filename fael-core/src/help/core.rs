//! The core surface `fael <cmd> --help` shows (PLAN-fael-agent-ergonomics
//! chunk 3, experiment; PLAN-cli-tiers chunk hide-flags): the same command
//! with its workflow flags hidden. The full section in `help.rs` stays one
//! `--help --all` away, and hidden flags keep working — hiding is input
//! discovery only, never a parser change. Core commands (add/find/close/
//! bump/claim/doctor/stats) hide what the tier table
//! (`product:cli-tiers-table`) marks workflow; hidden workflow commands
//! (chunk/plan/board/run/...) keep their full own --help, so an agent sent
//! there by a brief still sees everything.

/// (command, core section): synopsis plus the notes that stay. A hidden flag
/// names nothing here — neither the synopsis nor the prose.
const CORE: &[(&str, &str)] = &[
    (
        "add",
        "fael add <kind> \"<text>\" [--title t] [--files a,b] [--key k] [--to who] [--from user] [--supersedes id [--replace old --with new, no <text>]] [--json]
    (write rows in English; file each in the same message as your next tool call, never alone;
     no --files = the files this session edited, as the edit hook recorded;
     --title = the ≤15-word headline lists show, the body is pulled by id — give one to any
     text longer than a sentence (past 60 words or 400 chars the add is rejected without it);
     --from user = the user said or decided it (omit when you chose): lists say (from user),
     and a later agent asks the user before re-filing or closing it;
     --replace old --with new re-files the --supersedes row with that one passage changed
     (no text; files, key, title and from carry over; old must occur in the body exactly once);
     --to who routes an issue: it lists in full at the session start of whoever's git user.name
     is `who` (lowercased), or of every session of the agent client named `who` (opencode, codex,
     claude). The receipt prints the line to paste to them and,
     for a client, the headless command that starts it on the row (printed, never run);
     key it `<topic>:handoff` and `fael stats` counts it when picked up; the receiver closes it
     with how it went (fael close <id> \"...\") and the sender reads that on the closed row;
     working an open issue? fael claim <id> first — others then see (held @<branch>), never a lock;
     a topic list (; / ·), a long text with no --title, or a plan key off the handoff
     convention (plan:<name>:handoff; chunk-<n> only for a chunk run in parallel) is
     rejected before the write, as is a path that looks like a typo;
     batch: fael add --json - < rows.json (a JSON array; a bad row reports alone, the rest save);
     text that starts with - goes after --: fael add note --files a.rs -- \"-h\")",
    ),
    (
        "find",
        "fael find [text|id ...] [--files a,b] [--key glob] [--limit N] [--offset M]
     (an exact id or unique prefix pulls that row's body — and, if it is closed,
      why: `closed: <text> (<sha>)`; several ids print every body in one call,
      a bad id reports alone and the exit is 1; several words with no id are
      one text query; an id-shaped query is never a text search — it rejects
      when no row owns it, naming the chunk or other repo that owns it and
      the rows that only mention it;
      an id counts as verified only after fael find printed it as - [<id>] —
      never type one from memory;
      an empty search counts each word, --files and filter on its own, so the
      one that matched nothing shows;
      text finds rows holding every word, in any order; on a busy file add
      one: fael find --files src/a.rs \"timeout\"; --key globs: --key 'feature:*' (a bare key also matches its k:* children);
      a cut list prints the exact next call — rerun it with the new --offset)",
    ),
    (
        "bump",
        "fael bump <id>
    (same id, text and files — one bump event; text and files never change through bump;
     a bare bump is the 'still true' verdict on a row whose files changed: restamps it from disk)",
    ),
    (
        "doctor",
        "fael doctor [--fix] [--json]
     (check the log and the repo for problems; --fix repairs what it can, including
      closing the confirmed [Shipped] status notes — rules and facts on a landed
      branch are listed as [Shipped kept], never closed; [Phantom] flags citations of ids
      with no row behind them — in rows, in close reasons and in the prose of
      every *.md in the repo (fenced code skipped); [NoVerdict] lists rows on a
      file over the 1 MiB push cap — push never says whether it changed since the
      row was written (note only, never --fix); [Unstamped] lists rows with no
      file stamp — `fael bump <id>` stamps one you checked is still true (note
      only, never --fix); [Secret] flags a row that
      holds a token — rotate it, then `fael purge <id>` (never --fix); --json prints each
      problem's full row ids for a cleanup pass)",
    ),
    (
        "stats",
        "fael stats [--json] [--since d]
    (tokens fael has put into context, per machine;
     --since = only usage from that day (YYYY-MM-DD, UTC) or RFC 3339 time on,
     all = every month; default = this and last month)",
    ),
];

/// The core section for `cmd`, or None when the full section is already core.
pub fn section(cmd: &str) -> Option<&'static str> {
    CORE.iter().find(|(n, ..)| *n == cmd).map(|(.., s)| *s)
}
