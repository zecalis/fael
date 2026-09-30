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
    /// Stop-hook phrase packs behind `[lang] marker` (PLAN-fael-languages).
    /// Default english+thai; empty switches the bug rule off entirely.
    pub lang_marker: Vec<String>,
    /// Accepted row-writing languages behind `[lang] rows` — a row with a
    /// letter outside every accepted script warns once, never rejects.
    /// Empty switches the check off (mirrors `lang_marker = []`).
    pub lang_rows: Vec<String>,
    /// Exposure of a cross-key self-heal act (`[selfheal] cross_key`).
    pub cross_key: CrossKey,
    /// `[capture] block` (PLAN-fael-dev-adoption chunk 1): `false` (default)
    /// = the Stop hook only files the reply's `fael <kind>:` lines and never
    /// blocks; `true` = the opt-in enforcement mode (block a turn that did
    /// work with no row). Capture itself never needs it.
    pub capture_block: bool,
    /// `[sync] auto` — the Stop hook runs `fael sync` once per session when
    /// `fael.remote` is set. `false` is the off-switch; the manual command
    /// is unaffected.
    pub sync_auto: bool,
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
            session_decisions: 0,
            warn_row_tokens: 400,
            warn_row_chars: 1200,
            resolve: true,
            store: Store::Tracked,
            lang_marker: vec!["english".into(), "thai".into()],
            lang_rows: vec!["english".into()],
            cross_key: CrossKey::Warn,
            capture_block: false,
            sync_auto: true,
        }
    }
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
            budget: Budget,
            warn: Warn,
            limit: Limit,
            lang: Lang,
            selfheal: Selfheal,
            capture: Capture,
            sync: Sync,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Sync {
            auto: Option<bool>,
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
        struct Capture {
            block: Option<bool>,
        }
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Selfheal {
            cross_key: Option<String>,
        }
        let f: File = toml::from_str(s).map_err(|e| e.to_string())?;
        let d = Config::default();
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
            session_decisions: f.budget.session_decisions.unwrap_or(d.session_decisions),
            warn_row_tokens: f.warn.row_tokens.unwrap_or(d.warn_row_tokens),
            warn_row_chars: f.warn.row_chars.unwrap_or(d.warn_row_chars),
            resolve: f.resolve.unwrap_or(d.resolve),
            store,
            lang_marker: check_lang("marker", f.lang.marker.unwrap_or(d.lang_marker))?,
            lang_rows: check_lang("rows", f.lang.rows.unwrap_or(d.lang_rows))?,
            cross_key: check_cross_key(f.selfheal.cross_key)?,
            capture_block: f.capture.block.unwrap_or(d.capture_block),
            sync_auto: f.sync.auto.unwrap_or(d.sync_auto),
        })
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
