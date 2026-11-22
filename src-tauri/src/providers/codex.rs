use crate::{
    core::preferences::is_trusted_executable_path,
    providers::{self, Collection, Observation, HISTORY_SECONDS},
};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Rpc {
    child: Child,
    messages: mpsc::Receiver<Value>,
    deadline: Instant,
}

impl Rpc {
    fn open(executable: &Path) -> Result<Self, &'static str> {
        let executable = is_trusted_executable_path(executable).ok_or("Could not find a trusted local Codex executable.")?;
        let mut command = Command::new(&executable);
        command.args(["app-server", "--listen", "stdio://"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut command, 0x08000000);
        let mut child = command.spawn().map_err(|_| "Could not start the Codex executable.")?;
        let output = child.stdout.take().expect("piped stdout");
        let (tx, messages) = mpsc::sync_channel(16);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut bytes = Vec::new();
                match reader.by_ref().take(2 * 1024 * 1024).read_until(b'\n', &mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if bytes.last() != Some(&b'\n') => break,
                    Ok(_) => if let Ok(message) = serde_json::from_slice::<Value>(&bytes) {
                        if message.get("id").is_some() && tx.send(message).is_err() { break; }
                    },
                }
            }
        });
        Ok(Self { child, messages, deadline: Instant::now() + Duration::from_secs(20) })
    }

    fn send(&mut self, request: Value) -> Result<(), ()> {
        let stdin = self.child.stdin.as_mut().ok_or(())?;
        writeln!(stdin, "{request}").map_err(|_| ())?;
        stdin.flush().map_err(|_| ())
    }

    fn call(&mut self, id: u8, method: &str, params: Value) -> Result<Value, ()> {
        self.send(json!({"id": id, "method":method, "params":params}))?;
        loop {
            let timeout = self.deadline.saturating_duration_since(Instant::now());
            let mut response = self.messages.recv_timeout(timeout).map_err(|_| ())?;
            if response["id"].as_u64() == Some(id.into()) {
                return response.as_object_mut().and_then(|object| object.remove("result")).ok_or(());
            }
        }
    }
}

impl Drop for Rpc {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

fn codex_limits(result: &Value, now: i64) -> Vec<Value> {
    let mut quotas = Vec::new();
    let mut add = |id: &str, scope: &str, limits: &Value| {
        for (slot, window) in ["primary", "secondary"].into_iter().map(|slot| (slot, &limits[slot])) {
            let kind = match window["windowDurationMins"].as_i64() { Some(300) => "five_hour", Some(10080) => "weekly", _ => continue };
            let mut row = providers::observed_quota("codex", kind, scope, &window["usedPercent"], &window["resetsAt"], now, now);
            row["id"] = json!(format!("codex-{kind}-{id}-{slot}"));
            quotas.push(row);
        }
    };
    if let Some(groups) = result["rateLimitsByLimitId"].as_object().filter(|groups| !groups.is_empty()) {
        for (id, group) in groups {
            let scope = if id == "codex" { String::new() } else { providers::safe_text(&group["limitName"], 80).unwrap_or_else(|| id.chars().take(80).collect()) };
            add(id, &scope, group);
        }
    } else { add("codex", "", &result["rateLimits"]); }
    quotas
}

pub fn read(executable: &Path, now: i64) -> Collection {
    let mut rpc = match Rpc::open(executable) {
        Ok(rpc) => rpc,
        Err(message) => return Collection::failed(message),
    };
    if rpc.call(1, "initialize", json!({"clientInfo":{"name":"tempo_ai","title":"TempoAI","version":env!("CARGO_PKG_VERSION")}})).is_err() ||
        rpc.send(json!({"method":"initialized","params":{}})).is_err() {
        return Collection::failed("Could not initialize the Codex App Server.");
    }
    let sample = codex_sample(now, |id, method, params| rpc.call(id, method, params));
    let mut sessions = Vec::new();
    let mut cursor = Value::Null;
    for page in 0..4 {
        let Ok(list) = rpc.call(10 + page, "thread/list", json!({"limit":100,"cursor":cursor,"sortKey":"updated_at"})) else { break; };
        let Some(rows) = list["data"].as_array() else { break; };
        let mut older = false;
        for row in rows {
            let Some(last) = row["updatedAt"].as_i64() else { continue; };
            if last < now - HISTORY_SECONDS { older = true; continue; }
            if last > now || row["ephemeral"] == true { continue; }
            let Some(id) = providers::safe_text(&row["id"], 120) else { continue; };
            if let Some(session) = providers::session("codex", &id, providers::project(&row["cwd"]), None, row["createdAt"].as_i64(), last, now) {
                sessions.push(session);
            }
        }
        cursor = list["nextCursor"].clone();
        if older || !cursor.is_string() { break; }
    }
    Collection { sample, sessions }
}

fn codex_sample(now: i64, mut call: impl FnMut(u8, &str, Value) -> Result<Value, ()>) -> Result<Observation, &'static str> {
    let account = call(2, "account/read", json!({"refreshToken":false})).map_err(|_| "Could not read the Codex account. Check your Codex sign-in.")?;
    let details = account.get("account").ok_or("Codex did not report its account status.")?;
    if details.is_null() && account["requiresOpenaiAuth"] == true {
        return Err("Codex sign-in is required. Sign in to Codex and try again.");
    }
    let subscribed = details["type"].as_str() == Some("chatgpt");
    let plan = subscribed.then(|| providers::safe_text(&details["planType"], 50)).flatten();
    let quotas = if subscribed {
        let limits = call(3, "account/rateLimits/read", json!({})).map_err(|_| "Could not read Codex usage limits. Check your Codex sign-in and try again.")?;
        codex_limits(&limits, now)
    } else { Vec::new() };
    let daily_tokens = if subscribed {
        call(4, "account/usage/read", json!({})).ok().map(|usage| parse_daily_usage(&usage)).unwrap_or_default()
    } else { Vec::new() };
    Ok(Observation { plan, quotas, at: now, daily_tokens })
}

fn parse_daily_usage(result: &Value) -> Vec<Value> {
    result["dailyUsageBuckets"].as_array().into_iter().flatten().take(366).filter_map(|row| {
        let day = row["startDate"].as_str()?;
        let parsed = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
        let tokens = row["tokens"].as_u64().filter(|count| *count <= 9_007_199_254_740_991)?;
        Some(json!({"day":parsed.to_string(), "tokens":tokens}))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_uses_documented_window_lengths_and_keeps_model_scopes() {
        let limits = json!({"rateLimitsByLimitId":{
            "codex":{"primary":{"usedPercent":88,"windowDurationMins":300,"resetsAt":2000},"secondary":{"usedPercent":50,"windowDurationMins":15}},
            "other":{"limitName":"Model A","secondary":{"usedPercent":92,"windowDurationMins":10080,"resetsAt":3000}}
        }});
        let rows = codex_limits(&limits, 1000);
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|row| row["type"] == "weekly" && row["scope"] == "Model A"));
        assert!(rows.iter().any(|row| row["type"] == "five_hour" && row["remainingPercent"] == 12.0));
    }

    #[test]
    fn codex_daily_usage_preserves_only_dated_token_totals() {
        let response = json!({"dailyUsageBuckets":[{"startDate":"2026-11-12","tokens":45},{"startDate":"not a date","tokens":100},{"startDate":"2026-11-13","tokens":null}]});
        assert_eq!(parse_daily_usage(&response), vec![json!({"day":"2026-11-12","tokens":45})]);
    }

    #[test]
    fn codex_reports_missing_executable_sign_in_and_quota_failures() {
        let directory = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        assert!(matches!(read(&directory.path().join("missing.exe"), 1000).sample, Err(message) if message.contains("local Codex executable")));
        let signed_out = codex_sample(1000, |_, method, _| {
            assert_eq!(method, "account/read");
            Ok(json!({"account":null,"requiresOpenaiAuth":true}))
        });
        assert!(matches!(signed_out, Err(message) if message.contains("sign-in is required")));
        let failed_quota = codex_sample(1000, |_, method, _| match method {
            "account/read" => Ok(json!({"account":{"type":"chatgpt"}})),
            "account/rateLimits/read" => Err(()),
            _ => panic!("unexpected method: {method}"),
        });
        assert!(matches!(failed_quota, Err(message) if message.contains("usage limits")));
        let api_key = codex_sample(1000, |_, method, _| {
            assert_eq!(method, "account/read");
            Ok(json!({"account":{"type":"apiKey"},"requiresOpenaiAuth":true}))
        });
        assert!(api_key.unwrap().quotas.is_empty());
    }
}
