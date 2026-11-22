pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod discovery;
pub mod jsonl;

use chrono::{DateTime, SecondsFormat};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub type Sessions = Vec<(i64, Value)>;
pub const HISTORY_SECONDS: i64 = 92 * 86400;
pub const OBSERVATION_MAX_AGE: i64 = 300;

pub fn observation_is_stale(at: i64, now: i64) -> bool {
    now.saturating_sub(at) > OBSERVATION_MAX_AGE
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Observation {
    pub plan: Option<String>,
    pub quotas: Vec<Value>,
    pub at: i64,
    #[serde(default)]
    pub daily_tokens: Vec<Value>,
}

pub struct Collection {
    pub sample: Result<Observation, &'static str>,
    pub sessions: Sessions,
}

impl Collection {
    pub(crate) fn failed(message: &'static str) -> Self {
        Self { sample: Err(message), sessions: Vec::new() }
    }
}

pub fn iso(epoch: i64) -> Option<String> {
    DateTime::from_timestamp(epoch, 0).map(|date| date.to_rfc3339_opts(SecondsFormat::Secs, true))
}

pub fn empty_quota(provider: &str, kind: &str, status: &str) -> Value {
    quota(provider, kind, "", None, None, status, None)
}

fn quota(
    provider: &str,
    kind: &str,
    scope: &str,
    value: Option<&Value>,
    observed: Option<i64>,
    status: &str,
    source: Option<&str>,
) -> Value {
    let used = value.and_then(Value::as_f64).filter(|used| (0.0..=100.0).contains(used));
    json!({
        "id": format!("{provider}-{kind}-{scope}"), "type": kind,
        "label": match kind { "five_hour" => "5-hour window", "weekly" => "Weekly limit", _ => "Additional limit" },
        "scope": (!scope.is_empty()).then_some(scope), "usedPercent": used, "remainingPercent": used.map(|value| 100.0 - value),
        "resetAt": null, "status": if used.is_some() { status } else { "unavailable" },
        "source": source, "updatedAt": observed.and_then(iso)
    })
}

pub(crate) fn observed_quota(provider: &str, kind: &str, scope: &str, used: &Value, reset: &Value, at: i64, now: i64) -> Value {
    let reset = reset.as_i64().filter(|reset| *reset > 0);
    let stale = observation_is_stale(at, now) || at > now || reset.is_some_and(|reset| reset <= now);
    let mut row = quota(provider, kind, scope, Some(used), Some(at), if stale { "stale" } else { "available" }, Some("cli"));
    row["resetAt"] = json!(reset.and_then(iso));
    row
}

pub(crate) fn safe_text(value: &Value, max: usize) -> Option<String> {
    value.as_str().filter(|text| !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control)).map(str::to_owned)
}

pub(crate) fn project(path: &Value) -> Option<String> {
    let path = path.as_str()?;
    safe_text(&json!(path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next()?), 180)
}

pub(crate) fn session(
    provider: &str,
    id: &str,
    project: Option<String>,
    model: Option<String>,
    started: Option<i64>,
    last: i64,
    updated: i64,
) -> Option<(i64, Value)> {
    Some((last, json!({
        "id": format!("{provider}:{id}"), "providerId": provider, "projectName": project, "model": model,
        "startedAt": started.and_then(iso), "lastActivityAt": iso(last)?, "updatedAt": iso(updated)?,
        "inputTokens": null, "outputTokens": null, "cacheTokens": null, "totalTokens": null, "source": "cli"
    })))
}

/// Windows Job Object guard configured with `KILL_ON_JOB_CLOSE` so child and grandchild
/// processes cannot outlive a completed or timed-out CLI check.
#[cfg(windows)]
pub(crate) struct ProcessJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl ProcessJob {
    pub(crate) fn attach(child: &std::process::Child) -> Option<Self> {
        use std::{ffi::c_void, mem::size_of, os::windows::io::AsRawHandle, ptr};
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
        };
        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast::<c_void>(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if configured == 0 {
                CloseHandle(job);
                return None;
            }
            if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Self(job))
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_quota_is_marked_stale_and_missing_never_becomes_zero() {
        assert_eq!(observed_quota("claude", "weekly", "", &json!(80), &json!(9000), 100, 401)["status"], "stale");
        assert_eq!(observed_quota("claude", "weekly", "", &json!(80), &json!(400), 399, 401)["status"], "stale");
        assert_eq!(observed_quota("claude", "weekly", "", &Value::Null, &json!(9000), 400, 401)["usedPercent"], Value::Null);
    }

    #[test]
    #[cfg(windows)]
    fn process_job_terminates_attached_process_on_drop() {
        use std::{os::windows::process::CommandExt, process::{Command, Stdio}, time::Instant};
        let mut command = Command::new("ping");
        command
            .args(["127.0.0.1", "-n", "15"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000);
        let mut child = command.spawn().expect("spawn ping");
        let job = ProcessJob::attach(&child).expect("attach Job Object");
        let started = Instant::now();
        drop(job);
        let _ = child.wait();
        assert!(started.elapsed().as_secs() < 5, "job close should terminate attached process immediately");
    }
}
