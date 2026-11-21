use crate::{
    core::{
        alerts, history,
        preferences::{require_window, DesktopState, Preferences},
    },
    providers::{self, antigravity, claude, codex, discovery, jsonl, Collection, Observation},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

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
    last_attempt: Option<Instant>,
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

#[derive(Clone)]
struct ClaudePoll {
    at: Instant,
    credentials_stamp: Option<claude::CredentialsStamp>,
    wait: Duration,
    consecutive_errors: u32,
    failed: bool,
}

pub struct Monitor {
    cache: Mutex<Cache>,
    running: AtomicBool,
    pending: AtomicBool,
    path: PathBuf,
    claude_logs: Mutex<jsonl::ClaudeLogCache>,
    codex_logs: Mutex<jsonl::CodexLogCache>,
    claude_poll: Mutex<Option<ClaudePoll>>,
    claude_poll_reset: AtomicBool,
    claude_manual_running: AtomicBool,
    claude_check: Mutex<()>,
    antigravity_reading: Mutex<Option<antigravity::Reading>>,
    antigravity_check: Mutex<Option<(Instant, bool)>>,
    collection: Mutex<()>,
    history_write: Mutex<()>,
}

impl Monitor {
    pub fn new(path: PathBuf) -> Self {
        let reading = history::open(&path)
            .and_then(|db| history::read_antigravity(&db))
            .ok()
            .flatten()
            .map(|(_, reading)| reading);
        Self {
            cache: Mutex::default(),
            running: AtomicBool::new(false),
            pending: AtomicBool::new(false),
            path,
            claude_logs: Mutex::default(),
            codex_logs: Mutex::default(),
            claude_poll: Mutex::default(),
            claude_poll_reset: AtomicBool::new(false),
            claude_manual_running: AtomicBool::new(false),
            claude_check: Mutex::default(),
            antigravity_reading: Mutex::new(reading),
            antigravity_check: Mutex::default(),
            collection: Mutex::default(),
            history_write: Mutex::default(),
        }
    }

    pub fn preferences_path(&self) -> PathBuf {
        self.path.with_file_name("preferences.json")
    }

    pub fn reset_changed_sources(&self, previous: &Preferences, next: &Preferences) -> bool {
        let claude_changed = previous.claude_enabled != next.claude_enabled;
        let codex_changed = previous.codex_enabled != next.codex_enabled;
        let agy_changed = previous.antigravity_enabled != next.antigravity_enabled;
        let locals_changed = previous.local_providers != next.local_providers;
        if claude_changed || codex_changed {
            let mut cache = self.cache.lock().unwrap_or_else(|error| error.into_inner());
            if claude_changed {
                cache.claude.reset();
                self.claude_poll_reset.store(true, Ordering::Release);
            }
            if codex_changed { cache.codex.reset(); }
            cache.last_attempt = None;
        }
        if agy_changed {
            if let Ok(mut check) = self.antigravity_check.try_lock() {
                *check = None;
            }
        }
        claude_changed || codex_changed || agy_changed || locals_changed
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

fn should_skip_antigravity_auto(last_check: &Option<(Instant, bool)>, manual: bool) -> bool {
    if manual {
        return false;
    }
    last_check.as_ref().is_some_and(|(at, failed)| {
        at.elapsed() < Duration::from_secs(if *failed { 300 } else { 120 })
    })
}

pub fn queue(app: &AppHandle, immediate: bool) {
    let claude_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || { let _ = check_claude(&claude_app, false); });
    let agy_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || { let _ = check_antigravity(&agy_app, false); });
    let state = app.state::<Monitor>();
    if immediate { state.pending.store(true, Ordering::Release); }
    if state.running.swap(true, Ordering::AcqRel) { return; }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Monitor>();
        {
            let _running = Running(&state.running);
            let kind = if state.pending.swap(false, Ordering::AcqRel) { Refresh::Requested } else { Refresh::Periodic };
            refresh(&app, &state, kind);
        }
        if state.pending.load(Ordering::Acquire) { queue(&app, false); }
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationCheck {
    provider_id: String,
    status: &'static str,
    message: &'static str,
}

fn refresh(app: &AppHandle, state: &Monitor, kind: Refresh) -> Vec<IntegrationCheck> {
    let _collection = state.collection.lock().unwrap_or_else(|error| error.into_inner());
    let desktop = app.state::<DesktopState>();
    let (settings, claude_revision, codex_revision) = {
        let settings = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let mut cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        if !kind.can_start(cache.last_attempt) { return Vec::new(); }
        if settings.claude_enabled || settings.codex_enabled { cache.last_attempt = Some(Instant::now()); }
        (settings.clone(), cache.claude.revision, cache.codex.revision)
    };
    let now = Utc::now().timestamp();
    let claude_sessions = settings.claude_enabled.then(|| {
        jsonl::claude_projects_dir()
            .map(|dir| jsonl::read_claude_sessions(&dir, now, &mut state.claude_logs.lock().unwrap_or_else(|error| error.into_inner())))
            .unwrap_or_default()
    });
    let codex_data = settings.codex_enabled.then(|| {
        let logs = jsonl::codex_sessions_dir()
            .map(|dir| jsonl::read_codex_sessions(&dir, now, &mut state.codex_logs.lock().unwrap_or_else(|error| error.into_inner())))
            .unwrap_or(jsonl::CodexLogResult { sessions: Vec::new(), latest_observation: None });
        let rpc = discovery::codex_executable().as_deref().map(|path| codex::read(path, now));
        merge_codex(rpc, logs, now)
    });
    let (checks, history_rows, current) = {
        let current = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let mut cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        let claude_rows = claude_sessions.filter(|_| cache.claude.revision == claude_revision).map(|sessions| (sessions, None));
        let (codex_check, codex_rows) = record_check(&mut cache.codex, "codex", codex_revision, codex_data, now);
        (
            vec![codex_check],
            [("claude", "Claude Code", false, claude_rows), ("codex", "OpenAI Codex", current.codex_alerts, codex_rows)],
            current.clone(),
        )
    };
    let portuguese = *desktop.portuguese.lock().unwrap_or_else(|error| error.into_inner());
    if history_rows.iter().any(|(_, _, _, rows)| rows.is_some()) {
        let _write = state.history_write.lock().unwrap_or_else(|error| error.into_inner());
        if let Ok(mut db) = history::open(&state.path) {
            if let Ok(cutoff) = history::clear_cutoff(&db) {
                for (id, display_name, alert_enabled, rows) in history_rows {
                    let Some((mut sessions, alert_sample)) = rows else { continue; };
                    sessions.retain(|(at, _)| *at > cutoff);
                    let _ = history::write(&mut db, id, &sessions);
                    if let (Some(portuguese), Some(sample)) = (portuguese, alert_sample.filter(|_| current.notifications && alert_enabled && now > cutoff)) {
                        alerts::notify(app, &db, id, display_name, &sample, Utc::now().timestamp(), &current.thresholds, portuguese);
                    }
                }
                let _ = history::prune(&db, now.saturating_sub(providers::HISTORY_SECONDS), now);
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
pub fn request_refresh(app: AppHandle) { queue(&app, true); }

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
    let previous_errors = {
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
        last.as_ref().map_or(0, |poll| poll.consecutive_errors)
    };
    let now = Utc::now().timestamp();
    let (result, retry_after) = claude::read_with_backoff(now);
    let post_stamp = claude::credentials_stamp();
    let consecutive_errors = if result.is_ok() { 0 } else { previous_errors.saturating_add(1) };
    let wait = claude_auto_wait(&result, retry_after, consecutive_errors);
    let (preserve_fresh, alert_context) = {
        let current = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let mut cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        if !current.claude_enabled || cache.claude.revision != revision {
            return Err("Saved settings changed during the check. Try again.");
        }
        let preserve = should_preserve_fresh_claude_sample(&cache.claude, &result, manual, now);
        let alert = if preserve {
            None
        } else {
            let alert_sample = result.as_ref().ok().cloned();
            cache.claude.record(result.clone());
            alert_sample.filter(|_| current.notifications && current.claude_alerts)
                .map(|sample| (sample, current.thresholds.clone()))
        };
        (preserve, alert)
    };
    *state.claude_poll.lock().unwrap_or_else(|error| error.into_inner()) = Some(ClaudePoll {
        at: Instant::now(),
        credentials_stamp: post_stamp,
        wait,
        consecutive_errors,
        failed: result.is_err(),
    });
    if preserve_fresh {
        return Ok(());
    }
    if let Some((sample, thresholds)) = alert_context {
        let portuguese = *desktop.portuguese.lock().unwrap_or_else(|error| error.into_inner());
        if let Some(portuguese) = portuguese {
            let _write = state.history_write.lock().unwrap_or_else(|error| error.into_inner());
            if let Ok(db) = history::open(&state.path) {
                if let Ok(cutoff) = history::clear_cutoff(&db) {
                    if now > cutoff {
                        alerts::notify(app, &db, "claude", "Claude Code", &sample, Utc::now().timestamp(), &thresholds, portuguese);
                    }
                }
            }
        }
    }
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
    let enabled = desktop.config.lock().unwrap_or_else(|error| error.into_inner()).antigravity_enabled;
    if !enabled {
        return Err("Connect Google Antigravity first.");
    }
    let Ok(mut last_check) = state.antigravity_check.try_lock() else { return Err("Antigravity usage is already being checked."); };
    if should_skip_antigravity_auto(&last_check, manual) { return Ok(()); }
    let result = antigravity::read();
    *last_check = Some((Instant::now(), result.is_err()));
    let reading = result?;
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
    let _ = app.emit("usage-updated", ());
    saved.map_err(|_| "Antigravity usage was checked but could not be saved for next time.")
}

fn antigravity_snapshot(state: &Monitor, now: i64) -> Value {
    let saved = state.antigravity_reading.lock().unwrap_or_else(|error| error.into_inner());
    match saved.as_ref() {
        Some(reading) => {
            let mut value = reading.snapshot("Google Antigravity", now);
            value["connectionSaved"] = json!(true);
            value
        }
        None => json!({
            "providerId": antigravity::ID,
            "displayName": "Google Antigravity",
            "connectionSaved": true,
            "planName": null,
            "integration": "not_connected",
            "integrationError": null,
            "quotas": [],
            "updatedAt": null
        }),
    }
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

#[tauri::command]
pub async fn usage_snapshot(app: AppHandle, include_sessions: Option<bool>) -> Result<Value, String> {
    let with_sessions = include_sessions.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        snapshot(&app.state::<Monitor>(), &app.state::<DesktopState>(), Utc::now().timestamp(), with_sessions)
    })
    .await
    .map_err(|_| "Could not load usage.".into())
}

#[tauri::command]
pub async fn session_history(app: AppHandle, window: WebviewWindow) -> Result<Value, String> {
    require_window(window.label(), &["main"], "Open the dashboard to view session history.")?;
    tauri::async_runtime::spawn_blocking(move || {
        read_sessions_snapshot(&app.state::<Monitor>(), Utc::now().timestamp())
    })
    .await
    .map_err(|_| "Could not load session history.".into())
}

fn read_sessions_snapshot(state: &Monitor, now: i64) -> Value {
    let history = history::open(&state.path).and_then(|db| history::read(&db, now - providers::HISTORY_SECONDS));
    json!({
        "historyError": history.is_err(),
        "sessions": history.unwrap_or_default(),
    })
}

fn snapshot(state: &Monitor, desktop: &DesktopState, now: i64, include_sessions: bool) -> Value {
    let (mut snapshot, antigravity_enabled, sources) = {
        let config = desktop.config.lock().unwrap_or_else(|error| error.into_inner());
        let cache = state.cache.lock().unwrap_or_else(|error| error.into_inner());
        (quota_snapshot(&config, &cache, now), config.antigravity_enabled, config.local_providers.clone())
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
    if include_sessions {
        let sessions = read_sessions_snapshot(state, now);
        snapshot["historyError"] = sessions["historyError"].clone();
        snapshot["sessions"] = sessions["sessions"].clone();
    } else {
        snapshot["historyError"] = json!(false);
        snapshot["sessions"] = json!([]);
    }
    snapshot
}

fn quota_snapshot(config: &Preferences, cache: &Cache, now: i64) -> Value {
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
        json!({
            "providerId":id, "displayName":name, "connectionSaved":true, "planName": sample.and_then(|data| data.plan.as_ref()),
            "integration": if error.is_some() { "error" } else if stale { "stale" } else if sample.is_some() { "connected" } else { "not_connected" },
            "integrationError": error,
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
        let fresh = snapshot(&reopened, &desktop, 1100, false);
        assert_eq!(fresh["providers"][0]["providerId"], "antigravity");
        assert_eq!(fresh["providers"][0]["connectionSaved"], true);
        assert_eq!(fresh["providers"][0]["integration"], "connected");
        let stale = snapshot(&reopened, &desktop, 1301, false);
        assert_eq!(stale["providers"][0]["connectionSaved"], true);
        assert_eq!(stale["providers"][0]["integration"], "stale");
        assert_eq!(stale["providers"][0]["quotas"][0]["usedPercent"], 25.0);
    }

    fn sample(at: i64) -> Observation {
        Observation { plan: None, quotas: vec![json!({"type":"five_hour","status":"available","usedPercent":70.1})], at,
            daily_tokens: vec![json!({"day":"2026-11-12","tokens":100})] }
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
        let light = snapshot(&state, &desktop, 1010, false);
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
        let recent_failure = Some((Instant::now(), true));
        assert!(should_skip_antigravity_auto(&recent_failure, false));
        assert!(!should_skip_antigravity_auto(&recent_failure, true));
        let expired_failure = Instant::now().checked_sub(Duration::from_secs(301)).map(|at| (at, true));
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
    fn claude_auto_retry_handles_credential_renewal_transient_backoff_and_manual_override() {
        let stamp_a = claude::CredentialsStamp { modified: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(10), len: 100 };
        let stamp_b = claude::CredentialsStamp { modified: std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(20), len: 120 };

        assert_eq!(claude_auto_wait(&Ok(sample(1000)), None, 0), Duration::from_secs(120));
        assert_eq!(claude_auto_wait(&Err(claude::BUSY), None, 1), Duration::from_secs(180));
        assert_eq!(claude_auto_wait(&Err(claude::BUSY), Some(Duration::from_secs(45)), 1), Duration::from_secs(45));
        assert_eq!(claude_auto_wait(&Err(claude::COULD_NOT_READ), None, 1), Duration::from_secs(30));
        assert_eq!(claude_auto_wait(&Err(claude::COULD_NOT_READ), None, 2), Duration::from_secs(60));
        assert_eq!(claude_auto_wait(&Err(claude::COULD_NOT_READ), None, 3), Duration::from_secs(120));

        // Same credentials after failure stay throttled automatically, but manual retry always runs immediately.
        let recent_failure = Some(ClaudePoll {
            at: Instant::now(),
            credentials_stamp: Some(stamp_a),
            wait: Duration::from_secs(180),
            consecutive_errors: 1,
            failed: true,
        });
        assert!(should_skip_claude_auto(&recent_failure, Some(stamp_a), false));
        assert!(!should_skip_claude_auto(&recent_failure, Some(stamp_a), true));
        // Updating .credentials.json after a failure triggers immediate auto-reconnect.
        assert!(!should_skip_claude_auto(&recent_failure, Some(stamp_b), false));
        // Missing credentials followed by new sign-in also reconnects immediately.
        let missing_creds = Some(ClaudePoll {
            at: Instant::now(),
            credentials_stamp: None,
            wait: Duration::from_secs(120),
            consecutive_errors: 1,
            failed: true,
        });
        assert!(should_skip_claude_auto(&missing_creds, None, false));
        assert!(!should_skip_claude_auto(&missing_creds, Some(stamp_a), false));
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
