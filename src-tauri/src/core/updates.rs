use crate::core::preferences::require_window;
use serde::Serialize;
use serde_json::Value;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::OnceLock,
    time::Duration,
};
use tauri::WebviewWindow;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/BevilacquaJulio/tempo_ai_monitor/releases/latest";
const RELEASES_URL_PREFIX: &str = "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/";
const RELEASES_FALLBACK_URL: &str = "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/latest";
const MAX_RESPONSE_BYTES: u64 = 128 * 1024;
const MAX_URL_LEN: usize = 512;

const COULD_NOT_CHECK: &str = "Could not check for updates. Check your connection and try again.";
const RATE_LIMITED: &str = "GitHub is limiting update checks right now. Try again in a few minutes.";
const INVALID_URL: &str = "Only official TempoAI GitHub release links can be opened.";
const COULD_NOT_OPEN: &str = "Could not open the download link in your browser.";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheck {
    pub status: &'static str,
    pub current_version: &'static str,
    pub latest_version: Option<String>,
    pub published_at: Option<String>,
    pub notes: Option<String>,
    pub release_url: String,
    pub installer_url: Option<String>,
    pub portable_url: Option<String>,
    pub running_portable: bool,
}

fn http_client() -> Result<&'static reqwest::blocking::Client, &'static str> {
    static CLIENT: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .pool_max_idle_per_host(0)
                .redirect(reqwest::redirect::Policy::none())
                .https_only(true)
                .user_agent(concat!("tempo-ai/", env!("CARGO_PKG_VERSION")))
                .build()
                .ok()
        })
        .as_ref()
        .ok_or(COULD_NOT_CHECK)
}

fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw.trim();
    let version = trimmed.strip_prefix('v').or_else(|| trimmed.strip_prefix('V')).unwrap_or(trimmed);
    let mut parts = version.split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(major), Some(minor), Some(patch), None) => {
            Some((major.parse().ok()?, minor.parse().ok()?, patch.parse().ok()?))
        }
        _ => None,
    }
}

pub fn is_allowed_release_url(url: &str) -> bool {
    if url.len() > MAX_URL_LEN || !url.starts_with(RELEASES_URL_PREFIX) || url.contains("..") {
        return false;
    }
    let tail = &url[RELEASES_URL_PREFIX.len()..];
    !tail.is_empty()
        && tail.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'%' | b'~' | b'+')
        })
}

fn summarize_notes(raw: &str) -> Option<String> {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let cleaned: String = line
            .trim()
            .trim_start_matches(['#', '-', '*', ' '])
            .chars()
            .filter(|ch| !ch.is_control())
            .collect();
        let cleaned = cleaned.trim();
        if !cleaned.is_empty() {
            lines.push(cleaned.to_owned());
            if lines.len() >= 3 {
                break;
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    let joined = lines.join(" · ");
    let mut chars = joined.chars();
    let preview: String = chars.by_ref().take(240).collect();
    if chars.next().is_some() {
        Some(format!("{preview}…"))
    } else {
        Some(preview)
    }
}

fn is_running_portable() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_ascii_lowercase().contains("portable")))
        .unwrap_or(false)
}

fn parse_release(release: &Value, current_version: &'static str, running_portable: bool) -> Result<UpdateCheck, &'static str> {
    if release["draft"].as_bool() == Some(true) || release["prerelease"].as_bool() == Some(true) {
        return Ok(UpdateCheck {
            status: "no_release",
            current_version,
            latest_version: None,
            published_at: None,
            notes: None,
            release_url: RELEASES_FALLBACK_URL.to_owned(),
            installer_url: None,
            portable_url: None,
            running_portable,
        });
    }
    let tag = release["tag_name"].as_str().ok_or(COULD_NOT_CHECK)?;
    let latest_tuple = parse_semver(tag).ok_or(COULD_NOT_CHECK)?;
    let current_tuple = parse_semver(current_version).ok_or(COULD_NOT_CHECK)?;
    let latest_version = format!("{}.{}.{}", latest_tuple.0, latest_tuple.1, latest_tuple.2);
    let release_url = release["html_url"]
        .as_str()
        .filter(|url| is_allowed_release_url(url))
        .unwrap_or(RELEASES_FALLBACK_URL)
        .to_owned();
    let published_at = release["published_at"]
        .as_str()
        .filter(|at| chrono::DateTime::parse_from_rfc3339(at).is_ok())
        .map(str::to_owned);
    let notes = release["body"].as_str().and_then(summarize_notes);

    let mut installer_url = None;
    let mut portable_url = None;
    if let Some(assets) = release["assets"].as_array() {
        for asset in assets.iter().take(32) {
            let Some(name) = asset["name"].as_str() else { continue };
            let Some(url) = asset["browser_download_url"].as_str().filter(|url| is_allowed_release_url(url)) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if installer_url.is_none() && (lower.ends_with("-setup.exe") || lower.ends_with("_setup.exe")) {
                installer_url = Some(url.to_owned());
            } else if portable_url.is_none() && (lower.ends_with("-portable.exe") || lower.ends_with("_portable.exe")) {
                portable_url = Some(url.to_owned());
            }
        }
    }

    let status = if latest_tuple > current_tuple { "update_available" } else { "up_to_date" };
    Ok(UpdateCheck {
        status,
        current_version,
        latest_version: Some(latest_version),
        published_at,
        notes,
        release_url,
        installer_url,
        portable_url,
        running_portable,
    })
}

fn fetch_latest_release() -> Result<UpdateCheck, &'static str> {
    let current_version = env!("CARGO_PKG_VERSION");
    let running_portable = is_running_portable();
    let client = http_client()?;
    let response = client
        .get(LATEST_RELEASE_API)
        .header("accept", "application/vnd.github+json")
        .header("x-github-api-version", "2022-11-28")
        .send()
        .map_err(|_| COULD_NOT_CHECK)?;
    match response.status().as_u16() {
        200 => (),
        404 => {
            return Ok(UpdateCheck {
                status: "no_release",
                current_version,
                latest_version: None,
                published_at: None,
                notes: None,
                release_url: RELEASES_FALLBACK_URL.to_owned(),
                installer_url: None,
                portable_url: None,
                running_portable,
            });
        }
        403 | 429 => return Err(RATE_LIMITED),
        _ => return Err(COULD_NOT_CHECK),
    }
    let mut body = Vec::new();
    if response.take(MAX_RESPONSE_BYTES + 1).read_to_end(&mut body).is_err() || body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(COULD_NOT_CHECK);
    }
    let release: Value = serde_json::from_slice(&body).map_err(|_| COULD_NOT_CHECK)?;
    parse_release(&release, current_version, running_portable)
}

#[cfg(windows)]
fn launch_url(url: &str) -> Result<(), &'static str> {
    let explorer = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(|root| PathBuf::from(root).join("explorer.exe"))
        .filter(|path| path.is_file())
        .ok_or(COULD_NOT_OPEN)?;
    let mut command = Command::new(explorer);
    command.arg(url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x08000000);
    command.spawn().map(|_| ()).map_err(|_| COULD_NOT_OPEN)
}

#[cfg(not(windows))]
fn launch_url(_url: &str) -> Result<(), &'static str> {
    Err(COULD_NOT_OPEN)
}

#[tauri::command]
pub async fn check_for_updates(window: WebviewWindow) -> Result<UpdateCheck, String> {
    require_window(window.label(), &["main"], "Open Settings to check for updates.")?;
    tauri::async_runtime::spawn_blocking(fetch_latest_release)
        .await
        .map_err(|_| COULD_NOT_CHECK.to_owned())?
        .map_err(str::to_owned)
}

#[tauri::command]
pub fn open_release_url(window: WebviewWindow, url: String) -> Result<(), String> {
    require_window(window.label(), &["main"], "Open Settings to download updates.")?;
    if !is_allowed_release_url(&url) {
        return Err(INVALID_URL.to_owned());
    }
    launch_url(&url).map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_semver_and_detects_newer_releases_with_validated_assets() {
        assert_eq!(parse_semver("v0.2.1"), Some((0, 2, 1)));
        assert_eq!(parse_semver("1.0.0"), Some((1, 0, 0)));
        assert_eq!(parse_semver("v1.0.0-beta.1"), None);
        assert_eq!(parse_semver("not-a-version"), None);

        let payload = json!({
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/tag/v0.2.0",
            "published_at": "2026-11-22T12:00:00Z",
            "body": "## What's new\n- Faster Claude reconnection\n- Live retry countdown",
            "draft": false,
            "prerelease": false,
            "assets": [
                {
                    "name": "TempoAI-0.2.0-setup.exe",
                    "browser_download_url": "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/download/v0.2.0/TempoAI-0.2.0-setup.exe"
                },
                {
                    "name": "TempoAI-0.2.0-portable.exe",
                    "browser_download_url": "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/download/v0.2.0/TempoAI-0.2.0-portable.exe"
                },
                {
                    "name": "evil-setup.exe",
                    "browser_download_url": "https://evil.example/TempoAI-0.2.0-setup.exe"
                }
            ]
        });

        let newer = parse_release(&payload, "0.1.0", true).unwrap();
        assert_eq!(newer.status, "update_available");
        assert_eq!(newer.latest_version.as_deref(), Some("0.2.0"));
        assert_eq!(
            newer.installer_url.as_deref(),
            Some("https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/download/v0.2.0/TempoAI-0.2.0-setup.exe")
        );
        assert_eq!(
            newer.portable_url.as_deref(),
            Some("https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/download/v0.2.0/TempoAI-0.2.0-portable.exe")
        );
        assert!(newer.running_portable);
        assert!(newer.notes.as_deref().unwrap().contains("Faster Claude reconnection"));

        let same = parse_release(&payload, "0.2.0", false).unwrap();
        assert_eq!(same.status, "up_to_date");

        let older = parse_release(&payload, "0.3.0", false).unwrap();
        assert_eq!(older.status, "up_to_date");
    }

    #[test]
    fn allows_only_strict_github_release_urls_for_this_repository() {
        assert!(is_allowed_release_url(
            "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/tag/v0.2.0"
        ));
        assert!(is_allowed_release_url(
            "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/download/v0.2.0/TempoAI-0.2.0-setup.exe"
        ));
        for bad in [
            "http://github.com/BevilacquaJulio/tempo_ai_monitor/releases/tag/v0.2.0",
            "https://github.com/other/repo/releases/tag/v0.2.0",
            "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/../settings",
            "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/tag/v0.2.0?x=1",
            "https://github.com/BevilacquaJulio/tempo_ai_monitor/releases/tag/v0.2.0&calc",
            "javascript:alert(1)",
            "file:///C:/Windows/System32/cmd.exe",
        ] {
            assert!(!is_allowed_release_url(bad), "unexpectedly accepted: {bad}");
        }
    }
}
