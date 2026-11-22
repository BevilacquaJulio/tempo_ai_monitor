use crate::{
    core::preferences::{is_local_path, resolve_local_dir, resolve_local_file},
    providers::{self, Observation},
};
use chrono::DateTime;
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime},
};

// Defensive parsing for the read-only OAuth usage endpoint.
const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const CREDENTIALS_BYTES: u64 = 16 * 1024;
const RESPONSE_BYTES: u64 = 64 * 1024;
pub const SIGN_IN: &str = "Sign in to Claude Code with your Claude subscription to check usage here.";
pub const EXPIRED: &str = "Your Claude Code credentials expired. Start a Claude Code session (or open the CLI) and TempoAI will update automatically.";
pub const COULD_NOT_READ: &str = "Could not check Claude usage. Check your connection and try again.";
pub const BUSY: &str = "Claude is limiting usage checks right now. TempoAI will try again shortly.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CredentialsStamp {
    pub(crate) modified: SystemTime,
    pub(crate) len: u64,
}

struct Credentials {
    token: String,
    plan: Option<String>,
}

fn http_client() -> Result<&'static reqwest::blocking::Client, &'static str> {
    static CLIENT: OnceLock<Option<reqwest::blocking::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                // Low-frequency polling avoids stale TLS sockets after Windows sleep or network changes.
                .pool_max_idle_per_host(0)
                .redirect(reqwest::redirect::Policy::none())
                .https_only(true)
                .user_agent(concat!("tempo-ai/", env!("CARGO_PKG_VERSION")))
                .build()
                .ok()
        })
        .as_ref()
        .ok_or(COULD_NOT_READ)
}

fn credentials_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR").filter(|value| !value.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude")))
        .filter(|folder| is_local_path(folder))
}

fn resolved_credentials_in(folder: &Path) -> Option<PathBuf> {
    let resolved_dir = resolve_local_dir(folder)?;
    let resolved_file = resolve_local_file(&resolved_dir.join(".credentials.json"))?;
    resolved_file.starts_with(&resolved_dir).then_some(resolved_file)
}

pub fn has_credentials() -> bool {
    credentials_dir().and_then(|dir| resolved_credentials_in(&dir)).is_some()
}

pub fn credentials_stamp() -> Option<CredentialsStamp> {
    let path = credentials_dir().and_then(|dir| resolved_credentials_in(&dir))?;
    let metadata = fs::symlink_metadata(path).ok()?;
    (metadata.file_type().is_file() && !metadata.file_type().is_symlink())
        .then(|| metadata.modified().ok().map(|modified| CredentialsStamp { modified, len: metadata.len() }))
        .flatten()
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    let raw = headers.get(reqwest::header::RETRY_AFTER)?.to_str().ok()?.trim();
    raw.parse::<u64>().ok().map(|seconds| Duration::from_secs(seconds.clamp(30, 300)))
}

#[cfg(test)]
pub fn read(now: i64) -> Result<Observation, &'static str> {
    read_with_backoff(now).0
}

pub fn read_with_backoff(now: i64) -> (Result<Observation, &'static str>, Option<Duration>) {
    let dir = match credentials_dir() {
        Some(dir) => dir,
        None => return (Err(SIGN_IN), None),
    };
    let credentials = match credentials_from_dir(&dir, now) {
        Ok(credentials) => credentials,
        Err(error) => return (Err(error), None),
    };
    // The token stays in memory for this request only; it is never logged, stored, or sent to the webview.
    let client = match http_client() {
        Ok(client) => client,
        Err(error) => return (Err(error), None),
    };
    let response = match client.get(USAGE_URL).bearer_auth(&credentials.token)
        .header("anthropic-beta", "oauth-2025-04-20").header("accept", "application/json")
        .send()
    {
        Ok(response) => response,
        Err(_) => return (Err(COULD_NOT_READ), None),
    };
    drop(credentials.token);
    match response.status().as_u16() {
        200 => (),
        401 | 403 => return (Err(EXPIRED), None),
        429 => return (Err(BUSY), retry_after(response.headers())),
        _ => return (Err(COULD_NOT_READ), None),
    }
    let mut body = Vec::new();
    if response.take(RESPONSE_BYTES + 1).read_to_end(&mut body).is_err() || body.len() as u64 > RESPONSE_BYTES {
        return (Err(COULD_NOT_READ), None);
    }
    let usage = match serde_json::from_slice(&body) {
        Ok(usage) => usage,
        Err(_) => return (Err(COULD_NOT_READ), None),
    };
    (parse_usage(&usage, credentials.plan, now).ok_or(COULD_NOT_READ), None)
}

fn credentials_from_dir(folder: &Path, now: i64) -> Result<Credentials, &'static str> {
    let path = resolved_credentials_in(folder).ok_or(SIGN_IN)?;
    let file = fs::File::open(path).map_err(|_| SIGN_IN)?;
    let mut bytes = Vec::new();
    file.take(CREDENTIALS_BYTES + 1).read_to_end(&mut bytes).map_err(|_| SIGN_IN)?;
    if bytes.len() as u64 > CREDENTIALS_BYTES { return Err(SIGN_IN); }
    parse_credentials(&serde_json::from_slice(&bytes).map_err(|_| SIGN_IN)?, now)
}

fn parse_credentials(file: &Value, now: i64) -> Result<Credentials, &'static str> {
    let oauth = &file["claudeAiOauth"];
    let token = oauth["accessToken"].as_str()
        .filter(|token| !token.is_empty() && token.len() <= 4096 && token.bytes().all(|byte| byte.is_ascii_graphic()))
        .ok_or(SIGN_IN)?;
    // Read-only: TempoAI never refreshes the token so Claude Code stays signed in.
    if oauth["expiresAt"].as_i64().is_some_and(|expires| expires / 1000 <= now) { return Err(EXPIRED); }
    let plan = oauth["subscriptionType"].as_str()
        .filter(|plan| !plan.is_empty() && plan.len() <= 30 && plan.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
        .map(|plan| { let mut name = plan.replace('_', " "); name[..1].make_ascii_uppercase(); name });
    Ok(Credentials { token: token.to_owned(), plan })
}

fn parse_usage(usage: &Value, plan: Option<String>, now: i64) -> Option<Observation> {
    let windows = [("five_hour", "five_hour", ""), ("seven_day", "weekly", ""), ("seven_day_opus", "weekly", "Opus"), ("seven_day_sonnet", "weekly", "Sonnet")];
    let mut quotas = Vec::new();
    for (key, kind, scope) in windows {
        let window = &usage[key];
        let used = &window["utilization"];
        // Model-specific weekly windows are often null; only the main windows always appear (as unavailable when missing).
        if !scope.is_empty() && used.as_f64().is_none() { continue; }
        let reset = window["resets_at"].as_str().and_then(|at| DateTime::parse_from_rfc3339(at).ok()).map(|at| at.timestamp());
        quotas.push(providers::observed_quota("claude", kind, scope, used, &json!(reset), now, now));
    }
    quotas.iter().any(|row| row["usedPercent"].is_number()).then_some(Observation { plan, quotas, at: now, daily_tokens: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_access_token_and_rejects_expired_or_missing_sign_ins() {
        let file = json!({"claudeAiOauth":{"accessToken":"sk-test","refreshToken":"other","expiresAt":2_000_000,"subscriptionType":"max"}});
        let credentials = parse_credentials(&file, 1000).unwrap();
        assert_eq!(credentials.token, "sk-test");
        assert_eq!(credentials.plan.as_deref(), Some("Max"));
        assert_eq!(parse_credentials(&file, 2000).err(), Some(EXPIRED));
        assert_eq!(parse_credentials(&json!({"claudeAiOauth":{"accessToken":""}}), 1000).err(), Some(SIGN_IN));
        assert_eq!(parse_credentials(&json!({"claudeAiOauth":{"accessToken":"bad\ntoken"}}), 1000).err(), Some(SIGN_IN));
        assert_eq!(parse_credentials(&json!({}), 1000).err(), Some(SIGN_IN));
    }

    #[test]
    fn reads_local_credentials_file_and_rejects_directory_junction_and_unc_targets() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let real_dir = dir.path().join("real-claude");
        fs::create_dir(&real_dir).unwrap();
        fs::write(
            real_dir.join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"sk-synthetic","expiresAt":2000000,"subscriptionType":"pro"}}"#,
        )
        .unwrap();

        let creds = credentials_from_dir(&real_dir, 1000).unwrap();
        assert_eq!(creds.token, "sk-synthetic");
        assert_eq!(creds.plan.as_deref(), Some("Pro"));
        assert_eq!(credentials_from_dir(Path::new(r"\\server\share\.claude"), 1000).err(), Some(SIGN_IN));

        #[cfg(windows)]
        {
            let junction = dir.path().join("linked-claude");
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J", junction.to_str().unwrap(), real_dir.to_str().unwrap()])
                .output();
            if status.is_ok_and(|out| out.status.success()) {
                assert_eq!(credentials_from_dir(&junction, 1000).err(), Some(SIGN_IN));
                let _ = fs::remove_dir(&junction);
            }
        }
    }

    #[test]
    fn maps_usage_windows_and_skips_empty_model_limits() {
        let usage = json!({
            "five_hour":{"utilization":37.0, "resets_at":"1970-01-01T01:00:00+00:00"},
            "seven_day":{"utilization":12.5, "resets_at":"1970-01-08T00:00:00Z"},
            "seven_day_opus":null,
            "seven_day_sonnet":{"utilization":4.0, "resets_at":null},
        });
        let observation = parse_usage(&usage, Some("Pro".into()), 1000).unwrap();
        assert_eq!(observation.plan.as_deref(), Some("Pro"));
        assert_eq!(observation.quotas.len(), 3);
        assert_eq!(observation.quotas[0]["type"], "five_hour");
        assert_eq!(observation.quotas[0]["usedPercent"], 37.0);
        assert_eq!(observation.quotas[0]["status"], "available");
        assert_eq!(observation.quotas[0]["resetAt"], "1970-01-01T01:00:00Z");
        assert_eq!(observation.quotas[1]["type"], "weekly");
        assert!(observation.quotas[1]["scope"].is_null());
        assert_eq!(observation.quotas[2]["scope"], "Sonnet");
        assert!(parse_usage(&json!({"five_hour":null}), None, 1000).is_none());
        assert!(parse_usage(&json!({"error":"x"}), None, 1000).is_none());
    }

    #[test]
    fn parses_retry_after_header_within_safe_bounds() {
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(retry_after(&headers), None);
        headers.insert(reqwest::header::RETRY_AFTER, "5".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(30)));
        headers.insert(reqwest::header::RETRY_AFTER, "90".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(90)));
        headers.insert(reqwest::header::RETRY_AFTER, "9999".parse().unwrap());
        assert_eq!(retry_after(&headers), Some(Duration::from_secs(300)));
        headers.insert(reqwest::header::RETRY_AFTER, "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap());
        assert_eq!(retry_after(&headers), None);
    }

    #[test]
    #[ignore = "calls Anthropic with the local Claude Code sign-in"]
    fn live_usage_check() {
        let observation = read(chrono::Utc::now().timestamp()).unwrap();
        for quota in &observation.quotas { println!("{} {} {} {}", quota["type"], quota["scope"], quota["usedPercent"], quota["resetAt"]); }
    }
}
