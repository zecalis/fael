//! `fael restore`: revert a supersede edge with an event row, never by
//! rewriting — every automatic decision stays reversible. The row keeps its
//! id; only the edge goes.

use crate::{core, hook};

/// `fael restore [<id>] [--edge <id>]`: `target` names the row to reopen,
/// `edge` names the superseding row directly. An already-open row or an
/// already-reverted edge prints info and writes nothing (exit 0).
pub(crate) fn restore(
    r: &crate::Repo,
    a: &crate::Args,
    target: Option<&str>,
) -> Result<(), String> {
    let log = crate::read(r);
    let out = core::restore_row(
        &r.fael,
        r.journal.as_deref(),
        &log,
        &r.cfg,
        &crate::stamp(r),
        target,
        a.one("edge").as_deref(),
    )?;
    out.warns.iter().for_each(|w| eprintln!("{w}"));
    hook::record_asks("cli", hook::ASK_WARN, "restore", Some(&r.root), &out.warns);
    if a.has("json") {
        match &out.row {
            Some(row) => println!("{}", row.to_line()),
            None => println!("{}", serde_json::json!({"info": out.message})),
        }
    } else {
        println!("{}", out.message);
    }
    Ok(())
}
