# Fontes e limites das integrações

No primeiro acesso ao app do Windows, a **Visão geral** exibe a varredura local com radar (**Escanear minhas IAs**). Você também pode abrir **Visão geral → Adicionar ferramenta** (*Overview → Add AI tool*) a qualquer momento. A busca verifica o PATH, credenciais locais e pastas de sessões sem executar os agentes nem alterar arquivos de configuração das suas CLIs.

Só os provedores que você adicionar aparecem na Visão geral e no Monitor rápido. Para tirar uma ferramenta da tela, use **⋯ → Remover do TempoAI** no cartão dela ou **Remover** em Configurações.

## Claude Code (Zero-config)

Em **Escanear minhas IAs** (ou **Adicionar ferramenta**), clique em **Adicionar** em **Claude Code** — nenhuma edição em `~/.claude/settings.json` é necessária.

- **Cotas ao vivo (5 horas e 7 dias)**: o TempoAI lê (somente leitura) o token OAuth salvo pelo Claude Code em `%USERPROFILE%\.claude\.credentials.json` (ou `CLAUDE_CONFIG_DIR`) e consulta `https://api.anthropic.com/api/oauth/usage`.
  - O token permanece apenas em memória durante a requisição HTTPS (sem seguir redirecionamentos) e nunca é gravado, logado ou enviado à interface.
  - O TempoAI **nunca renova nem sobrescreve** `.credentials.json`, evitando desconectar o Claude Code. Se o token expirar, basta abrir o Claude Code uma vez para que ele mesmo renove.
  - Leituras saudáveis são cacheadas por 2 minutos; falhas transitórias de rede tentam novamente a partir de 30 segundos, renovações de `.credentials.json` reconectam automaticamente e respostas HTTP 429 respeitam `Retry-After` (ou aguardam 3 minutos) — além de reconexão imediata pelo botão **Tentar conectar novamente** / **Consultar uso do Claude**.
- **Histórico de sessões, tokens e cache**: o TempoAI lê em streaming (somente leitura, com cache incremental por arquivo) os metadados numéricos de `%USERPROFILE%\.claude\projects\**\*.jsonl`, extraindo exclusivamente `sessionId`, nome da pasta do projeto (`cwd` basename), `gitBranch`, `model`, timestamps e contadores de `usage` (`input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`), deduplicando chunks de streaming por `message.id`. **Nenhum texto de prompt, resposta ou ferramenta é lido para a interface ou salvo no SQLite.**

## OpenAI Codex (Zero-config)

Clique em **Adicionar** em **OpenAI Codex** após a varredura.

- **Histórico de sessões, tokens e cache**: o TempoAI lê em streaming (somente leitura) `%USERPROFILE%\.codex\sessions\**\rollout-*.jsonl` (ou `CODEX_HOME/sessions`), extraindo metadados de `session_meta`, `turn_context` e eventos `token_count` (`input_tokens`, `cached_input_tokens`, `output_tokens`, `total_tokens`, modelo, pasta do projeto e `git.branch`), sem guardar o conteúdo das conversas.
- **Cotas ao vivo e fallback**:
  - Quando o executável nativo `codex.exe` é detectado automaticamente no sistema, o TempoAI consulta o [Codex App Server](https://learn.chatgpt.com/docs/app-server) via stdio (`account/read`, `account/rateLimits/read`, `account/usage/read`, `thread/list`) sem executar prompts.
  - Quando o Codex é usado sem `.exe` nativo (por exemplo via `codex.cmd` do npm) ou o App Server falha temporariamente, o TempoAI usa o snapshot mais recente de `rate_limits` gravado nos logs locais `.jsonl`.
- O TempoAI não envia nem armazena credenciais do Codex; a autenticação continua sob controle do próprio Codex.

## Google Antigravity CLI

Com o Antigravity CLI `1.1.11` ou mais recente autenticado com sua conta Google, clique em **Adicionar** em **Google Antigravity**.

- O TempoAI executa o comando oficial somente-leitura `agy -p /usage --output-format json` (verificando a versão antes para garantir que nenhum prompt seja enviado ao modelo).
- As leituras são atualizadas periodicamente (a cada 2 minutos) e, em caso de erro transitório, o app aplica backoff de 5 minutos antes de tentar novamente automaticamente (ou imediatamente ao clicar em **Consultar uso do Antigravity**).

## Outros provedores

Escolha qualquer outro agente detectado (Gemini CLI, Copilot CLI, Cursor, OpenCode, Windsurf, Aider, Amp) ou **Outro provedor** e informe um nome e o caminho absoluto de um arquivo JSON local:

```json
{
  "observedAt": "2026-11-20T12:00:00-03:00",
  "fiveHour": { "usedPercent": 42 },
  "weekly": { "usedPercent": 18, "resetAt": "2026-11-27T10:00:00-03:00" }
}
```

## Armazenamento, privacidade e alertas

- O histórico agregado (até 92 dias para suportar visualizações de 7, 30 e 90 dias) fica em `history.sqlite`, com limpeza automática de registros expirados e opção manual de apagar em **Configurações → Seus dados**.
- As preferências ficam em `preferences.json` com escrita atômica e backup automático em caso de corrupção.
- Os custos exibidos na aba **Atividade** são **estimativas locais equivalentes a API (USD)** calculadas por tabela de referência por modelo — são sempre sinalizados como separados da fatura da sua assinatura.
- O **Share Studio** gera SVG e Markdown 100% localmente, com ocultação de nomes de projetos e branches ativada por padrão.
- Alertas nativos do Windows (opt-in em Configurações) funcionam para Claude Code, OpenAI Codex e Google Antigravity, deduplicados por provedor, janela, renovação e limiar.
