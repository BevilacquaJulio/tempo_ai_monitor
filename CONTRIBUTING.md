# Contributing to TempoAI

Thank you for your interest in contributing to **TempoAI** (`BevilacquaJulio/tempo_ai_monitor`)!

## Core Principles

TempoAI is built around three non-negotiable guarantees:

1. **Local-first & privacy-preserving**: Never send user prompts, code, responses, or credentials to third-party servers. When parsing local `.jsonl` session logs (`~/.claude/projects`, `~/.codex/sessions`), extract only numeric usage metadata, timestamps, model names, folder basenames, and git branch names — discard conversation content immediately.
2. **Never fabricate numbers**: Missing, stale, or unrecognized values must stay `null` / `"unavailable"` / `"stale"`, never coerced to `0%` or `$0`.
3. **Read-only credentials**: Never rotate or overwrite OAuth credentials owned by other CLIs (such as Claude Code's `.credentials.json`).

## Prerequisites (Windows 11)

- Node.js `22.12+` or `24+` with `npm`
- Rust stable (MSVC target)
- Visual Studio Build Tools with **Desktop development with C++** and Windows SDK
- WebView2 Runtime ([Tauri Windows prerequisites](https://v2.tauri.app/start/prerequisites/#windows))

## Development Commands

```powershell
npm ci
npm run tauri -- dev     # Desktop app with hot reload
npm run dev              # Browser UI with demo fixtures (http://127.0.0.1:1420)
```

## Verification Before Opening a PR

Run all checks locally:

```powershell
npm run typecheck
npm run lint
npm test
npm run test:rust
npm run test:e2e
```

## Project Layout

- `src/` — React 19, TypeScript, Tailwind CSS v4, TanStack Query, Radix UI, Recharts
- `src-tauri/src/` — Rust / Tauri 2 desktop backend:
  - `core/` — Refresh monitor (`monitor.rs`), atomic preferences & UI patches (`preferences.rs`), SQLite history (`history.rs`), tray & Quick Monitor (`desktop.rs`), and Windows alerts (`alerts.rs`)
  - `providers/` — Shared quota helpers (`mod.rs`), zero-config JSONL readers (`jsonl.rs`), Claude OAuth reader (`claude.rs`), Codex App Server RPC (`codex.rs`), Antigravity CLI reader (`antigravity.rs`), and local discovery/custom JSON sources (`discovery.rs`)
- `e2e/` — Playwright end-to-end tests
- `scripts/` — Windows release packager (`build-release.ps1`) and Rust test runner (`test-rust.ps1`)
- `docs/` — Architecture, integrations, and Windows validation checklists

## Localization

User-facing strings are written in English (`en-US`) in components and translated to Brazilian Portuguese (`pt-BR`) in `src/lib/pt.ts`. Please update `src/lib/pt.ts` whenever you introduce new copy.
