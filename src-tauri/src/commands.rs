//! Comandos do Tauri: camada fina sobre o `Engine`. Quem tem erro devolve `String` com
//! uma frase curta em português. Os nomes são os que a interface chama (`src/api.ts`).

use crate::apps::{self, RunningApp};
use crate::audio::types::{AudioError, DeviceId, Flow};
use crate::engine::Engine;
use crate::profiles::{NameError, Profile};
use crate::shell;
use crate::state::{AppState, Settings};
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

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
pub fn set_blur_hide_suspended(suspended: bool) {
    shell::set_blur_hide_suspended(suspended);
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

#[tauri::command(async)]
pub fn save_settings(engine: Eng<'_>, settings: Settings) {
    engine.save_settings(settings);
}
