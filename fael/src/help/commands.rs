//! The COMMANDS table, split out of help.rs at the 400-line ratchet.

/// One row per command: name, the one-line summary the full usage lists,
/// and the section `fael <cmd> --help` prints (synopsis, then notes). The
/// only copy — the command list and each section both come from here, and
/// `docs_match_flags` holds docs/architecture.md to the same flags.
pub(super) const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "add",
        "write a row",
        "fael add <kind> \"<text>\" [--title t] [--files a,b] [--key k] [--to who] [--from user] [--revisit date|text] [--urgent|--urgent-before id] [--supersedes id [--replace old --with new, no <text>]] [--force] [--dry-run] [--json]
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
     claude); fael find --to who lists theirs. The receipt prints the line to paste to them and,
     for a client, the headless command that starts it on the row (printed, never run);
     key it `<topic>:handoff` and `fael stats` counts it when picked up; the receiver closes it
     with how it went (fael close <id> \"...\") and the sender reads that on fael find --all;
     working an open issue? fael claim <id> first — others then see (held @<branch>), never a lock;
     a topic list (; / ·), a long text with no --title, or a plan key off the handoff
     convention (plan:<name>:handoff; chunk-<n> only for a chunk run in parallel) is
     rejected before the write — --force files it, or a path that looks like a typo;
     --dry-run prints the verdict and the row the real add would write, writing nothing;
     a repeat on these files or key, \"Supersedes <id>\" in the text, and the
     only key on these files are filled in for you;
     batch: fael add --json - < rows.json (a JSON array; a bad row reports alone, the rest save);
     text that starts with - goes after --: fael add note --files a.rs -- \"-h\")",
    ),
    (
        "close",
        "close a row",
        "fael close (<id> | --key <key>) \"<why>\"
    (an exact id or unique prefix names the row; closing twice is rejected;
     --key closes the one open row on that key, and lists them if there are several)",
    ),
    (
        "bump",
        "same text/files, new to/urgent/revisit",
        "fael bump <id> [--to who] [--revisit date|text] [--urgent|--urgent-before id|--not-urgent]
    (same id, text and files — one bump event; text and files never change through bump)",
    ),
    (
        "claim",
        "mark an open issue held by this branch",
        "fael claim <id> [--force]
    (find --kind issue then shows (held @<branch>) so another agent picks
     something else; two agents racing for one issue: the first wins, the
     second is told who holds it — --force takes it over, and so does a hold
     whose branch no longer exists; it gates the claim, never your edits;
     closing the issue ends it)",
    ),
    (
        "next",
        "claim the best free issue and print it",
        "fael next
    (open, not waiting, routed to nobody or to you, not held by a live branch;
     yours first, then urgent, then newest; an issue routed to someone else or
     to an agent client (--to opencode): fael claim <id>)",
    ),
    (
        "find",
        "search rows",
        "fael find [text|id ...] [--files a,b] [--key glob] [--kind k] [--since yyyy-mm[-dd]] [--by writer] [--to who] [--revisit[=text]] [--all] [--full] [--limit N] [--offset M] [--groups] [--text query]
     (an exact id or unique prefix pulls that row's body — and, if it is closed,
      why: `closed: <text> (<sha>)`; several ids print every body in one call,
      a bad id reports alone and the exit is 1; several words with no id are
      one text query; an id-shaped query is never a text search — it rejects
      when no row owns it, naming the chunk or other repo that owns it and
      the rows that only mention it; --text forces a text search;
      an id counts as verified only after fael find printed it as - [<id>] —
      never type one from memory;
      --full shows every body — a first page of one or two rows (no --limit)
      shows them anyway when they fit find_tokens; several ids stop at the same
      budget and the cut line names the ids left; its cut line asks for the
      rest in one call;
      an empty search counts each word, --files and filter on its own, so the
      one that matched nothing shows;
      --kind issue lists issues ready to work first, (waiting: …) ones last;
      --groups groups every match by shared files — what to fix in one PR
      (*.md and anchors never link);
      text finds rows holding every word, in any order; on a busy file add
      one: fael find --files src/a.rs \"timeout\"; --key globs: --key 'feature:*' (a bare key also matches its k:* children);
     a cut list prints the exact next call — rerun it with the new --offset)",
    ),
    (
        "keys",
        "list keys with row counts",
        "fael keys [glob]
    (lists each key with its row count; a glob narrows, e.g. fael keys \"auth:*\")",
    ),
    (
        "kickoff",
        "the session brief",
        "fael kickoff [file|anchor] [--full] [--limit N] [--offset M]
    (the session brief: open rows ranked for a file, anchor or the whole repo;
    an explicit --limit N is not cut by budget.kickoff_tokens)",
    ),
    (
        "mv",
        "record a move git can't see",
        "fael mv <old> <new>
    (record a move git can't see — anchors, uncommitted rewrites, repos without
     git; appends an alias row, the log stays append-only, nothing is rewritten)",
    ),
    (
        "chunk",
        "add, start, wait on and finish the chunks of a db plan",
        "fael chunk <verb> … [--plan p] [--label l] [--title t] [--brief b] [--size S|M|L] [--model m] [--client c] [--scope a/,b.rs] [--reads key,plan/key] [--after uid] [--run R] [--force] [--no-worktree] [--on owner|data] [--until date] [--option A] [--pr N] [--out path]
fael chunk add \"<title>\" [--plan inbox|<plan>] [--label l] [--brief b] [--size S|M|L] [--model m] [--client c] [--scope a/,b.rs] [--reads key,plan/key] [--after uid,…] [--issue id,…]
fael chunk show <uid|label> [--json]
fael chunk start <uid> [--client c] [--model m] [--run R] [--force] [--no-worktree]
fael chunk edit <uid> [--title t] [--label l] [--brief b] [--size S|M|L] [--model m] [--client c] [--scope …] [--reads …]
fael chunk move <uid> --plan <p>
fael chunk note <uid> \"<text>\"
fael chunk wait <uid> \"<question>\" --on owner|data [--until YYYY-MM-DD] [--option \"A\" --option \"B\" …] [--run R]
fael chunk after <uid> <other-uid> \"<why>\" [--run R]
fael chunk unafter <uid> <other-uid> \"<why>\"
fael chunk done <uid> \"<handoff>\" [--pr N|owner/repo#N|URL | --out <path>] [--verdict fix|answer|consider|plan|decline] [--review clean|fixed|failed] [--run R]
fael chunk answer|drop|park <uid> \"<text>\" · fael chunk accept|unpark <uid>
fael chunk apply --json -   (stdin: [{\"op\":\"merge\",\"chunks\":[a,b]} · {\"op\":\"split\",\"chunk\":a,\"into\":[{\"title\":…,\"brief\":…},…]} · {\"op\":\"pair\",\"chunks\":[a,b]} · {\"op\":\"after\",\"chunk\":a,\"on\":b,\"why\":…}])
fael chunk approve <uid> [--model M] [--client C] · fael chunk unapprove <uid>
    (inbox or a plan from `fael plan new`; add prints
     the new uid, no brief = draft; add and edit warn on a --scope path whose directory is
     missing or a typo of a name beside it; --issue links open ledger issues it fixes (its
     brief prints them; close each once the fix holds, done never does); a --label is unique in its plan and every verb
     takes it for the uid; --reads names the plan sections it works from (another
     plan's as plan/key): its brief prints Goal, Done and those, then an index of the
     rest (no --reads: Goal, Scope, Done, Constraints; one still empty is skipped); show prints one chunk as the board has it, its hand-off,
     the brief and notes its next start prints; start claims the chunk and its pair in one step (an after between them is met by plan order)
     and prints the brief, R on its first line — a second start is told who holds it,
     --force takes it over, and starts one blocked by an after; --no-worktree (a client that writes a file and exits)
     takes no worktree slot, and from Claude Code without --run binds its session,
     whose Stop hook keeps the run seen from any cwd; wait, after and done answer only to the worktree of the
     latest run and end it, or to --run R naming that run; wait --on owner --option offers the answers
     the owner picks from; done --pr N lands in done (push pr is the ok) — an owner/repo#N or URL of
     another repo than this one's origin is refused; done without a PR in review until accept; done takes the run's branch's diff against origin/HEAD, since the chunk's first start, as the scope
     and prints each path outside the declared one, unowned or in <plan>/<label>; --out is copied to <git-common-dir>/fael/out/<uid>/<R>/; --verdict
     only on a ticket chunk (fael intake), shown as ticket.verdict on board --json; edit
     never changes the state and clears an approval; unafter drops one after edge and keeps it; move puts a chunk at the end of
     another db plan of this repo with its uid, runs, notes and edges — refused while it
     runs, into a parked or done plan, or onto a label the plan has; apply runs every op or none —
     merge and split leave the old chunks replaced and move their after edges to the
     new ones)",
    ),
    (
        "board",
        "every registered repo's plans and chunks: needs you, queue, running, blocked",
        "fael board [--json] [--open] [--plan p] | fael board --json --watch | fael board --drop <root>
    (the owner's view, run from anywhere: each repo whose fael.db a `fael plan`
     or `fael chunk` command wrote is listed in ~/.local/state/fael/projects;
     --json is the app's contract (\"v\": 1, tests/golden/board-v1.json); --open keeps
     only open chunks — the planner's one read of scope, after and overlaps; --plan
     keeps one plan's chunks; the text view folds blocked into per-plan counts and
     --plan lists them; a path in .fael/shared (one prefix per line) is
     nobody's claim: it never makes an overlap; a repo whose fael.db cannot be read
     stays listed with its error and the command exits 1; --watch (the app's one
     read) stays up, rereads each second only the project whose db, ledger jsonl or
     git refs moved, prints a compact board line at start and on each change, and
     exits when stdin closes; --drop takes a finished repo off the registry until a
     chunk command there adds it back)",
    ),
    (
        "intake",
        "bring tickets from the team's other systems in as inbox chunks",
        "fael intake [<source>]
    (runs each source's `list` in .fael/sources.json (or the one named) with sh -c — cmd /C
     on Windows — at the repo root; every JSONL line must carry exactly ref, kind, title,
     reopen, closed, else nothing is written and the command exits 1 naming the line; a
     ticket round (source+ref+reopen) gets one chunk in any plan: an open one with none is
     added to inbox (label lower(ref), -rN when reopened, size S, a brief with its read,
     open, claim and draft commands), one that has a chunk is left alone; fael runs only `list` and never writes to the
     source; prints one line per change, then a count; board --json shows chunk.ticket)",
    ),
    (
        "plan",
        "create a plan, export or edit its sections and chunks, park or finish it",
        "fael plan new <name> \"<title>\" [--app A] [--area dev|marketing] [--parent <plan>] | fael plan list | fael plan export [<plan>] [--json] | fael plan export <plan> --section <key> | fael plan export --since <event-id> | fael plan apply <file|-> [--dry-run]
     | fael plan park|unpark|done <plan>
    (a plan is sections and chunks in <git-common-dir>/fael/fael.db, no file;
     new: a plan whose chunks live in the db from the start (`fael chunk add --plan`),
     its text four skeleton sections (1 Goal, 2 Scope, 3 Done criteria, 4 Constraints);
     --parent <plan> nests it under an umbrella plan of the same app, one level (board
     --json plans[].parent; the bill header's parent sets it, \"\" clears it);
     list: one line per plan — the name export takes, its state, its title, and · parent P
     when it is nested;
     export: every plan, or the one named (name or app/name), as markdown to stdout — uid,
     state and fields under each chunk, then the plan's sections in order, each under its
     heading with its key and the day it last changed; export <plan> --section <key>: that one section; export <plan>
     --json: the plan as one bill (\"v\": 1) — rev, title, area, parent, state, its
     sections ({key, title, body}) and a line per planned chunk (uid, label, title, brief,
     size, model, scope, reads, after); export --since <event-id>: the change feed
     for backup and fael-sync (\"v\": 1) — db_id, head (the next --since), the events
     after the id and the current rows they touched, whole (plans, every section of those
     plans, chunks, the edges out of those chunks, their runs); 0 = everything;
     apply: the bill edited and sent back, one transaction — a stale rev rejects it and
     writes nothing; a header key left out stays, a section is matched by key (a new key
     needs a title, {key, drop:true} drops it, one left out stays), a line without uid
     adds a chunk (no brief = draft), a line with uid edits it (a changed field clears its
     approval), {uid, drop:true} drops it, a line missing from the bill is left alone, and
     any change to a running or finished chunk rejects the bill; an optional \"ops\" array
     takes the `chunk apply` ops; every row shares one event batch; --dry-run prints what
     it would write and rolls back;
     park|unpark|done: the plan's state — parked or done leaves the board; refused while
     one of its chunks runs)",
    ),
    (
        "run",
        "end one chunk start, or mark it seen",
        "fael run end R [--cost USD] | fael run beat R
    (ends every live run row of start R — the launcher's cleanup after the agent
     exits; never another start, never a chunk's state; none left exits 0; claims
     each row's branch diff as done would, and done keeps it (a lead rebases the
     lane after), and says each path outside the declared scope as done does; beat:
     start R's live rows are seen now — the launcher calls it while a headless client
     runs, so a long run never reads stalled)
  --cost USD   what the client reported the run cost (claude's total_cost_usd), a
               decimal >= 0; kept on start R's rows and shown as run.cost on the board",
    ),
    (
        "restore",
        "revert a supersede edge",
        "fael restore [<id>] [--edge id]
    (revert a supersede edge with an event row — the row keeps its id and opens
     again; <id> reopens that row when one edge still hides it, --edge names
     the superseding row when several do; an already-open row or an
     already-reverted edge is info, never an error)",
    ),
    (
        "purge",
        "delete a row for good",
        "fael purge <id>
    (permanently remove a leaked test row or a mistake: the row and its close
     and bump events go from every month file, tree and journal; refused when
     another row supersedes or restores it, when the id names a close event
     (use fael restore for those), when it lives in an immutable compact file,
     or when the file has lines `read` would skip — run `fael doctor --fix` first;
     copies already in another clone stay until purged there too)",
    ),
    (
        "hook",
        "stdin in, stdout out; always exits 0",
        "fael hook <event> [--client c]
    (event: stop, session-start, read, edit or search; stdin in, stdout out; always exits 0)",
    ),
    (
        "stats",
        "tokens fael has put into context",
        "fael stats [--json] [--rows] [--day] [--misses] [--since d]
    (tokens fael has put into context, per machine;
     --since = only usage from that day (YYYY-MM-DD, UTC) or RFC 3339 time on,
     all = every month; default = this and last month;
     --rows = per-row pushes against open/closed/superseded, flagging noise?, plus
     what followed (shown, cut by reason, cited, pulled, acted, retrieved after cut);
     --misses = the newest empty text searches (machine-local), each with the
     per-word counts — the data a vector index would be judged by;
     --day = today's panels per repo and summed (local day: FAEL_TZ_OFFSET
     like +07:00 wins, else the machine zone); --day --json prints DayView)",
    ),
    (
        "report",
        "one offline HTML page for a lead",
        "fael report [--out f] [--open] [--since d]
    (one offline HTML page answering three questions: what memory reached the
     agents, what is noise, did fael add friction — every number is a field of
     `fael stats --json` for the same --since (default: this and last month); default --out is report.html in
     the state dir, never .fael/; --open hands it to the OS opener)",
    ),
    (
        "doctor",
        "check the log; --fix repairs what it can",
        "fael doctor [--fix] [--fat]
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
      problem's full row ids for a cleanup pass; --fat lists every fat row,
      including pre-self-heal legacy rows that stay collapsed to one line by
      default)",
    ),
    (
        "compact",
        "fold old rows per writer",
        "fael compact [--writer id] [--before yyyy-mm] [--prune]
    (fold old rows into per-writer summaries; --prune deletes)",
    ),
    (
        "import",
        "import another clone's log",
        "fael import <path> [--map old/=new/]
    (import rows from a log file, a log dir or a repo root (its journal plus any .fael/log); a close folds into its
     row; importing twice is safe; --map rewrites a path prefix, repeatable)",
    ),
    (
        "mcp",
        "MCP server on stdio",
        "fael mcp [--pin]
    (serve find/add/close over stdio for MCP clients)
    --pin: every call acts on this cwd only — set it before exposing over HTTP",
    ),
    (
        "install",
        "install hooks and skills for a client",
        "fael install [--client claude|codex|opencode] [--dry-run]
    (install hooks and skills for an agent client; --dry-run prints without writing)",
    ),
    (
        "upgrade",
        "update the binary, hooks and skills (alias: update)",
        "fael upgrade [--client claude|codex|opencode] [--dry-run] [--yes]
    (updates the binary by the channel it came from — brew, npm or the shell/ps1 installer's fael-update — then the new binary writes hooks and skills; asks first, --yes skips the question, --dry-run shows the channel, the command and the wiring changes; `fael update` is the same; session-start runs `--auto` itself once a day, detached — off with FAEL_NO_AUTO_UPDATE=1 or `auto_update = false` in ~/.config/fael/config.toml)",
    ),
];
