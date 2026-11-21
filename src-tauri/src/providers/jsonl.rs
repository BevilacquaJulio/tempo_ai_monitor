use crate::{
    core::preferences::is_local_path,
    providers::{self, Observation, Sessions, HISTORY_SECONDS},
};
use chrono::DateTime;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const MAX_JSONL_FILES: usize = 2_500;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOKEN_VALUE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Default)]
pub struct ParsedClaudeFile {
    session_id: Option<String>,
    project_name: Option<String>,
    git_branch: Option<String>,
    model: Option<String>,
    started_at: Option<i64>,
    last_activity_at: Option<i64>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    request_count: u64,
    has_usage: bool,
}

#[derive(Clone, Default)]
pub struct ParsedCodexFile {
    session_id: Option<String>,
    project_name: Option<String>,
    git_branch: Option<String>,
    model: Option<String>,
    started_at: Option<i64>,
    last_activity_at: Option<i64>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    total_tokens: u64,
    request_count: u64,
    has_usage: bool,
    rate_limits: Option<(i64, Option<String>, Value)>,
}

pub type ClaudeLogCache = HashMap<PathBuf, (SystemTime, u64, ParsedClaudeFile)>;
pub type CodexLogCache = HashMap<PathBuf, (SystemTime, u64, ParsedCodexFile)>;

pub struct CodexLogResult {
    pub sessions: Sessions,
    pub latest_observation: Option<Observation>,
}

pub fn claude_projects_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".claude")))
        .filter(|folder| is_local_path(folder))
        .map(|folder| folder.join("projects"))
}

pub fn codex_home_dir() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join(".codex")))
        .filter(|folder| is_local_path(folder))
}

pub fn codex_sessions_dir() -> Option<PathBuf> {
    codex_home_dir().map(|folder| folder.join("sessions"))
}

pub fn has_jsonl_files(root: &Path, max_depth: usize) -> bool {
    if !is_local_path(root) {
        return false;
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut inspected = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            inspected += 1;
            if inspected > 512 {
                return false;
            }
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl")) {
                return true;
            }
            if kind.is_dir() && depth < max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
    false
}

fn collect_jsonl_files(root: &Path, max_depth: usize, cutoff: SystemTime) -> Vec<(SystemTime, PathBuf, u64)> {
    if !is_local_path(root) {
        return Vec::new();
    }
    let mut files = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut visited_dirs = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if visited_dirs >= 2_000 {
            break;
        }
        visited_dirs += 1;
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() && depth < max_depth {
                stack.push((path, depth + 1));
            } else if kind.is_file() && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl")) {
                let Ok(meta) = entry.metadata() else { continue };
                let Ok(modified) = meta.modified() else { continue };
                if modified >= cutoff && meta.len() <= MAX_FILE_BYTES {
                    files.push((modified, path, meta.len()));
                }
            }
        }
    }
    files.sort_unstable_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    files.truncate(MAX_JSONL_FILES);
    files
}

fn safe_text(value: &Value, max: usize) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty() && text.len() <= max && !text.chars().any(char::is_control))
        .map(str::to_owned)
}

fn project_basename(path: &Value) -> Option<String> {
    let raw = path.as_str()?.trim().trim_end_matches(['/', '\\']);
    let leaf = raw.rsplit(['/', '\\']).next()?;
    safe_text(&json!(leaf), 180)
}

fn parse_timestamp(value: &Value) -> Option<i64> {
    if let Some(text) = value.as_str() {
        return DateTime::parse_from_rfc3339(text).ok().map(|dt| dt.timestamp());
    }
    value.as_i64().filter(|ts| *ts > 0)
}

fn capped_u64(value: &Value) -> u64 {
    value.as_u64().filter(|n| *n <= MAX_TOKEN_VALUE).unwrap_or(0)
}

/// Reads lines bounded to MAX_LINE_BYTES; oversized lines are drained safely without allocation spikes.
fn for_each_line<F>(path: &Path, mut visit: F) -> std::io::Result<()>
where
    F: FnMut(usize, &[u8]),
{
    let file = fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(32 * 1024, file);
    let mut buffer = Vec::with_capacity(8 * 1024);
    let mut line_index = 0usize;
    loop {
        buffer.clear();
        let mut oversized = false;
        loop {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            if let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
                if !oversized && buffer.len() + pos <= MAX_LINE_BYTES {
                    buffer.extend_from_slice(&chunk[..pos]);
                } else {
                    oversized = true;
                }
                reader.consume(pos + 1);
                break;
            }
            let len = chunk.len();
            if !oversized && buffer.len() + len <= MAX_LINE_BYTES {
                buffer.extend_from_slice(chunk);
            } else {
                oversized = true;
            }
            reader.consume(len);
        }
        if buffer.is_empty() && !oversized {
            break;
        }
        if !oversized {
            if buffer.last() == Some(&b'\r') {
                buffer.pop();
            }
            if !buffer.is_empty() {
                visit(line_index, &buffer);
            }
        }
        line_index += 1;
    }
    Ok(())
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn parse_claude_file(path: &Path, fallback_id: Option<String>, fallback_project: Option<String>) -> ParsedClaudeFile {
    let mut parsed = ParsedClaudeFile {
        session_id: fallback_id,
        project_name: fallback_project,
        ..ParsedClaudeFile::default()
    };
    // Claude Code can emit multiple streaming chunks sharing one message.id; keep the latest/highest output per id.
    let mut requests: HashMap<String, (u64, u64, u64, u64)> = HashMap::new();

    let _ = for_each_line(path, |index, line| {
        let should_parse = index < 4
            || line.len() <= 4_096
            || contains_bytes(line, b"\"usage\"")
            || contains_bytes(line, b"\"assistant\"");
        if !should_parse {
            return;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(line) else { return };
        if let Some(id) = safe_text(&entry["sessionId"], 120) {
            parsed.session_id = Some(id);
        }
        if parsed.project_name.is_none() || entry.get("cwd").is_some() {
            if let Some(name) = project_basename(&entry["cwd"]) {
                parsed.project_name = Some(name);
            }
        }
        if let Some(branch) = safe_text(&entry["gitBranch"], 120) {
            parsed.git_branch = Some(branch);
        }
        if let Some(ts) = parse_timestamp(&entry["timestamp"]) {
            parsed.started_at = Some(parsed.started_at.map_or(ts, |prev| prev.min(ts)));
            parsed.last_activity_at = Some(parsed.last_activity_at.map_or(ts, |prev| prev.max(ts)));
        }
        let message = &entry["message"];
        if let Some(model) = safe_text(&message["model"], 120).filter(|m| !m.starts_with('<')) {
            parsed.model = Some(model);
        }
        let usage = &message["usage"];
        if usage.is_object() {
            let input = capped_u64(&usage["input_tokens"]);
            let output = capped_u64(&usage["output_tokens"]);
            let cache_read = capped_u64(&usage["cache_read_input_tokens"]);
            let cache_create = capped_u64(&usage["cache_creation_input_tokens"]);
            if input + output + cache_read + cache_create > 0 {
                let key = safe_text(&message["id"], 120)
                    .or_else(|| safe_text(&entry["requestId"], 120))
                    .unwrap_or_else(|| format!("line:{index}"));
                let replace = requests
                    .get(&key)
                    .is_none_or(|prev| output >= prev.1 && (input + output + cache_read + cache_create) >= (prev.0 + prev.1 + prev.2 + prev.3));
                if replace {
                    requests.insert(key, (input, output, cache_read, cache_create));
                }
            }
        }
    });

    if !requests.is_empty() {
        parsed.has_usage = true;
        parsed.request_count = requests.len() as u64;
        for (input, output, cache_read, cache_create) in requests.into_values() {
            parsed.input_tokens = parsed.input_tokens.saturating_add(input);
            parsed.output_tokens = parsed.output_tokens.saturating_add(output);
            parsed.cache_read_tokens = parsed.cache_read_tokens.saturating_add(cache_read);
            parsed.cache_creation_tokens = parsed.cache_creation_tokens.saturating_add(cache_create);
        }
    }
    parsed
}

pub fn read_claude_sessions(root: &Path, now: i64, cache: &mut ClaudeLogCache) -> Sessions {
    let cutoff_epoch = now.saturating_sub(HISTORY_SECONDS).max(0);
    let cutoff = SystemTime::UNIX_EPOCH + Duration::from_secs(cutoff_epoch as u64);
    let candidates = collect_jsonl_files(root, 4, cutoff);
    let mut next_cache = ClaudeLogCache::new();
    let mut merged: HashMap<String, ParsedClaudeFile> = HashMap::new();

    for (modified, file, length) in candidates {
        let parsed = match cache.remove(&file) {
            Some((time, size, existing)) if time == modified && size == length => existing,
            _ => {
                let stem = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty() && s.len() <= 120 && !s.chars().any(char::is_control))
                    .map(str::to_owned);
                parse_claude_file(&file, stem, None)
            }
        };
        let (_, _, entry) = next_cache.entry(file).or_insert((modified, length, parsed));
        let Some(id) = entry.session_id.clone() else { continue };
        let Some(last) = entry.last_activity_at.filter(|ts| *ts <= now + 300 && *ts >= cutoff_epoch) else { continue };
        let slot = merged.entry(id.clone()).or_insert_with(|| ParsedClaudeFile {
            session_id: Some(id),
            ..ParsedClaudeFile::default()
        });
        if entry.project_name.is_some() {
            slot.project_name = entry.project_name.clone();
        }
        if entry.git_branch.is_some() {
            slot.git_branch = entry.git_branch.clone();
        }
        if entry.model.is_some() {
            slot.model = entry.model.clone();
        }
        if let Some(start) = entry.started_at {
            slot.started_at = Some(slot.started_at.map_or(start, |prev| prev.min(start)));
        }
        slot.last_activity_at = Some(slot.last_activity_at.map_or(last, |prev| prev.max(last)));
        if entry.has_usage {
            slot.has_usage = true;
            slot.input_tokens = slot.input_tokens.saturating_add(entry.input_tokens);
            slot.output_tokens = slot.output_tokens.saturating_add(entry.output_tokens);
            slot.cache_read_tokens = slot.cache_read_tokens.saturating_add(entry.cache_read_tokens);
            slot.cache_creation_tokens = slot.cache_creation_tokens.saturating_add(entry.cache_creation_tokens);
            slot.request_count = slot.request_count.saturating_add(entry.request_count);
        }
    }
    *cache = next_cache;

    let mut sessions = Vec::with_capacity(merged.len());
    for (id, item) in merged {
        let Some(last) = item.last_activity_at else { continue };
        let last_clamped = last.min(now);
        let cache_total = item.cache_read_tokens.saturating_add(item.cache_creation_tokens);
        let total = item
            .input_tokens
            .saturating_add(item.output_tokens)
            .saturating_add(cache_total);
        let (input, output, cache_read, cache_create, cache_all, total_val, reqs) = if item.has_usage {
            (
                json!(item.input_tokens),
                json!(item.output_tokens),
                json!(item.cache_read_tokens),
                json!(item.cache_creation_tokens),
                json!(cache_total),
                json!(total),
                json!(item.request_count),
            )
        } else {
            (Value::Null, Value::Null, Value::Null, Value::Null, Value::Null, Value::Null, Value::Null)
        };
        let Some(last_iso) = providers::iso(last_clamped) else { continue };
        let Some(updated_iso) = providers::iso(now) else { continue };
        sessions.push((
            last_clamped,
            json!({
                "id": format!("claude:{id}"),
                "providerId": "claude",
                "projectName": item.project_name,
                "gitBranch": item.git_branch,
                "model": item.model,
                "startedAt": item.started_at.map(|ts| ts.min(last_clamped)).and_then(providers::iso),
                "lastActivityAt": last_iso,
                "updatedAt": updated_iso,
                "inputTokens": input,
                "outputTokens": output,
                "cacheReadTokens": cache_read,
                "cacheCreationTokens": cache_create,
                "cacheTokens": cache_all,
                "totalTokens": total_val,
                "requestCount": reqs,
                "source": "local"
            }),
        ));
    }
    sessions
}

fn parse_codex_file(path: &Path, fallback_id: Option<String>) -> ParsedCodexFile {
    let mut parsed = ParsedCodexFile {
        session_id: fallback_id,
        ..ParsedCodexFile::default()
    };
    let mut max_cumulative_total = 0u64;
    let mut cumulative_found = false;
    let mut fallback_input = 0u64;
    let mut fallback_output = 0u64;
    let mut fallback_cache_read = 0u64;
    let mut fallback_total = 0u64;

    let _ = for_each_line(path, |_, line| {
        let interesting = contains_bytes(line, b"session_meta")
            || contains_bytes(line, b"turn_context")
            || contains_bytes(line, b"token_count");
        if !interesting {
            return;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(line) else { return };
        let ts = parse_timestamp(&entry["timestamp"]);
        if let Some(ts) = ts {
            parsed.started_at = Some(parsed.started_at.map_or(ts, |prev| prev.min(ts)));
            parsed.last_activity_at = Some(parsed.last_activity_at.map_or(ts, |prev| prev.max(ts)));
        }
        let kind = entry["type"].as_str().unwrap_or("");
        let payload = &entry["payload"];
        match kind {
            "session_meta" => {
                if let Some(id) = safe_text(&payload["id"], 120) {
                    parsed.session_id = Some(id);
                }
                if let Some(name) = project_basename(&payload["cwd"]) {
                    parsed.project_name = Some(name);
                }
                if let Some(branch) = safe_text(&payload["git"]["branch"], 120) {
                    parsed.git_branch = Some(branch);
                }
                if let Some(start) = parse_timestamp(&payload["timestamp"]) {
                    parsed.started_at = Some(parsed.started_at.map_or(start, |prev| prev.min(start)));
                }
            }
            "turn_context" => {
                if let Some(model) = safe_text(&payload["model"], 120) {
                    parsed.model = Some(model);
                }
                if parsed.project_name.is_none() {
                    if let Some(name) = project_basename(&payload["cwd"]) {
                        parsed.project_name = Some(name);
                    }
                }
            }
            "event_msg" if payload["type"].as_str() == Some("token_count") => {
                let info = &payload["info"];
                if info["total_token_usage"].is_object() {
                    let usage = &info["total_token_usage"];
                    let raw_input = capped_u64(&usage["input_tokens"]);
                    let cache_read = capped_u64(&usage["cached_input_tokens"])
                        .max(capped_u64(&usage["cache_read_input_tokens"]));
                    let output = capped_u64(&usage["output_tokens"]);
                    let total = capped_u64(&usage["total_tokens"]).max(raw_input.saturating_add(output));
                    if total > max_cumulative_total {
                        cumulative_found = true;
                        max_cumulative_total = total;
                        // OpenAI input_tokens includes cached_input_tokens; separate them so pricing matches Claude's schema.
                        parsed.input_tokens = raw_input.saturating_sub(cache_read);
                        parsed.cache_read_tokens = cache_read;
                        parsed.output_tokens = output;
                        parsed.total_tokens = total;
                        parsed.has_usage = true;
                        parsed.request_count = parsed.request_count.saturating_add(1);
                    }
                } else if !cumulative_found && info["last_token_usage"].is_object() {
                    let usage = &info["last_token_usage"];
                    let raw_input = capped_u64(&usage["input_tokens"]);
                    let cache_read = capped_u64(&usage["cached_input_tokens"])
                        .max(capped_u64(&usage["cache_read_input_tokens"]));
                    let output = capped_u64(&usage["output_tokens"]);
                    let total = capped_u64(&usage["total_tokens"]).max(raw_input.saturating_add(output));
                    if total > 0 {
                        fallback_input = fallback_input.saturating_add(raw_input.saturating_sub(cache_read));
                        fallback_cache_read = fallback_cache_read.saturating_add(cache_read);
                        fallback_output = fallback_output.saturating_add(output);
                        fallback_total = fallback_total.saturating_add(total);
                        parsed.has_usage = true;
                        parsed.request_count = parsed.request_count.saturating_add(1);
                    }
                }
                if payload["rate_limits"].is_object() {
                    if let Some(at) = ts {
                        let plan = safe_text(&payload["rate_limits"]["plan_type"], 50);
                        let should_update = parsed.rate_limits.as_ref().is_none_or(|(prev, _, _)| at >= *prev);
                        if should_update {
                            parsed.rate_limits = Some((at, plan, payload["rate_limits"].clone()));
                        }
                    }
                }
            }
            _ => {}
        }
    });

    if !cumulative_found && fallback_total > 0 {
        parsed.input_tokens = fallback_input;
        parsed.cache_read_tokens = fallback_cache_read;
        parsed.output_tokens = fallback_output;
        parsed.total_tokens = fallback_total;
    }
    parsed
}

fn codex_jsonl_quotas(rate_limits: &Value, at: i64, now: i64) -> Vec<Value> {
    let mut quotas = Vec::new();
    for slot in ["primary", "secondary"] {
        let window = &rate_limits[slot];
        if !window.is_object() {
            continue;
        }
        let duration = window["window_minutes"]
            .as_i64()
            .or_else(|| window["windowDurationMins"].as_i64());
        let kind = match duration {
            Some(300) => "five_hour",
            Some(10080) => "weekly",
            _ => continue,
        };
        let used = if !window["used_percent"].is_null() {
            &window["used_percent"]
        } else {
            &window["usedPercent"]
        };
        let reset = if !window["resets_at"].is_null() {
            &window["resets_at"]
        } else {
            &window["resetsAt"]
        };
        let mut row = providers::observed_quota("codex", kind, "", used, reset, at, now);
        row["id"] = json!(format!("codex-{kind}-codex-{slot}"));
        row["source"] = json!("local");
        quotas.push(row);
    }
    quotas
}

pub fn read_codex_sessions(root: &Path, now: i64, cache: &mut CodexLogCache) -> CodexLogResult {
    let cutoff_epoch = now.saturating_sub(HISTORY_SECONDS).max(0);
    let cutoff = SystemTime::UNIX_EPOCH + Duration::from_secs(cutoff_epoch as u64);
    let candidates = collect_jsonl_files(root, 5, cutoff);
    let mut next_cache = CodexLogCache::new();
    let mut sessions = Vec::new();
    let mut latest_limits: Option<(i64, Option<String>, Value)> = None;

    for (modified, file, length) in candidates {
        let parsed = match cache.remove(&file) {
            Some((time, size, existing)) if time == modified && size == length => existing,
            _ => {
                let stem = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty() && s.len() <= 120 && !s.chars().any(char::is_control))
                    .map(str::to_owned);
                parse_codex_file(&file, stem)
            }
        };
        let (_, _, entry) = next_cache.entry(file).or_insert((modified, length, parsed));
        if let Some((at, plan, limits)) = &entry.rate_limits {
            if *at <= now + 300 && *at >= cutoff_epoch && latest_limits.as_ref().is_none_or(|(prev, _, _)| at > prev) {
                latest_limits = Some((*at, plan.clone(), limits.clone()));
            }
        }
        let Some(id) = entry.session_id.as_deref() else { continue };
        let Some(last) = entry.last_activity_at.filter(|ts| *ts <= now + 300 && *ts >= cutoff_epoch) else { continue };
        let last_clamped = last.min(now);
        let Some(last_iso) = providers::iso(last_clamped) else { continue };
        let Some(updated_iso) = providers::iso(now) else { continue };
        let (input, output, cache_read, cache_create, cache_all, total, reqs) = if entry.has_usage {
            (
                json!(entry.input_tokens),
                json!(entry.output_tokens),
                json!(entry.cache_read_tokens),
                json!(0u64),
                json!(entry.cache_read_tokens),
                json!(entry.total_tokens),
                json!(entry.request_count),
            )
        } else {
            (Value::Null, Value::Null, Value::Null, Value::Null, Value::Null, Value::Null, Value::Null)
        };
        sessions.push((
            last_clamped,
            json!({
                "id": format!("codex:{id}"),
                "providerId": "codex",
                "projectName": entry.project_name,
                "gitBranch": entry.git_branch,
                "model": entry.model,
                "startedAt": entry.started_at.map(|ts| ts.min(last_clamped)).and_then(providers::iso),
                "lastActivityAt": last_iso,
                "updatedAt": updated_iso,
                "inputTokens": input,
                "outputTokens": output,
                "cacheReadTokens": cache_read,
                "cacheCreationTokens": cache_create,
                "cacheTokens": cache_all,
                "totalTokens": total,
                "requestCount": reqs,
                "source": "local"
            }),
        ));
    }
    *cache = next_cache;

    let latest_observation = latest_limits.and_then(|(at, plan, limits)| {
        let clamped = at.min(now);
        let quotas = codex_jsonl_quotas(&limits, clamped, now);
        (!quotas.is_empty()).then_some(Observation {
            plan,
            quotas,
            at: clamped,
            daily_tokens: Vec::new(),
        })
    });

    CodexLogResult {
        sessions,
        latest_observation,
    }
}

/// Merges two session vectors by id, preferring records with known token totals or newer activity.
pub fn merge_sessions(primary: Sessions, secondary: Sessions) -> Sessions {
    let mut by_id: HashMap<String, (i64, Value)> = HashMap::new();
    for (at, row) in secondary.into_iter().chain(primary) {
        let Some(id) = row["id"].as_str().map(str::to_owned) else { continue };
        let replace = match by_id.get(&id) {
            None => true,
            Some((prev_at, prev_row)) => {
                let has_tokens = row["totalTokens"].is_number();
                let prev_tokens = prev_row["totalTokens"].is_number();
                (has_tokens && !prev_tokens) || (has_tokens == prev_tokens && at >= *prev_at)
            }
        };
        if replace {
            by_id.insert(id, (at, row));
        }
    }
    by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn claude_jsonl_deduplicates_streaming_messages_and_ignores_conversation_text() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let project = dir.path().join("C--work-tempo-ai");
        fs::create_dir_all(&project).unwrap();
        let file = project.join("sess-1.jsonl");
        let now = chrono::Utc::now().timestamp();
        let t1 = providers::iso(now - 120).unwrap();
        let t2 = providers::iso(now - 60).unwrap();
        let mut handle = fs::File::create(&file).unwrap();
        writeln!(
            handle,
            "{}",
            json!({"type":"user","sessionId":"sess-1","cwd":"C:\\work\\tempo-ai","gitBranch":"feat/zero","timestamp":t1,"message":{"content":"SECRET PROMPT NEVER STORED"}})
        )
        .unwrap();
        writeln!(
            handle,
            "{}",
            json!({"type":"assistant","sessionId":"sess-1","cwd":"C:\\work\\tempo-ai","gitBranch":"feat/zero","timestamp":t1,"message":{"id":"msg_1","model":"claude-sonnet-4-5-20250929","content":"secret reply","usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":500,"cache_creation_input_tokens":50}}})
        )
        .unwrap();
        writeln!(
            handle,
            "{}",
            json!({"type":"assistant","sessionId":"sess-1","cwd":"C:\\work\\tempo-ai","gitBranch":"feat/zero","timestamp":t2,"message":{"id":"msg_1","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":100,"output_tokens":80,"cache_read_input_tokens":500,"cache_creation_input_tokens":50}}})
        )
        .unwrap();
        writeln!(
            handle,
            "{}",
            json!({"type":"assistant","sessionId":"sess-1","timestamp":t2,"message":{"id":"msg_2","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":200,"output_tokens":40,"cache_read_input_tokens":1000,"cache_creation_input_tokens":0}}})
        )
        .unwrap();

        let mut cache = ClaudeLogCache::new();
        let sessions = read_claude_sessions(dir.path(), now, &mut cache);
        assert_eq!(sessions.len(), 1);
        let row = &sessions[0].1;
        assert_eq!(row["id"], "claude:sess-1");
        assert_eq!(row["projectName"], "tempo-ai");
        assert_eq!(row["gitBranch"], "feat/zero");
        assert_eq!(row["model"], "claude-sonnet-4-5-20250929");
        assert_eq!(row["inputTokens"], 300);
        assert_eq!(row["outputTokens"], 120);
        assert_eq!(row["cacheReadTokens"], 1500);
        assert_eq!(row["cacheCreationTokens"], 50);
        assert_eq!(row["cacheTokens"], 1550);
        assert_eq!(row["totalTokens"], 1970);
        assert_eq!(row["requestCount"], 2);
        assert!(!row.to_string().contains("SECRET PROMPT"));
        assert!(!row.to_string().contains("secret reply"));
    }

    #[test]
    fn codex_jsonl_extracts_cumulative_tokens_cached_input_and_rate_limits() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let nested = dir.path().join("2026").join("11").join("20");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("rollout-abc.jsonl");
        let now = chrono::Utc::now().timestamp();
        let t1 = providers::iso(now - 90).unwrap();
        let t2 = providers::iso(now - 30).unwrap();
        let mut handle = fs::File::create(&file).unwrap();
        writeln!(
            handle,
            "{}",
            json!({"timestamp":t1,"type":"session_meta","payload":{"id":"thread-42","cwd":"C:\\code\\api","timestamp":t1,"git":{"branch":"main"}}})
        )
        .unwrap();
        writeln!(
            handle,
            "{}",
            json!({"timestamp":t1,"type":"turn_context","payload":{"model":"gpt-5.3-codex","cwd":"C:\\code\\api"}})
        )
        .unwrap();
        writeln!(
            handle,
            "{}",
            json!({"timestamp":t1,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":600,"output_tokens":200,"total_tokens":1200}},"rate_limits":null}})
        )
        .unwrap();
        writeln!(
            handle,
            "{}",
            json!({"timestamp":t2,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3000,"cached_input_tokens":2000,"output_tokens":500,"total_tokens":3500}},"rate_limits":{"plan_type":"plus","primary":{"used_percent":34.5,"window_minutes":300,"resets_at":now + 3600},"secondary":{"used_percent":62.0,"window_minutes":10080,"resets_at":now + 86400}}}})
        )
        .unwrap();

        let mut cache = CodexLogCache::new();
        let result = read_codex_sessions(dir.path(), now, &mut cache);
        assert_eq!(result.sessions.len(), 1);
        let row = &result.sessions[0].1;
        assert_eq!(row["id"], "codex:thread-42");
        assert_eq!(row["projectName"], "api");
        assert_eq!(row["gitBranch"], "main");
        assert_eq!(row["model"], "gpt-5.3-codex");
        assert_eq!(row["inputTokens"], 1000);
        assert_eq!(row["cacheReadTokens"], 2000);
        assert_eq!(row["outputTokens"], 500);
        assert_eq!(row["totalTokens"], 3500);
        assert_eq!(row["requestCount"], 2);
        let obs = result.latest_observation.expect("expected rate limits fallback");
        assert_eq!(obs.plan.as_deref(), Some("plus"));
        assert_eq!(obs.quotas.len(), 2);
        assert_eq!(obs.quotas[0]["usedPercent"], 34.5);
        assert_eq!(obs.quotas[1]["usedPercent"], 62.0);
    }
}
