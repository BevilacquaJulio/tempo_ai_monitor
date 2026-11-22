use crate::{
    core::preferences::{is_local_path, resolve_local_dir, resolve_local_file},
    providers::{self, Observation, Sessions, HISTORY_SECONDS},
};
use chrono::DateTime;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const MAX_JSONL_FILES: usize = 2_500;
const MAX_SCANNED_ENTRIES: usize = 10_000;
const MAX_FILES_PARSED_PER_ROUND: usize = 250;
const MAX_PARSE_BYTES_PER_ROUND: u64 = 32 * 1024 * 1024;
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TOKEN_VALUE: u64 = 9_007_199_254_740_991;

#[derive(Clone, Default)]
pub struct ParsedClaudeFile {
    cutoff: i64,
    byte_offset: u64,
    line_count: usize,
    session_id: Option<String>,
    project_name: Option<String>,
    git_branch: Option<String>,
    model: Option<String>,
    started_at: Option<i64>,
    last_activity_at: Option<i64>,
    requests: HashMap<String, (u64, u64, u64, u64)>,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    request_count: u64,
    has_usage: bool,
}

impl ParsedClaudeFile {
    fn recompute_totals(&mut self) {
        self.input_tokens = 0;
        self.output_tokens = 0;
        self.cache_read_tokens = 0;
        self.cache_creation_tokens = 0;
        self.request_count = self.requests.len() as u64;
        self.has_usage = !self.requests.is_empty();
        for &(input, output, cache_read, cache_create) in self.requests.values() {
            self.input_tokens = self.input_tokens.saturating_add(input);
            self.output_tokens = self.output_tokens.saturating_add(output);
            self.cache_read_tokens = self.cache_read_tokens.saturating_add(cache_read);
            self.cache_creation_tokens = self.cache_creation_tokens.saturating_add(cache_create);
        }
    }
}

#[derive(Clone, Default)]
pub struct ParsedCodexFile {
    cutoff: i64,
    byte_offset: u64,
    line_count: usize,
    session_id: Option<String>,
    project_name: Option<String>,
    git_branch: Option<String>,
    model: Option<String>,
    started_at: Option<i64>,
    last_activity_at: Option<i64>,
    base_input: u64,
    base_cache_read: u64,
    base_output: u64,
    base_total: u64,
    base_reqs: u64,
    max_cumulative_total: u64,
    cumulative_found: bool,
    cum_input: u64,
    cum_cache_read: u64,
    cum_output: u64,
    cum_total: u64,
    cum_reqs: u64,
    fallback_input: u64,
    fallback_cache_read: u64,
    fallback_output: u64,
    fallback_total: u64,
    fallback_reqs: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    total_tokens: u64,
    request_count: u64,
    has_usage: bool,
    rate_limits: Option<(i64, Option<String>, Value)>,
}

impl ParsedCodexFile {
    fn recompute_totals(&mut self) {
        if self.cumulative_found && self.cum_total > self.base_total {
            self.input_tokens = self.cum_input.saturating_sub(self.base_input);
            self.cache_read_tokens = self.cum_cache_read.saturating_sub(self.base_cache_read);
            self.output_tokens = self.cum_output.saturating_sub(self.base_output);
            self.total_tokens = self.cum_total.saturating_sub(self.base_total);
            self.request_count = self.cum_reqs.saturating_sub(self.base_reqs).max(1);
            self.has_usage = true;
        } else if !self.cumulative_found && self.fallback_total > 0 {
            self.input_tokens = self.fallback_input;
            self.cache_read_tokens = self.fallback_cache_read;
            self.output_tokens = self.fallback_output;
            self.total_tokens = self.fallback_total;
            self.request_count = self.fallback_reqs;
            self.has_usage = true;
        } else {
            self.input_tokens = 0;
            self.cache_read_tokens = 0;
            self.output_tokens = 0;
            self.total_tokens = 0;
            self.request_count = 0;
            self.has_usage = false;
        }
    }
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
    let Some(resolved_root) = resolve_local_dir(root) else {
        return false;
    };
    let mut stack = vec![(resolved_root.clone(), 0usize)];
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
            if kind.is_file()
                && path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
                && resolve_local_file(&path).is_some_and(|resolved| resolved.starts_with(&resolved_root))
            {
                return true;
            }
            if kind.is_dir() && depth < max_depth {
                stack.push((path, depth + 1));
            }
        }
    }
    false
}

fn collect_jsonl_files(resolved_root: &Path, max_depth: usize, cutoff: SystemTime) -> Vec<(SystemTime, PathBuf, u64)> {
    let mut files = Vec::new();
    let mut stack = vec![(resolved_root.to_path_buf(), 0usize)];
    let mut visited_dirs = 0usize;
    let mut scanned_entries = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        if visited_dirs >= 2_000 || scanned_entries >= MAX_SCANNED_ENTRIES {
            break;
        }
        visited_dirs += 1;
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            scanned_entries += 1;
            if scanned_entries > MAX_SCANNED_ENTRIES {
                break;
            }
            // DirEntry::file_type does not follow symlinks or directory junctions.
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

/// Streams lines from `start_offset` bounded to MAX_LINE_BYTES without stopping on blank lines.
/// Returns the byte offset after the last newline-terminated line and the line count for incremental resumes.
fn for_each_line_from<F>(path: &Path, start_offset: u64, start_line: usize, mut visit: F) -> std::io::Result<(u64, usize)>
where
    F: FnMut(usize, &[u8]),
{
    let mut file = fs::File::open(path)?;
    if start_offset > 0 {
        file.seek(SeekFrom::Start(start_offset))?;
    }
    let mut reader = BufReader::with_capacity(32 * 1024, file);
    let mut buffer = Vec::with_capacity(8 * 1024);
    let mut line_index = start_line;
    let mut consumed = start_offset;
    let mut safe_offset = start_offset;
    let mut safe_line_count = start_line;
    loop {
        buffer.clear();
        let mut oversized = false;
        let mut saw_any = false;
        let mut ended_with_newline = false;
        loop {
            let chunk = reader.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            saw_any = true;
            if let Some(pos) = chunk.iter().position(|&b| b == b'\n') {
                if !oversized && buffer.len() + pos <= MAX_LINE_BYTES {
                    buffer.extend_from_slice(&chunk[..pos]);
                } else {
                    oversized = true;
                }
                let advance = pos + 1;
                reader.consume(advance);
                consumed = consumed.saturating_add(advance as u64);
                ended_with_newline = true;
                break;
            }
            let len = chunk.len();
            if !oversized && buffer.len() + len <= MAX_LINE_BYTES {
                buffer.extend_from_slice(chunk);
            } else {
                oversized = true;
            }
            reader.consume(len);
            consumed = consumed.saturating_add(len as u64);
        }
        if !saw_any {
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
        if ended_with_newline {
            safe_offset = consumed;
            safe_line_count = line_index;
        }
    }
    Ok((safe_offset, safe_line_count))
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

fn parse_claude_into(parsed: &mut ParsedClaudeFile, path: &Path, cutoff: i64) {
    // Claude Code can emit multiple streaming chunks sharing one message.id; keep the latest/highest output per id.
    let result = for_each_line_from(path, parsed.byte_offset, parsed.line_count, |index, line| {
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
        let message = &entry["message"];
        if let Some(model) = safe_text(&message["model"], 120).filter(|m| !m.starts_with('<')) {
            parsed.model = Some(model);
        }
        let ts = parse_timestamp(&entry["timestamp"]);
        // After a manual history clear, ignore activity and tokens recorded at or before the cutoff.
        if cutoff > 0 && ts.is_none_or(|t| t <= cutoff) {
            return;
        }
        if let Some(ts) = ts {
            parsed.started_at = Some(parsed.started_at.map_or(ts, |prev| prev.min(ts)));
            parsed.last_activity_at = Some(parsed.last_activity_at.map_or(ts, |prev| prev.max(ts)));
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
                let replace = parsed.requests
                    .get(&key)
                    .is_none_or(|prev| output >= prev.1 && (input + output + cache_read + cache_create) >= (prev.0 + prev.1 + prev.2 + prev.3));
                if replace {
                    parsed.requests.insert(key, (input, output, cache_read, cache_create));
                }
            }
        }
    });
    if let Ok((offset, line_count)) = result {
        parsed.byte_offset = offset;
        parsed.line_count = line_count;
    }
    parsed.cutoff = cutoff;
    parsed.recompute_totals();
}

pub fn read_claude_sessions(root: &Path, now: i64, clear_cutoff: i64, cache: &mut ClaudeLogCache) -> Sessions {
    let Some(resolved_root) = resolve_local_dir(root) else {
        return Vec::new();
    };
    let cutoff_epoch = now.saturating_sub(HISTORY_SECONDS).max(0);
    let cutoff_time = SystemTime::UNIX_EPOCH + Duration::from_secs(cutoff_epoch as u64);
    let candidates = collect_jsonl_files(&resolved_root, 4, cutoff_time);
    let mut next_cache = ClaudeLogCache::new();
    let mut merged: HashMap<String, ParsedClaudeFile> = HashMap::new();
    let mut files_parsed = 0usize;
    let mut bytes_parsed = 0u64;

    for (modified, file, length) in candidates {
        let Some(resolved_file) = resolve_local_file(&file).filter(|p| p.starts_with(&resolved_root)) else {
            continue;
        };
        let cached = cache.remove(&resolved_file);
        let parsed = match cached {
            Some((time, size, existing)) if time == modified && size == length && existing.cutoff == clear_cutoff => existing,
            Some((time, size, mut existing)) if existing.cutoff == clear_cutoff && length >= existing.byte_offset => {
                let delta = length.saturating_sub(existing.byte_offset);
                if files_parsed < MAX_FILES_PARSED_PER_ROUND && bytes_parsed.saturating_add(delta) <= MAX_PARSE_BYTES_PER_ROUND {
                    parse_claude_into(&mut existing, &resolved_file, clear_cutoff);
                    files_parsed += 1;
                    bytes_parsed = bytes_parsed.saturating_add(delta);
                    existing
                } else {
                    // Keep the previous parsed snapshot for this round and catch up on the next tick.
                    next_cache.insert(resolved_file, (time, size, existing));
                    continue;
                }
            }
            _ => {
                if files_parsed >= MAX_FILES_PARSED_PER_ROUND || (files_parsed > 0 && bytes_parsed.saturating_add(length) > MAX_PARSE_BYTES_PER_ROUND) {
                    continue;
                }
                let stem = resolved_file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty() && s.len() <= 120 && !s.chars().any(char::is_control))
                    .map(str::to_owned);
                let mut fresh = ParsedClaudeFile {
                    cutoff: clear_cutoff,
                    session_id: stem,
                    ..ParsedClaudeFile::default()
                };
                parse_claude_into(&mut fresh, &resolved_file, clear_cutoff);
                files_parsed += 1;
                bytes_parsed = bytes_parsed.saturating_add(length);
                fresh
            }
        };
        next_cache.insert(resolved_file, (modified, length, parsed));
    }

    for (_, _, entry) in next_cache.values() {
        let Some(id) = entry.session_id.clone() else { continue };
        let Some(last) = entry.last_activity_at.filter(|ts| *ts <= now + 300 && *ts >= cutoff_epoch && *ts > clear_cutoff) else { continue };
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

fn parse_codex_into(parsed: &mut ParsedCodexFile, path: &Path, cutoff: i64) {
    let result = for_each_line_from(path, parsed.byte_offset, parsed.line_count, |_, line| {
        let interesting = contains_bytes(line, b"session_meta")
            || contains_bytes(line, b"turn_context")
            || contains_bytes(line, b"token_count");
        if !interesting {
            return;
        }
        let Ok(entry) = serde_json::from_slice::<Value>(line) else { return };
        let ts = parse_timestamp(&entry["timestamp"]);
        let after_cutoff = cutoff <= 0 || ts.is_some_and(|t| t > cutoff);
        if after_cutoff {
            if let Some(ts) = ts {
                parsed.started_at = Some(parsed.started_at.map_or(ts, |prev| prev.min(ts)));
                parsed.last_activity_at = Some(parsed.last_activity_at.map_or(ts, |prev| prev.max(ts)));
            }
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
                if after_cutoff {
                    if let Some(start) = parse_timestamp(&payload["timestamp"]) {
                        parsed.started_at = Some(parsed.started_at.map_or(start, |prev| prev.min(start)));
                    }
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
                    // OpenAI input_tokens includes cached_input_tokens; separate them so pricing matches Claude's schema.
                    let net_input = raw_input.saturating_sub(cache_read);
                    if total > parsed.max_cumulative_total {
                        parsed.cumulative_found = true;
                        parsed.max_cumulative_total = total;
                        parsed.cum_reqs = parsed.cum_reqs.saturating_add(1);
                        parsed.cum_input = net_input;
                        parsed.cum_cache_read = cache_read;
                        parsed.cum_output = output;
                        parsed.cum_total = total;
                        if !after_cutoff {
                            parsed.base_input = net_input;
                            parsed.base_cache_read = cache_read;
                            parsed.base_output = output;
                            parsed.base_total = total;
                            parsed.base_reqs = parsed.cum_reqs;
                        }
                    }
                } else if !parsed.cumulative_found && after_cutoff && info["last_token_usage"].is_object() {
                    let usage = &info["last_token_usage"];
                    let raw_input = capped_u64(&usage["input_tokens"]);
                    let cache_read = capped_u64(&usage["cached_input_tokens"])
                        .max(capped_u64(&usage["cache_read_input_tokens"]));
                    let output = capped_u64(&usage["output_tokens"]);
                    let total = capped_u64(&usage["total_tokens"]).max(raw_input.saturating_add(output));
                    if total > 0 {
                        parsed.fallback_input = parsed.fallback_input.saturating_add(raw_input.saturating_sub(cache_read));
                        parsed.fallback_cache_read = parsed.fallback_cache_read.saturating_add(cache_read);
                        parsed.fallback_output = parsed.fallback_output.saturating_add(output);
                        parsed.fallback_total = parsed.fallback_total.saturating_add(total);
                        parsed.fallback_reqs = parsed.fallback_reqs.saturating_add(1);
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
    if let Ok((offset, line_count)) = result {
        parsed.byte_offset = offset;
        parsed.line_count = line_count;
    }
    parsed.cutoff = cutoff;
    parsed.recompute_totals();
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

pub fn read_codex_sessions(root: &Path, now: i64, clear_cutoff: i64, cache: &mut CodexLogCache) -> CodexLogResult {
    let Some(resolved_root) = resolve_local_dir(root) else {
        return CodexLogResult { sessions: Vec::new(), latest_observation: None };
    };
    let cutoff_epoch = now.saturating_sub(HISTORY_SECONDS).max(0);
    let cutoff_time = SystemTime::UNIX_EPOCH + Duration::from_secs(cutoff_epoch as u64);
    let candidates = collect_jsonl_files(&resolved_root, 5, cutoff_time);
    let mut next_cache = CodexLogCache::new();
    let mut sessions = Vec::new();
    let mut latest_limits: Option<(i64, Option<String>, Value)> = None;
    let mut files_parsed = 0usize;
    let mut bytes_parsed = 0u64;

    for (modified, file, length) in candidates {
        let Some(resolved_file) = resolve_local_file(&file).filter(|p| p.starts_with(&resolved_root)) else {
            continue;
        };
        let cached = cache.remove(&resolved_file);
        let parsed = match cached {
            Some((time, size, existing)) if time == modified && size == length && existing.cutoff == clear_cutoff => existing,
            Some((time, size, mut existing)) if existing.cutoff == clear_cutoff && length >= existing.byte_offset => {
                let delta = length.saturating_sub(existing.byte_offset);
                if files_parsed < MAX_FILES_PARSED_PER_ROUND && bytes_parsed.saturating_add(delta) <= MAX_PARSE_BYTES_PER_ROUND {
                    parse_codex_into(&mut existing, &resolved_file, clear_cutoff);
                    files_parsed += 1;
                    bytes_parsed = bytes_parsed.saturating_add(delta);
                    existing
                } else {
                    next_cache.insert(resolved_file, (time, size, existing));
                    continue;
                }
            }
            _ => {
                if files_parsed >= MAX_FILES_PARSED_PER_ROUND || (files_parsed > 0 && bytes_parsed.saturating_add(length) > MAX_PARSE_BYTES_PER_ROUND) {
                    continue;
                }
                let stem = resolved_file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty() && s.len() <= 120 && !s.chars().any(char::is_control))
                    .map(str::to_owned);
                let mut fresh = ParsedCodexFile {
                    cutoff: clear_cutoff,
                    session_id: stem,
                    ..ParsedCodexFile::default()
                };
                parse_codex_into(&mut fresh, &resolved_file, clear_cutoff);
                files_parsed += 1;
                bytes_parsed = bytes_parsed.saturating_add(length);
                fresh
            }
        };
        next_cache.insert(resolved_file, (modified, length, parsed));
    }

    for (_, _, entry) in next_cache.values() {
        if let Some((at, plan, limits)) = &entry.rate_limits {
            if *at <= now + 300 && *at >= cutoff_epoch && latest_limits.as_ref().is_none_or(|(prev, _, _)| at > prev) {
                latest_limits = Some((*at, plan.clone(), limits.clone()));
            }
        }
        let Some(id) = entry.session_id.as_deref() else { continue };
        let Some(last) = entry.last_activity_at.filter(|ts| *ts <= now + 300 && *ts >= cutoff_epoch && *ts > clear_cutoff) else { continue };
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
    fn claude_jsonl_deduplicates_streaming_messages_ignores_blank_lines_and_discards_text() {
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
        // Blank LF and CRLF lines mid-file must not stop parsing subsequent records.
        handle.write_all(b"\n\r\n").unwrap();
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
        handle.write_all(b"\n").unwrap();
        writeln!(
            handle,
            "{}",
            json!({"type":"assistant","sessionId":"sess-1","timestamp":t2,"message":{"id":"msg_2","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":200,"output_tokens":40,"cache_read_input_tokens":1000,"cache_creation_input_tokens":0}}})
        )
        .unwrap();

        let mut cache = ClaudeLogCache::new();
        let sessions = read_claude_sessions(dir.path(), now, 0, &mut cache);
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

        // Incremental tail append adds only new records without re-parsing from byte 0.
        let t3 = providers::iso(now - 10).unwrap();
        let mut append = fs::OpenOptions::new().append(true).open(&file).unwrap();
        writeln!(
            append,
            "{}",
            json!({"type":"assistant","sessionId":"sess-1","timestamp":t3,"message":{"id":"msg_3","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":50,"output_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}})
        )
        .unwrap();
        drop(append);
        let updated = read_claude_sessions(dir.path(), now, 0, &mut cache);
        assert_eq!(updated[0].1["totalTokens"], 2030);
        assert_eq!(updated[0].1["requestCount"], 3);
    }

    #[test]
    fn clear_cutoff_excludes_pre_clear_tokens_even_when_old_sessions_resume() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let now = chrono::Utc::now().timestamp();
        let cutoff = now - 60;
        let before = providers::iso(now - 120).unwrap();
        let after = providers::iso(now - 20).unwrap();

        let claude_dir = dir.path().join("claude");
        fs::create_dir_all(&claude_dir).unwrap();
        let claude_file = claude_dir.join("sess-resumed.jsonl");
        let mut cf = fs::File::create(&claude_file).unwrap();
        writeln!(cf, "{}", json!({"type":"assistant","sessionId":"sess-resumed","cwd":"C:\\work\\app","timestamp":before,"message":{"id":"old_1","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":5000,"output_tokens":1000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}})).unwrap();
        writeln!(cf, "{}", json!({"type":"assistant","sessionId":"sess-resumed","cwd":"C:\\work\\app","timestamp":after,"message":{"id":"new_1","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":100,"output_tokens":50,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}})).unwrap();

        let mut c_cache = ClaudeLogCache::new();
        let claude_sessions = read_claude_sessions(&claude_dir, now, cutoff, &mut c_cache);
        assert_eq!(claude_sessions.len(), 1);
        assert_eq!(claude_sessions[0].1["inputTokens"], 100);
        assert_eq!(claude_sessions[0].1["outputTokens"], 50);
        assert_eq!(claude_sessions[0].1["totalTokens"], 150);
        assert_eq!(claude_sessions[0].1["requestCount"], 1);

        let codex_dir = dir.path().join("codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let codex_file = codex_dir.join("rollout-resumed.jsonl");
        let mut xf = fs::File::create(&codex_file).unwrap();
        writeln!(xf, "{}", json!({"timestamp":before,"type":"session_meta","payload":{"id":"thread-resumed","cwd":"C:\\work\\api","timestamp":before,"git":{"branch":"main"}}})).unwrap();
        writeln!(xf, "{}", json!({"timestamp":before,"type":"turn_context","payload":{"model":"gpt-5.3-codex"}})).unwrap();
        writeln!(xf, "{}", json!({"timestamp":before,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5000,"cached_input_tokens":2000,"output_tokens":500,"total_tokens":5500}}}})).unwrap();
        writeln!(xf, "{}", json!({"timestamp":after,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":5400,"cached_input_tokens":2200,"output_tokens":600,"total_tokens":6000}}}})).unwrap();

        let mut x_cache = CodexLogCache::new();
        let codex_res = read_codex_sessions(&codex_dir, now, cutoff, &mut x_cache);
        assert_eq!(codex_res.sessions.len(), 1);
        let row = &codex_res.sessions[0].1;
        assert_eq!(row["inputTokens"], 200);
        assert_eq!(row["cacheReadTokens"], 200);
        assert_eq!(row["outputTokens"], 100);
        assert_eq!(row["totalTokens"], 500);
        assert_eq!(row["requestCount"], 1);
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
        handle.write_all(b"\n").unwrap();
        writeln!(
            handle,
            "{}",
            json!({"timestamp":t2,"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":3000,"cached_input_tokens":2000,"output_tokens":500,"total_tokens":3500}},"rate_limits":{"plan_type":"plus","primary":{"used_percent":34.5,"window_minutes":300,"resets_at":now + 3600},"secondary":{"used_percent":62.0,"window_minutes":10080,"resets_at":now + 86400}}}})
        )
        .unwrap();

        let mut cache = CodexLogCache::new();
        let result = read_codex_sessions(dir.path(), now, 0, &mut cache);
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

    #[test]
    fn large_history_bounds_work_per_round_and_skips_directory_junctions() {
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let root = dir.path().join("projects");
        let outside = dir.path().join("outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let now = chrono::Utc::now().timestamp();
        let ts = providers::iso(now - 30).unwrap();

        // Create 260 synthetic files (> MAX_FILES_PARSED_PER_ROUND) to verify bounded work and catch-up.
        for i in 0..260 {
            let f = root.join(format!("s-{i:03}.jsonl"));
            fs::write(
                &f,
                format!("{}\n", json!({"type":"assistant","sessionId":format!("s-{i:03}"),"timestamp":ts,"message":{"id":"m","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":10,"output_tokens":5}}})),
            )
            .unwrap();
        }
        fs::write(
            outside.join("escaped.jsonl"),
            format!("{}\n", json!({"type":"assistant","sessionId":"escaped","timestamp":ts,"message":{"id":"m","model":"claude-sonnet-4-5-20250929","usage":{"input_tokens":999,"output_tokens":1}}})),
        )
        .unwrap();

        #[cfg(windows)]
        let junction = root.join("linked-outside");
        #[cfg(windows)]
        let junction_created = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J", junction.to_str().unwrap(), outside.to_str().unwrap()])
            .output()
            .is_ok_and(|out| out.status.success());

        let mut cache = ClaudeLogCache::new();
        let round1 = read_claude_sessions(&root, now, 0, &mut cache);
        assert_eq!(round1.len(), MAX_FILES_PARSED_PER_ROUND);
        let round2 = read_claude_sessions(&root, now, 0, &mut cache);
        assert_eq!(round2.len(), 260);
        assert!(!round2.iter().any(|(_, row)| row["id"] == "claude:escaped"));

        #[cfg(windows)]
        if junction_created {
            let _ = fs::remove_dir(&junction);
        }
    }
}
