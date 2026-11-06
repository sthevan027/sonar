//! Comandos do Tauri: camada fina sobre o `Engine`. Quem tem erro devolve `String` com
//! uma frase curta em português. Os nomes são os que a interface chama (`src/api.ts`).

use crate::actions::{Action, ActionExecutor, ActionResult};
use crate::apps::{self, RunningApp};
use crate::audio::types::{AudioError, DeviceId, Flow};
use crate::bindings::{self, Binding};
use crate::engine::Engine;
use crate::hotkeys::{self, BindingStatusEntry};
use crate::profiles::{NameError, Profile};
use crate::shell::{self, ShortcutRegistrar};
use crate::state::{AppState, Settings};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

/// Evento com o snapshot completo (`AppState`), emitido pelo notificador do `Engine`.
pub const STATE_CHANGED: &str = "state-changed";
/// Evento com o pico do microfone padrão, cerca de 15 vezes por segundo com o painel aberto.
pub const MIC_LEVEL: &str = "mic-level";

/// Payload de `mic-level`.
#[derive(Debug, Clone, Serialize)]
pub struct MicLevel {
    pub id: String,
    pub peak: f32,
}

type Eng<'a> = State<'a, Arc<Engine>>;

fn audio_error(err: AudioError) -> String {
    match err {
        AudioError::Unsupported(_) => "Este Windows não permite essa operação.".into(),
        AudioError::Gone => "O dispositivo ou o app não está mais disponível.".into(),
        AudioError::Os(code) => format!("O Windows recusou a operação (código {code:#010x})."),
    }
}

fn name_error(err: NameError) -> String {
    match err {
        NameError::TooLong => "O nome pode ter no máximo 20 caracteres.".into(),
        NameError::ControlChar => "O nome não pode ter caracteres de controle.".into(),
    }
}

#[tauri::command(async)]
pub fn get_state(engine: Eng<'_>) -> AppState {
    engine.state()
}

#[tauri::command(async)]
pub fn set_app_route(engine: Eng<'_>, exe: String, flow: Flow, device: Option<String>) {
    engine.set_app_route(&exe, flow, device.map(DeviceId));
}

#[tauri::command(async)]
pub fn set_session_volume(
    engine: Eng<'_>,
    pid: u32,
    flow: Flow,
    volume: f32,
) -> Result<(), String> {
    engine
        .set_session_volume(pid, flow, volume)
        .map_err(audio_error)
}

#[tauri::command(async)]
pub fn set_session_muted(
    engine: Eng<'_>,
    pid: u32,
    flow: Flow,
    muted: bool,
) -> Result<(), String> {
    engine
        .set_session_muted(pid, flow, muted)
        .map_err(audio_error)
}

#[tauri::command(async)]
pub fn set_device_volume(engine: Eng<'_>, id: String, volume: f32) -> Result<(), String> {
    engine
        .set_device_volume(&DeviceId(id), volume)
        .map_err(audio_error)
}

#[tauri::command(async)]
pub fn set_device_muted(engine: Eng<'_>, id: String, muted: bool) -> Result<(), String> {
    engine
        .set_device_muted(&DeviceId(id), muted)
        .map_err(audio_error)
}

#[tauri::command(async)]
pub fn set_default_device(engine: Eng<'_>, id: String, flow: Flow) -> Result<(), String> {
    engine
        .set_default_device(&DeviceId(id), flow)
        .map_err(audio_error)
}

#[tauri::command(async)]
pub fn update_device_meta(
    engine: Eng<'_>,
    id: String,
    name: String,
    icon: Option<String>,
    hidden: bool,
) -> Result<(), String> {
    engine
        .update_device_meta(&DeviceId(id), &name, icon, hidden)
        .map_err(name_error)
}

#[tauri::command(async)]
pub fn save_profile(engine: Eng<'_>, profile: Profile) -> Result<(), String> {
    engine.save_profile(profile).map_err(name_error)
}

#[tauri::command(async)]
pub fn delete_profile(engine: Eng<'_>, id: String) {
    engine.delete_profile(&id);
}

#[tauri::command(async)]
pub fn activate_profile(engine: Eng<'_>, id: String) -> Result<(), String> {
    if engine.activate_profile(&id) {
        Ok(())
    } else {
        Err("Perfil não encontrado.".into())
    }
}

#[tauri::command(async)]
pub fn get_settings(engine: Eng<'_>) -> Settings {
    engine.settings()
}

/// Apps com janela aberta, para o seletor do editor de perfil.
#[tauri::command(async)]
pub fn list_running_apps() -> Vec<RunningApp> {
    contained("lista de apps", apps::list_running_apps)
}

/// Ícone do app como data URL PNG; `null` quando não há (o app não está aberto, por exemplo).
#[tauri::command(async)]
pub fn app_icon(exe: String) -> Option<String> {
    contained("ícone do app", || apps::app_icon(&exe))
}

/// Liga/desliga o esconder-ao-perder-foco do painel (true enquanto o diálogo de arquivo abre).
#[tauri::command(async)]
pub fn set_blur_hide_suspended(app: AppHandle, suspended: bool) {
    shell::set_blur_hide_suspended(&app, suspended);
}

/// Roda `f` segurando pânico: um erro nos ícones ou na lista não derruba o app. Devolve o
/// padrão do tipo quando ocorre pânico (em build de release `panic = "unwind"` faz isso valer).
fn contained<T: Default>(label: &str, f: impl FnOnce() -> T) -> T {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(value) => value,
        Err(_) => {
            eprintln!("sonar: {label} falhou com pânico; seguindo");
            T::default()
        }
    }
}

/// O que chega pelo IPC `save_settings`.
///
/// O campo `bindings` é opcional **de propósito**: ausente (ou `null`) quer dizer **"não
/// mexe nos atalhos"** (Ruling 14). Sem essa distinção, um cliente que manda só
/// `{shortcut, autostart}` apagaria todos os atalhos do usuário — em silêncio, porque a
/// migração versionada não repara: o arquivo já está na versão 2.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SettingsPayload {
    /// O atalho pedido. Ausente = não muda.
    #[serde(default)]
    pub shortcut: Option<String>,
    /// "Iniciar com o Windows". Ausente = não muda.
    #[serde(default)]
    pub autostart: Option<bool>,
    /// A lista inteira de atalhos. Ausente = **não mexe** nos atalhos.
    #[serde(default)]
    pub bindings: Option<Vec<Binding>>,
}

/// A lista de atalhos que vale depois do que chegou pelo IPC.
///
/// `bindings` ausente não mexe na lista (Ruling 14). O atalho legado, quando é diferente
/// do atalho do binding `toggle_panel`, vira o binding `toggle_panel` — é assim que a
/// tela de configurações continua funcionando (RF-20).
fn lista_efetiva(anterior: &Settings, payload: &SettingsPayload) -> Vec<Binding> {
    let mut lista = payload
        .bindings
        .clone()
        .unwrap_or_else(|| anterior.bindings.clone());
    if let Some(atalho) = payload.shortcut.as_deref() {
        let atalho = atalho.trim();
        if !atalho.is_empty() && atalho != hotkeys::espelho_do_painel(&lista) {
            lista = hotkeys::unify_shortcut(lista, atalho);
        }
    }
    lista
}

/// O resultado de aplicar uma lista de atalhos.
#[derive(Debug)]
struct Aplicacao {
    /// As configurações a gravar.
    settings: Settings,
    /// O estado de cada binding, na ordem da lista.
    status: Vec<BindingStatusEntry>,
    /// A lista que ficou registrada de verdade (a que o handler do atalho consulta).
    registrados: Vec<Binding>,
}

/// Valida a lista, registra os atalhos e devolve o que deve ser gravado.
///
/// A validação é da lista **inteira** e vem antes de qualquer registro: um erro devolve a
/// frase e não muda nada — nem no Windows, nem no disco.
fn aplicar_lista(
    registrar: &dyn ShortcutRegistrar,
    atual: &[Binding],
    lista: Vec<Binding>,
    autostart: bool,
) -> Result<Aplicacao, String> {
    if let Err((_, erro)) = bindings::validate_bindings(&lista) {
        return Err(erro.message().to_string());
    }
    let (status, registrados) = hotkeys::apply_bindings(registrar, atual, &lista);
    let entradas = hotkeys::entradas(&lista, &status);
    let espelho = hotkeys::espelho_do_painel(&lista);
    Ok(Aplicacao {
        settings: Settings {
            shortcut: espelho,
            autostart,
            bindings: lista,
        },
        status: entradas,
        registrados,
    })
}

/// Guarda o resultado do registro e avisa o painel do estado de cada binding.
fn publica_bindings(
    app: &AppHandle,
    registrados: Vec<Binding>,
    status: Vec<BindingStatusEntry>,
) -> Vec<BindingStatusEntry> {
    hotkeys::definir(registrados, status.clone());
    let _ = app.emit(hotkeys::BINDINGS_STATUS, &status);
    status
}

/// Aplica atalho e "Iniciar com o Windows" antes de gravar. Falha devolve a frase para a tela
/// e nada é gravado.
///
/// O `bindings` do payload é opcional: ausente = **não mexe** nos atalhos (Ruling 14). O
/// `shortcut` gravado é o espelho do acelerador do primeiro binding `toggle_panel` — as
/// duas coisas não podem divergir.
#[tauri::command(async)]
pub fn save_settings(
    app: AppHandle,
    engine: Eng<'_>,
    settings: SettingsPayload,
) -> Result<(), String> {
    let anterior = engine.settings();
    let lista = lista_efetiva(&anterior, &settings);
    let autostart = settings.autostart.unwrap_or(anterior.autostart);
    // "Iniciar com o Windows" vem antes dos atalhos: se ele falhar, nenhum atalho muda.
    shell::set_autostart(autostart)?;
    let aplicacao = aplicar_lista(
        &shell::registrador(&app),
        &hotkeys::registrados(),
        lista,
        autostart,
    )?;
    let Aplicacao {
        settings,
        status,
        registrados,
    } = aplicacao;
    engine.save_settings(settings);
    publica_bindings(&app, registrados, status);
    Ok(())
}

/// Grava a lista inteira de atalhos: valida, registra e só então persiste.
///
/// Erro de validação devolve a frase em português e **não grava nada**. Um atalho que o
/// Windows recusa não impede os outros de ficarem registrados: ele volta como
/// "indisponível", e o atalho anterior continua valendo.
#[tauri::command(async)]
pub fn save_bindings(
    app: AppHandle,
    engine: Eng<'_>,
    bindings: Vec<Binding>,
) -> Result<Vec<BindingStatusEntry>, String> {
    let anterior = engine.settings();
    let aplicacao = aplicar_lista(
        &shell::registrador(&app),
        &hotkeys::registrados(),
        bindings,
        anterior.autostart,
    )?;
    let Aplicacao {
        settings,
        status,
        registrados,
    } = aplicacao;
    engine.save_settings(settings);
    Ok(publica_bindings(&app, registrados, status))
}

/// O estado de registro de cada binding: uma entrada por binding, na ordem da lista.
#[tauri::command(async)]
pub fn get_binding_status() -> Vec<BindingStatusEntry> {
    hotkeys::status()
}

/// Os atalhos padrão do app. O backend é a fonte da verdade: o TypeScript não guarda cópia
/// (Ruling 42), senão as duas listas divergem.
#[tauri::command(async)]
pub fn get_default_bindings() -> Vec<Binding> {
    bindings::default_bindings()
}

/// Executa uma ação vinda da interface, direto pelo executor do atalho.
///
/// O comando **não** passa pela fila: a fila existe para quem produz de fora (atalho,
/// bandeja) e não pode esperar. Aqui a interface espera o resultado, então a ação roda na
/// hora e devolve o que o executor devolveu.
///
/// O executor é o mesmo do atalho (`AppExecutor`): assim o caminho por IPC e o caminho por
/// atalho dão **o mesmo resultado** (SPEC §8) e o `toggle_panel` funciona nos dois.
///
/// **A trava `apply` nunca é segurada aqui.** Ela é um `Mutex` do std (não reentrante) e
/// pertence ao `Engine`: cada `act_*` a segura por conta própria.
#[tauri::command(async)]
pub fn dispatch_action(app: AppHandle, engine: Eng<'_>, action: Action) -> ActionResult {
    let executor = crate::action_runtime::AppExecutor::new(engine.inner().clone(), app);
    executor.execute(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ErrorCode;
    use crate::audio::default_device::WindowsDefaultDevice;
    use crate::audio::devices::WindowsAudioSource;
    use crate::audio::endpoint::WindowsEndpoint;
    use crate::audio::policy::WindowsRouteBackend;
    use crate::hotkeys::BindingStatus;
    use crate::store::Store;
    use std::cell::RefCell;

    /// Motor de verdade, com os backends do Windows: a ação escolhida não toca no sistema
    /// (o `Engine` ainda não sabe mexer no painel), o que mantém o teste hermético.
    fn motor(dir: &std::path::Path) -> Engine {
        Engine::new(
            Arc::new(WindowsAudioSource),
            Arc::new(WindowsRouteBackend::new()),
            Arc::new(WindowsEndpoint),
            Arc::new(WindowsDefaultDevice),
            Store::open(dir.to_path_buf()),
            Arc::new(|_state: &AppState| {}),
        )
    }

    /// Registro falso: a combinação ocupada (`Ctrl+Alt+O`) é recusada; o resto é aceito.
    #[derive(Default)]
    struct FakeRegistrar {
        calls: RefCell<Vec<String>>,
    }

    impl ShortcutRegistrar for FakeRegistrar {
        fn register(&self, accelerator: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("register {accelerator}"));
            if accelerator == "Ctrl+Alt+O" {
                return Err(format!("Atalho inválido: {accelerator}."));
            }
            Ok(())
        }

        fn unregister(&self, accelerator: &str) {
            self.calls.borrow_mut().push(format!("unregister {accelerator}"));
        }
    }

    fn binding(accelerator: &str, action: Action) -> Binding {
        Binding {
            accelerator: accelerator.to_string(),
            action,
        }
    }

    fn settings(bindings: Vec<Binding>) -> Settings {
        Settings {
            shortcut: hotkeys::espelho_do_painel(&bindings),
            autostart: false,
            bindings,
        }
    }

    // ---------- o payload de `save_settings` (Ruling 14) ----------

    #[test]
    fn old_settings_payload_without_bindings_still_deserializes() {
        // O payload antigo (sem `bindings`) deserializa com o campo **ausente** — e
        // "ausente" quer dizer "não mexe nos atalhos", não "lista vazia".
        let payload: SettingsPayload =
            serde_json::from_str(r#"{"shortcut":"Super+Alt+V","autostart":false}"#).unwrap();

        assert!(
            payload.bindings.is_none(),
            "campo ausente é ausente, não lista vazia"
        );
        assert_eq!(payload.shortcut.as_deref(), Some("Super+Alt+V"));
        assert_eq!(payload.autostart, Some(false));
    }

    #[test]
    fn payload_without_bindings_keeps_the_bindings_of_the_user() {
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        let antes = settings(vec![
            binding("Super+Alt+V", Action::TogglePanel),
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
        ]);
        engine.save_settings(antes.clone());

        let payload: SettingsPayload =
            serde_json::from_str(r#"{"shortcut":"Super+Alt+V","autostart":false}"#).unwrap();
        let lista = lista_efetiva(&engine.settings(), &payload);
        assert_eq!(
            lista, antes.bindings,
            "os atalhos do usuário não podem sumir por causa de um payload antigo"
        );

        let aplicacao = aplicar_lista(
            &FakeRegistrar::default(),
            &antes.bindings,
            lista,
            false,
        )
        .unwrap();
        engine.save_settings(aplicacao.settings);
        assert_eq!(
            engine.settings().bindings,
            antes.bindings,
            "a lista gravada continua igual"
        );
    }

    #[test]
    fn payload_with_some_bindings_replaces_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        let antes = settings(vec![binding("Ctrl+Alt+M", Action::ToggleOutputMute)]);
        engine.save_settings(antes);

        let nova = vec![
            binding("Ctrl+Alt+K", Action::TogglePanel),
            binding("Ctrl+Alt+Up", Action::StepOutputVolume { delta: 0.05 }),
        ];
        let payload = SettingsPayload {
            shortcut: None,
            autostart: None,
            bindings: Some(nova.clone()),
        };

        let lista = lista_efetiva(&engine.settings(), &payload);
        let aplicacao = aplicar_lista(&FakeRegistrar::default(), &[], lista, false).unwrap();
        engine.save_settings(aplicacao.settings);

        assert_eq!(
            engine.settings().bindings,
            nova,
            "a lista persistida é exatamente a nova, na ordem dada"
        );
    }

    #[test]
    fn save_settings_with_new_shortcut_updates_toggle_panel_binding() {
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        let antes = settings(vec![
            binding("Super+Alt+V", Action::TogglePanel),
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
        ]);
        engine.save_settings(antes);

        let payload = SettingsPayload {
            shortcut: Some("Ctrl+Alt+K".to_string()),
            autostart: None,
            bindings: None,
        };
        let lista = lista_efetiva(&engine.settings(), &payload);
        let aplicacao = aplicar_lista(&FakeRegistrar::default(), &[], lista, false).unwrap();
        engine.save_settings(aplicacao.settings);

        let depois = engine.settings();
        let painel = depois
            .bindings
            .iter()
            .find(|b| b.action == Action::TogglePanel)
            .expect("o binding do painel continua na lista");
        assert_eq!(painel.accelerator, "Ctrl+Alt+K", "o atalho do painel mudou");
        assert_eq!(
            depois.shortcut, "Ctrl+Alt+K",
            "o atalho legado é o espelho do binding do painel"
        );
        assert!(
            depois.bindings.contains(&binding(
                "Ctrl+Alt+M",
                Action::ToggleOutputMute
            )),
            "os outros atalhos continuam na lista"
        );
    }

    #[test]
    fn invalid_list_is_rejected_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        let antes = settings(vec![binding("Ctrl+Alt+M", Action::ToggleOutputMute)]);
        engine.save_settings(antes.clone());
        let arquivo = dir.path().join("settings.json");
        let bytes_antes = std::fs::read(&arquivo).unwrap();

        let duplicada = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding("ctrl+alt+m", Action::TogglePanel),
        ];
        let resultado = aplicar_lista(&FakeRegistrar::default(), &[], duplicada, false);

        assert!(
            resultado.is_err(),
            "a lista com atalho repetido tem de ser recusada"
        );
        assert_eq!(
            resultado.unwrap_err(),
            "Esse atalho já está em uso.",
            "a recusa vem com a frase em português"
        );
        assert_eq!(engine.settings(), antes, "as configurações não podem mudar");
        assert_eq!(
            std::fs::read(&arquivo).unwrap(),
            bytes_antes,
            "o arquivo em disco não pode mudar"
        );
    }

    #[test]
    fn failed_registration_keeps_old_bindings() {
        // O Windows recusa o atalho novo: o atalho anterior continua valendo (não é solto)
        // e o binding continua na lista, marcado como indisponível.
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        let antes = vec![binding("Ctrl+Alt+K", Action::TogglePanel)];
        engine.save_settings(settings(antes.clone()));
        let registrar = FakeRegistrar::default();

        let aplicacao = aplicar_lista(
            &registrar,
            &antes,
            vec![binding("Ctrl+Alt+O", Action::TogglePanel)],
            false,
        )
        .unwrap();

        assert_eq!(
            aplicacao.registrados, antes,
            "o atalho anterior continua registrado: {:?}",
            aplicacao.registrados
        );
        let calls = registrar.calls.borrow().clone();
        assert!(
            !calls.iter().any(|c| c.contains("unregister")),
            "o atalho anterior não pode ser solto: {calls:?}"
        );
        assert_eq!(aplicacao.status.len(), 1, "uma entrada por binding");
        assert!(
            matches!(aplicacao.status[0].status, BindingStatus::Unavailable { .. }),
            "o atalho recusado fica indisponível"
        );
    }

    #[test]
    fn binding_to_deleted_profile_is_kept_and_action_fails() {
        // RF-18: apagar o perfil não apaga o atalho. O que falha é a ação.
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        engine
            .save_profile(Profile {
                id: "jogos".to_string(),
                icon: String::new(),
                name: Some("Jogos".to_string()),
                items: Vec::new(),
            })
            .unwrap();
        let atalho = binding(
            "Ctrl+Alt+P",
            Action::ActivateProfile {
                profile_id: "jogos".to_string(),
            },
        );
        engine.save_settings(settings(vec![atalho.clone()]));

        engine.delete_profile("jogos");

        let resultado = engine.execute(Action::ActivateProfile {
            profile_id: "jogos".to_string(),
        });
        assert!(!resultado.ok, "a ação tem de falhar");
        assert_eq!(resultado.error_code, Some(ErrorCode::ProfileNotFound));
        assert!(
            engine.settings().bindings.contains(&atalho),
            "o binding continua na lista depois do perfil sumir"
        );
    }

    // ---------- o IPC usa o mesmo executor do atalho (Ruling 43) ----------

    #[test]
    fn dispatch_action_uses_the_same_executor_as_the_shortcut() {
        // O `Engine` sozinho não mexe no painel: devolve "não disponível". O executor do
        // atalho (`AppExecutor`) mexe — e é o que o IPC usa agora.
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        let acao = Action::TogglePanel;

        let pelo_engine = engine.execute(acao.clone());
        assert!(!pelo_engine.ok, "o Engine não mexe no painel");

        let executor =
            crate::action_runtime::AppExecutor::com_painel(Arc::new(engine), || true);
        let pelo_ipc = executor.execute(acao);
        assert!(pelo_ipc.ok, "o executor do atalho mexe no painel");
        assert!(pelo_ipc.changed, "o painel mudou de estado");
    }

    #[test]
    fn acao_desconhecida_e_rejeitada_pelo_ipc() {
        // O tipo da ação vem da interface: um tipo desconhecido não vira ação.
        let erro = serde_json::from_str::<Action>(r#"{"type":"mute_everything"}"#).unwrap_err();
        assert!(erro.to_string().contains("mute_everything"));
    }
}
