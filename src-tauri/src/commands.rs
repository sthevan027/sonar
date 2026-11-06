//! Comandos do Tauri: camada fina sobre o `Engine`. Quem tem erro devolve `String` com
//! uma frase curta em português. Os nomes são os que a interface chama (`src/api.ts`).

use crate::actions::{Action, ActionExecutor, ActionResult};
use crate::apps::{self, RunningApp};
use crate::audio::types::{AudioError, DeviceId, Flow};
use crate::engine::Engine;
use crate::profiles::{NameError, Profile};
use crate::shell;
use crate::state::{AppState, Settings};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, State};

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

/// Aplica atalho e "Iniciar com o Windows" antes de gravar. Falha devolve a frase para a tela
/// e nada é gravado.
#[tauri::command(async)]
pub fn save_settings(app: AppHandle, engine: Eng<'_>, settings: Settings) -> Result<(), String> {
    shell::apply_settings(&app, &engine.settings(), &settings)?;
    engine.save_settings(settings);
    Ok(())
}

/// Executa uma ação vinda da interface, direto pelo `Engine`.
///
/// O comando **não** passa pela fila: a fila existe para quem produz de fora (atalho,
/// bandeja) e não pode esperar. Aqui a interface espera o resultado, então a ação roda na
/// hora e devolve o que o executor devolveu.
///
/// **A trava `apply` nunca é segurada aqui.** Ela é um `Mutex` do std (não reentrante) e
/// pertence ao `Engine`: cada `act_*` a segura por conta própria.
#[tauri::command(async)]
pub fn dispatch_action(engine: Eng<'_>, action: Action) -> ActionResult {
    executa(&engine, action)
}

/// O núcleo do comando: o executor decide, o comando só repassa o resultado.
///
/// Camada fina de propósito — é o que o comando `dispatch_action` é.
fn executa(engine: &Engine, action: Action) -> ActionResult {
    engine.execute(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::default_device::WindowsDefaultDevice;
    use crate::audio::devices::WindowsAudioSource;
    use crate::audio::endpoint::WindowsEndpoint;
    use crate::audio::policy::WindowsRouteBackend;
    use crate::store::Store;

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

    #[test]
    fn dispatch_action_returns_the_executor_result() {
        // O comando é uma camada fina: devolve exatamente o que o executor devolveu,
        // sem trocar nada pelo caminho.
        let dir = tempfile::tempdir().unwrap();
        let engine = motor(dir.path());
        let acao = Action::TogglePanel;

        let esperado = engine.execute(acao.clone());
        let obtido = executa(&engine, acao);

        assert_eq!(obtido, esperado, "o comando alterou o resultado do executor");
        assert!(!obtido.ok, "o painel ainda não é do `Engine`: a ação não está disponível");
    }

    #[test]
    fn acao_desconhecida_e_rejeitada_pelo_ipc() {
        // O tipo da ação vem da interface: um tipo desconhecido não vira ação.
        let erro = serde_json::from_str::<Action>(r#"{"type":"mute_everything"}"#).unwrap_err();
        assert!(erro.to_string().contains("mute_everything"));
    }
}
