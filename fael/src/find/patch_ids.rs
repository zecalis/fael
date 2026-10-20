//! Patch-ids for the squash-merge proof (`merged`), cached on disk. A commit's
//! patch-id and a `base..tip` diff's never change, so `<git-common-dir>/fael/cache/
//! patch-ids` keeps them for good, shared by every worktree: lines of
//! `<key> <patch-id>`, key a commit sha or `<base>..<tip>`, `-` for a commit
//! with no patch. Append-only; a torn or foreign line is skipped on read. A
//! cache, never row state — losing it costs one proof (fael:01M5CPNJ).

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{LazyLock, Mutex};

// ponytail: a cache past this many bytes is dropped whole and rebuilt; a
// compaction pass if a repo ever outgrows it between rebuilds
const CACHE_MAX: u64 = 8 << 20;
const NONE: &str = "-";

// `head_patch_ids` per (root, base, HEAD sha): one `rev-list` per process
// however many tips share the merge base. Dropped wholesale past MEMO_MAX.
type Range = HashMap<(PathBuf, String, String), Vec<String>>;
static RANGE: LazyLock<Mutex<Range>> = LazyLock::new(Default::default);
pub(super) const MEMO_MAX: usize = 256;

#[cfg(test)]
pub(super) fn forget_memo() {
    RANGE.lock().unwrap().clear();
}

struct Cache {
    file: Option<PathBuf>,
    map: HashMap<String, String>,
}

impl Cache {
    fn load(root: &Path) -> Self {
        let file = crate::journal::root(root).map(|j| j.join("cache").join("patch-ids"));
        let mut map = HashMap::new();
        if let Some(f) = &file {
            if std::fs::metadata(f).is_ok_and(|m| m.len() > CACHE_MAX) {
                let _ = std::fs::remove_file(f);
            }
            for l in std::fs::read_to_string(f).unwrap_or_default().lines() {
                if let Some((k, v)) = l.split_once(' ') {
                    map.insert(k.to_string(), v.to_string());
                }
            }
        }
        Cache { file, map }
    }

    /// Fails open: unwritable = the next process proves it again.
    fn put(&mut self, entries: Vec<(String, String)>) {
        let body: String = entries.iter().map(|(k, v)| format!("{k} {v}\n")).collect();
        self.map.extend(entries);
        let Some(f) = &self.file else { return };
        let _ = f.parent().map(std::fs::create_dir_all);
        // one write on an O_APPEND handle: two worktrees never interleave a line
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(f)
            .and_then(|mut h| h.write_all(body.as_bytes()));
    }
}

/// The patch-id of `git diff <base> <tip>`, `None` when it has none or git failed.
pub(super) fn diff_id(root: &Path, base: &str, tip: &str) -> Option<String> {
    let key = format!("{base}..{tip}");
    let mut cache = Cache::load(root);
    if let Some(v) = cache.map.get(&key) {
        return (v != NONE).then(|| v.clone());
    }
    let src = spawn(root, &["diff", base, tip], false)?;
    let id = pipe_ids(root, src)?.pop().map(|(p, _)| p);
    cache.put(vec![(key, id.clone().unwrap_or_else(|| NONE.into()))]);
    id
}

/// The patch-ids of `git log -p --no-merges base..HEAD`. Only the commits the
/// cache has not seen are diffed, in one `log -p --stdin`; `head` (HEAD's sha,
/// when known) memoises the answer for the rest of the process.
pub(super) fn head_ids(root: &Path, base: &str, head: Option<&str>) -> Vec<String> {
    let key = head.map(|h| (root.to_path_buf(), base.to_string(), h.to_string()));
    if let Some(v) = key
        .as_ref()
        .and_then(|k| RANGE.lock().ok()?.get(k).cloned())
    {
        return v;
    }
    let v = range_ids(root, base);
    if let (Some(k), Ok(mut m)) = (key, RANGE.lock()) {
        if m.len() >= MEMO_MAX {
            m.clear();
        }
        m.insert(k, v.clone());
    }
    v
}

fn range_ids(root: &Path, base: &str) -> Vec<String> {
    let Some(list) =
        super::merged::git(root, &["rev-list", "--no-merges", &format!("{base}..HEAD")])
    else {
        return vec![];
    };
    let commits: Vec<&str> = list.lines().collect();
    let mut cache = Cache::load(root);
    let new: Vec<&str> = commits
        .iter()
        .copied()
        .filter(|c| !cache.map.contains_key(*c))
        .collect();
    if !new.is_empty() {
        // `log` rather than `diff-tree`: same rename and format config as the
        // range walk, so the ids are the ones `log -p base..HEAD` gives
        if let Some(found) = log_ids(root, &new) {
            let mut put: Vec<(String, String)> = new
                .iter()
                .map(|c| (c.to_string(), NONE.to_string()))
                .collect();
            put.extend(found.into_iter().map(|(p, c)| (c, p)));
            // a later entry wins on read, so the found id overrides its `-`
            cache.put(put);
        }
    }
    commits
        .iter()
        .filter_map(|c| cache.map.get(*c))
        .filter(|v| *v != NONE)
        .cloned()
        .collect()
}

fn log_ids(root: &Path, shas: &[&str]) -> Option<Vec<(String, String)>> {
    let mut src = spawn(
        root,
        &["log", "-p", "--no-merges", "--no-walk=unsorted", "--stdin"],
        true,
    )?;
    // git reads every rev before it walks, so writing them all first cannot stall
    let mut w = src.stdin.take()?;
    w.write_all(shas.join("\n").as_bytes()).ok()?;
    drop(w);
    pipe_ids(root, src)
}

fn spawn(root: &Path, args: &[&str], stdin: bool) -> Option<Child> {
    #[cfg(test)]
    super::merged::SPAWNED.with(|s| s.borrow_mut().push(args.join(" ")));
    Command::new("git")
        .args(args)
        .current_dir(root)
        .stdin(if stdin { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// `src | git patch-id --stable` as (patch-id, commit) pairs; `None` when either
/// side failed. A real pipe, so neither side buffers a whole `log -p` in memory.
fn pipe_ids(root: &Path, mut src: Child) -> Option<Vec<(String, String)>> {
    let pipe = src.stdout.take()?;
    let out = Command::new("git")
        .args(["patch-id", "--stable"])
        .current_dir(root)
        .stdin(pipe)
        .stderr(Stdio::null())
        .output();
    let src_ok = src.wait().is_ok_and(|s| s.success());
    let out = out.ok().filter(|o| o.status.success() && src_ok)?;
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect(),
    )
}
