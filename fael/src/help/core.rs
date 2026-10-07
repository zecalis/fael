//! The core surface `fael <cmd> --help` shows (PLAN-fael-agent-ergonomics
//! chunk 3, experiment): the same command with fewer flags. The full section
//! in `help.rs` stays one `--help --all` away, and hidden flags keep working —
//! hiding is input discovery only, never a parser change. Only find/add hide
//! anything; every other command's core is its full section.

/// (command, core section): synopsis plus the notes that stay. A hidden flag
/// names nothing here — neither the synopsis nor the prose.
const CORE: &[(&str, &str)] = &[
    (
        "add",
        "fael add <kind> \"<text>\" [--files a,b] [--key k] [--title t] [--to who] [--from user] [--revisit date|text] [--urgent] [--supersedes id [--replace old --with new]] [--json]
    (write rows in English; file each in the same message as your next tool call, never alone;
     no --files = the files this session edited, as the edit hook recorded;
     --title = the ≤15-word headline lists show, the body is pulled by id;
     --from user = the user said or decided it (omit when you chose): lists say (from user),
     and a later agent asks the user before re-filing or closing it;
     --replace old --with new re-files the --supersedes row with that one passage changed
     (no text; files, key, title and from carry over; old must occur in the body exactly once);
     --to who routes an issue: it lists in full at the session start of whoever's git user.name
     is `who` (lowercased), or of every session of the agent client named `who` (opencode, codex,
     claude); fael find --to who lists theirs. The receipt prints the line to paste to them and,
     for a client, the headless command that starts it on the row (printed, never run);
     key it `<topic>:handoff` and `fael stats` counts it when picked up; the receiver closes it
     with how it went (fael close <id> \"...\") and the sender reads that on fael find --all;
     working an open issue? fael claim <id> first — others then see (held @<branch>), never a lock;
     a topic list (; / ·), a long text with no --title, or a plan key off the handoff
     convention (plan:<name>:handoff; chunk-<n> only for a chunk run in parallel) is
     rejected before the write, as is a path that looks like a typo;
     batch: fael add --json - < rows.json (a JSON array; a bad row reports alone, the rest save);
     text that starts with - goes after --: fael add note --files a.rs -- \"-h\")",
    ),
    (
        "find",
        "fael find [text|id ...] [--files a,b] [--key glob] [--kind k] [--full] [--all] [--limit N] [--offset M] [--text query]
     (an exact id or unique prefix pulls that row's body — and, if it is closed,
      why: `closed: <text> (<sha>)`; several ids print every body in one call,
      a bad id reports alone and the exit is 1; an id-shaped query is
      never a text search — it rejects when no row owns it, naming the rows
      that only mention it; --text forces a text search;
      an id counts as verified only after fael find printed it as - [<id>] —
      never type one from memory;
      --full shows every body — a first page of one or two rows (no --limit)
      shows them anyway when they fit find_tokens; several ids stop at the same
      budget and the cut line names the ids left; its cut line asks for the
      rest in one call;
      an empty search counts each word, --files and filter on its own, so the
      one that matched nothing shows;
      --kind issue lists issues ready to work first, (waiting: …) ones last;
      text finds rows holding every word, in any order; on a busy file add
      one: fael find --files src/a.rs \"timeout\"; --key globs: --key 'feature:*';
      a cut list prints the exact next call — rerun it with the new --offset)",
    ),
];

/// The core section for `cmd`, or None when the full section is already core.
pub(crate) fn section(cmd: &str) -> Option<&'static str> {
    CORE.iter().find(|(n, ..)| *n == cmd).map(|(.., s)| *s)
}
