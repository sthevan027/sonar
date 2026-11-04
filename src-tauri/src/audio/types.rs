//! Tipos compartilhados do roteamento de áudio por app.

use serde::{Deserialize, Serialize};

/// Sentido do fluxo de áudio: saída (render) ou entrada (capture).
/// Na serialização vira `"render"` / `"capture"`, como a interface espera.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    Render,
    Capture,
}

/// Id puro do endpoint de áudio, no formato que o `MMDevice` devolve
/// (ex.: `{0.0.0.00000000}.{2329d0e9-306a-408e-9c4b-4061e553cdb4}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceId(pub String);

/// Backend que grava e lê a rota de áudio de um processo.
pub trait RouteBackend: Send + Sync {
    fn set_route(&self, pid: u32, flow: Flow, device: &DeviceId) -> Result<(), AudioError>;
    fn clear_route(&self, pid: u32, flow: Flow) -> Result<(), AudioError>;
    fn get_route(&self, pid: u32, flow: Flow) -> Result<Option<DeviceId>, AudioError>;
}

/// Endpoint de áudio (saída ou entrada), conectado ou não.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: DeviceId,
    pub flow: Flow,
    /// Nome amigável (`PKEY_Device_FriendlyName`).
    pub name: String,
    /// Verdadeiro só quando o estado do endpoint é ACTIVE.
    pub connected: bool,
    /// Padrão do Windows para o papel multimídia (`GetDefaultAudioEndpoint`).
    pub is_default: bool,
}

/// Sessão de áudio de um processo num endpoint.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AudioSession {
    pub pid: u32,
    /// Nome do executável em minúsculo, só o nome do arquivo (ex.: `spotify.exe`).
    pub exe: String,
    pub flow: Flow,
    /// Endpoint cujo gerenciador de sessões devolveu esta sessão.
    pub device: DeviceId,
    /// Verdadeiro quando o estado da sessão é ACTIVE.
    pub active: bool,
    /// Volume mestre, de 0.0 a 1.0.
    pub volume: f32,
    pub muted: bool,
}

/// Leitura de dispositivos e sessões do sistema.
pub trait AudioSource: Send + Sync {
    fn devices(&self) -> Result<Vec<Device>, AudioError>;
    fn sessions(&self) -> Result<Vec<AudioSession>, AudioError>;
}

/// Notificação emitida por `watch` quando o estado de áudio muda.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioEvent {
    /// Dispositivo adicionado, removido, com estado alterado ou padrão trocado.
    DevicesChanged,
    /// Surgiu uma sessão de áudio nova.
    SessionsChanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// Variante do Windows não suportada, ou ativação da API falhou.
    Unsupported(String),
    /// O processo ou dispositivo não existe mais.
    Gone,
    /// HRESULT devolvido pelo Windows.
    Os(i32),
}
