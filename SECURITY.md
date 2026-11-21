# Security Policy

## Local-First & Privacy Guarantees

TempoAI is designed from the ground up to run locally on your Windows machine with minimal permissions:

- **No telemetry or remote backend**: TempoAI has no server of its own and never uploads prompts, source code, responses, or usage history.
- **Read-only credential handling**: When checking Claude Code quotas, the local OAuth access token is read into memory strictly for that single HTTPS request (with redirects disabled), immediately dropped, never logged, never written to disk, and never exposed to the WebView. TempoAI never refreshes or overwrites credentials owned by other CLIs.
- **Metadata-only log parsing**: Local `.jsonl` readers stream files with strict byte/line/file limits and extract only numeric token counters, model names, timestamps, project folder basenames, and git branch names — conversation content is discarded immediately.
- **Window-scoped IPC & local-path guards**: Sensitive Tauri commands verify the calling window label, and file readers reject UNC/network drives, non-local paths, and oversized payloads.

## Reporting a Vulnerability

If you discover a security or privacy issue, please report it privately using [GitHub Security Advisories](https://github.com/BevilacquaJulio/tempo_ai_monitor/security/advisories/new) instead of opening a public issue.

Include a clear description of the issue, reproduction steps, and affected version so it can be investigated and fixed promptly.
