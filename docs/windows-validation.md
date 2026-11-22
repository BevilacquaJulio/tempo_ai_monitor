# Validação no Windows 11

## Como gerar a build

Com Node.js `22.12+` / `24+`, Rust stable (MSVC), Visual Studio Build Tools com o SDK do Windows e WebView2:

```powershell
npm.cmd ci
npm.cmd run typecheck
npm.cmd run lint
npm.cmd test
npm.cmd run test:rust
npm.cmd run test:e2e
npm.cmd run release
```

`npm run release` prepara o ambiente do Visual Studio e do SDK do Windows, compila o aplicativo e gera em `release/`: `TempoAI-<versão>-setup.exe` (instalador NSIS), `TempoAI-<versão>-portable.exe` (executa sem instalar) e `SHA256SUMS.txt` (resumo SHA-256 dos binários). Versões anteriores nessa pasta são substituídas. Para distribuição pública, assine ambos os executáveis com certificado Authenticode (verificando com `Get-AuthenticodeSignature`), regere o `SHA256SUMS.txt` após assinar e publique os arquivos junto com os hashes no GitHub Releases.

## Checklist antes de publicar uma versão

Em uma máquina de teste Windows 11:

1. Instale, abra e confirme que nenhum provedor começa conectado e que a tela inicial exibe a varredura local (**Escanear minhas IAs**). Confira também os estados vazios em Atividade e no Monitor rápido.
2. Abra o Monitor rápido pela bandeja e com `Ctrl+Alt+W` enquanto outro aplicativo está em foco. Confira alternância, esconder ao perder foco, fixar (permanecendo aberto ao abrir o painel), redimensionar/arrastar, fechar para a bandeja e encerrar por **Sair do TempoAI** (*Quit TempoAI*).
3. Abra uma segunda instância e confirme que a primeira recebe foco. Teste um atalho já ocupado e confirme que o app preserva o anterior. Valide a opção **Iniciar com o Windows** após reiniciar.
4. Siga o [guia de integração](integrations.md) com contas de teste (Claude Code, OpenAI Codex, Google Antigravity e arquivo JSON local). Compare janelas e horários de renovação com cada provedor e teste ausência de login, indisponibilidade e leituras desatualizadas.
5. Com notificações do Windows habilitadas, verifique entrega e ausência de duplicação no mesmo ciclo. Suspenda e retome o computador e confira atualização e marcação de dados desatualizados. Verifique uso de CPU e memória com o painel aberto e em segundo plano.
6. Valide atualização sobre uma versão anterior, desinstalação e reinstalação. Confirme que `history.sqlite` e `preferences.json` não armazenam prompts, respostas ou credenciais.
