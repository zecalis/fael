//! A push's count lines: what the row cap and the budget hid, each naming
//! the exact call that reaches it.

use crate::core;

/// The directory calls that reach the hidden same-dir ring: every distinct
/// parent of the queried files, each with a trailing `/` (a directory query).
/// `None` when no file sits in a directory (root-level files).
fn dirs_arg(files: &[String]) -> Option<String> {
    let mut dirs: Vec<&str> = files
        .iter()
        .filter_map(|f| f.rsplit_once('/').map(|(d, _)| d))
        .collect();
    dirs.sort_unstable();
    dirs.dedup();
    (!dirs.is_empty()).then(|| {
        dirs.iter()
            .map(|d| format!("{d}/"))
            .collect::<Vec<_>>()
            .join(",")
    })
}

/// The count lines under the rendered rows — one per class, each naming the
/// exact call that reaches it: tier-0 cuts by the file (the row cap and the
/// budget cut), the same-dir ring by the query's directory, and each hidden
/// key (folded into one line past the first). `rendered` is how many rows render actually said. `hidden` routes the
/// budget cut by each row's L1 tier too, so a budget-cut same-dir or
/// shared-key row (a Now row the cap never touched) names the right call.
/// Each line comes with what it counts (`file`, `dir:<dirs>`, `key:<key>`,
/// `keys`), its once-per-session key beside the file set.
pub(crate) fn counts(
    sel: &core::Selection,
    rendered: usize,
    files: &[String],
) -> Vec<(String, String)> {
    let mut out = vec![];
    let hidden = sel.hidden(rendered);
    if hidden.file > 0 {
        let what = if files.len() == 1 {
            "this file"
        } else {
            "these files"
        };
        out.push((
            "file".into(),
            format!(
                "… +{} more about {what} — fael find --files {}",
                hidden.file,
                crate::find::quoted(&files.join(","))
            ),
        ));
    }
    if hidden.dirs > 0
        && let Some(dirs) = dirs_arg(files)
    {
        out.push((
            format!("dir:{dirs}"),
            format!(
                "… +{} more in {dirs} — fael find --files {}",
                hidden.dirs,
                crate::find::quoted(&dirs)
            ),
        ));
    }
    match hidden.keys.as_slice() {
        [] => {}
        [(key, n)] => out.push((
            format!("key:{key}"),
            format!(
                "… +{n} more with #{key} — fael find --key {}",
                crate::find::quoted(key)
            ),
        )),
        // one line however many keys: a file whose rows carry a dozen keys
        // used to spend more tokens on the footer than on the rows
        many => {
            let mut top = many.to_vec();
            top.sort_by_key(|a| std::cmp::Reverse(a.1)); // stable: ties keep encounter order
            let named: Vec<String> = top
                .iter()
                .take(3)
                .map(|(k, n)| format!("#{k} ({n})"))
                .collect();
            let rest = match many.len() - named.len() {
                0 => String::new(),
                r => format!(", +{r} keys"),
            };
            out.push((
                "keys".into(),
                format!(
                    "… +{} more under {} keys: {}{rest} — fael find --key <key>",
                    many.iter().map(|(_, n)| n).sum::<usize>(),
                    many.len(),
                    named.join(", ")
                ),
            ));
        }
    }
    out
}
