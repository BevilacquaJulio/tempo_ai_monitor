use crate::{
    core::{
        alerts, history,
        preferences::{is_trusted_executable_path, require_window, DesktopState, Preferences},
    },
    providers::{self, antigravity, claude, codex, discovery, jsonl, Collection, Observation},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

const PRUNE_INTERVAL_SECS: u64 = 6 * 3600;
const CODEX_MOVED: &str = "The Codex executable moved or changed. Reconnect OpenAI Codex to confirm the new path.";
const ANTIGRAVITY_MOVED: &str = "The Antigravity CLI moved or changed. Reconnect Google Antigravity to confirm the new path.";

#[derive(Default)]
struct ProviderCache {
    sample: Option<Observation>,
    error: Option<&'static str>,
    revision: u64,
}

impl ProviderCache {
    fn reset(&mut self) {
        *self = Self { revision: self.revision.wrapping_add(1), ..Self::default() };
    }

    fn record(&mut self, sample: Result<Observation, &'static str>) {
        self.error = match sample {
            Ok(sample) => { self.sample = Some(sample); None }
            Err(error) => Some(error),
        };
    }

    fn stale(&self, now: i64) -> bool {
        self.sample.as_ref().is_some_and(|sample| providers::observation_is_stale(sample.at, now))
    }
}

#[derive(Default)]
struct Cache {
    claude: ProviderCache,
    codex: ProviderCache,
    codex_rpc_at: Option<Instant>,
    last_attempt: Option<Instant>,
}

const CODEX_RPC_CADENCE_SECS: u64 = 120;

fn should_query_codex_rpc(cache: &ProviderCache, last_rpc: Option<Instant>, kind: Refresh, now: i64) -> bool {
    if matches!(kind, Refresh::Forced) {
        return true;
    }
    if cache.sample.is_none() || cache.error.is_some() || cache.stale(now) {
        return true;
    }
    last_rpc.is_none_or(|at| at.elapsed() >= Duration::from_secs(CODEX_RPC_CADENCE_SECS))
}

#[derive(Clone, Copy)]
enum Refresh {
    Periodic,
    Requested,
    Forced,
}

impl Refresh {
    fn can_start(self, last: Option<Instant>) -> bool {
        let interval = match self {
            Self::Periodic => 120,
            Self::Requested => 15,
            Self::Forced => return true,
        };
        last.is_none_or(|last| last.elapsed() >= Duration::from_secs(interval))
    }
}

struct Running<'a>(&'a AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

struct CheckingGuard<'a> {
    flag: &'a AtomicBool,
    app: AppHandle,
    released: bool,
}

impl<'a> CheckingGuard<'a> {
    fn enter(flag: &'a AtomicBool, app: &AppHandle) -> Self {
        flag.store(true, Ordering::Release);
        let _ = app.emit("usage-updated", ());
        Self { flag, app: app.clone(), released: false }
    }

    fn finish(mut self) {
        self.flag.store(false, Ordering::Release);
        self.released = true;
    }
}

impl Drop for CheckingGuard<'_> {
    fn drop(&mut self) {
        if !self.released {
            self.flag.store(false, Ordering::Release);
            let _ = self.app.emit("usage-updated", ());
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClaudeWaitKind {
    None,
    Credentials,
    Retry,
}

#[derive(Clone)]
struct ClaudePoll {
    at: Instant,
    checked_at_epoch: i64,
    next_retry_epoch: Option<i64>,
    wait_kind: ClaudeWaitKind,
    credentials_stamp: Option<claude::CredentialsStamp>,
    wait: Duration,
    consecutive_errors: u32,
    failed: bool,
}

#[derive(Clone)]
struct AntigravityPoll {
    at: Instant,
    checked_at_epoch: i64,
    next_retry_epoch: Option<i64>,
    failed: bool,
    error: Option<&'static str>,
}

pub struct Monitor {
    cache: Mutex<Cache>,
    running: AtomicBool,
    pending: AtomicBool,
    forced: AtomicBool,
    path: PathBuf,
    claude_logs: Mutex<jsonl::ClaudeLogCache>,
    codex_logs: Mutex<jsonl::CodexLogCache>,
    claude_poll: Mutex<Option<ClaudePoll>>,
    claude_poll_reset: AtomicBool,
    claude_manual_running: AtomicBool,
    claude_checking: AtomicBool,
    claude_watch_fast: AtomicBool,
    claude_check: Mutex<()>,
    antigravity_reading: Mutex<Option<antigravity::Reading>>,
    antigravity_poll: Mutex<Option<AntigravityPoll>>,
    antigravity_run: Mutex<()>,
    antigravity_checking: AtomicBool,
    antigravity_watch_fast: AtomicBool,
    last_prune: Mutex<Option<Instant>>,
    history_write_error: AtomicBool,
    collection: Mutex<()>,
    history_write: Mutex<()>,
}

impl Monitor {
    pub fn new(path: PathBuf) -> Self {
        let now = Utc::now().timestamp();
        let mut write_error = false;
        let (reading, claude_reading) = match history::open(&path) {
            Ok(db) => {
                // Always enforce the 92-day retention limit on startup, even when all providers are disconnected.
                if history::prune(&db, now.saturating_sub(providers::HISTORY_SECONDS), now).is_err() {
                    write_error = true;
                }
                let agy = history::read_antigravity(&db).ok().flatten().map(|(_, reading)| reading);
                let claude = history::read_claude(&db).ok().flatten();
                (agy, claude)
            }
            Err(_) => {
                write_error = path.exists();
                (None, None)
            }
        };
        let mut cache = Cache::default();
        cache.claude.sample = claude_reading;
        Self {
            cache: Mutex::new(cache),
            running: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            forced: AtomicBool::new(false),
            path,
            claude_logs: Mutex::default(),
            codex_logs: Mutex::default(),
            claude_poll: Mutex::default(),
            claude_poll_reset: AtomicBool::new(false),
            claude_manual_running: AtomicBool::new(false),
            claude_checking: AtomicBool::new(false),
            claude_watch_fast: AtomicBool::new(false),
            claude_check: Mutex::default(),
            antigravity_reading: Mutex::new(reading),
            antigravity_poll: Mutex::default(),
            antigravity_run: Mutex::default(),
            antigravity_checking: AtomicBool::new(false),
            antigravity_watch_fast: AtomicBool::new(false),
            last_prune: Mutex::new(Some(Instant::now())),
            history_write_error: AtomicBool::new(write_error),
            collection: Mutex::default(),
            history_write: Mutex::default(),
        }
    }

    pub fn preferences_path(&self) -> PathBuf {
        self.path.with_file_name("preferences.json")
    }

    pub fn reset_changed_sources(&self, previous: &Preferences, next: &Preferences) -> bool {
        let claude_changed = previous.claude_enabled != next.claude_enabled;
        let codex_changed = previous.codex_enabled != next.codex_enabled || previous.codex_executable != next.codex_executable;
        let agy_changed = previous.antigravity_enabled != next.antigravity_enabled || previous.antigravity_executable != next.antigravity_executable;
        let locals_changed = previous.local_providers != next.local_providers;
        if claude_changed || codex_changed {
            let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
            if claude_changed {
                cache.claude.reset();
                self.claude_poll_reset.store(true, Ordering::Release);
                self.claude_watch_fast.store(false, Ordering::Release);
            }
            if codex_changed {
                cache.codex.reset();
                cache.codex_rpc_at = None;
            }
            cache.last_attempt = None;
        }
        if claude_changed && !next.claude_enabled {
            let _write = self.history_write.lock().unwrap_or_else(|error| error.into_inner());
            if let Ok(db) = history::open(&self.path) {
                let _ = history::clear_claude(&db);
            }
        }
        if agy_changed {
            self.antigravity_watch_fast.store(false, Ordering::Release);
            *self.antigravity_poll.lock().unwrap_or_else(|error| error.into_inner()) = None;
        }
        claude_changed || codex_changed || agy_changed || locals_changed
    }

    fn maybe_prune_expired(&self, now: i64) {
        let should_prune = {
            let last = self.last_prune.lock().unwrap_or_else(|error| error.into_inner());
            last.is_none_or(|at| at.elapsed() >= Duration::from_secs(PRUNE_INTERVAL_SECS))
        };
        if !should_prune {
            return;
        }
        let _write = self.history_write.lock().unwrap_or_else(|error| error.into_inner());
        match history::open(&self.path).and_then(|db| history::prune(&db, now.saturating_sub(providers::HISTORY_SECONDS), now)) {
            Ok(()) => {
                *self.last_prune.lock().unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
            }
            Err(_) => {
                self.history_write_error.store(true, Ordering::Release);
            }
        }
    }
}

fn claude_wait_kind(result: &Result<Observation, &'static str>) -> ClaudeWaitKind {
    match result {
        Ok(_) => ClaudeWaitKind::None,
        Err(error) if *error == claude::SIGN_IN || *error == claude::EXPIRED => ClaudeWaitKind::Credentials,
        Err(_) => ClaudeWaitKind::Retry,
    }
}

fn claude_auto_wait(result: &Result<Observation, &'static str>, retry_after: Option<Duration>, consecutive_errors: u32) -> Duration {
    match result {
        Ok(_) => Duration::from_secs(120),
        Err(error) if *error == claude::BUSY => retry_after.unwrap_or(Duration::from_secs(180)),
        Err(error) if *error == claude::SIGN_IN || *error == claude::EXPIRED => Duration::from_secs(120),
        // Retry transient startup/network failures sooner before backing off to the regular cadence.
        Err(_) => match consecutive_errors {
            0 | 1 => Duration::from_secs(30),
            2 => Duration::from_secs(60),
            _ => Duration::from_secs(120),
        },
    }
}

fn should_skip_claude_auto(
    last: &Option<ClaudePoll>,
    current_stamp: Option<claude::CredentialsStamp>,
    manual: bool,
) -> bool {
    if manual {
        return false;
    }
    let Some(last) = last else {
        return false;
    };
    // Reconnect right away once Claude Code creates or renews .credentials.json after a failure.
    if last.failed && current_stamp.is_some() && current_stamp != last.credentials_stamp {
        return false;
    }
    last.at.elapsed() < last.wait
}

fn should_trigger_claude_fast_watch(
    last: &Option<ClaudePoll>,
    current_stamp: Option<claude::CredentialsStamp>,
) -> bool {
    let Some(last) = last else {
        return false;
    };
    if !last.failed {
        return false;
    }
    let stamp_changed = current_stamp.is_some() && current_stamp != last.credentials_stamp;
    match last.wait_kind {
        ClaudeWaitKind::Credentials => stamp_changed,
        ClaudeWaitKind::Retry => stamp_changed || last.at.elapsed() >= last.wait,
        ClaudeWaitKind::None => false,
    }
}

fn should_preserve_fresh_claude_sample(
    cache: &ProviderCache,
    result: &Result<Observation, &'static str>,
    manual: bool,
    now: i64,
) -> bool {
    !manual
        && matches!(result, Err(error) if *error == claude::BUSY)
        && cache.sample.is_some()
        && !cache.stale(now)
}

fn resolve_pinned_executable(
    pinned: Option<&str>,
    discovered: Option<PathBuf>,
    moved_error: &'static str,
) -> Result<Option<PathBuf>, &'static str> {
    let Some(raw) = pinned.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let trusted = is_trusted_executable_path(Path::new(raw)).ok_or(moved_error)?;
    if let Some(found) = discovered {
        if found != trusted {
            return Err(moved_error);
        }
    }
    Ok(Some(trusted))
}

/// Combines native Codex App Server output with zero-config JSONL sessions and rate-limit fallback.
fn merge_codex(rpc: Option<Collection>, logs: jsonl::CodexLogResult, now: i64) -> Collection {
    match rpc {
        Some(mut collection) => {
            collection.sessions = jsonl::merge_sessions(logs.sessions, collection.sessions);
            if collection.sample.is_err() {
                if let Some(fallback) = logs.latest_observation.filter(|obs| !providers::observation_is_stale(obs.at, now)) {
                    collection.sample = Ok(fallback);
                }
            }
            collection
        }
        None => {
            let sample = match logs.latest_observation {
                Some(obs) => Ok(obs),
                None if !logs.sessions.is_empty() => Ok(Observation {
                    plan: None,
                    quotas: Vec::new(),
                    at: now,
                    daily_tokens: Vec::new(),
                }),
                None => Err("No local Codex sessions or executable found yet. Run Codex once and try again."),
            };
            Collection { sample, sessions: logs.sessions }
        }
    }
}

const ANTIGRAVITY_FAILURE_BACKOFF_SECS: u64 = 300;
const ANTIGRAVITY_SUCCESS_CADENCE_SECS: u64 = 120;

fn should_skip_antigravity_auto(last_check: &Option<AntigravityPoll>, manual: bool) -> bool {
    if manual {
        return false;
    }
    last_check.as_ref().is_some_and(|poll| {
        let interval = if poll.failed { ANTIGRAVITY_FAILURE_BACKOFF_SECS } else { ANTIGRAVITY_SUCCESS_CADENCE_SECS };
        poll.at.elapsed() < Duration::from_secs(interval)
    })
}

pub fn queue(app: &AppHandle, immediate: bool) {
    queue_with_mode(app, immediate, false);
}

fn queue_with_mode(app: &AppHandle, immediate: bool, manual: bool) {
    let claude_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || { let _ = check_claude(&claude_app, manual); });
    let agy_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || { let _ = check_antigravity(&agy_app, manual); });
    let state = app.state::<Monitor>();
    if manual { state.forced.store(true, Ordering::Release); }
    if immediate { state.pending.store(true, Ordering::Release); }
    if state.running.swap(true, Ordering::AcqRel) { return; }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Monitor>();
        {
            let _running = Running(&state.running);
            let kind = if state.forced.swap(false, Ordering::AcqRel) {
                state.pending.store(false, Ordering::Release);
                Refresh::Forced
            } else if state.pending.swap(false, Ordering::AcqRel) {
                Refresh::Requested
            } else {
                Refresh::Periodic
            };
            refresh(&app, &state, kind);
        }
        if state.forced.load(Ordering::Acquire) {
            queue_with_mode(&app, true, true);
        } else if state.pending.load(Ordering::Acquire) {
            queue(&app, false);
        }
    });
}

/// Lightweight 2-second watcher while a provider is waiting on credential renewal or a retry countdown.
pub fn poll_fast_watches(app: &AppHandle) {
    let state = app.state::<Monitor>();
    if state.claude_watch_fast.load(Ordering::Acquire) {
        let should_run = state.claude_poll.try_lock().ok().is_some_and(|last| {
            let stamp = claude::credentials_stamp();
            should_trigger_claude_fast_watch(&last, stamp)
        });
        if should_run {
            let claude_app = app.clone();
            tauri::async_runtime::spawn_blocking(move || { let _ = check_claude(&claude_app, false); });
        }
    }
    if state.antigravity_watch_fast.load(Ordering::Acquire) {
        let should_run = state.antigravity_poll.try_lock().ok().is_some_and(|last| {
            last.as_ref().is_some_and(|poll| {
                poll.failed && poll.at.elapsed() >= Duration::from_secs(ANTIGRAVITY_FAILURE_BACKOFF_SECS)
            })
        });
        if should_run {
            let agy_app = app.clone();
            tauri::async_runtime::spawn_blocking(move || { let _ = check_antigravity(&agy_app, false); });
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationCheck {
    provider_id: String,
    status: &'static str,
    message: &'static str,
}

fn persist_collected_history(
    state: &Monitor,
    rows: &[(&str, &[(i64, Value)])],
    now: i64,
) -> Result<(rusqlite::Connection, i64), rusqlite::Error> {
    let mut db = history::open(&state.path)?;
    let cutoff = history::clear_cutoff(&db)?;
    for (id, sessions) in rows {
        let filtered: Vec<(i64, Value)> = sessions.iter().filter(|(at, _)| *at > cutoff).cloned().collect();
        history::write(&mut db, id, &filtered)?;
    }
    history::prune(&db, now.saturating_sub(providers::HISTORY_SECONDS), now)?;
    Ok((db, cutoff))
}

fn refresh(app: &AppHandle, state: &Monitor, kind: Refresh) -> Vec<IntegrationCheck> {
    let _collection = state.collection.lock().unwrap_or_else(|error| error.into_inner());
    let now = Utc::now().timestamp();
    state.maybe_prune_expired(now);
    let desktop = app.state::<DesktopState>();
    let (settings, claude_revision, codex_revision, query_codex_rpc) = {
        let settings = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let mut cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        if !kind.can_start(cache.last_attempt) { return Vec::new(); }
        if settings.claude_enabled || settings.codex_enabled { cache.last_attempt = Some(Instant::now()); }
        let query_rpc = settings.codex_enabled && should_query_codex_rpc(&cache.codex, cache.codex_rpc_at, kind, now);
        (settings.clone(), cache.claude.revision, cache.codex.revision, query_rpc)
    };
    let clear_cutoff = if settings.claude_enabled || settings.codex_enabled {
        let _write = state.history_write.lock().unwrap_or_else(|error| error.into_inner());
        history::open(&state.path)
            .and_then(|db| history::clear_cutoff(&db))
            .unwrap_or(0)
    } else {
        0
    };
    let claude_sessions = settings.claude_enabled.then(|| {
        jsonl::claude_projects_dir()
            .map(|dir| jsonl::read_claude_sessions(&dir, now, clear_cutoff, &mut state.claude_logs.lock().unwrap_or_else(|error| error.into_inner())))
            .unwrap_or_default()
    });
    let mut ran_codex_rpc = false;
    let (codex_collection, codex_log_only_sessions) = if settings.codex_enabled {
        let logs = jsonl::codex_sessions_dir()
            .map(|dir| jsonl::read_codex_sessions(&dir, now, clear_cutoff, &mut state.codex_logs.lock().unwrap_or_else(|error| error.into_inner())))
            .unwrap_or(jsonl::CodexLogResult { sessions: Vec::new(), latest_observation: None });
        if query_codex_rpc {
            let collection = match resolve_pinned_executable(settings.codex_executable.as_deref(), discovery::codex_executable(), CODEX_MOVED) {
                Ok(Some(exe)) => {
                    ran_codex_rpc = true;
                    merge_codex(Some(codex::read(&exe, now)), logs, now)
                }
                Ok(None) => merge_codex(None, logs, now),
                Err(moved_error) => Collection { sample: Err(moved_error), sessions: logs.sessions },
            };
            (Some(collection), None)
        } else {
            // Focus/panel refreshes keep local JSONL history fresh without respawning codex.exe while the sample is fresh.
            (None, Some(logs.sessions))
        }
    } else {
        (None, None)
    };
    let (checks, history_rows, current) = {
        let current = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let mut cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        let claude_rows = claude_sessions.filter(|_| cache.claude.revision == claude_revision).map(|sessions| (sessions, None));
        let (codex_check, codex_rows) = if let Some(sessions) = codex_log_only_sessions {
            let rows = (cache.codex.revision == codex_revision).then_some((sessions, None));
            let check = IntegrationCheck { provider_id: "codex".into(), status: "connected", message: "Connected." };
            (check, rows)
        } else {
            if ran_codex_rpc && cache.codex.revision == codex_revision {
                cache.codex_rpc_at = Some(Instant::now());
            }
            record_check(&mut cache.codex, "codex", codex_revision, codex_collection, now)
        };
        (
            vec![codex_check],
            [("claude", "Claude Code", false, claude_rows), ("codex", "OpenAI Codex", current.codex_alerts, codex_rows)],
            current.clone(),
        )
    };
    let portuguese = *desktop.portuguese.lock().unwrap_or_else(|error| error.into_inner());
    if history_rows.iter().any(|(_, _, _, rows)| rows.is_some()) {
        let _write = state.history_write.lock().unwrap_or_else(|error| error.into_inner());
        let batch: Vec<(&str, &[(i64, Value)])> = history_rows
            .iter()
            .filter_map(|(id, _, _, rows)| rows.as_ref().map(|(sessions, _)| (*id, sessions.as_slice())))
            .collect();
        match persist_collected_history(state, &batch, now) {
            Ok((db, cutoff)) => {
                state.history_write_error.store(false, Ordering::Release);
                *state.last_prune.lock().unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
                for (id, display_name, alert_enabled, rows) in history_rows {
                    let Some((_, alert_sample)) = rows else { continue; };
                    if let (Some(portuguese), Some(sample)) = (portuguese, alert_sample.filter(|_| current.notifications && alert_enabled && now > cutoff)) {
                        alerts::notify(app, &db, id, display_name, &sample, Utc::now().timestamp(), &current.thresholds, portuguese);
                    }
                }
            }
            Err(_) => {
                state.history_write_error.store(true, Ordering::Release);
            }
        }
    }
    let _ = app.emit("usage-updated", ());
    checks
}

fn record_check(
    cache: &mut ProviderCache,
    id: &'static str,
    revision: u64,
    collection: Option<Collection>,
    now: i64,
) -> (IntegrationCheck, Option<(providers::Sessions, Option<Observation>)>) {
    let (status, message, rows) = if cache.revision != revision {
        ("error", "Saved settings changed during the check. Try again.", None)
    } else if let Some(Collection { sample, sessions }) = collection {
        let alert_sample = sample.as_ref().ok().cloned();
        cache.record(sample);
        let (status, message) = if let Some(error) = cache.error {
            ("error", error)
        } else if cache.stale(now) {
            ("stale", "Connected, but the last reading is out of date.")
        } else {
            ("connected", "Connected.")
        };
        (status, message, Some((sessions, alert_sample)))
    } else {
        ("disabled", "Disabled in saved preferences.", None)
    };
    (IntegrationCheck { provider_id: id.into(), status, message }, rows)
}

#[tauri::command]
pub fn request_refresh(app: AppHandle) { queue_with_mode(&app, true, true); }

#[tauri::command]
pub async fn refresh_claude(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_window(window.label(), &["main", "quick"], "Open TempoAI to check usage.")?;
    tauri::async_runtime::spawn_blocking(move || check_claude(&app, true))
        .await
        .map_err(|_| "Could not complete the Claude usage check.")?
        .map_err(str::to_owned)
}

fn check_claude(app: &AppHandle, manual: bool) -> Result<(), &'static str> {
    let state = app.state::<Monitor>();
    let desktop = app.state::<DesktopState>();
    if !desktop.config.lock().unwrap_or_else(|error| error.into_inner()).claude_enabled {
        return Err("Connect Claude Code first.");
    }
    let _manual_running = if manual {
        if state.claude_manual_running.swap(true, Ordering::AcqRel) {
            return Err("Claude usage is already being checked.");
        }
        Some(Running(&state.claude_manual_running))
    } else {
        None
    };
    let _check_guard = if manual {
        state.claude_check.lock().unwrap_or_else(|error| error.into_inner())
    } else {
        let Ok(guard) = state.claude_check.try_lock() else {
            return Ok(());
        };
        guard
    };
    let revision = {
        let settings = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        if !settings.claude_enabled {
            return Err("Connect Claude Code first.");
        }
        state.cache.lock().unwrap_or_else(|error| error.into_inner()).claude.revision
    };
    let (previous_errors, announce_checking) = {
        let mut last = state.claude_poll.lock().unwrap_or_else(|error| error.into_inner());
        if state.claude_poll_reset.swap(false, Ordering::AcqRel) {
            *last = None;
        }
        let current_stamp = if !manual && last.as_ref().is_some_and(|poll| poll.failed) {
            claude::credentials_stamp()
        } else {
            None
        };
        if should_skip_claude_auto(&last, current_stamp, manual) {
            return Ok(());
        }
        // Avoid flashing "Checking..." on background re-checks when credentials are still untouched.
        let announce = manual
            || last.as_ref().is_none_or(|poll| {
                poll.wait_kind != ClaudeWaitKind::Credentials || current_stamp != poll.credentials_stamp
            });
        (last.as_ref().map_or(0, |poll| poll.consecutive_errors), announce)
    };
    let checking_guard = announce_checking.then(|| CheckingGuard::enter(&state.claude_checking, app));
    let now = Utc::now().timestamp();
    let (result, retry_after) = claude::read_with_backoff(now);
    let post_stamp = claude::credentials_stamp();
    let consecutive_errors = if result.is_ok() { 0 } else { previous_errors.saturating_add(1) };
    let wait = claude_auto_wait(&result, retry_after, consecutive_errors);
    let wait_kind = claude_wait_kind(&result);
    let checked_at_epoch = Utc::now().timestamp();
    let next_retry_epoch = (wait_kind == ClaudeWaitKind::Retry)
        .then(|| checked_at_epoch.saturating_add(wait.as_secs() as i64));
    let (preserve_fresh, persisted_sample, alert_context) = {
        let current = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let mut cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        if !current.claude_enabled || cache.claude.revision != revision {
            return Err("Saved settings changed during the check. Try again.");
        }
        let preserve = should_preserve_fresh_claude_sample(&cache.claude, &result, manual, now);
        if preserve {
            (true, None, None)
        } else {
            let ok_sample = result.as_ref().ok().cloned();
            cache.claude.record(result.clone());
            let alert = ok_sample.as_ref()
                .filter(|_| current.notifications && current.claude_alerts)
                .map(|sample| (sample.clone(), current.thresholds.clone()));
            (false, ok_sample, alert)
        }
    };
    *state.claude_poll.lock().unwrap_or_else(|error| error.into_inner()) = Some(ClaudePoll {
        at: Instant::now(),
        checked_at_epoch,
        next_retry_epoch,
        wait_kind,
        credentials_stamp: post_stamp,
        wait,
        consecutive_errors,
        failed: result.is_err(),
    });
    state.claude_watch_fast.store(result.is_err() && !preserve_fresh, Ordering::Release);
    if preserve_fresh {
        if let Some(guard) = checking_guard { guard.finish(); }
        let _ = app.emit("usage-updated", ());
        return Ok(());
    }
    if persisted_sample.is_some() || alert_context.is_some() {
        let portuguese = *desktop.portuguese.lock().unwrap_or_else(|error| error.into_inner());
        let _write = state.history_write.lock().unwrap_or_else(|error| error.into_inner());
        if let Ok(db) = history::open(&state.path) {
            if let Some(ref sample) = persisted_sample {
                let _ = history::save_claude(&db, sample);
            }
            if let (Some(portuguese), Some((sample, thresholds))) = (portuguese, alert_context) {
                if let Ok(cutoff) = history::clear_cutoff(&db) {
                    if now > cutoff {
                        alerts::notify(app, &db, "claude", "Claude Code", &sample, Utc::now().timestamp(), &thresholds, portuguese);
                    }
                }
            }
        }
    }
    if let Some(guard) = checking_guard { guard.finish(); }
    let _ = app.emit("usage-updated", ());
    result.map(|_| ())
}

fn check_claude_integration(app: &AppHandle) -> IntegrationCheck {
    let enabled = app.state::<DesktopState>().config.lock().unwrap_or_else(|error| error.into_inner()).claude_enabled;
    if !enabled {
        return IntegrationCheck { provider_id: "claude".into(), status: "disabled", message: "Disabled in saved preferences." };
    }
    let (status, message) = match check_claude(app, true) {
        Ok(()) => {
            let state = app.state::<Monitor>();
            let cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
            if cache.claude.stale(Utc::now().timestamp()) {
                ("stale", "Connected, but the last reading is out of date.")
            } else {
                ("connected", "Connected.")
            }
        }
        Err(error) => ("error", error),
    };
    IntegrationCheck { provider_id: "claude".into(), status, message }
}

#[tauri::command]
pub async fn refresh_antigravity(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    require_window(window.label(), &["main", "quick"], "Open TempoAI to check usage.")?;
    tauri::async_runtime::spawn_blocking(move || check_antigravity(&app, true))
        .await.map_err(|_| "Could not complete the Antigravity usage check.")?.map_err(str::to_owned)
}

fn check_antigravity(app: &AppHandle, manual: bool) -> Result<(), &'static str> {
    let state = app.state::<Monitor>();
    let desktop = app.state::<DesktopState>();
    let (enabled, pinned_executable) = {
        let config = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        (config.antigravity_enabled, config.antigravity_executable.clone())
    };
    if !enabled {
        return Err("Connect Google Antigravity first.");
    }
    let Ok(_run_guard) = state.antigravity_run.try_lock() else { return Err("Antigravity usage is already being checked."); };
    {
        let last_check = state.antigravity_poll.lock().unwrap_or_else(|error| error.into_inner());
        if should_skip_antigravity_auto(&last_check, manual) { return Ok(()); }
    }
    let checking_guard = CheckingGuard::enter(&state.antigravity_checking, app);
    let executable = match resolve_pinned_executable(pinned_executable.as_deref(), discovery::antigravity_executable(), ANTIGRAVITY_MOVED) {
        Ok(Some(exe)) => exe,
        Ok(None) | Err(_) => {
            let checked_at = Utc::now().timestamp();
            *state.antigravity_poll.lock().unwrap_or_else(|error| error.into_inner()) = Some(AntigravityPoll {
                at: Instant::now(),
                checked_at_epoch: checked_at,
                next_retry_epoch: Some(checked_at.saturating_add(ANTIGRAVITY_FAILURE_BACKOFF_SECS as i64)),
                failed: true,
                error: Some(ANTIGRAVITY_MOVED),
            });
            state.antigravity_watch_fast.store(true, Ordering::Release);
            checking_guard.finish();
            let _ = app.emit("usage-updated", ());
            return Err(ANTIGRAVITY_MOVED);
        }
    };
    let result = antigravity::read(&executable);
    let checked_at = Utc::now().timestamp();
    let failed = result.is_err();
    *state.antigravity_poll.lock().unwrap_or_else(|error| error.into_inner()) = Some(AntigravityPoll {
        at: Instant::now(),
        checked_at_epoch: checked_at,
        next_retry_epoch: failed.then(|| checked_at.saturating_add(ANTIGRAVITY_FAILURE_BACKOFF_SECS as i64)),
        failed,
        error: result.as_ref().err().copied(),
    });
    state.antigravity_watch_fast.store(failed, Ordering::Release);
    let reading = match result {
        Ok(reading) => reading,
        Err(error) => {
            checking_guard.finish();
            let _ = app.emit("usage-updated", ());
            return Err(error);
        }
    };
    let (notifications, alert_enabled, thresholds, portuguese) = {
        let settings = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        if !settings.antigravity_enabled {
            return Err("Saved settings changed during the check. Try again.");
        }
        let portuguese = *desktop.portuguese.lock().unwrap_or_else(|error| error.into_inner());
        (settings.notifications, settings.antigravity_alerts, settings.thresholds.clone(), portuguese)
    };
    let saved = {
        let _write = state.history_write.lock().unwrap_or_else(|error| error.into_inner());
        history::open(&state.path).and_then(|db| {
            history::save_antigravity(&db, "native", &reading)?;
            if let Some(portuguese) = portuguese.filter(|_| notifications && alert_enabled) {
                let now = Utc::now().timestamp();
                let snapshot = reading.snapshot("Google Antigravity", now);
                if let Some(quotas) = snapshot["quotas"].as_array() {
                    let observation = Observation { plan: None, quotas: quotas.clone(), at: reading.at, daily_tokens: Vec::new() };
                    alerts::notify(app, &db, antigravity::ID, "Google Antigravity", &observation, now, &thresholds, portuguese);
                }
            }
            Ok(())
        })
    };
    *state.antigravity_reading.lock().unwrap_or_else(|error| error.into_inner()) = Some(reading);
    checking_guard.finish();
    let _ = app.emit("usage-updated", ());
    saved.map_err(|_| "Antigravity usage was checked but could not be saved for next time.")
}

fn antigravity_snapshot(state: &Monitor, now: i64) -> Value {
    let saved = state.antigravity_reading.lock().unwrap_or_else(|error| error.into_inner()).clone();
    let poll = state.antigravity_poll.lock().unwrap_or_else(|error| error.into_inner()).clone();
    let checking = state.antigravity_checking.load(Ordering::Acquire);
    let mut value = match saved.as_ref() {
        Some(reading) => {
            let mut value = reading.snapshot("Google Antigravity", now);
            value["connectionSaved"] = json!(true);
            if value["integration"] != "connected" {
                if let Some(error) = poll.as_ref().and_then(|p| p.error) {
                    value["integration"] = json!("error");
                    value["integrationError"] = json!(error);
                }
            }
            value
        }
        None => json!({
            "providerId": antigravity::ID,
            "displayName": "Google Antigravity",
            "connectionSaved": true,
            "planName": null,
            "integration": if poll.as_ref().is_some_and(|p| p.failed) { "error" } else { "not_connected" },
            "integrationError": poll.as_ref().and_then(|p| p.error),
            "quotas": [],
            "updatedAt": null
        }),
    };
    let degraded = value["integration"] != "connected";
    let active_wait = poll.as_ref().filter(|p| p.failed && degraded);
    value["checking"] = json!(checking);
    value["waitingFor"] = json!(active_wait.map(|_| "retry"));
    value["nextRetryAt"] = json!(active_wait.and_then(|p| p.next_retry_epoch).and_then(providers::iso));
    value["lastCheckedAt"] = json!(poll.as_ref().and_then(|p| providers::iso(p.checked_at_epoch)));
    value
}

#[tauri::command]
pub async fn test_saved_integrations(app: AppHandle, window: WebviewWindow) -> Result<Vec<IntegrationCheck>, String> {
    require_window(window.label(), &["main"], "Use the dashboard to test saved integrations.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let monitor = app.state::<Monitor>();
        let mut checks = vec![check_claude_integration(&app)];
        checks.extend(refresh(&app, &monitor, Refresh::Forced));
        let (antigravity_enabled, sources) = {
            let desktop = app.state::<DesktopState>();
            let config = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
            (config.antigravity_enabled, config.local_providers.clone())
        };
        let now = Utc::now().timestamp();
        if antigravity_enabled {
            let (status, message) = match check_antigravity(&app, true) {
                Ok(()) => ("connected", "Connected."),
                Err(error) => ("error", error),
            };
            checks.push(IntegrationCheck { provider_id: antigravity::ID.into(), status, message });
        }
        checks.extend(sources.iter().take(discovery::MAX_LOCAL_SOURCES).map(|source| {
            let (status, message) = discovery::check_source(source, now);
            IntegrationCheck { provider_id: source.id.clone(), status, message }
        }));
        checks
    })
    .await
    .map_err(|_| "Could not complete the integration checks.".into())
}

fn usage_snapshot_for_window(window_label: &str, state: &Monitor, desktop: &DesktopState, now: i64) -> Result<Value, &'static str> {
    require_window(window_label, &["main", "quick"], "Open TempoAI to check usage.")?;
    Ok(snapshot(state, desktop, now))
}

fn session_history_for_window(window_label: &str, state: &Monitor, now: i64) -> Result<Value, &'static str> {
    require_window(window_label, &["main"], "Open the dashboard to view session history.")?;
    Ok(read_sessions_snapshot(state, now))
}

#[tauri::command]
pub async fn usage_snapshot(app: AppHandle, window: WebviewWindow) -> Result<Value, String> {
    let label = window.label().to_owned();
    require_window(&label, &["main", "quick"], "Open TempoAI to check usage.")?;
    tauri::async_runtime::spawn_blocking(move || {
        usage_snapshot_for_window(&label, &app.state::<Monitor>(), &app.state::<DesktopState>(), Utc::now().timestamp())
    })
    .await
    .map_err(|_| "Could not load usage.")?
    .map_err(str::to_owned)
}

#[tauri::command]
pub async fn session_history(app: AppHandle, window: WebviewWindow) -> Result<Value, String> {
    let label = window.label().to_owned();
    require_window(&label, &["main"], "Open the dashboard to view session history.")?;
    tauri::async_runtime::spawn_blocking(move || {
        session_history_for_window(&label, &app.state::<Monitor>(), Utc::now().timestamp())
    })
    .await
    .map_err(|_| "Could not load session history.")?
    .map_err(str::to_owned)
}

fn read_sessions_snapshot(state: &Monitor, now: i64) -> Value {
    let history = history::open(&state.path).and_then(|db| history::read(&db, now - providers::HISTORY_SECONDS));
    let write_failed = state.history_write_error.load(Ordering::Acquire);
    json!({
        "historyError": history.is_err() || write_failed,
        "sessions": history.unwrap_or_default(),
    })
}

fn snapshot(state: &Monitor, desktop: &DesktopState, now: i64) -> Value {
    let claude_checking = state.claude_checking.load(Ordering::Acquire);
    let claude_poll = state.claude_poll.lock().unwrap_or_else(|error| error.into_inner()).clone();
    let (mut snapshot, antigravity_enabled, sources) = {
        let config = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        (
            quota_snapshot_with_status(&config, &cache, claude_checking, claude_poll.as_ref(), now),
            config.antigravity_enabled,
            config.local_providers.clone(),
        )
    };
    if let Some(providers) = snapshot["providers"].as_array_mut() {
        if antigravity_enabled {
            providers.push(antigravity_snapshot(state, now));
        }
        providers.extend(sources.iter().take(discovery::MAX_LOCAL_SOURCES).map(|source| {
            let mut provider = discovery::local_snapshot(source, now);
            provider["connectionSaved"] = json!(true);
            provider
        }));
        let newest = providers.iter().filter_map(|provider| provider["updatedAt"].as_str()).max().map(str::to_owned);
        snapshot["observedAt"] = json!(newest);
    }
    snapshot["historyError"] = json!(false);
    snapshot["sessions"] = json!([]);
    snapshot
}

#[cfg(test)]
fn quota_snapshot(config: &Preferences, cache: &Cache, now: i64) -> Value {
    quota_snapshot_with_status(config, cache, false, None, now)
}

fn quota_snapshot_with_status(
    config: &Preferences,
    cache: &Cache,
    claude_checking: bool,
    claude_poll: Option<&ClaudePoll>,
    now: i64,
) -> Value {
    let mut observed = None;
    let providers: Vec<_> = [
        ("claude", "Claude Code", config.claude_enabled, &cache.claude),
        ("codex", "OpenAI Codex", config.codex_enabled, &cache.codex),
    ].into_iter().filter(|(_, _, enabled, _)| *enabled).map(|(id, name, _, provider)| {
        let sample = provider.sample.as_ref();
        let error = provider.error;
        let stale = provider.stale(now);
        let mut quotas = sample.map(|data| data.quotas.clone()).unwrap_or_default();
        for row in &mut quotas {
            if (stale || error.is_some()) && row["status"] == "available" { row["status"] = json!("stale"); }
        }
        for kind in ["five_hour", "weekly"] {
            if !quotas.iter().any(|row| row["type"] == kind && row["scope"].is_null()) {
                quotas.insert(if kind == "five_hour" { 0 } else { quotas.len() }, providers::empty_quota(id, kind, "unavailable"));
            }
        }
        if let Some(sample) = sample { observed = observed.max(Some(sample.at)); }
        let (checking, waiting_for, next_retry_at, last_checked_at) = if id == "claude" {
            let degraded = error.is_some() || stale;
            let active_wait = claude_poll.filter(|poll| poll.failed && degraded);
            let wait_label = active_wait.and_then(|poll| match poll.wait_kind {
                ClaudeWaitKind::Credentials => Some("credentials"),
                ClaudeWaitKind::Retry => Some("retry"),
                ClaudeWaitKind::None => None,
            });
            let retry_at = active_wait
                .filter(|poll| poll.wait_kind == ClaudeWaitKind::Retry)
                .and_then(|poll| poll.next_retry_epoch)
                .and_then(providers::iso);
            let checked_at = claude_poll.and_then(|poll| providers::iso(poll.checked_at_epoch));
            (claude_checking, wait_label, retry_at, checked_at)
        } else {
            (false, None, None, None)
        };
        json!({
            "providerId":id, "displayName":name, "connectionSaved":true, "planName": sample.and_then(|data| data.plan.as_ref()),
            "integration": if error.is_some() { "error" } else if stale { "stale" } else if sample.is_some() { "connected" } else { "not_connected" },
            "integrationError": error,
            "checking": checking,
            "waitingFor": waiting_for,
            "nextRetryAt": next_retry_at,
            "lastCheckedAt": last_checked_at,
            "quotas": quotas, "updatedAt": sample.and_then(|data| providers::iso(data.at))
        })
    }).collect();
    let codex = cache.codex.sample.as_ref().filter(|_| config.codex_enabled);
    json!({"mode":"live", "providers":providers, "observedAt":observed.and_then(providers::iso),
        "codexDailyTokens": codex.map(|row| &row.daily_tokens), "codexDailyUpdatedAt": codex.and_then(|row| providers::iso(row.at))})
}

#[tauri::command]
pub fn data_location(state: State<'_, Monitor>) -> String { state.path.to_string_lossy().into_owned() }

#[tauri::command]
pub fn clear_history(app: AppHandle, window: WebviewWindow, state: State<'_, Monitor>) -> Result<(), String> {
    require_window(window.label(), &["main"], "Use the dashboard to clear local session history.")?;
    let _write = state.history_write.lock().unwrap_or_else(|error| error.into_inner());
    let mut db = history::open(&state.path).map_err(|_| "Could not open history.")?;
    let cutoff = Utc::now().timestamp();
    history::clear_history(&mut db, cutoff).map_err(|_| "Could not clear history.")?;
    state.history_write_error.store(false, Ordering::Release);
    let _ = app.emit("usage-updated", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_antigravity_reading_on_restart_and_keeps_its_original_age() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("history.sqlite");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(include_str!("../../migrations/001_history.sql")).unwrap();
        drop(db);
        let reading = serde_json::from_value::<antigravity::Reading>(json!({"at":1000, "quotas":[
            {"id":"gemini-5h", "status":"available", "usedPercent":25.0, "resetAt":null}
        ]})).unwrap();
        let db = history::open(&path).unwrap();
        history::save_antigravity(&db, "native", &reading).unwrap();
        drop(db);
        let desktop = DesktopState::default();
        desktop.config.lock().unwrap().antigravity_enabled = true;
        let reopened = Monitor::new(path);
        let fresh = snapshot(&reopened, &desktop, 1100);
        assert_eq!(fresh["providers"][0]["providerId"], "antigravity");
        assert_eq!(fresh["providers"][0]["connectionSaved"], true);
        assert_eq!(fresh["providers"][0]["integration"], "connected");
        let stale = snapshot(&reopened, &desktop, 1301);
        assert_eq!(stale["providers"][0]["connectionSaved"], true);
        assert_eq!(stale["providers"][0]["integration"], "stale");
        assert_eq!(stale["providers"][0]["quotas"][0]["usedPercent"], 25.0);
    }

    fn sample(at: i64) -> Observation {
        Observation { plan: None, quotas: vec![json!({"type":"five_hour","status":"available","usedPercent":70.1})], at,
            daily_tokens: vec![json!({"day":"2026-11-12","tokens":100})] }
    }

    #[test]
    fn quick_window_gets_only_quotas_and_is_denied_session_history() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("history.sqlite");
        let now = Utc::now().timestamp();
        let mut db = history::open(&path).unwrap();
        history::write(&mut db, "claude", &[(now - 60, json!({"id":"claude:secret-proj","projectName":"SecretRepo","totalTokens":123}))]).unwrap();
        drop(db);

        let state = Monitor::new(path);
        let desktop = DesktopState::default();
        desktop.config.lock().unwrap().codex_enabled = true;
        state.cache.lock().unwrap().codex.record(Ok(sample(now - 10)));

        let quick_summary = usage_snapshot_for_window("quick", &state, &desktop, now).unwrap();
        assert_eq!(quick_summary["sessions"], json!([]));
        assert_eq!(quick_summary["providers"][0]["quotas"][0]["usedPercent"], 70.1);
        assert!(session_history_for_window("quick", &state, now).is_err());
        assert!(usage_snapshot_for_window("unknown", &state, &desktop, now).is_err());

        let main_history = session_history_for_window("main", &state, now).unwrap();
        assert_eq!(main_history["historyError"], false);
        assert_eq!(main_history["sessions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn startup_prunes_expired_sessions_even_with_all_providers_disabled() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("history.sqlite");
        let now = Utc::now().timestamp();
        let expired = now - providers::HISTORY_SECONDS - 3600;
        let recent = now - 3600;
        let mut db = history::open(&path).unwrap();
        history::write(&mut db, "claude", &[
            (expired, json!({"id":"claude:old","projectName":"ExpiredProj","totalTokens":100})),
            (recent, json!({"id":"claude:new","projectName":"ActiveProj","totalTokens":200})),
        ]).unwrap();
        drop(db);

        let _monitor = Monitor::new(path.clone());
        let db = history::open(&path).unwrap();
        let remaining = history::read(&db, 0).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0]["id"], "claude:new");
    }

    #[test]
    fn write_failure_surfaces_history_error_and_recovers_after_successful_write() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("history.sqlite");
        let now = Utc::now().timestamp();
        let mut db = history::open(&path).unwrap();
        history::write(&mut db, "codex", &[(now - 60, json!({"id":"codex:1","totalTokens":100}))]).unwrap();
        drop(db);

        let state = Monitor::new(path);
        state.history_write_error.store(true, Ordering::Release);
        let degraded = read_sessions_snapshot(&state, now);
        assert_eq!(degraded["historyError"], true);
        assert_eq!(degraded["sessions"].as_array().unwrap().len(), 1);

        let batch = [("codex", &[(now - 10, json!({"id":"codex:2","totalTokens":250}))][..])];
        assert!(persist_collected_history(&state, &batch, now).is_ok());
        state.history_write_error.store(false, Ordering::Release);
        let recovered = read_sessions_snapshot(&state, now);
        assert_eq!(recovered["historyError"], false);
        assert_eq!(recovered["sessions"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn pinned_executable_rejects_moved_or_changed_cli_targets() {
        let dir = crate::core::preferences::trusted_test_dir();
        let exe_a = dir.path().join("codex.exe");
        let exe_b_dir = dir.path().join("other");
        std::fs::create_dir(&exe_b_dir).unwrap();
        let exe_b = exe_b_dir.join("codex.exe");
        std::fs::write(&exe_a, "a").unwrap();
        std::fs::write(&exe_b, "b").unwrap();

        let trusted_a = is_trusted_executable_path(&exe_a).unwrap();
        let trusted_b = is_trusted_executable_path(&exe_b).unwrap();
        assert_eq!(resolve_pinned_executable(trusted_a.to_str(), Some(trusted_a.clone()), CODEX_MOVED), Ok(Some(trusted_a.clone())));
        assert_eq!(resolve_pinned_executable(trusted_a.to_str(), Some(trusted_b), CODEX_MOVED), Err(CODEX_MOVED));
        assert_eq!(resolve_pinned_executable(None, Some(trusted_a), CODEX_MOVED), Ok(None));
    }

    #[test]
    fn quotas_only_snapshot_skips_sqlite_and_session_history_reports_unreadable_db() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("history.sqlite");
        std::fs::create_dir(&path).unwrap();
        let state = Monitor::new(path);
        let desktop = DesktopState::default();
        desktop.config.lock().unwrap().codex_enabled = true;
        state.cache.lock().unwrap().codex.record(Ok(sample(1000)));

        // Quick Monitor / Overview path never touches SQLite.
        let light = snapshot(&state, &desktop, 1010);
        assert_eq!(light["historyError"], false);
        assert_eq!(light["sessions"], json!([]));
        assert_eq!(light["providers"][0]["quotas"][0]["usedPercent"], 70.1);
        assert_eq!(light["codexDailyTokens"][0]["tokens"], 100);

        // Activity on-demand history read reports unreadable storage cleanly.
        let history_result = read_sessions_snapshot(&state, 1010);
        assert_eq!(history_result["historyError"], true);
        assert_eq!(history_result["sessions"], json!([]));
    }

    #[test]
    fn failed_check_marks_provider_error_without_destroying_sample_and_recovers() {
        let config = Preferences { codex_enabled: true, ..Preferences::default() };
        let mut cache = Cache::default();
        cache.codex.record(Ok(sample(1000)));
        let failure = Collection { sample: Err("Codex unavailable."), sessions: Vec::new() };
        let (check, _) = record_check(&mut cache.codex, "codex", 0, Some(failure), 1010);
        assert_eq!(check.status, "error");
        assert_eq!(check.message, "Codex unavailable.");
        let snapshot = quota_snapshot(&config, &cache, 1010);
        assert_eq!(snapshot["providers"][0]["integration"], "error");
        assert_eq!(snapshot["providers"][0]["updatedAt"], providers::iso(1000).unwrap());
        assert_eq!(snapshot["providers"][0]["quotas"][0]["usedPercent"], 70.1);
        assert_eq!(snapshot["providers"][0]["quotas"][0]["status"], "stale");
        assert_eq!(snapshot["codexDailyTokens"][0]["tokens"], 100);

        let success = Collection { sample: Ok(sample(1020)), sessions: Vec::new() };
        let (check, _) = record_check(&mut cache.codex, "codex", 0, Some(success), 1020);
        assert_eq!(check.status, "connected");
        assert!(cache.codex.error.is_none());
        assert_eq!(cache.codex.sample.unwrap().at, 1020);
    }

    #[test]
    fn forced_checks_bypass_the_refresh_limit_and_explain_disabled_or_stale_sources() {
        let last = Some(Instant::now());
        assert!(!Refresh::Requested.can_start(last));
        assert!(!Refresh::Periodic.can_start(last));
        assert!(Refresh::Forced.can_start(last));

        let mut cache = ProviderCache::default();
        let (disabled, _) = record_check(&mut cache, "codex", 0, None, 1400);
        assert_eq!(disabled.status, "disabled");
        let stale = Collection { sample: Ok(sample(1000)), sessions: Vec::new() };
        let (check, _) = record_check(&mut cache, "claude", 0, Some(stale), 1400);
        assert_eq!(check.status, "stale");
        let config = Preferences { claude_enabled: true, ..Preferences::default() };
        let snapshot = quota_snapshot(&config, &Cache { claude: cache, ..Cache::default() }, 1400);
        assert_eq!(snapshot["providers"][0]["integration"], "stale");
        assert!(snapshot["providers"][0]["integrationError"].is_null());
        assert_eq!(snapshot["providers"][0]["quotas"][0]["usedPercent"], 70.1);
        let mut cache = ProviderCache::default();
        cache.reset();
        let obsolete = Collection { sample: Ok(sample(1400)), sessions: Vec::new() };
        let (check, rows) = record_check(&mut cache, "claude", 0, Some(obsolete), 1400);
        assert_eq!(check.status, "error");
        assert!(rows.is_none());
        assert!(cache.sample.is_none());
    }

    #[test]
    fn collection_slot_is_released_after_a_panic() {
        let running = AtomicBool::new(true);
        let result = std::panic::catch_unwind(|| {
            let _running = Running(&running);
            panic!("synthetic collector panic");
        });
        assert!(result.is_err());
        assert!(!running.swap(true, Ordering::AcqRel));
    }

    #[test]
    fn antigravity_auto_check_recovers_after_failure_backoff() {
        let recent_failure = Some(AntigravityPoll {
            at: Instant::now(),
            checked_at_epoch: 1000,
            next_retry_epoch: Some(1300),
            failed: true,
            error: Some("Could not check Antigravity usage."),
        });
        assert!(should_skip_antigravity_auto(&recent_failure, false));
        assert!(!should_skip_antigravity_auto(&recent_failure, true));
        let expired_failure = Instant::now().checked_sub(Duration::from_secs(301)).map(|at| AntigravityPoll {
            at,
            checked_at_epoch: 1000,
            next_retry_epoch: Some(1300),
            failed: true,
            error: Some("Could not check Antigravity usage."),
        });
        if expired_failure.is_some() {
            assert!(!should_skip_antigravity_auto(&expired_failure, false));
        }
    }

    #[test]
    fn codex_zero_config_falls_back_to_jsonl_sessions_and_limits() {
        let logs = jsonl::CodexLogResult {
            sessions: vec![(1000, json!({"id":"codex:1","totalTokens":500}))],
            latest_observation: Some(sample(990)),
        };
        let merged = merge_codex(None, logs, 1000);
        assert_eq!(merged.sample.unwrap().at, 990);
        assert_eq!(merged.sessions.len(), 1);
    }

    #[test]
    fn codex_rpc_skips_redundant_spawns_on_focus_while_sample_is_fresh() {
        let mut cache = ProviderCache::default();
        assert!(should_query_codex_rpc(&cache, None, Refresh::Requested, 1000));

        cache.record(Ok(sample(1000)));
        let recent = Some(Instant::now());
        assert!(!should_query_codex_rpc(&cache, recent, Refresh::Requested, 1030));
        assert!(should_query_codex_rpc(&cache, recent, Refresh::Forced, 1030));
        assert!(should_query_codex_rpc(&cache, recent, Refresh::Requested, 1301));

        if let Some(old_rpc) = Instant::now().checked_sub(Duration::from_secs(CODEX_RPC_CADENCE_SECS + 1)) {
            assert!(should_query_codex_rpc(&cache, Some(old_rpc), Refresh::Requested, 1030));
        }

        cache.record(Err("Codex unavailable."));
        assert!(should_query_codex_rpc(&cache, recent, Refresh::Requested, 1030));
    }

    #[test]
    fn claude_auto_retry_handles_credential_renewal_transient_backoff_and_manual_override() {
        let stamp_a = claude::CredentialsStamp { modified: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(10), len: 100 };
        let stamp_b = claude::CredentialsStamp { modified: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(20), len: 120 };

        assert_eq!(claude_auto_wait(&Ok(sample(1000)), None, 0), Duration::from_secs(120));
        assert_eq!(claude_auto_wait(&Err(claude::BUSY), None, 1), Duration::from_secs(180));
        assert_eq!(claude_auto_wait(&Err(claude::BUSY), Some(Duration::from_secs(45)), 1), Duration::from_secs(45));
        assert_eq!(claude_auto_wait(&Err(claude::COULD_NOT_READ), None, 1), Duration::from_secs(30));
        assert_eq!(claude_auto_wait(&Err(claude::COULD_NOT_READ), None, 2), Duration::from_secs(60));
        assert_eq!(claude_auto_wait(&Err(claude::COULD_NOT_READ), None, 3), Duration::from_secs(120));
        assert_eq!(claude_wait_kind(&Err(claude::EXPIRED)), ClaudeWaitKind::Credentials);
        assert_eq!(claude_wait_kind(&Err(claude::SIGN_IN)), ClaudeWaitKind::Credentials);
        assert_eq!(claude_wait_kind(&Err(claude::BUSY)), ClaudeWaitKind::Retry);
        assert_eq!(claude_wait_kind(&Ok(sample(1000))), ClaudeWaitKind::None);

        // Same credentials after failure stay throttled automatically, but manual retry always runs immediately.
        let recent_failure = Some(ClaudePoll {
            at: Instant::now(),
            checked_at_epoch: 1000,
            next_retry_epoch: Some(1180),
            wait_kind: ClaudeWaitKind::Retry,
            credentials_stamp: Some(stamp_a),
            wait: Duration::from_secs(180),
            consecutive_errors: 1,
            failed: true,
        });
        assert!(should_skip_claude_auto(&recent_failure, Some(stamp_a), false));
        assert!(!should_skip_claude_auto(&recent_failure, Some(stamp_a), true));
        assert!(!should_trigger_claude_fast_watch(&recent_failure, Some(stamp_a)));
        // Updating .credentials.json after a failure triggers immediate auto-reconnect.
        assert!(!should_skip_claude_auto(&recent_failure, Some(stamp_b), false));
        assert!(should_trigger_claude_fast_watch(&recent_failure, Some(stamp_b)));
        // Missing credentials followed by new sign-in also reconnects immediately via fast watch.
        let missing_creds = Some(ClaudePoll {
            at: Instant::now(),
            checked_at_epoch: 1000,
            next_retry_epoch: None,
            wait_kind: ClaudeWaitKind::Credentials,
            credentials_stamp: None,
            wait: Duration::from_secs(120),
            consecutive_errors: 1,
            failed: true,
        });
        assert!(should_skip_claude_auto(&missing_creds, None, false));
        assert!(!should_trigger_claude_fast_watch(&missing_creds, None));
        assert!(!should_skip_claude_auto(&missing_creds, Some(stamp_a), false));
        assert!(should_trigger_claude_fast_watch(&missing_creds, Some(stamp_a)));
    }

    #[test]
    fn restores_claude_reading_on_restart_and_exposes_transparent_wait_metadata() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let path = directory.path().join("history.sqlite");
        let db = history::open(&path).unwrap();
        let saved = Observation {
            plan: Some("Pro".into()),
            quotas: vec![json!({"id":"claude-five_hour-","type":"five_hour","label":"5-hour window","scope":null,"usedPercent":32.5,"remainingPercent":67.5,"resetAt":null,"status":"available","source":"cli","updatedAt":"1970-01-01T00:16:40Z"})],
            at: 1000,
            daily_tokens: Vec::new(),
        };
        history::save_claude(&db, &saved).unwrap();
        drop(db);

        let desktop = DesktopState::default();
        desktop.config.lock().unwrap().claude_enabled = true;
        let reopened = Monitor::new(path);
        let stale = snapshot(&reopened, &desktop, 1400);
        assert_eq!(stale["providers"][0]["providerId"], "claude");
        assert_eq!(stale["providers"][0]["integration"], "stale");
        assert_eq!(stale["providers"][0]["quotas"][0]["usedPercent"], 32.5);
        assert_eq!(stale["providers"][0]["quotas"][0]["status"], "stale");
        assert_eq!(stale["providers"][0]["updatedAt"], providers::iso(1000).unwrap());

        // When credentials expire, the saved reading remains visible alongside explicit wait state.
        reopened.cache.lock().unwrap().claude.record(Err(claude::EXPIRED));
        *reopened.claude_poll.lock().unwrap() = Some(ClaudePoll {
            at: Instant::now(),
            checked_at_epoch: 1400,
            next_retry_epoch: None,
            wait_kind: ClaudeWaitKind::Credentials,
            credentials_stamp: None,
            wait: Duration::from_secs(120),
            consecutive_errors: 1,
            failed: true,
        });
        let expired = snapshot(&reopened, &desktop, 1400);
        assert_eq!(expired["providers"][0]["integration"], "error");
        assert_eq!(expired["providers"][0]["waitingFor"], "credentials");
        assert!(expired["providers"][0]["nextRetryAt"].is_null());
        assert_eq!(expired["providers"][0]["lastCheckedAt"], providers::iso(1400).unwrap());
        assert_eq!(expired["providers"][0]["quotas"][0]["usedPercent"], 32.5);

        // On rate-limit retry backoff, expose the exact retry deadline and active checking flag.
        reopened.cache.lock().unwrap().claude.record(Err(claude::BUSY));
        *reopened.claude_poll.lock().unwrap() = Some(ClaudePoll {
            at: Instant::now(),
            checked_at_epoch: 1400,
            next_retry_epoch: Some(1580),
            wait_kind: ClaudeWaitKind::Retry,
            credentials_stamp: None,
            wait: Duration::from_secs(180),
            consecutive_errors: 1,
            failed: true,
        });
        reopened.claude_checking.store(true, Ordering::Release);
        let retrying = snapshot(&reopened, &desktop, 1400);
        assert_eq!(retrying["providers"][0]["waitingFor"], "retry");
        assert_eq!(retrying["providers"][0]["nextRetryAt"], providers::iso(1580).unwrap());
        assert_eq!(retrying["providers"][0]["checking"], true);
    }

    #[test]
    fn claude_auto_busy_preserves_fresh_sample_while_manual_and_stale_surface_error() {
        let mut cache = ProviderCache::default();
        assert!(!should_preserve_fresh_claude_sample(&cache, &Err(claude::BUSY), false, 1000));

        cache.record(Ok(sample(1000)));
        assert!(should_preserve_fresh_claude_sample(&cache, &Err(claude::BUSY), false, 1120));
        assert!(!should_preserve_fresh_claude_sample(&cache, &Err(claude::BUSY), true, 1120));
        assert!(!should_preserve_fresh_claude_sample(&cache, &Err(claude::EXPIRED), false, 1120));
        assert!(!should_preserve_fresh_claude_sample(&cache, &Err(claude::BUSY), false, 1301));
    }
}
