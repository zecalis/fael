//! `fael close` sees the chunks that link an issue (issue 01M5AQ2T follow-up).
//! Read-only through `core::work` — fail-open, so core never depends on crew.

/// Open chunks still linked to `id`: one warning per chunk, never a reject —
/// the close already landed elsewhere is not this close's fault to refuse.
/// Each line names the drop that finishes the job.
pub(crate) fn lines(r: &crate::Repo, id: &str) -> Vec<String> {
    let db = crate::core::work::db_for(r.journal.as_deref(), &r.fael);
    crate::core::work::linked_open_chunks(&db, id)
        .into_iter()
        .map(|(uid, state)| {
            format!(
                "warning: chunk {uid} ({state}) still links {id} — done elsewhere? fael chunk drop {uid} …"
            )
        })
        .collect()
}
