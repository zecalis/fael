use crate::ROW_BYTES_MAX;
use serde::Deserialize;

/// Where `add`/`close`/`bump`/`mv` rows land (PLAN-fael-durable-log chunk 1):
/// `tracked` writes the tree log (`.fael/log`, the transport) on top of the
/// journal; `local` writes the journal only — for repos that gitignore `.fael`
/// or are public, where the tree must carry no memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Store {
    #[default]
    Tracked,
    Local,
}

/// What a cross-key self-heal act prints (PLAN-fael-selfheal-verdict chunk
/// 3): the key moved, so the act is the weakest-evidence one — `warn` says so
/// in one `warning:` line (an ask), `info` keeps the info line, `off` acts
/// silently. The act itself always happens: holding would pile notes back
/// into the Stop-hook debt the rule exists to drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossKey {
    #[default]
    Warn,
    Info,
    Off,
}

/// Per-repo settings from `.fael/config.toml` (every field has a default; see `Config::from_toml`).
#[derive(Debug, Clone)]
pub struct Config {
    /// Extra kinds this repo declares on top of `CORE_KINDS`.
    pub kinds: Vec<String>,
    /// Row size limit in bytes — clamped to `ROW_BYTES_MAX`.
    pub row_bytes: usize,
    /// Allowed first segments of `key`; empty = any. Outside it is a warning, never a reject.
    pub key_domains: Vec<String>,
    /// Which `<PREFIX><name>.md` filenames widen kickoff to a
    /// `<prefix>:<name>` anchor (`PLAN-` → `plan:`, so another team's
    /// `HANDOFF-*.md` maps to `handoff:*`). Empty matches nothing; the
    /// default keeps the long-standing `PLAN-` behaviour.
    pub anchor_prefixes: Vec<String>,
    /// Token budget for the session brief (`kickoff`, `find` with no filter).
    pub kickoff_tokens: usize,
    /// Token budget for `find` output.
    pub find_tokens: usize,
    /// Token budget for the read/edit hook push.
    pub push_tokens: usize,
    /// At most this many rows per read/edit push (PLAN-fael-push-focus
    /// chunk 1). 0 = no row cap, token budget only.
    pub push_rows: usize,
    /// The push policy a repo runs on search pushes (`query::GATES`): unset =
    /// `auto`, the repo's own stage machine (shadow → canary → ramp, rolled
    /// back on evidence); a set value is a human's pin and always wins —
    /// `baseline@1` opts out, `touch@1` is the validation experiment
    /// (SPEC-fael-learn-loop §E).
    pub push_policy: String,
    /// Share of sessions (percent) held out on `baseline@1` while `touch@1`
    /// is pinned. `auto` splits by stage instead.
    pub push_holdout: usize,
    /// How many of the freshest open decisions session-start lists above the
    /// count line (PLAN-fael-direction chunk 1). 0 = count line only.
    pub session_decisions: usize,
    /// Warn when a row's text is estimated over this many tokens.
    pub warn_row_tokens: usize,
    /// Warn when a row's text is over this many characters — catches a long
    /// single-topic-looking row the token estimate still reads as cheap
    /// (was the hardcoded 600 in `fat_reasons`).
    pub warn_row_chars: usize,
    /// Resolve renamed paths through the L2 alias set (`git log -M` + `fael mv`
    /// rows). `false` returns to pre-resolver matching — the escape hatch.
    pub resolve: bool,
    /// Tree transport on top of the journal (`tracked`), or journal only
    /// (`local`, for gitignored or public repos). Ignored without a journal
    /// (no git): the tree is all there is.
    pub store: Store,
    /// `store` was written in `config.toml`. When it was not, the adapter
    /// picks `local` — a tree log already in the repo is read, never written.
    pub store_set: bool,
    /// Stop-hook phrase packs behind `[lang] marker` (PLAN-fael-languages).
    /// Default english+thai; empty switches the bug rule off entirely.
    pub lang_marker: Vec<String>,
    /// Accepted row-writing languages behind `[lang] rows` — a row with a
    /// letter outside every accepted script warns once, never rejects.
    /// Empty switches the check off (mirrors `lang_marker = []`).
    pub lang_rows: Vec<String>,
    /// Exposure of a cross-key self-heal act (`[selfheal] cross_key`).
    pub cross_key: CrossKey,
    /// `[sync] auto` — the Stop hook runs `fael sync` once per session when
    /// `fael.remote` is set. `false` is the off-switch; the manual command
    /// is unaffected.
    pub sync_auto: bool,
    /// `[notify] user` (PLAN-fael-visible-secretary chunk 4): `true`
    /// (default) = the hooks report to the user in one line per beat (brief,
    /// reminder, receipt) on a channel the agent never reads; `false` = off.
    pub notify_user: bool,
    /// `[hint] stop` (01M42GFH): prompt words the key hint never matches, ASCII
    /// case-insensitive — generic English heads like "workspace" that would
    /// fire `vela:workspace-icons` on every prompt saying them. Fed from the
    /// `via "<word>"` the hint line prints. Empty = no word is stopped. A key
    /// typed whole in the prompt still hints.
    pub hint_stop: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            kinds: vec![],
            row_bytes: ROW_BYTES_MAX,
            key_domains: vec![],
            anchor_prefixes: vec!["PLAN-".into()],
            kickoff_tokens: 800,
            find_tokens: 800,
            push_tokens: 800,
            push_rows: 5,
            push_policy: crate::AUTO.into(),
            push_holdout: 20,
            session_decisions: 0,
            warn_row_tokens: 400,
            warn_row_chars: 1200,
            resolve: true,
            store: Store::Tracked,
            store_set: false,
            lang_marker: vec!["english".into(), "thai".into()],
            lang_rows: vec!["english".into()],
            cross_key: CrossKey::Warn,
            sync_auto: true,
            notify_user: true,
            hint_stop: vec![],
        }
    }
}

/// `[notify]` in `.fael/config.toml` — outside `from_toml` only to keep that
/// function under the line lint.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Notify {
    user: Option<bool>,
}

/// `[sync]` in `.fael/config.toml` — outside `from_toml` for the same reason.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Sync {
    auto: Option<bool>,
}

/// `[hint]` in `.fael/config.toml` — outside `from_toml` only to keep that
/// function under the line lint.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Hint {
    stop: Vec<String>,
}

impl Config {
    /// Parse `.fael/config.toml` text — every field optional. The caller reads the bytes
    /// from wherever the repo lives; a missing file is `Config::default()`, not this.
    pub fn from_toml(s: &str) -> Result<Config, String> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct File {
            kinds: Vec<String>,
            key_domains: Vec<String>,
            anchor: Anchor,
            resolve: Option<bool>,
            store: Option<String>,
            push_policy: Option<String>,
            push_holdout: Option<usize>,
            budget: Budget,
            warn: Warn,
            limit: Limit,
            lang: Lang,
            selfheal: Selfheal,
            sync: Sync,
            notify: Notify,
            hint: Hint,
        }

        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Budget {
            kickoff_tokens: Option<usize>,
            find_tokens: Option<usize>,
            push_tokens: Option<usize>,
            push_rows: Option<usize>,
            session_decisions: Option<usize>,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Warn {
            row_tokens: Option<usize>,
            row_chars: Option<usize>,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Limit {
            row_bytes: Option<usize>,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Anchor {
            prefixes: Option<Vec<String>>,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Lang {
            marker: Option<Vec<String>>,
            rows: Option<Vec<String>>,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Selfheal {
            cross_key: Option<String>,
        }
        let f: File = toml::from_str(s).map_err(|e| e.to_string())?;
        let d = Config::default();
        let (push_policy, push_holdout) = check_push(f.push_policy, f.push_holdout, &d)?;
        let store = match f.store.as_deref() {
            None | Some("tracked") => Store::Tracked,
            Some("local") => Store::Local,
            Some(v) => {
                return Err(format!(
                    "rejected: store = {v:?} — want \"tracked\" or \"local\""
                ));
            }
        };
        Ok(Config {
            kinds: f.kinds,
            key_domains: f.key_domains,
            anchor_prefixes: f.anchor.prefixes.unwrap_or(d.anchor_prefixes),
            row_bytes: f.limit.row_bytes.unwrap_or(d.row_bytes),
            kickoff_tokens: f.budget.kickoff_tokens.unwrap_or(d.kickoff_tokens),
            find_tokens: f.budget.find_tokens.unwrap_or(d.find_tokens),
            push_tokens: f.budget.push_tokens.unwrap_or(d.push_tokens),
            push_rows: f.budget.push_rows.unwrap_or(d.push_rows),
            push_policy,
            push_holdout,
            session_decisions: f.budget.session_decisions.unwrap_or(d.session_decisions),
            warn_row_tokens: f.warn.row_tokens.unwrap_or(d.warn_row_tokens),
            warn_row_chars: f.warn.row_chars.unwrap_or(d.warn_row_chars),
            resolve: f.resolve.unwrap_or(d.resolve),
            store,
            store_set: f.store.is_some(),
            lang_marker: check_lang("marker", f.lang.marker.unwrap_or(d.lang_marker))?,
            lang_rows: check_lang("rows", f.lang.rows.unwrap_or(d.lang_rows))?,
            cross_key: check_cross_key(f.selfheal.cross_key)?,
            sync_auto: f.sync.auto.unwrap_or(d.sync_auto),
            notify_user: f.notify.user.unwrap_or(d.notify_user),
            hint_stop: f
                .hint
                .stop
                .iter()
                .map(|w| w.trim().to_ascii_lowercase())
                .filter(|w| !w.is_empty())
                .collect(),
        })
    }
}

/// `push_policy` / `push_holdout` — an unknown policy or a share past 100 is a
/// config error: a silent typo would run (or skip) the experiment.
fn check_push(
    policy: Option<String>,
    holdout: Option<usize>,
    d: &Config,
) -> Result<(String, usize), String> {
    let policy = policy.unwrap_or_else(|| d.push_policy.clone());
    if !crate::query::GATES.contains(&policy.as_str()) {
        return Err(format!(
            "rejected: push_policy = {policy:?} — want one of {:?}",
            crate::query::GATES
        ));
    }
    match holdout.unwrap_or(d.push_holdout) {
        h if h > 100 => Err("rejected: push_holdout is a percent, 0–100".into()),
        h => Ok((policy, h)),
    }
}

/// Parse `[selfheal] cross_key` — anything outside `warn|info|off` is a
/// config error, same reject style as `store`: a silent typo would flip a
/// cross-key act from an ask to silence.
fn check_cross_key(v: Option<String>) -> Result<CrossKey, String> {
    match v.as_deref() {
        None | Some("warn") => Ok(CrossKey::Warn),
        Some("info") => Ok(CrossKey::Info),
        Some("off") => Ok(CrossKey::Off),
        Some(v) => Err(format!(
            "rejected: [selfheal] cross_key = {v:?} — want \"warn\"|\"info\"|\"off\""
        )),
    }
}

/// Reject an unknown `[lang]` pack name — silently matching nothing would
/// leave the hook blind, worse than an error. Same reject style as `store`;
/// keep the want-list in sync with `lang::by_name`.
fn check_lang(key: &str, names: Vec<String>) -> Result<Vec<String>, String> {
    for n in &names {
        if crate::lang::by_name(n).is_none() {
            return Err(format!(
                "rejected: [lang] {key} = {n:?} — want english|thai \
                 (add a pack in fael-core/src/lang.rs or file an issue)"
            ));
        }
    }
    Ok(names)
}

/// `auto_update` in the per-machine `~/.config/fael/config.toml` (PLAN-fael-auto-update
/// chunk 3): `false` stops session-start from updating the binary. A file that
/// cannot be parsed reads as off — an opt-out that failed to load must not be ignored.
pub fn auto_update_on(toml: &str) -> bool {
    #[derive(Deserialize)]
    struct Machine {
        auto_update: Option<bool>,
    }
    toml::from_str::<Machine>(toml).is_ok_and(|m| m.auto_update.unwrap_or(true))
}

#[cfg(test)]
mod machine_tests {
    use super::auto_update_on;

    #[test]
    fn only_an_explicit_false_or_a_broken_file_turns_it_off() {
        assert!(auto_update_on(""));
        assert!(auto_update_on("auto_update = true\nother = 1"));
        assert!(!auto_update_on("auto_update = false"));
        assert!(!auto_update_on("auto_update = \"no\""));
    }
}
