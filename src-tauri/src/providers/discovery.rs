use crate::{
    core::preferences::{is_local_path, is_trusted_executable_path, require_window, resolve_local_file, strip_windows_verbatim, LocalProvider},
    providers::{self, claude, jsonl},
};
use chrono::DateTime;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashSet, fs, io::Read, path::{Path, PathBuf}};
use tauri::{AppHandle, WebviewWindow};

const MAX_SOURCE_BYTES: u64 = 32_768;
pub const MAX_LOCAL_SOURCES: usize = 20;

struct Agent {
    id: &'static str,
    names: &'static [&'static str],
}

const AGENTS: &[Agent] = &[
    Agent { id: "claude", names: &["claude"] },
    Agent { id: "codex", names: &["codex"] },
    Agent { id: "antigravity", names: &["agy", "antigravity"] },
    Agent { id: "gemini", names: &["gemini"] },
    Agent { id: "copilot", names: &["copilot"] },
    Agent { id: "cursor", names: &["cursor-agent", "cursor"] },
    Agent { id: "opencode", names: &["opencode"] },
    Agent { id: "windsurf", names: &["windsurf"] },
    Agent { id: "aider", names: &["aider"] },
    Agent { id: "amp", names: &["amp"] },
];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAgent {
    id: &'static str,
    executable: Option<String>,
    source_path: Option<String>,
    credentials_found: bool,
    logs_found: bool,
}

#[derive(Default)]
struct LocalSignals {
    claude_credentials: bool,
    claude_logs: bool,
    codex_credentials: bool,
    codex_logs: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    agents: Vec<DetectedAgent>,
}

fn safe_path(path: &Path) -> Option<String> {
    strip_windows_verbatim(path)?.to_str().map(str::to_owned)
}

fn is_provider_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("json")) && is_local_path(path)
}

fn resolved_json_file(path: &Path) -> Option<PathBuf> {
    if !is_provider_file(path) { return None; }
    let resolved = resolve_local_file(path)?;
    is_provider_file(&resolved).then_some(resolved)
}

fn find_executable(roots: &[PathBuf], names: &[&str], extensions: &[&str]) -> Option<PathBuf> {
    roots.iter().filter(|root| is_local_path(root)).find_map(|root| {
        names.iter().find_map(|name| {
            extensions.iter().map(|ext| root.join(format!("{name}.{ext}")))
                .find(|path| path.is_file())
        })
    })
}

fn find_trusted_exe(roots: &[PathBuf], names: &[&str]) -> Option<PathBuf> {
    roots.iter().filter(|root| is_local_path(root)).find_map(|root| {
        names.iter().find_map(|name| is_trusted_executable_path(&root.join(format!("{name}.exe"))))
    })
}

fn scan_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut add = |variable: &str, roots: &[&str]| {
        if let Some(root) = std::env::var_os(variable).map(PathBuf::from) {
            paths.extend(roots.iter().map(|suffix| root.join(suffix)));
        }
    };
    add("USERPROFILE", &[".local/bin", ".cargo/bin", "scoop/shims"]);
    add("APPDATA", &["npm"]);
    add("LOCALAPPDATA", &[
        "Programs/OpenAI/Codex/bin", "agy/bin", "Programs/Antigravity", "Programs/cursor",
        "Programs/Windsurf", "Microsoft/WinGet/Links", "Microsoft/WindowsApps",
    ]);
    add("ProgramFiles", &["Cursor", "Windsurf", "Antigravity", "nodejs"]);
    add("ProgramW6432", &["Cursor", "Windsurf", "Antigravity"]);
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&path).take(128));
    }
    let mut seen = HashSet::new();
    paths.retain(|path| seen.insert(path.clone()));
    paths
}

pub fn antigravity_executable() -> Option<PathBuf> {
    find_trusted_exe(&scan_paths(), &["agy"])
}

pub fn codex_executable() -> Option<PathBuf> {
    find_trusted_exe(&scan_paths(), &["codex"])
}

fn discover(roots: &[PathBuf], signals: &LocalSignals) -> Vec<DetectedAgent> {
    AGENTS.iter().filter_map(|agent| {
        let executable = find_executable(roots, agent.names, &["exe", "cmd", "bat"]).and_then(|path| safe_path(&path));
        let (credentials_found, logs_found) = match agent.id {
            "claude" => (signals.claude_credentials, signals.claude_logs),
            "codex" => (signals.codex_credentials, signals.codex_logs),
            _ => (false, false),
        };
        if executable.is_none() && !credentials_found && !logs_found { return None; }
        let source_path = match agent.id {
            "codex" => find_trusted_exe(roots, agent.names).and_then(|path| safe_path(&path)),
            "antigravity" => find_trusted_exe(roots, &["agy"]).and_then(|path| safe_path(&path)),
            _ => None,
        };
        Some(DetectedAgent { id: agent.id, executable, source_path, credentials_found, logs_found })
    }).collect()
}

#[tauri::command]
pub async fn scan_providers(_app: AppHandle, window: WebviewWindow) -> Result<ScanResult, String> {
    require_window(window.label(), &["main"], "Open the dashboard to scan for providers.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let signals = LocalSignals {
            claude_credentials: claude::has_credentials(),
            claude_logs: jsonl::claude_projects_dir().is_some_and(|dir| jsonl::has_jsonl_files(&dir, 4)),
            codex_credentials: jsonl::codex_home_dir().is_some_and(|dir| resolve_local_file(&dir.join("auth.json")).is_some()),
            codex_logs: jsonl::codex_sessions_dir().is_some_and(|dir| jsonl::has_jsonl_files(&dir, 5)),
        };
        ScanResult {
            agents: discover(&scan_paths(), &signals),
        }
    }).await.map_err(|_| "Could not scan for providers.".into())
}

pub fn validate_local_providers(sources: &[LocalProvider]) -> Result<(), &'static str> {
    if sources.len() > MAX_LOCAL_SOURCES { return Err("You can connect up to 20 local usage files."); }
    let mut ids = HashSet::new();
    for source in sources {
        let valid_id = source.id.strip_prefix("local-").is_some_and(|id| {
            !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
        if !valid_id || !ids.insert(&source.id) || source.name.trim().is_empty() || source.name.chars().count() > 80 ||
            source.name.chars().any(char::is_control) || !is_provider_file(Path::new(&source.path)) {
            return Err("Enter a provider name and an absolute path to a local JSON usage file.");
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageFile {
    observed_at: String,
    five_hour: Option<Window>,
    weekly: Option<Window>,
    #[serde(default)]
    buckets: Vec<Bucket>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    used_percent: f64,
    reset_at: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bucket {
    label: String,
    used_percent: f64,
    reset_at: Option<String>,
}

pub fn local_snapshot(source: &LocalProvider, now: i64) -> Value {
    let usage = read_usage(Path::new(&source.path), now);
    let (observed_at, quotas, error) = match usage {
        Ok((at, quotas)) => (providers::iso(at), quotas, None),
        Err(error) => (None, Vec::new(), Some(error)),
    };
    json!({ "providerId":source.id, "displayName":source.name, "planName":null,
        "integration": if error.is_some() { "error" } else if quotas.iter().all(|quota| quota["status"] == "stale") { "stale" } else { "connected" },
        "integrationError": error, "quotas":quotas, "updatedAt": observed_at })
}

pub fn check_source(source: &LocalProvider, now: i64) -> (&'static str, &'static str) {
    match read_usage(Path::new(&source.path), now) {
        Ok((_, quotas)) if quotas.iter().all(|quota| quota["status"] == "stale") => ("stale", "Connected, but the last reading is out of date."),
        Ok(_) => ("connected", "Connected."),
        Err(error) => ("error", error),
    }
}

fn read_usage(path: &Path, now: i64) -> Result<(i64, Vec<Value>), &'static str> {
    const INVALID: &str = "The usage file is invalid. Check the example format and timestamps.";
    const UNREADABLE: &str = "Could not read the local usage file. Check the saved path and access.";
    let path = resolved_json_file(path).ok_or(UNREADABLE)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| UNREADABLE)?;
    if !metadata.file_type().is_file() { return Err(UNREADABLE); }
    if metadata.len() > MAX_SOURCE_BYTES { return Err(INVALID); }
    let mut bytes = Vec::new();
    fs::File::open(&path).map_err(|_| UNREADABLE)?.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes).map_err(|_| UNREADABLE)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES { return Err(INVALID); }
    let usage: UsageFile = serde_json::from_slice(&bytes).map_err(|_| INVALID)?;
    let at = DateTime::parse_from_rfc3339(&usage.observed_at).map_err(|_| INVALID)?.timestamp();
    if at > now { return Err(INVALID); }
    let mut quotas = Vec::new();
    let fixed = [("five_hour", "5-hour window", usage.five_hour), ("weekly", "Weekly limit", usage.weekly)]
        .into_iter().filter_map(|(kind, label, window)| window.map(|window| (kind, label.to_owned(), window.used_percent, window.reset_at)));
    if usage.buckets.len() > 32 { return Err(INVALID); }
    let buckets = usage.buckets.into_iter().map(|bucket| ("custom", bucket.label, bucket.used_percent, bucket.reset_at));
    for (index, (kind, label, used, reset_at)) in fixed.chain(buckets).enumerate() {
        if !(0.0..=100.0).contains(&used) { return Err(INVALID); }
        if label.trim().is_empty() || label.chars().count() > 80 || label.chars().any(char::is_control) { return Err(INVALID); }
        let reset = reset_at.as_deref().map(DateTime::parse_from_rfc3339).transpose().map_err(|_| INVALID)?.map(|time| time.timestamp());
        quotas.push(json!({"id":format!("{kind}-{index}"), "type":kind, "label":label, "scope":null,
            "usedPercent":used, "remainingPercent":100.0-used,
            "resetAt":reset.and_then(providers::iso), "status":if providers::observation_is_stale(at, now) || reset.is_some_and(|time| time <= now) {"stale"} else {"available"},
            "source":"local", "updatedAt":providers::iso(at)}));
    }
    if quotas.is_empty() { return Err(INVALID); }
    Ok((at, quotas))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_files_without_launching_and_distinguishes_a_shim_from_native_codex() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let root = dir.path();
        for name in ["agy.exe", "codex.cmd", "not-an-agent.exe"] {
            fs::write(root.join(name), "this is not executable").unwrap();
        }
        fs::create_dir(root.join("claude.exe")).unwrap();
        let agents = discover(&[root.to_path_buf()], &LocalSignals::default());
        assert_eq!(agents.iter().map(|agent| agent.id).collect::<Vec<_>>(), ["codex", "antigravity"]);
        assert!(agents[0].source_path.is_none());
        fs::write(root.join("codex.exe"), "test").unwrap();
        assert!(discover(&[root.to_path_buf()], &LocalSignals::default())[0].source_path.is_some());
        let zero_config = discover(&[], &LocalSignals { claude_credentials: true, codex_logs: true, ..LocalSignals::default() });
        assert_eq!(zero_config.iter().map(|agent| agent.id).collect::<Vec<_>>(), ["claude", "codex"]);
        assert!(zero_config[0].credentials_found && zero_config[1].logs_found);
        assert!(safe_path(&fs::canonicalize(root.join("codex.exe")).unwrap()).is_some());
    }

    #[test]
    fn manual_sources_report_only_valid_observations_and_mark_old_data() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let file = dir.path().join("usage.json");
        let source = LocalProvider { id: "local-test".into(), name: "Test".into(), path: file.to_str().unwrap().into() };
        assert!(validate_local_providers(std::slice::from_ref(&source)).is_ok());
        assert!(validate_local_providers(&[source.clone(), source.clone()]).is_err());
        assert_eq!(local_snapshot(&source, 1000)["integration"], "error");
        fs::write(&file, br#"{"observedAt":"1970-01-01T00:16:40Z","fiveHour":{"usedPercent":42}}"#).unwrap();
        assert_eq!(local_snapshot(&source, 1001)["quotas"][0]["usedPercent"], 42.0);
        assert_eq!(local_snapshot(&source, 1301)["quotas"][0]["status"], "stale");
        assert_eq!(local_snapshot(&source, 1301)["integration"], "stale");
        assert_eq!(check_source(&source, 1301).0, "stale");
        assert_eq!(local_snapshot(&source, 999)["integration"], "error");
        fs::write(&file, br#"{"observedAt":"1970-01-01T00:16:40Z","buckets":[{"label":"model-weekly","usedPercent":25,"resetAt":"1970-01-01T00:20:00Z"}]}"#).unwrap();
        let result = local_snapshot(&source, 1001);
        assert_eq!(result["quotas"][0]["type"], "custom");
        assert_eq!(result["quotas"][0]["label"], "model-weekly");
        assert_eq!(result["quotas"][0]["usedPercent"], 25.0);
        assert_eq!(local_snapshot(&source, 1201)["quotas"][0]["status"], "stale");
        fs::write(&file, br#"{"observedAt":"1970-01-01T00:16:40Z","weekly":{"usedPercent":102}}"#).unwrap();
        assert_eq!(local_snapshot(&source, 1001)["integration"], "error");
    }
}
