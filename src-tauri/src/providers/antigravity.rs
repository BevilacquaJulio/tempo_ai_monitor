use crate::{
    core::preferences::is_trusted_executable_path,
    providers,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::{collections::HashSet, io::Read, path::Path, process::{Command, Stdio}, sync::mpsc, time::{Duration, Instant}};

pub const ID: &str = "antigravity";
const UPDATE_REQUIRED: &str = "Update Antigravity CLI to version 1.1.11 or newer to check usage here.";
const COULD_NOT_READ: &str = "Could not check Antigravity usage. Check your Google sign-in in Antigravity and try again.";

pub fn read(executable: &Path) -> Result<Reading, &'static str> {
    let executable = is_trusted_executable_path(executable).ok_or("Could not find a trusted local Antigravity CLI executable.")?;
    let version = run(&executable, &["--version"], Duration::from_secs(5)).ok_or(UPDATE_REQUIRED)?;
    if !supports_usage(&String::from_utf8_lossy(&version)) { return Err(UPDATE_REQUIRED); }
    // Older CLI versions can send unrecognized slash commands as model prompts.
    let output = run(&executable, &["-p", "/usage", "--output-format", "json"], Duration::from_secs(20)).ok_or(COULD_NOT_READ)?;
    let result = serde_json::from_slice(&output).map_err(|_| COULD_NOT_READ)?;
    parse_usage(&result, Utc::now().timestamp()).ok_or(COULD_NOT_READ)
}

fn supports_usage(version: &str) -> bool {
    let version = version.trim();
    let mut components = version.strip_prefix('v').unwrap_or(version).split('.').map(str::parse::<u64>);
    match (components.next(), components.next(), components.next(), components.next()) {
        (Some(Ok(major)), Some(Ok(minor)), Some(Ok(patch)), None) => (major, minor, patch) >= (1, 1, 11),
        _ => false,
    }
}

fn run(executable: &Path, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
    const MAX_BYTES: u64 = 128 * 1024;
    let mut command = Command::new(executable);
    command.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x08000000);
    let mut child = command.spawn().ok()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()
            .filter(|length| *length as u64 <= MAX_BYTES).map(|_| bytes);
        let _ = send.send(result);
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return receive.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok().flatten(),
            Ok(Some(_)) => return None,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Reading {
    pub at: i64,
    quotas: Vec<Value>,
}

impl Reading {
    pub fn snapshot(&self, name: &str, now: i64) -> Value {
        let mut quotas = self.quotas.clone();
        for quota in &mut quotas {
            let expired = quota["resetAt"].as_str().and_then(|at| DateTime::parse_from_rfc3339(at).ok())
                .is_some_and(|at| at.timestamp() <= now);
            if providers::observation_is_stale(self.at, now) || expired { quota["status"] = json!("stale"); }
        }
        let status = if quotas.iter().all(|quota| quota["status"] == "stale") { "stale" } else { "connected" };
        json!({"providerId":ID, "displayName":name, "planName":null, "integration":status, "quotas":quotas, "updatedAt":providers::iso(self.at)})
    }
}

fn parse_usage(result: &Value, at: i64) -> Option<Reading> {
    let groups = result["command"]["data"]["groups"].as_array()?;
    let mut ids = HashSet::new();
    let mut quotas = Vec::new();
    for bucket in groups.iter().filter_map(|group| group["buckets"].as_array()).flatten().take(32) {
        let Some(id) = bucket["id"].as_str().filter(|id| !id.is_empty() && id.len() <= 80
            && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))) else { continue; };
        let kind = match bucket["window"].as_str() { Some("5h") => "five_hour", Some("weekly") => "weekly", _ => continue };
        let Some(left) = bucket["remaining_fraction"].as_f64().filter(|left| (0.0..=1.0).contains(left)) else { continue; };
        if bucket["disabled"] == true || !ids.insert(id) { continue; }
        let reset = bucket["reset_time"].as_str().and_then(|time| DateTime::parse_from_rfc3339(time).ok()).map(|time| time.timestamp());
        let mut quota = providers::observed_quota(ID, kind, id, &json!((1.0 - left) * 100.0), &json!(reset), at, at);
        quota["label"] = json!(id);
        quota["scope"] = Value::Null;
        quota["type"] = json!("custom");
        quotas.push(quota);
    }
    (!quotas.is_empty()).then_some(Reading { at, quotas })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_command_requires_a_supported_release() {
        for version in ["1.1.10", "1.1.11-rc.1", "1.5", "", "agy"] { assert!(!supports_usage(version)); }
        for version in ["1.1.11", "v1.5.8\r\n", "2.0.0"] { assert!(supports_usage(version)); }
    }

    #[test]
    fn parses_partial_usage_without_treating_disabled_groups_as_empty_allowances() {
        let result = json!({"command":{"data":{"groups":[{"buckets":[
            {"id":"gemini-5h", "window":"5h", "remaining_fraction":0.75, "reset_time":"1970-01-01T00:18:20Z"},
            {"id":"gemini-weekly", "window":"weekly", "disabled":true, "remaining_fraction":0},
            {"id":"3p-weekly", "window":"weekly", "remaining_fraction":0},
        ]}]}}});
        let reading = parse_usage(&result, 1000).unwrap();
        let snapshot = reading.snapshot("Antigravity", 1050);
        let quotas = snapshot["quotas"].as_array().unwrap();
        assert_eq!(quotas.len(), 2);
        assert_eq!(quotas[0]["usedPercent"], 25.0);
        assert_eq!(quotas[1]["usedPercent"], 100.0);
        assert_eq!(reading.snapshot("Antigravity", 1100)["quotas"][0]["status"], "stale");
        assert_eq!(reading.snapshot("Antigravity", 1301)["integration"], "stale");
        assert!(parse_usage(&json!({"result":"hello"}), 1000).is_none());
    }
}
