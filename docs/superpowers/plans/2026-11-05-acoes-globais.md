# Sonar v1.1 — Comandos globais: plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Entregar a camada de ações tipadas (`Action`), o dispatcher (fila + worker), os atalhos globais configuráveis e as ações A2, A3, A4, A6, A7, A8, A9 e A10, sem mudar o contrato atual do painel.

**Architecture:** Toda entrada (atalho, bandeja, IPC) vira uma `Action` (enum com tag `serde`). Atalho e bandeja só enfileiram (`mpsc` limitado a 64); um worker executa sob `Engine::apply`. O IPC chama o mesmo executor, de forma síncrona. Bindings (`accelerator` → `Action`) ficam em `Settings.bindings`, com migração do `shortcut` legado. O `Engine` não conhece Tauri: o que precisa do `AppHandle` (painel, evento) fica numa camada fina (`action_runtime.rs`).

**Tech Stack:** Rust (Tauri 2, crate `windows` 0.62, `tauri-plugin-global-shortcut`), React/TypeScript, Vitest, Bun.

**Spec:** `docs/SPEC.md` (o quê e em que ordem) e `docs/sonar-system-design.md` (o porquê). Decisões do autor de 2026-11-05 abaixo valem sobre os dois.

## Decisões fechadas (2026-11-05)

- **Q1:** o atalho de mudo (`Ctrl+Alt+M`) é **toggle**, resolvido sob `apply` (lê o estado fresco, chama `set` com o inverso). A API interna continua com `set`.
- **Q2:** o cursor do ciclo de perfil **não persiste** (usa `last_profile`, só em memória).
- **Q3 (B):** padrão de volume `Ctrl+Alt+Up` / `Ctrl+Alt+Down` (±0,05). A8, A9 e A10 não têm atalho padrão. Registro que falhar vira "indisponível".
- **Q4:** execução subagent-driven. **B1 e B2 liberados** pelo autor. **S4 continua pendente** e é a Task 9.
- **Branch:** `feat/acoes-globais` (dentro de `D:\Projetos\sonar`), um PR por fase, nada direto no `main`.

## Global Constraints

- Só Windows 11 build 26300. Windows 10 fica fora (não testado), como na v1.
- JS/TS: **Bun** (`bun run test`, `bun run build`). Não trocar o lockfile (`bun.lock`).
- Linha base que não pode regredir: `cargo test` com **93 passando e 4 ignorados**; `bun run test` com **122 passando (25 arquivos)**; `bun run build` sem avisos de compilador. Cada fase só avança se os números forem iguais ou maiores.
- Ordem de travas do `Engine`: `apply` > `persist` > `emit` > `inner`. **Nenhuma trava de dados nova.** Nenhum COM nem trava de dados no handler do atalho.
- Fila de ações: **64**. Janela de coalescência de volume: **75 ms**. Atalho: no máximo **64 caracteres**.
- Mensagens ao usuário em português, sem HRESULT hexadecimal nas ações de atalho. Os comandos IPC `set_*` mantêm o texto atual (RF-20).
- Contrato preservado: `AppState` (forma fixada em `app_state_json_shape_is_pinned`), evento `state-changed`, contador `generation`. Estado novo de atalhos vai por comando e evento próprios, **não** em `AppState`.
- Sem rede, sem telemetria.
- Não implementar nada da §1 "Não entra" do SPEC (EQ, microfone A5, mudo geral A11, `--action`).
- Trabalhar só em `D:\`. Commits em português, estilo `feat:`/`test:`/`docs:`, terminando com `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`. PR com prints ou imagens na descrição e a linha `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.

## Quem faz o quê

| Papel | Modelo | Faz |
|---|---|---|
| Orquestração, plano, revisão de cada fase | Sonnet 5.5 | Despacha tarefas, lê diffs, roda a revisão de fase |
| Escrever código e testes de cada tarefa | Haiku 5.5 (subagent nativo) | Implementa a tarefa exatamente como descrita |
| Rascunho de testes e docs (opcional) | Modelos free do NIM | Só rascunho, via Task 0. Nunca código de produção sem revisão |

Se um escritor falhar duas vezes na mesma tarefa, a tarefa sobe para o Sonnet 5.5.

## Review Focus

Entradas que o spec implica e nenhuma tarefa pediria sozinha. Cada linha tem o teste na tarefa dona.

1. **Sem saída padrão** (headset desligado, nenhum render ativo): mudo e volume devolvem `ok:false` com `no_output_device`, sem pânico e sem emitir estado. → Task 3 e Task 4.
2. **Mesmo atalho com caixa diferente** (`ctrl+alt+m` e `Ctrl+Alt+M`) conta como duplicata. → Task 11.
3. **Volume nos limites** (0,0 com delta negativo; 1,0 com delta positivo) e valor não finito (`NaN`): clamp, `changed:false` e `invalid_value` respectivamente. → Task 4.
4. **`settings.json` da v1 com `shortcut` vazio ou lixo**: migração não cria binding inválido e o app abre. → Task 12.
5. **Um atalho que falha no registro** (tecla em uso) não impede os outros nem a inicialização. → Task 13.

## Estrutura de arquivos

| Arquivo | Responsabilidade |
|---|---|
| Criar `src-tauri/src/actions.rs` | `Action`, `Direction`, `ErrorCode`, `ActionResult`, trait `ActionExecutor`, `impl ActionExecutor for Engine` |
| Criar `src-tauri/src/dispatcher.rs` | Fila limitada, worker, coalescência de volume |
| Criar `src-tauri/src/bindings.rs` | `Binding`, validação, padrões, migração, sugestão de atalhos de perfil |
| Criar `src-tauri/src/hotkeys.rs` | Tabela atalho→ação, registro por binding, estado "ativo/indisponível" |
| Criar `src-tauri/src/action_runtime.rs` | `AppExecutor` (painel + evento `action-result`) e ligação com o plugin |
| Modificar `src-tauri/src/engine.rs` | Métodos `act_*` e resolvedores (`default_render`, `session_pids`, `visible_outputs`) |
| Modificar `src-tauri/src/state.rs` | `Settings.bindings` |
| Modificar `src-tauri/src/store.rs` | `load_versioned`, `CURRENT_VERSION = 2` |
| Modificar `src-tauri/src/shell.rs`, `commands.rs`, `lib.rs` | Bandeja e atalho pelo dispatcher, novos comandos IPC |
| Modificar `src/types.ts`, `src/api.ts`, `src/screens/settingsModel.ts`, `src/screens/Settings.tsx` | Tipos, chamadas, captura de setas, lista de bindings |

---

## Task 0 (opcional, fora do repo): rascunhador NIM

Só se a Task 0 for pedida pelo autor. Não bloqueia nada.

**Files:**
- Create: `D:\Projetos\sonar-tools\nim-draft.mjs` (fora do repo, sem chave dentro)

**Interfaces:**
- Produces: `node nim-draft.mjs <arquivo-de-prompt> [--model <id>]` imprime o rascunho no stdout. Código de saída 2 quando nenhum modelo responde.

- [ ] **Step 1:** Ler a chave de `NVIDIA_API_KEY` no ambiente; se ausente, ler de `env.NVIDIA_API_KEY` em `~/.claude/settings.local.json`. Nunca imprimir a chave nem gravá-la em arquivo.
- [ ] **Step 2:** Teste de vida (`max_tokens: 10`, prompt `responda so: ok`) em `https://integrate.api.nvidia.com/v1/chat/completions`, na ordem: `nvidia/nemotron-3.5-lightning-30b-a3b`, `mistralai/mistral-large-4-1t-a52b`, `nvidia/nemotron-3.5-bolt-e4b`. Usar o primeiro com HTTP 200 em até 15 s.
- [ ] **Step 3:** Enviar o prompt com timeout de 120 s; imprimir a resposta. Verificar: `node nim-draft.mjs prompt.txt` com um prompt de uma linha devolve texto; com a chave removida sai com código 2 e mensagem em português.

---

# FASE F0 — `actions.rs` sem mudar comportamento

## Task 1: tipos de ação

**Files:**
- Create: `src-tauri/src/actions.rs`
- Modify: `src-tauri/src/lib.rs` (adicionar `mod actions;` com `#[allow(dead_code)]` até a F1)

**Interfaces:**
- Produces (usado por todas as tarefas seguintes):
  - `enum Direction { Next, Prev }` (`serde` minúsculo: `"next"`, `"prev"`).
  - `enum Action` com `#[serde(tag = "type", rename_all = "snake_case")]`: `TogglePanel`, `ActivateProfile { profile_id: String }`, `CycleProfile { direction: Direction }`, `SetOutputMute { muted: bool }`, `ToggleOutputMute`, `StepOutputVolume { delta: f32 }`, `SetOutputVolume { value: f32 }`, `SetAppMute { exe: String, muted: bool }`, `MoveApp { exe: String, device: Option<String> }`, `CycleDefaultOutput { direction: Direction }`. Deriva `Debug, Clone, PartialEq, Serialize, Deserialize`.
  - `enum ErrorCode` (`snake_case`): `ProfileNotFound`, `NoProfiles`, `NoOutputDevice`, `NotEnoughOutputs`, `DeviceGone`, `Unsupported`, `OsError`, `InvalidExe`, `InvalidValue`, `QueueFull`.
  - `struct ActionResult { ok: bool, changed: bool, resolved: Option<String>, error_code: Option<ErrorCode>, message: Option<String> }`, com `ActionResult::done(changed: bool, resolved: Option<String>) -> ActionResult` e `ActionResult::failed(code: ErrorCode, message: &str) -> ActionResult`.
  - `fn audio_failure(err: &AudioError) -> ActionResult` (`Gone` → `DeviceGone`; `Unsupported` → `Unsupported`; `Os` → `OsError` com a frase "O Windows recusou a operação.", sem código hexadecimal).
  - `trait ActionExecutor: Send + Sync { fn execute(&self, action: Action) -> ActionResult; }`

- [ ] **Step 1: Escrever os testes** em `actions.rs` (`#[cfg(test)]`):
  - `parses_every_action_variant`: cada variante vem de um JSON do SPEC §7.2 (`{"type":"set_output_mute","muted":true}`, `{"type":"step_output_volume","delta":0.05}`, `{"type":"cycle_profile","direction":"next"}`, `{"type":"move_app","exe":"chrome.exe","device":null}` etc.) e volta igual ao valor esperado.
  - `unknown_action_type_is_rejected` (RF-01): `serde_json::from_str::<Action>(r#"{"type":"mute_everything"}"#)` é `Err`.
  - `action_result_json_shape_is_pinned`: `ActionResult::done(false, None)` serializa para `{"ok":true,"changed":false,"resolved":null,"error_code":null,"message":null}`.
  - `os_failure_message_has_no_hex`: `audio_failure(&AudioError::Os(-2147024891))` tem `message` sem `0x`.
- [ ] **Step 2: Rodar e ver falhar.** `cd src-tauri && cargo test actions::` → erro de compilação (tipos não existem).
- [ ] **Step 3: Implementar** os tipos e `audio_failure` acima em `actions.rs`.
- [ ] **Step 4: Rodar.** `cargo test actions::` → 4 passando.
- [ ] **Step 5: Commit.** `git add src-tauri/src/actions.rs src-tauri/src/lib.rs && git commit -m "feat: tipos de ação global (Action, ActionResult)"`

## Task 2: resolvedores de alvo no Engine

**Files:**
- Modify: `src-tauri/src/engine.rs` (junto de `default_capture`, linha ~366; testes junto de `default_capture_is_the_connected_default_microphone`, linha ~1647)

**Interfaces:**
- Consumes: `Flow`, `DeviceId` (`audio/types.rs`).
- Produces:
  - `Engine::default_render(&self) -> Option<DeviceId>` (saída padrão conectada, do último sincronismo; não toca no sistema).
  - `Engine::session_pids(&self, exe: &str, flow: Flow) -> Vec<u32>` (sessões do snapshot cujo `exe` bate sem diferenciar caixa).
  - `Engine::visible_outputs(&self) -> Vec<DeviceId>` (saídas conectadas e **não ocultas**, na ordem de `devices`; é a ordem do painel, que só reordena conectados primeiro em `devicesModel.ts:35`).

- [ ] **Step 1: Testes** (usar `World`, `device(...)`, `session(...)` existentes):
  - `default_render_is_the_connected_default_output`: duas saídas, a padrão desconectada é ignorada; sem padrão conectado devolve `None`.
  - `session_pids_matches_exe_case_insensitively_per_flow`: `Discord.EXE` casa `discord.exe`; sessão de captura não entra no render.
  - `visible_outputs_skips_hidden_and_disconnected_keeping_order`: 3 saídas, uma oculta por `update_device_meta(..., hidden: true)`, uma desconectada.
- [ ] **Step 2: Rodar e ver falhar.** `cargo test engine::tests::default_render` etc. → erro de compilação.
- [ ] **Step 3: Implementar** os três métodos lendo `self.lock()`, sem tocar no sistema.
- [ ] **Step 4: Rodar a suíte inteira.** `cargo test` → ≥ 96 passando e 4 ignorados; `bun run test` → 122. Aceite manual da F0: trocar saída de um app, mutar, volume e perfil pela UI; nada muda.
- [ ] **Step 5: Commit** `feat: resolvedores de alvo (saída padrão, pids por exe, saídas visíveis)`. Abrir o PR da F0 (`feat/acoes-globais` → `main`) com um print do painel igual ao de antes.

---

# FASE F1 — Dispatcher e ações básicas (A2, A4, A6, A7, A8)

## Task 3: A4, mudo da saída padrão (set e toggle)

**Files:**
- Modify: `src-tauri/src/engine.rs` (testes ao lado de `set_device_muted_forwards_and_shows_new_value`, linha ~1528)

**Interfaces:**
- Consumes: `Engine::default_render` (Task 2), `ActionResult`, `audio_failure` (Task 1).
- Produces: `pub enum MuteMode { Set(bool), Toggle }` e `Engine::act_output_mute(&self, mode: MuteMode) -> ActionResult`. Segura `apply` durante todo o método, lê o mudo **fresco** por `ctl.muted(id)` e só escreve se o valor muda. Extrair o corpo de `set_device_muted` para um privado `write_device_muted(&self, id, m)` (sem `apply`) que os dois usam; o contrato público de `set_device_muted` não muda. `resolved` é o id do dispositivo afetado.

- [ ] **Step 1: Testes** (RF-06, RF-07, RF-14, RNF-04, RNF-05, Review Focus 1):
  - `set_mute_twice_changes_once`: `Set(true)` duas vezes → 1ª `changed:true`, 2ª `changed:false`; `FakeCtl.calls()` tem exatamente 1 `SetMuted`; o notificador é chamado 1 vez.
  - `toggle_reads_fresh_state`: `ctl.set_mute_of(id, true)` por fora entre duas chamadas de `Toggle` → a segunda desmuta.
  - `mute_without_default_output_fails_without_emit`: sem padrão conectado → `ok:false`, `no_output_device`, nenhuma chamada ao `ctl`, nenhum aviso.
  - `mute_on_gone_device_returns_device_gone`: `FakeCtl` que devolve `AudioError::Gone` → `ok:false`, `device_gone`, estado igual ao anterior, nenhum aviso.
- [ ] **Step 2: Rodar e ver falhar.** `cargo test engine::tests::set_mute_twice` → erro de compilação.
- [ ] **Step 3: Implementar** `MuteMode`, `write_device_muted` e `act_output_mute` como descrito. Sem `unwrap` no caminho.
- [ ] **Step 4: Rodar.** `cargo test engine::` → tudo verde.
- [ ] **Step 5: Commit** `feat: ação de mudo da saída padrão (set e toggle sob apply)`

## Task 4: A6 e A7, volume da saída padrão

**Files:**
- Modify: `src-tauri/src/engine.rs`

**Interfaces:**
- Consumes: `clamp_volume` (`audio/endpoint.rs`), `Engine::default_render`.
- Produces: `pub enum VolumeMode { Step(f32), Set(f32) }` e `Engine::act_output_volume(&self, mode: VolumeMode) -> ActionResult`. Mesmo padrão da Task 3: `apply`, leitura fresca (`ctl.volume`), clamp 0 a 1, `changed` só se a diferença passa de `0.0001`. Valor não finito → `invalid_value` sem tocar no sistema. Extrair `write_device_volume` de `set_device_volume`, mesmo critério.

- [ ] **Step 1: Testes** (RF-08 parcial, RF-09, Review Focus 3):
  - `step_volume_clamps_and_reports_change`: de 0,98 com `Step(0.05)` → 1,0, `changed:true`.
  - `step_at_limit_is_not_a_change`: de 1,0 com `Step(0.05)` e de 0,0 com `Step(-0.05)` → `ok:true`, `changed:false`, nenhuma escrita.
  - `set_volume_is_absolute_and_clamped`: `Set(1.7)` grava 1,0; `Set(-1.0)` grava 0,0.
  - `non_finite_volume_is_rejected`: `Step(f32::NAN)` e `Set(f32::INFINITY)` → `invalid_value`, nenhuma chamada ao `ctl`.
  - `volume_without_default_output_fails`: `no_output_device`.
- [ ] **Step 2: Rodar e ver falhar.** `cargo test engine::tests::step_volume` → erro de compilação.
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test engine::` → verde.
- [ ] **Step 5: Commit** `feat: ações de volume da saída padrão (relativa e absoluta)`

## Task 5: A8, mudo de app por executável

**Files:**
- Modify: `src-tauri/src/engine.rs`

**Interfaces:**
- Consumes: `apps::parse_exe_request(exe: &str) -> Option<ExeRequest<'_>>` (`apps.rs:76`; rejeita UNC, verbatim e caminho relativo), `Engine::session_pids`, `ctl.set_session_muted`.
- Produces: `Engine::act_app_mute(&self, exe: &str, muted: bool) -> ActionResult`. Exe inválido → `invalid_exe`. Lê as sessões **frescas** por `source.sessions()` (fora das travas de dados), aplica a todas as sessões de render do exe, ignora `Gone` de uma sessão sem abortar as outras. Sem sessão → `ok:true`, `changed:false`. `changed` é verdadeiro se alguma sessão mudou de estado. Termina com `refresh()` quando algo mudou.

- [ ] **Step 1: Testes** (RF-10, Review Focus):
  - `app_mute_affects_all_pids_of_the_exe_only`: duas sessões do `discord.exe` e uma do `spotify.exe` → só as duas do Discord recebem `SetSessionMuted`.
  - `app_mute_without_session_is_ok_and_unchanged`.
  - `app_mute_rejects_unc_path`: `\\servidor\share\x.exe` → `invalid_exe`, nada tocado.
  - `app_mute_survives_one_gone_session`: o `FakeCtl` devolve `Gone` para um pid; o outro é mutado; `ok:true`, `changed:true`.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test engine::` → verde.
- [ ] **Step 5: Commit** `feat: ação de mudo de app por executável`

## Task 6: A2, ativar perfil + executor do Engine

**Files:**
- Modify: `src-tauri/src/engine.rs`, `src-tauri/src/actions.rs`

**Interfaces:**
- Consumes: `Engine::activate_profile(&self, id: &str) -> bool`; tasks 3, 4, 5.
- Produces:
  - `Engine::act_activate_profile(&self, id: &str) -> ActionResult` (id inexistente → `profile_not_found`, "Perfil não encontrado.", estado igual; sucesso → `changed:true`, `resolved: Some(id)`).
  - `impl ActionExecutor for Engine` em `actions.rs`: mapeia `ActivateProfile`, `SetOutputMute`, `ToggleOutputMute`, `StepOutputVolume`, `SetOutputVolume`, `SetAppMute` para os métodos `act_*`. `TogglePanel`, `CycleProfile`, `MoveApp` e `CycleDefaultOutput` devolvem `failed(Unsupported, "Ação ainda não disponível.")` até as tasks 15, 19 e 20 (e `TogglePanel` fica com o `AppExecutor` da Task 8).

- [ ] **Step 1: Testes** (RF-03):
  - `activate_unknown_profile_fails_and_changes_nothing`: `ok:false`, `profile_not_found`, `state()` igual ao anterior.
  - `executor_maps_each_implemented_action`: um `Action` de cada tipo implementado passa por `Engine::execute` e produz o efeito (mesma verificação das tasks 3 a 5).
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test` completo → verde.
- [ ] **Step 5: Commit** `feat: executor de ações no Engine e ativação de perfil por ação`

## Task 7: dispatcher (fila + worker + coalescência)

**Files:**
- Create: `src-tauri/src/dispatcher.rs`
- Modify: `src-tauri/src/lib.rs` (`mod dispatcher;`)

**Interfaces:**
- Consumes: `Action`, `ActionResult`, `ActionExecutor` (Task 1).
- Produces:
  - `struct Dispatcher` (guarda um `SyncSender<Action>` de capacidade **64**).
  - `Dispatcher::spawn(exec: Arc<dyn ActionExecutor>, report: Arc<dyn Fn(&Action, &ActionResult) + Send + Sync>, window: Duration) -> Dispatcher`.
  - `Dispatcher::submit(&self, action: Action) -> Result<(), SubmitError>` com `enum SubmitError { Full, Closed }`; usa `try_send`, **nunca bloqueia**.
  - Worker: pega uma ação; se for `StepOutputVolume`, junta as seguintes do mesmo tipo que chegarem dentro de `window` somando os `delta` (por `recv_timeout`); uma ação de outro tipo encerra o lote e roda depois dele, na ordem de chegada. Executa com `catch_unwind` (um pânico não derruba o worker) e chama `report` com o resultado.

- [ ] **Step 1: Testes** com um `FakeExecutor` que registra as chamadas (RF-02, RF-08, RNF-02, RNF-05):
  - `actions_run_in_arrival_order`: 5 ações distintas → executor as recebe na mesma ordem.
  - `volume_burst_becomes_one_call`: 10 × `StepOutputVolume{delta:0.05}` enfileirados juntos → o executor recebe **1** chamada com `delta` ≈ 0,5 (tolerância 1e-4).
  - `other_action_breaks_the_batch_and_keeps_order`: `Step(+0.05)`, `ToggleOutputMute`, `Step(+0.05)` → 3 chamadas nessa ordem.
  - `full_queue_does_not_block_the_producer`: executor bloqueado numa barreira; enfileirar 80 ações termina em menos de 200 ms e as excedentes devolvem `SubmitError::Full`.
  - `failed_action_does_not_stop_the_worker`: a 1ª devolve `failed(DeviceGone, ..)`, a 2ª ainda executa.
  - `panicking_executor_does_not_kill_the_worker`.
- [ ] **Step 2: Rodar e ver falhar.** `cargo test dispatcher::` → erro de compilação.
- [ ] **Step 3: Implementar** `dispatcher.rs`. `report` é chamado depois de cada execução (o `AppExecutor` da Task 8 usa para emitir `action-result`).
- [ ] **Step 4: Rodar.** `cargo test dispatcher::` → 6 passando.
- [ ] **Step 5: Commit** `feat: dispatcher de ações (fila limitada, worker, coalescência de volume)`

## Task 8: ligar tudo (IPC, bandeja, atalho) e padrões

**Files:**
- Create: `src-tauri/src/bindings.rs` (só `Binding` e `default_bindings` por enquanto), `src-tauri/src/action_runtime.rs`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/shell.rs`, `src-tauri/capabilities/default.json` (só se a verificação abaixo exigir)

**Interfaces:**
- Consumes: Tasks 1 a 7.
- Produces:
  - `struct Binding { accelerator: String, action: Action }` (`Serialize, Deserialize, Clone, PartialEq, Debug`).
  - `fn default_bindings() -> Vec<Binding>`: `Super+Alt+V` → `TogglePanel`; `Ctrl+Alt+M` → `ToggleOutputMute`; `Ctrl+Alt+Up` → `StepOutputVolume{0.05}`; `Ctrl+Alt+Down` → `StepOutputVolume{-0.05}`.
  - `struct AppExecutor { engine: Arc<Engine>, app: AppHandle }` com `impl ActionExecutor`: `TogglePanel` chama `shell::toggle_from_shortcut`; o resto delega ao `Engine`.
  - Constante `ACTION_RESULT: &str = "action-result"`; o `report` do dispatcher emite esse evento só quando `ok == false` (o painel pode estar oculto; também grava em `eprintln!`).
  - Comando IPC `dispatch_action(engine: Eng<'_>, action: Action) -> ActionResult`, `#[tauri::command(async)]`, executa **direto** pelo `Engine` (não pela fila) e devolve o resultado.
  - Handler do plugin em `lib.rs`: em `Pressed`, procura a ação do atalho na tabela e chama `Dispatcher::submit`. Atalho sem ação na tabela é ignorado. Itens da bandeja que alternam o painel passam por `Action::TogglePanel` e `submit`.

- [ ] **Step 1: Testes:**
  - `default_bindings_have_no_duplicate_accelerators` (comparação sem caixa).
  - `shortcut_handler_only_enqueues` (RF-02, RNF-01): com `FakeExecutor` bloqueado, `submit` retorna na hora; o handler não chama o executor.
  - `dispatch_action_returns_the_executor_result`: via `Engine::execute` (o comando em si é uma camada fina).
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.** Registrar os atalhos de `default_bindings()` na abertura (`shell::apply_at_startup`), cada um independente: erro vai para `eprintln!` e não impede os demais. **Verificar a permissão Tauri v2:** confirmar em `src-tauri/build.rs` se há `AppManifest::commands`; se não houver, comandos do `invoke_handler` não precisam de permissão extra. Registrar o achado em `docs/sonar-system-design.md` §6 (resolve o PENDENTE). Se precisar, adicionar a permissão em `capabilities/default.json`.
- [ ] **Step 4: Rodar.** `cargo test` completo e `bun run build` → verde.
- [ ] **Step 5: Commit** `feat: bandeja, atalho e IPC passam pelo dispatcher`

## Task 9: spike S4 (conflito de atalho) e ajuste das setas

**Files:**
- Modify: `src-tauri/src/shell.rs` (função de sonda só em `#[cfg(debug_assertions)]`), `src/screens/settingsModel.ts`, `src/screens/Settings.test.tsx`, `docs/sonar-system-design.md` (§10 S4: resultado)

**Interfaces:**
- Produces: `fn probe_shortcuts(registrar: &dyn ShortcutRegistrar, accelerators: &[&str]) -> Vec<(String, Result<(), String>)>`; só registra e solta cada tecla, na ordem. Em debug, `SONAR_PROBE_SHORTCUTS="Ctrl+Alt+M,Ctrl+Alt+Up,Win+L,..."` roda a sonda na abertura e imprime o resultado de cada tecla em `stderr`.
- `mainKeyOf` em `settingsModel.ts` passa a aceitar `ArrowUp` e `ArrowDown` (viram `Up` e `Down`).

- [ ] **Step 1: Testes:**
  - Rust: `probe_reports_each_key_independently` com `FakeRegistrar` (as chaves que começam com `bad` falham, as demais passam; a falha de uma não interrompe as outras).
  - Rust: estender `every_shortcut_the_settings_screen_can_produce_parses` (`shell.rs`, linha ~437) para incluir `Up` e `Down`, e atualizar a contagem esperada para 14 × 50.
  - Vitest: `shortcutFromKeys({code:"ArrowUp", ctrlKey:true, altKey:true, ...})` → `{status:"ok", shortcut:"Ctrl+Alt+Up"}`.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** a sonda e a mudança de `mainKeyOf`.
- [ ] **Step 4: Manual no Win11 26300** (autor ou agente com o app aberto): rodar com `SONAR_PROBE_SHORTCUTS` cobrindo os padrões (`Ctrl+Alt+M`, `Ctrl+Alt+Up`, `Ctrl+Alt+Down`, `Ctrl+Alt+P`, `Ctrl+Alt+1`) mais teclas reservadas (`Win+L`, `Ctrl+Alt+Delete` não é registrável; use `Win+D`). Registrar a tabela de resultados em `docs/sonar-system-design.md` §10 S4 e marcar B3 liberado no SPEC §2. Se um padrão falhar, trocar o padrão em `default_bindings()`.
- [ ] **Step 5: Commit** `feat: sonda de atalhos (S4) e captura de setas`

## Task 10: aceite da F1

- [ ] **Step 1:** `cargo test` (≥ linha base), `bun run test` (≥ 122), `bun run build` sem avisos.
- [ ] **Step 2: Manual no Win11 26300**, conforme SPEC §9.3: `Ctrl+Alt+M` muta e desmuta duas vezes (ícone do Windows acompanha); `Ctrl+Alt+Up`/`Down` cinco vezes rápido muda em um ajuste; ativar perfil por binding de teste; desligar o headset e apertar o mudo → mensagem de erro, app segue; rede desligada e foco em outro app → ação funciona.
- [ ] **Step 3: Medir RNF-09:** log temporário (debug) do tempo entre `Pressed` e o fim da execução; registrar o valor no SPEC §5 (RNF-09) e remover o log.
- [ ] **Step 4:** Revisão de fase (Sonnet 5.5) do diff inteiro da F1 contra SPEC §4 (RF-01 a RF-03, RF-06 a RF-10, RF-13, RF-14) e §5 (RNF-01 a RNF-05). Corrigir achados.
- [ ] **Step 5:** PR da F1 com prints do painel e do evento de erro; atualizar o status da F1 no SPEC §7.

---

# FASE F2 — Bindings, validação, registro por binding, migração, A9

## Task 11: modelo e validação de bindings

**Files:**
- Modify: `src-tauri/src/bindings.rs`

**Interfaces:**
- Produces:
  - `enum BindingError { Empty, TooLong, Syntax, Duplicate }` com `fn message(&self) -> &'static str` em português.
  - `fn validate_bindings(list: &[Binding]) -> Result<(), (usize, BindingError)>` (índice do item com problema). Sintaxe checada com `accelerator.parse::<Shortcut>()` (de `tauri_plugin_global_shortcut`); duplicata comparada pelos `Shortcut` parseados, então sem diferenciar caixa; máximo 64 caracteres.

- [ ] **Step 1: Testes** (RF-16, Review Focus 2): `rejects_duplicate_accelerators_ignoring_case` (`ctrl+alt+m` e `Ctrl+Alt+M`); `rejects_over_64_chars`; `rejects_bad_syntax` (`Ctrl+`, `Banana`); `rejects_empty`; `accepts_defaults` (`default_bindings()` passa).
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test bindings::` → verde.
- [ ] **Step 5: Commit** `feat: validação de bindings de atalho`

## Task 12: `Settings.bindings`, Store versionado e migração

**Files:**
- Modify: `src-tauri/src/state.rs`, `src-tauri/src/store.rs`, `src-tauri/src/engine.rs` (`Engine::new`), `src-tauri/src/bindings.rs`

**Interfaces:**
- Produces:
  - `Settings.bindings: Vec<Binding>` (com `#[serde(default)]` do struct; `shortcut` e `autostart` continuam).
  - `Store::load_versioned<T: DeserializeOwned + Default>(&self, name: &str) -> (u32, T)` (versão 0 quando o arquivo não existe ou é ilegível), e `CURRENT_VERSION = 2`.
  - `fn migrate_settings(version: u32, settings: Settings) -> (Settings, bool)` em `bindings.rs`: para `version < 2`, vira `bindings = [ {shortcut → TogglePanel} ] + default_bindings()` sem duplicar `shortcut`; `shortcut` vazio ou inválido não gera binding; o `bool` diz se migrou.
- `Engine::new` carrega `settings` com `load_versioned`, migra e, se migrou, persiste na hora (o `Store::save` já copia o arquivo antigo para `.bak`).
- **Nota para a revisão:** o RF-19 literal cria só o binding `toggle_panel`. Aqui a migração também semeia os padrões (M, Up, Down) para o autor não precisar recriá-los. Se o autor preferir o literal, remover `+ default_bindings()` e adicionar o botão "Restaurar padrões" na Task 16.

- [ ] **Step 1: Testes** (RF-19, RNF-06, Review Focus 4):
  - `v1_settings_file_migrates_and_keeps_bak`: gravar em tempdir um `settings.json` real da v1 (`{"version":1,"data":{"shortcut":"Super+Alt+P","autostart":true}}`), abrir o `Engine`; `settings().bindings` contém `Super+Alt+P` → `TogglePanel` e os padrões; `settings.json.bak` guarda o conteúdo da v1; `autostart` preservado.
  - `garbage_legacy_shortcut_creates_no_binding`: `shortcut` vazio e `"???"` → só os padrões, sem pânico.
  - `current_version_file_is_not_migrated_again`: abrir o `Engine` duas vezes não duplica nem regrava.
  - `app_state_json_shape_is_pinned` continua passando (só o conteúdo de `settings` ganha `bindings`; **atualizar o teste fixado e documentar o campo novo**, é a única mudança permitida no teste).
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test` completo → verde.
- [ ] **Step 5: Commit** `feat: Settings.bindings com migração do atalho legado (Store v2)`

## Task 13: registro por binding e estado "indisponível"

**Files:**
- Create: `src-tauri/src/hotkeys.rs`
- Modify: `src-tauri/src/shell.rs`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `ShortcutRegistrar`, `switch_shortcut` (`shell.rs:250-271`), `Binding`.
- Produces:
  - `enum BindingStatus { Active, Unavailable(String) }` (`Serialize`, uma entrada por binding, na mesma ordem).
  - `fn apply_bindings(registrar: &dyn ShortcutRegistrar, current: &[Binding], next: &[Binding]) -> (Vec<BindingStatus>, Vec<Binding>)`: o segundo valor é o conjunto efetivamente registrado. Atalho que não mudou **não é tocado**. Atalho novo é registrado **antes** de soltar o antigo (mantém o comportamento atual de `switch_shortcut`, que garante o RF-17; isto **desvia** do RF-15 literal, "desregistrar antes de registrar", que era hipótese; registrar o desvio no SPEC §6). Falha em um binding não impede os outros.
  - Tabela atalho→ação (`Mutex<HashMap<Shortcut, Action>>`, ou chave por `Shortcut::id()` se `Shortcut` não implementar `Hash`) lida pelo handler da Task 8.
  - Evento `bindings-status` com o `Vec<BindingStatus>`.

- [ ] **Step 1: Testes** com `FakeRegistrar` (RF-15, RF-17, Review Focus 5):
  - `unchanged_accelerators_are_not_touched`: `calls` vazio.
  - `failing_binding_is_unavailable_and_others_still_register`: 3 bindings, o do meio começa com `bad` → status `[Active, Unavailable, Active]`.
  - `failed_swap_keeps_previous_registered`: trocar `A` por `bad+B` → `A` segue na lista registrada e nenhum `unregister A` ocorre.
  - `moved_action_reuses_accelerator_without_double_register`: dois bindings trocam entre si as ações com o mesmo atalho → sem registro duplicado.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar** e usar `apply_bindings` em `apply_at_startup` (no lugar do registro fixo da Task 8).
- [ ] **Step 4: Rodar.** `cargo test hotkeys:: shell::` → verde.
- [ ] **Step 5: Commit** `feat: registro de atalhos por binding com estado indisponível`

## Task 14: IPC de bindings e compatibilidade de `save_settings`

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/shell.rs` (`apply_settings`)

**Interfaces:**
- Produces:
  - `save_bindings(app, engine, bindings: Vec<Binding>) -> Result<Vec<BindingStatus>, String>`: valida a lista inteira (Task 11), registra (Task 13) e só então grava em `Settings`. Erro de validação devolve a frase e **não grava nada**.
  - `get_binding_status() -> Vec<BindingStatus>`.
  - Compatibilidade (RF-20): `save_settings` continua aceitando `{ shortcut, autostart, bindings }`. Se `shortcut` mudou, o binding `TogglePanel` correspondente é atualizado (validado e registrado como em `save_bindings`); `apply_settings` deixa de registrar o atalho por conta própria e passa a usar `apply_bindings`. `Settings.shortcut` é mantido igual ao atalho do primeiro `TogglePanel`.

- [ ] **Step 1: Testes** (RF-16, RF-17, RF-20):
  - `invalid_list_is_rejected_without_writing`: duplicata → `Err`, `settings()` igual ao anterior, arquivo igual.
  - `failed_registration_keeps_old_bindings`.
  - `save_settings_with_new_shortcut_updates_toggle_panel_binding`.
  - `binding_to_deleted_profile_is_kept_and_action_fails` (RF-18): apagar o perfil, executar a ação → `ok:false`, `profile_not_found`, e o binding continua em `settings().bindings`.
  - `old_settings_payload_without_bindings_still_deserializes`: JSON `{ "shortcut": "...", "autostart": false }` → `bindings` vazio vira o valor atual (não apaga).
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test` completo → verde.
- [ ] **Step 5: Commit** `feat: comandos de bindings e compatibilidade do save_settings`

## Task 15: A9, mover app

**Files:**
- Modify: `src-tauri/src/engine.rs`, `src-tauri/src/actions.rs`

**Interfaces:**
- Consumes: `Engine::set_app_route(&self, exe: &str, flow: Flow, device: Option<DeviceId>)`, `apps::parse_exe_request`.
- Produces: `Engine::act_move_app(&self, exe: &str, device: Option<String>) -> ActionResult`: valida o exe; chama `set_app_route(exe, Flow::Render, device.map(DeviceId))`; se depois disso `state().routing_available` for falso → `failed(Unsupported, "Este Windows não permite essa operação.")`. Dispositivo ausente: regra guardada, `ok:true`. `MoveApp` no executor passa a usar este método.

- [ ] **Step 1: Testes** (RF-11): `move_app_stores_rule_for_absent_device` (a regra existe em `state().rules` e o `FakeBackend` não recebe `set_route`); `move_app_with_none_clears_the_rule`; `move_app_reports_unsupported_when_backend_unsupported` (modelo: `routing_unavailable_when_backend_unsupported`); `move_app_rejects_invalid_exe`.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test engine::` → verde.
- [ ] **Step 5: Commit** `feat: ação de mover app para outra saída`

## Task 16: tela de bindings

**Pré-requisito (gate):** desenhar a lista de bindings, o estado "indisponível" e a edição de cada um no **Doop** e obter a **aprovação do autor** (SPEC §6: os mockups são vinculantes). Sem o desenho aprovado, esta tarefa não começa. O desenho e a aprovação são feitos pelo Sonnet 5.5 com o autor, não por um escritor.

**Files:**
- Modify: `src/types.ts`, `src/api.ts`, `src/screens/Settings.tsx`, `src/screens/settingsModel.ts`
- Test: `src/api.test.ts`, `src/screens/Settings.test.tsx`, `src/screens/settingsModel` (novo `settingsModel.test.ts` se não existir)

**Interfaces:**
- Consumes: comandos `save_bindings`, `get_binding_status`, `dispatch_action`; eventos `bindings-status` e `action-result` (Tasks 8, 13, 14).
- Produces: tipos `Action`, `Binding`, `BindingStatus`, `ActionResult` em `types.ts` (espelho exato do JSON do Rust); `getBindingStatus()`, `saveBindings(bindings)`, `dispatchAction(action)` em `api.ts`; `suggestProfileBindings(profiles, existing)` em `settingsModel.ts` (propõe `Ctrl+Alt+1` a `Ctrl+Alt+9` para os 9 primeiros perfis sem binding de perfil, sem repetir atalho em uso). A tela lista os bindings, deixa adicionar, editar (captura com `shortcutFromKeys`), remover, mostra "Indisponível: <mensagem>" no binding que falhou e o botão "Sugerir atalhos de perfil". Falhas de `action-result` aparecem como `Toast` quando o painel está visível.

- [ ] **Step 1: Testes** Vitest: `api.test.ts` (as três chamadas usam os nomes de comando certos); `suggestProfileBindings` (perfis 1 a 3 sem binding → 3 sugestões; atalho já usado é pulado; mais de 9 perfis → só 9); `Settings.test.tsx` (lista renderiza os bindings; duplicata mostra a mensagem do backend; binding "indisponível" mostra o motivo; remover chama `saveBindings` com a lista sem o item).
- [ ] **Step 2: Rodar e ver falhar.** `bun run test` → falhas nos testes novos.
- [ ] **Step 3: Implementar** tipos, API, função pura e a tela, seguindo o desenho aprovado e os componentes existentes (`Card`, `Toggle`, `Chip`, `Toast`).
- [ ] **Step 4: Rodar.** `bun run test` ≥ 122 + novos, `bun run build` sem avisos.
- [ ] **Step 5: Commit** `feat: tela de bindings de atalho nas configurações`

## Task 17: aceite da F2

- [ ] **Step 1:** `cargo test`, `bun run test`, `bun run build` ≥ linha base.
- [ ] **Step 2: Manual no Win11 26300** (SPEC §7, F2 e §9.3): trocar um atalho com o painel aberto e fechado; reiniciar o app e confirmar que o atalho persiste; migração com uma **cópia real** do `settings.json` da v1 (`.bak` criado); duplicata recusada; tecla ocupada por outro app aparece como "Indisponível"; A9 por binding de teste ("Spotify para Headset") troca a saída; app de destino fechado → A9 guarda a regra.
- [ ] **Step 3:** Revisão de fase (Sonnet 5.5) contra RF-11, RF-15 a RF-20 e RNF-06/RNF-08. Corrigir achados.
- [ ] **Step 4:** PR da F2 com prints da tela de bindings (normal, com duplicata, com binding indisponível). Atualizar o SPEC §7.

---

# FASE F3 — Ciclo de perfil e próxima saída (A3, A10)

## Task 18: A3, próximo ou anterior perfil

**Files:**
- Modify: `src-tauri/src/engine.rs`, `src-tauri/src/actions.rs`

**Interfaces:**
- Consumes: `Engine::activate_profile`, `last_profile` (`Inner`, só em memória, Q2).
- Produces: `Engine::act_cycle_profile(&self, dir: Direction) -> ActionResult`. Ordem = ordem de `inner.profiles`. Cursor = `last_profile`; se for `None` ou não existir mais, `Next` vai ao primeiro e `Prev` ao último. Sem perfis → `failed(NoProfiles, "Nenhum perfil criado.")`, sem mudar nada. Sucesso: `changed:true`, `resolved: Some(id)`. `CycleProfile` no executor usa este método.

- [ ] **Step 1: Testes** (RF-04, RF-05, RF-14): `cycle_wraps_after_last` (3 perfis, 4 chamadas → 1, 2, 3, 1); `cycle_prev_wraps_to_last`; `deleted_current_profile_goes_to_first` (ativar o 2º, `delete_profile`, ciclar → 1º); `no_profiles_is_a_failure_that_changes_nothing`; `cycle_does_not_persist_cursor` (novo `Engine` sobre o mesmo diretório começa no primeiro).
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test engine::` → verde.
- [ ] **Step 5: Commit** `feat: ação de ciclar perfis`

## Task 19: A10, próxima saída padrão

**Files:**
- Modify: `src-tauri/src/engine.rs`, `src-tauri/src/actions.rs`

**Interfaces:**
- Consumes: `Engine::visible_outputs` (Task 2), `Engine::set_default_device(&self, id: &DeviceId, flow: Flow) -> Result<(), AudioError>`.
- Produces: `Engine::act_cycle_output(&self, dir: Direction) -> ActionResult`. Lista = `visible_outputs()`. Menos de 2 → `failed(NotEnoughOutputs, "É preciso ao menos duas saídas.")`. A atual é a de `is_default`; se a padrão não estiver na lista (oculta), `Next` vai à primeira. Troca por `set_default_device(.., Flow::Render)`; `Gone`/`Unsupported` viram `audio_failure`. `resolved` é o id escolhido.

- [ ] **Step 1: Testes** (RF-12, RF-14, RNF-05): `cycle_output_skips_hidden` (3 saídas, uma oculta: o ciclo passa pelas 2 visíveis e volta); `single_output_does_nothing` (nenhuma chamada em `FakeDefaults`); `cycle_output_when_default_is_hidden_goes_to_first`; `cycle_output_survives_gone_device`.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Rodar.** `cargo test engine::` → verde.
- [ ] **Step 5: Commit** `feat: ação de próxima saída padrão`

## Task 20: padrão `Ctrl+Alt+P`, catálogo da tela e aceite final

**Files:**
- Modify: `src-tauri/src/bindings.rs`, `src/screens/Settings.tsx`, `docs/SPEC.md`, `docs/sonar-system-design.md`

- [ ] **Step 1: Testes:** `default_bindings_include_cycle_profile_on_ctrl_alt_p`; Vitest: o seletor de ação da tela lista `cycle_profile` e `cycle_default_output`.
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar:** acrescentar `Ctrl+Alt+P` → `CycleProfile{Next}` em `default_bindings()` (sem alterar a migração de quem já migrou) e as duas ações no seletor.
- [ ] **Step 4: Aceite da versão (SPEC §8):** A2, A3, A4, A6, A7, A8, A9 e A10 rodam por atalho e por IPC com o mesmo resultado (um teste por ação compara `Engine::execute` com o caminho do dispatcher); `cargo test` ≥ 93 e `bun run test` ≥ 122; `bun run build` sem avisos; checklist §9.3 completo no Win11 26300 (inclui ciclar 3 perfis, apagar o atual e ciclar de novo, ciclar saídas com uma oculta); migração testada com `settings.json` real; S4 registrado no design; nada da §1 "Não entra" foi implementado.
- [ ] **Step 5:** Revisão final (Sonnet 5.5) do branch inteiro contra o SPEC. Marcar os checkboxes da §8 e o status F0 a F3 no §7. PR da F3 com prints. Ao fim da sessão, registrar o log de sessão no vault (`D:\meu2cerebro`), conforme o `CLAUDE.md` do autor.

---

## Self-review

- **Cobertura do SPEC:** RF-01 → T1; RF-02 → T7, T8; RF-03 → T6; RF-04/05 → T18; RF-06/07 → T3; RF-08/09 → T4, T7; RF-10 → T5; RF-11 → T15; RF-12 → T19; RF-13 → T10 (manual); RF-14 → T3, T4, T18, T19; RF-15 → T13 (com desvio registrado); RF-16 → T11, T14; RF-17 → T13, T14; RF-18 → **gap corrigido abaixo**; RF-19 → T12; RF-20 → T14 e linha base. RNF-01 → T8; RNF-02 → T7; RNF-03 → revisão de fase; RNF-04 → T3; RNF-05 → T3, T19; RNF-06 → T12; RNF-07 → revisão; RNF-08 → T1; RNF-09 → T10. S4 → T9; permissão Tauri → T8; desenho da tela → T16.
- **RF-18 (binding aponta para alvo apagado):** coberto por T6 (`profile_not_found`), T5/T15 (`invalid_exe`, dispositivo ausente guardado) e pela regra de `save_bindings` (T14) de **nunca remover binding sozinho**. Teste adicional na T14: `binding_to_deleted_profile_is_kept_and_action_fails` (apagar o perfil, executar a ação, `ok:false` e o binding continua em `settings().bindings`).
- **Consistência de tipos:** `ActionResult`/`ErrorCode`/`ActionExecutor` (T1) são os mesmos usados em T3 a T7, T15, T18, T19; `MuteMode`/`VolumeMode` (T3/T4) só aparecem no Engine; `Binding` (T8) é o de T11 a T14 e T16; `BindingStatus` (T13) é o de T14 e T16.
- **Proporção:** o plano tem ~470 linhas para um SPEC de ~230; corpos de função ficam fora, só assinaturas, testes e valores do SPEC.
