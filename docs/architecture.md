# Arquitetura do TempoAI

O TempoAI é um aplicativo desktop **local-first** para Windows 11 construído com **Tauri 2 + Rust** no backend nativo e **React 19 + TypeScript + Tailwind CSS v4** na interface. Não existe servidor próprio, banco remoto ou telemetria.

## Visão das camadas

| Camada | Diretório | Responsabilidade |
| --- | --- | --- |
| **Interface (Frontend)** | `src/` | Renderização das telas (`overview`, `activity`, `settings`, `quick-monitor`), formatação de datas/cotas, tabela de referência de custo equivalente de API, gráficos, mapa de calor, Share Studio e ponte tipada com o Tauri (`src/services/desktop.ts`). |
| **Núcleo desktop (Rust)** | `src-tauri/src/core/` | Agendamento e cache de atualizações (`monitor.rs`), preferências atômicas com patches parciais de UI (`preferences.rs`), histórico SQLite com migrações e retenção de 92 dias (`history.rs`), bandeja do sistema e janela flutuante (`desktop.rs`), e alertas nativos do Windows (`alerts.rs`). |
| **Provedores (Rust)** | `src-tauri/src/providers/` | Contrato compartilhado de cotas/observações (`mod.rs`), leitura OAuth somente-leitura do Claude (`claude.rs`), cliente stdio RPC do Codex App Server (`codex.rs`), leitor CLI do Antigravity (`antigravity.rs`), parser streaming incremental de arquivos `.jsonl` (`jsonl.rs`) e varredura/fontes JSON locais (`discovery.rs`). |
| **Persistência local** | `src-tauri/migrations/` | Esquema SQLite versionado via `PRAGMA user_version` para sessões agregadas, deduplicação de alertas por ciclo e últimas leituras salvas de Claude Code e Antigravity. |

## Fluxo de dados e desempenho

1. **Inicialização rápida**: `main.rs` carrega `preferences.json` e o estado de interface (`UiState`) antes de criar a bandeja e o Monitor rápido, restaurando idioma, tema, escala de fonte e posição/tamanho salvos.
2. **Coleta desacoplada da UI**: `core/monitor.rs` executa leituras em background com controle de concorrência (`AtomicBool` + `Mutex`), cache de curto prazo e backoff automático em erros transitórios (como HTTP 429 no Claude ou falhas do CLI do Antigravity).
3. **Leitura sob demanda de histórico**: a Visão geral e o Monitor rápido consomem `usage_snapshot(includeSessions: false)` sem abrir o SQLite; apenas a tela de Atividade solicita `session_history`.
4. **Sincronização entre janelas**: alterações visuais (tema, idioma, escala de fonte `85%`–`125%`, cards recolhidos) são enviadas via `UiStatePatch` parcial para evitar sobrescrita concorrente entre o painel principal e o Monitor rápido.

## Garantias de segurança e privacidade

- **Somente leitura em credenciais externas**: tokens OAuth do Claude Code permanecem em memória apenas durante a chamada HTTPS (com redirecionamentos desabilitados) e nunca são gravados, logados ou enviados à WebView.
- **Descarte de conteúdo de conversas**: os leitores `.jsonl` impõem limites rígidos de arquivos, bytes e linhas, extraindo exclusivamente contadores de tokens, modelo, timestamps, nome base da pasta do projeto e branch Git.
- **Comandos com escopo de janela e guarda de caminhos locais**: comandos IPC sensíveis validam `window.label()` (`main` / `quick`) e caminhos rejeitam unidades de rede/UNC (`is_local_path`).

## Referências técnicas

- [Tauri 2 com Vite](https://v2.tauri.app/start/frontend/vite/) e [pré-requisitos nativos do Windows](https://v2.tauri.app/start/prerequisites/#windows)
- [Tailwind CSS v4 com Vite](https://tailwindcss.com/docs/installation/using-vite) e [shadcn/ui](https://ui.shadcn.com/docs/installation/vite)
