//! `fael vault ingest` — the human side of the vault: `<vault>/inbox/*.md`
//! filed as `from: user` rows through the `fael add` write path.

use crate::{Repo, core};
use std::path::Path;

/// Anchor filed when an inbox note names no files: anchors pass the write
/// path's evidence check, so the note files without inventing a path.
const INBOX_FILES: &str = "doc:inbox";

/// `fael vault ingest` — file every `inbox/*.md` as a `from: user` row,
/// deleting each only after its row is written. The write path's `refresh`
/// projects the new row in the same call, so its file appears as the inbox
/// one disappears. A file that fails stays in place with its reason on
/// stderr; any failure exits 1.
pub(super) fn ingest(r: &Repo) -> Result<(), String> {
    let Some((vault, explicit)) = super::vault_dir(r) else {
        return Err(
            "vault: no vault dir — set git config fael.vault <dir> or run in a git repo".into(),
        );
    };
    // like `refresh`: an opted-in vault starts on first use; the default one
    // only `rebuild` creates — a note dropped where no vault exists says so
    // instead of materialising one.
    if !vault.is_dir() && !explicit {
        return Err(
            "vault: no vault here yet — run `fael vault rebuild` first, then drop notes in its inbox/"
                .into(),
        );
    }
    let inbox = vault.join("inbox");
    let blocked = |e: std::io::Error| format!("vault: {inbox:?} unreadable ({e})");
    std::fs::create_dir_all(&inbox).map_err(&blocked)?;
    let mut names: Vec<String> = vec![];
    for e in std::fs::read_dir(&inbox).map_err(&blocked)?.flatten() {
        if e.path().extension().is_some_and(|x| x == "md")
            && let Some(n) = e.file_name().to_str().map(String::from)
        {
            names.push(n);
        }
    }
    names.sort();
    let mut failed = 0;
    for n in &names {
        match file(r, &inbox.join(n)) {
            Ok(id) => println!("{id} ← inbox/{n}"),
            Err(e) => {
                eprintln!("inbox/{n}: {e}");
                failed += 1;
            }
        }
    }
    if failed > 0 {
        return Err(format!(
            "rejected: {failed} inbox file(s) left in place — fix the frontmatter or body and re-run `fael vault ingest`"
        ));
    }
    Ok(())
}

/// One inbox file → one row id. The row is written before the file is deleted,
/// so a crash between leaves both; the re-run then hits the content-hash key
/// below (`recall::exact` in the write path) and files nothing twice.
fn file(r: &Repo, p: &Path) -> Result<String, String> {
    let raw = std::fs::read(p).map_err(|e| format!("unreadable ({e})"))?;
    let (meta, body) = frontmatter(&raw)?;
    if body.trim().is_empty() {
        return Err("empty body — write the note below the frontmatter".into());
    }
    let body = body.trim().to_string();
    let kind = meta.kind.unwrap_or_else(|| "note".into());
    if !matches!(kind.as_str(), "note" | "issue" | "decision") {
        return Err(format!("unknown kind {kind:?} — want note|issue|decision"));
    }
    let title = meta
        .title
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| headline(&body));
    let files = match meta.files.is_empty() {
        true => vec![INBOX_FILES.to_string()],
        false => meta.files,
    };
    // dedupe anchor: no frontmatter key → the content hash, so a second
    // ingest of the same bytes names the same open row instead of filing again
    let key = match meta.key.filter(|k| !k.trim().is_empty()) {
        Some(k) => k,
        None => format!("vault:{}", hash(&raw)?),
    };
    let opts = crate::write::AddOpts {
        key: Some(key),
        to: None,
        from: Some("user".into()),
        title: Some(title),
        revisit: None,
        urgent: core::Urgent::Unset,
        supersedes: None,
        follows: None,
        force: false,
        by: None,
    };
    let (row, _, warns) = crate::write::add_row(r, &kind, &body, &files, opts)?;
    crate::batch::emit_warns(&warns);
    std::fs::remove_file(p)
        .map_err(|e| format!("filed as {} but the inbox file stayed ({e})", row.id))?;
    Ok(row.id)
}

/// Inbox frontmatter: `kind`, `title`, `files` (comma-separated, like
/// `--files`), `key`. Unknown keys are ignored, so the projection side can add
/// fields without breaking ingest.
#[derive(Default)]
struct InboxMeta {
    kind: Option<String>,
    title: Option<String>,
    files: Vec<String>,
    key: Option<String>,
}

/// The `---` block plus the body after it, or why the file stays in place.
fn frontmatter(raw: &[u8]) -> Result<(InboxMeta, String), String> {
    let s =
        std::str::from_utf8(raw).map_err(|_| "not UTF-8 — save the file as UTF-8".to_string())?;
    let all: Vec<&str> = s.lines().collect();
    if all.first().is_none_or(|l| l.trim() != "---") {
        return Err(
            "no frontmatter — start the file with ---, then kind/title/files/key, then --- and the note"
                .into(),
        );
    }
    let end = all[1..]
        .iter()
        .position(|l| l.trim() == "---")
        .ok_or("frontmatter never closes — end the block with ---")?
        + 1;
    let mut meta = InboxMeta::default();
    for l in &all[1..end] {
        let (k, v) = l
            .split_once(':')
            .ok_or(format!("frontmatter line {l:?} is not `key: value`"))?;
        match k.trim() {
            "kind" => meta.kind = Some(unquote(v).to_lowercase()),
            "title" => meta.title = Some(unquote(v)),
            "files" => meta.files = split_files(v),
            "key" => meta.key = Some(unquote(v)),
            _ => {}
        }
    }
    Ok((meta, all[end + 1..].join("\n")))
}

/// Strip one layer of matching quotes (`title: "my note"` reads as `my note`).
fn unquote(v: &str) -> String {
    let v = v.trim();
    let b = v.as_bytes();
    if b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0] {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// `files: a.rs, b.rs` → the entries `--files` would take.
fn split_files(v: &str) -> Vec<String> {
    unquote(v)
        .split(',')
        .map(unquote)
        .filter(|s| !s.is_empty())
        .collect()
}

/// `title` absent: the body's first line, cut to 15 words — the ≤15-word
/// headline lists show.
fn headline(body: &str) -> String {
    body.lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .take(15)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 12-hex content id for the dedupe key — the same git-blob digest the write
/// path stamps on real files, so the key reads like the rest of the log.
fn hash(raw: &[u8]) -> Result<String, String> {
    let mut cur = std::io::Cursor::new(raw);
    core::blob_id_stream(&mut cur, u64::MAX)
        .map_err(|e| format!("unhashable ({e})"))?
        .ok_or_else(|| "file changed while read — re-run `fael vault ingest`".into())
}
