//! Estado exposto à interface: configurações, metadados de dispositivo e a visão
//! montada pelo `Engine` (`AppState`). Tudo serializável; nada aqui fala com o sistema.

use crate::audio::types::{AudioSession, DeviceId, Flow};
use crate::profiles::Profile;
use crate::rules::Rule;
use serde::{Deserialize, Serialize};

/// Configurações do app (`settings.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub shortcut: String,
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shortcut: "Super+Alt+V".to_string(),
            autostart: false,
        }
    }
}

/// Metadados que o usuário dá a um dispositivo (`devices.json`, por id do endpoint).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceMeta {
    /// Nome personalizado; `None` mostra o nome do Windows.
    pub name: Option<String>,
    pub icon: Option<String>,
    pub hidden: bool,
}

impl DeviceMeta {
    /// Metadado sem nenhum campo preenchido não é guardado: equivale a não existir.
    pub fn is_meaningful(&self) -> bool {
        self.name.is_some() || self.icon.is_some() || self.hidden
    }
}

/// Dispositivo como a interface o vê: endpoint do Windows junto com o metadado.
/// `f32` impede `Eq`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceView {
    pub id: DeviceId,
    pub flow: Flow,
    pub windows_name: String,
    /// Nome personalizado, se houver.
    pub name: Option<String>,
    /// Nome personalizado ou, na falta dele, o nome do Windows.
    pub display_name: String,
    pub icon: Option<String>,
    /// A interface filtra; o estado guarda o dispositivo mesmo assim.
    pub hidden: bool,
    pub connected: bool,
    pub is_default: bool,
    /// Alguma regra aponta para este dispositivo.
    pub has_rules: bool,
    /// Volume mestre do endpoint, de 0.0 a 1.0. `1.0` quando desconectado ou sem leitura.
    pub volume: f32,
    /// Mudo do endpoint. `false` quando desconectado ou sem leitura.
    pub muted: bool,
}

/// Snapshot completo para a interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppState {
    pub devices: Vec<DeviceView>,
    pub sessions: Vec<AudioSession>,
    pub rules: Vec<Rule>,
    pub profiles: Vec<Profile>,
    pub settings: Settings,
    /// Falso quando o backend de roteamento não é suportado neste Windows.
    pub routing_available: bool,
    /// Último perfil ativado nesta execução (não persistido).
    pub last_profile: Option<String>,
    /// Mensagem quando a última gravação em disco falhou; `null` quando está tudo gravado.
    pub save_error: Option<String>,
}
