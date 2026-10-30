//! `chunk add --wait` and the `drop?` auto-close (sweep follow-ups): question
//! chunks in one command, and a dropped `drop?` chunk taking its issue with
//! it. Split from `chunk.rs` at the file-size ratchet — the dispatcher keeps
//! the arms, the how lives here.

use crate::plan::db;
use crate::{Args, Repo};
use fael_crew::{Here, Store};

/// `add --wait owner|data --ask "<q>"`: start without a worktree, then wait,
/// both under one R. The sweep wrote add + start --no-worktree + wait per
/// question (27 commands for 9); this is the one-command form.
pub(crate) fn add_wait(
    a: &Args,
    st: &mut Store,
    uid: &str,
    now: &str,
    wt: &str,
    branch: &Option<String>,
) -> Result<(), String> {
    let Some(on) = a.one("wait") else {
        return Ok(());
    };
    if !matches!(on.as_str(), "owner" | "data") {
        return Err("rejected: --wait takes owner|data — with --ask \"<question>\"".into());
    }
    if on == "data" && !a.many("option").is_empty() {
        return Err("rejected: --option needs --wait owner".into());
    }
    let q = a
        .one("ask")
        .ok_or("rejected: --wait needs --ask \"<question>\" — the text the owner reads")?;
    // validate before the start commits: a bad --until after a start leaves the
    // chunk running with a live no-worktree run (core's YYYY-MM-DD rule)
    if let Some(u) = a.one("until")
        && !is_date(&u)
    {
        return Err("rejected: --until takes a date YYYY-MM-DD".into());
    }
    // c5: a data wait names its end date — shape-checked before the start commits.
    if on == "data" && a.one("until").is_none() {
        return Err(
            "rejected: --wait data needs --until YYYY-MM-DD — the date that ends the wait".into(),
        );
    }
    let run = a
        .one("run")
        .unwrap_or_else(|| format!("wait-{}", &uid[..8.min(uid.len())]));
    let start = fael_crew::Start {
        run: Some(run.clone()),
        client: None,
        model: None,
        no_worktree: true,
        force: false,
        session: None,
        base: None,
        shared: Vec::new(),
        stale: String::new(),
        with: None,
        joint: None,
    };
    let here = || Here {
        worktree: wt,
        branch: branch.as_deref(),
        now,
        run: Some(&run),
    };
    st.start(uid, &start, &here())?;
    let (until, options) = (a.one("until"), a.many("option"));
    st.wait(uid, &on, &q, until.as_deref(), &options, &here())?;
    waiting(uid, &on, &q);
    Ok(())
}

/// The wait's receipt, then its format line when earned (issue 01M5Y6H4).
pub(crate) fn waiting(uid: &str, on: &str, text: &str) {
    println!("chunk {uid} → waiting on {on}");
    if let Some(h) = fael_core::work::wait_hint(on, text) {
        println!("{h}");
    }
}

/// A `drop?` chunk the owner drops takes its issue with it: the sweep tagged
/// the verdict in the title or brief, and the issue is not a bug (already
/// fixed, not a bug, outside this repo, not worth it). A fix chunk dropped
/// keeps its issue open — only `drop?` auto-closes. Call after `owner(drop)`.
pub(crate) fn drop_close(r: &Repo, uid: &str, text: &str) {
    let linked: Vec<String> = Store::open(&db(r))
        .ok()
        .and_then(|s| s.show(uid).ok())
        .map(|b| {
            let t = format!("{}\n{}", b.title, b.brief).to_lowercase();
            if t.contains("drop?") {
                b.issues.clone()
            } else {
                vec![]
            }
        })
        .unwrap_or_default();
    for id in linked {
        let short = id.split_whitespace().next().unwrap_or(&id);
        match crate::fael_bin::close(short, &format!("chunk {uid} dropped: {text}")) {
            Ok(()) => println!("closed {short} (dropped with {uid})"),
            Err(e) => eprintln!("skip: close {short}: {e}"),
        }
    }
}

/// YYYY-MM-DD, mirroring `fael_crew`'s date rule for `--until`.
fn is_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// The command and the state it lands in.
pub(crate) fn owner(verb: &str) -> (fael_crew::Owner, &'static str) {
    use fael_crew::Owner;
    match verb {
        "answer" => (Owner::Answer, "open"),
        "accept" => (Owner::Accept, "done"),
        "drop" => (Owner::Drop, "dropped"),
        "park" => (Owner::Park, "parked"),
        _ => (Owner::Unpark, "open"),
    }
}
