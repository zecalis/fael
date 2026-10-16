//! fael.db text never holds a secret: a brief, note or hand-off goes to the model provider
//! and out through `fael board --json` and `fael plan export`. The check is
//! [`crate::secret`], the one rows use; it runs in temp triggers on this connection, so every
//! write path (add, edit, apply, import, done, note, wait) passes it without its own call.

use super::store::err;
use rusqlite::Connection;
use rusqlite::functions::FunctionFlags;

/// `table: columns` — the free text a person or agent types. A column outside this list
/// is a path, a name or a state.
const TEXT: [(&str, &str); 4] = [
    ("plan", "title"),
    ("chunk", "title, label, brief, wait_text"),
    ("event", "text"),
    ("section", "title, body"),
];

pub(super) fn guard(conn: &Connection) -> Result<(), String> {
    conn.create_scalar_function(
        "fael_secret",
        -1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            for i in 0..ctx.len() {
                if let Some(what) = ctx
                    .get::<Option<String>>(i)?
                    .as_deref()
                    .and_then(crate::secret)
                {
                    return Ok(Some(what));
                }
            }
            Ok(None)
        },
    )
    .map_err(err)?;
    let mut sql = String::new();
    for (t, cols) in TEXT {
        // a build that never creates the table never writes it either
        let has: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM main.sqlite_master WHERE type = 'table' AND name = ?1",
                [t],
                |r| r.get(0),
            )
            .map_err(err)?;
        if !has {
            continue;
        }
        let new: Vec<String> = cols.split(", ").map(|c| format!("NEW.{c}")).collect();
        let when = format!("fael_secret({}) IS NOT NULL", new.join(", "));
        // RAISE takes a literal only: the label stays out, the table says where
        let raise = format!(
            "SELECT RAISE(ABORT, 'rejected: {t} text looks like a secret (key, token, password) — remove it; fael.db text goes to the model and the board');"
        );
        sql += &format!(
            "CREATE TEMP TRIGGER IF NOT EXISTS fael_secret_{t}_ins BEFORE INSERT ON main.{t}
               WHEN {when} BEGIN {raise} END;
             CREATE TEMP TRIGGER IF NOT EXISTS fael_secret_{t}_upd BEFORE UPDATE OF {cols} ON main.{t}
               WHEN {when} BEGIN {raise} END;"
        );
    }
    conn.execute_batch(&sql).map_err(err)
}

#[cfg(test)]
mod tests {
    use crate::plan::{Fields, Store};

    const NOW: &str = "2026-10-11T09:00:00Z";

    fn brief(b: &str) -> Fields {
        Fields {
            title: Some("deploy the board".into()),
            brief: Some(b.into()),
            ..Fields::default()
        }
    }

    #[test]
    fn a_secret_never_lands_in_the_db() {
        let mut s = Store::open_in_memory().unwrap();
        let key = format!("ghp_{}", "a".repeat(36));
        let e = s
            .add("inbox", &brief(&format!("token {key}")), &[], NOW)
            .unwrap_err();
        assert!(
            e.contains("chunk text looks like a secret") && !e.contains(&key),
            "{e}"
        );
        let uid = s.add("inbox", &brief("ship it"), &[], NOW).unwrap();
        // a db URL with its password is the common .env line
        let url = "DATABASE_URL=postgres://app:hunter2@db.internal:5432/app";
        assert!(s.edit(&uid, &brief(url), NOW).is_err());
        let n: i64 = s
            .conn
            .query_row(
                "SELECT COUNT(*) FROM chunk WHERE brief LIKE '%hunter2%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn a_secret_never_lands_in_a_plan_section() {
        let mut s = Store::open_in_memory().unwrap();
        s.new_plan("", "p", "P", "dev", None, NOW).unwrap();
        let key = format!("ghp_{}", "a".repeat(36));
        let e = s
            .conn
            .execute("UPDATE section SET body = ?1", [format!("token {key}")])
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("section text looks like a secret") && !e.contains(&key),
            "{e}"
        );
    }
}
