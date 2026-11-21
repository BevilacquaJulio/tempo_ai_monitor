# TempoAI

> All your AI. One place.

Aplicativo desktop open-source para **Windows 11** (Tauri 2 + Rust + React) que monitora, **sem configuração manual (zero-config)**, quanto você ainda pode usar das suas ferramentas de IA (**Claude Code**, **OpenAI Codex**, **Google Antigravity** e fontes JSON locais) e analisa seu consumo de tokens, eficiência de cache e custo equivalente de API.

Tudo roda localmente no seu computador: **o TempoAI nunca guarda conversas, prompts, respostas nem senhas.**

## Destaques

- **Zero-config no primeiro acesso**: varredura local com animação de radar na tela inicial que identifica suas ferramentas instaladas (Claude Code, OpenAI Codex, Google Antigravity e outros agentes) e permite conectar individualmente ou todas em um clique.
- **Leitura direta somente-leitura**:
  - **Claude Code**: cotas de 5 horas e 7 dias via credencial local (somente leitura, sem rotacionar token) + histórico granular de tokens, cache, modelo, projeto e branch direto de `~/.claude/projects/**/*.jsonl`.
  - **OpenAI Codex**: cotas ao vivo via `codex.exe app-server` (com fallback automático nos eventos `rate_limits` de `~/.codex/sessions/**/*.jsonl`) + tokens, cache, modelo, projeto e branch por sessão.
  - **Google Antigravity**: leitura direta de cotas por modelo via `agy` CLI (`>= 1.1.11`) com reconexão automática após backoff.
  - **Outros provedores**: até 20 fontes JSON locais customizadas.
- **Atividade & Analytics**:
  - Tokens detalhados por sessão (`input`, `output`, `cache read`, `cache creation`), número de requisições e eficiência de **Prompt Cache** (com estimativa de economia em USD).
  - **Custo equivalente em API (USD)** claramente separado da fatura da assinatura.
  - **Mapa de calor de atividade (30 / 90 dias)**, gráficos diários (7 / 30 / 90 dias), filtros por provedor, modelo, projeto e **branch**, e detalhamento por projeto e modelo.
  - **Share Studio**: gera cards SVG determinísticos e resumos Markdown locais com modo privacidade (oculta nomes de projetos e branches por padrão).
- **Feito para o desktop Windows 11**: ícone na bandeja, **Monitor rápido** flutuante (`Ctrl+Alt+W`), notificações nativas por limiar, modo claro/escuro/sistema, ajuste de escala de fonte (`85%`–`125%`), `pt-BR` e `en-US`, e recomendação acionável de qual ferramenta ainda tem folga.

## Como gerar os executáveis

Rode `npm run release`. Ao terminar, os arquivos para usar ou compartilhar ficam em **`release/`** (pasta ignorada no Git):

| Arquivo | Quando usar |
| --- | --- |
| `TempoAI-<versão>-setup.exe` | Instala o TempoAI com atalho no menu Iniciar. Recomendado. |
| `TempoAI-<versão>-portable.exe` | Roda sem instalar, de qualquer pasta. |

Ignore os executáveis em `src-tauri/target/`: essa pasta é apenas o cache do compilador.

## Desenvolvimento

Pré-requisitos: Windows 11, Node.js `22.12+` ou `24+`, Rust stable (MSVC), Visual Studio Build Tools com **Desktop development with C++** e WebView2. Veja os [pré-requisitos do Tauri](https://v2.tauri.app/start/prerequisites/#windows).

```powershell
git clone https://github.com/BevilacquaJulio/tempo_ai_monitor.git
cd tempo_ai_monitor
npm ci
npm run tauri -- dev
```

| Comando | O que faz |
| --- | --- |
| `npm run tauri -- dev` | Abre o app de desktop com recarregamento automático. |
| `npm run dev` | Só a interface, no navegador (`http://127.0.0.1:1420`), com dados de exemplo. |
| `npm run lint` / `npm run typecheck` | Verificações do ESLint e do TypeScript. |
| `npm test` | Testes unitários e de integração do frontend (Vitest). |
| `npm run test:rust` | Testes do backend em Rust (`cargo test --locked`). |
| `npm run test:e2e` | Gera a interface e roda os testes Playwright no Microsoft Edge. |
| `npm run release` | Gera o instalador NSIS e a versão portátil em `release/`. |

## Estrutura do projeto

```text
src/                       Interface (React 19, TypeScript, Tailwind CSS v4)
  app/                     Shell, rotas e testes de integração
  components/              Componentes compartilhados (cards, medidores, botões, estados)
  features/                Telas: overview, activity, settings e quick-monitor
  hooks/                   Hooks de dados e relógio
  lib/                     Idiomas (en-US, pt-BR), tema, escala de fonte e formatação
  providers/               Tipos, regras de cotas e tabela de referência de custo eq. API
  services/                Ponte tipada com o backend Tauri
  styles/                  Tokens de tema claro/escuro, animações e estilos
src-tauri/                 Backend desktop (Rust, Tauri 2)
  src/
    main.rs                Inicialização do app, plugins e registro de comandos IPC
    core/                  Monitor de atualização, preferências atômicas, SQLite, bandeja e alertas
    providers/             Leitores Claude, Codex, Antigravity, streaming JSONL e descoberta local
  capabilities/            Permissões mínimas das janelas Tauri
  migrations/              Migrações SQLite do histórico local
scripts/                   Scripts PowerShell de release e testes Rust
e2e/                       Testes ponta a ponta (Playwright)
docs/                      Arquitetura, integrações e checklist de validação no Windows
```

## Documentação, segurança e licença

- [Arquitetura](docs/architecture.md): visão técnica das camadas, fluxos de dados e armazenamento local.
- [Integrações](docs/integrations.md): como cada provedor se conecta em modo zero-config e garantias de privacidade.
- [Validação no Windows](docs/windows-validation.md): checklist antes de publicar uma versão.
- [Segurança](SECURITY.md) e [Guia de contribuição](CONTRIBUTING.md).

Idealizado e desenvolvido por [Julio Bevilacqua](https://github.com/BevilacquaJulio) sob licença [MIT](LICENSE).
