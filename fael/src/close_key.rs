//! `fael close --key <key> "<why>"` and MCP `close` with `key`: the agent
//! names the topic instead of looking the id up. One open row on the key →
//! its id; none or several → an error that closes nothing (fael never picks).

use crate::core::{self, Filter};

pub(crate) const BOTH: &str = "rejected: close takes an id or a key, not both";

/// The id of the one open row carrying exactly `key`.
pub(crate) fn open_row_on(r: &crate::Repo, key: &str) -> Result<String, String> {
    let log = crate::read(r);
    let f = Filter {
        key: Some(key.into()),
        ..Filter::default()
    };
    // the filter is a glob: keep only the rows whose key is exactly this one
    let (rows, _, _) = core::query(&log, &f, &r.cfg);
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|x| x.key.as_deref() == Some(key))
        .collect();
    match rows.as_slice() {
        [] => Err(format!("rejected: no open row on {key}")),
        [row] => Ok(row.id.clone()),
        many => {
            let ab = core::abbrev(&log);
            let list: Vec<String> = many
                .iter()
                .map(|x| {
                    let id = ab.short(&x.id);
                    format!(
                        "- [{id}] {} {} — fael close {id} \"<why>\"",
                        x.kind,
                        x.display_title()
                    )
                })
                .collect();
            Err(format!(
                "rejected: {} open rows on {key} — closed none; pick one:\n{}",
                many.len(),
                list.join("\n")
            ))
        }
    }
}

/// CLI: `fael close --key <key> "<why>"` — `rest` is what follows `close`.
pub(crate) fn cli(
    a: &crate::Args,
    key: &str,
    rest: &[String],
) -> Result<std::process::ExitCode, String> {
    let [why] = rest else {
        return Err(match rest.len() {
            0 => "rejected: close needs a reason — fael close --key <key> \"<why>\"".into(),
            _ => BOTH.into(),
        });
    };
    let id = open_row_on(&crate::repo()?, key)?;
    crate::batch::batch_close(a, &[id], why)
}
