//! Tipos compartilhados do roteamento de áudio por app.

/// Sentido do fluxo de áudio: saída (render) ou entrada (capture).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Render,
    Capture,
}

/// Id puro do endpoint de áudio, no formato que o `MMDevice` devolve
/// (ex.: `{0.0.0.00000000}.{2329d0e9-306a-408e-9c4b-4061e553cdb4}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceId(pub String);

/// Backend que grava e lê a rota de áudio de um processo.
pub trait RouteBackend {
    fn set_route(&self, pid: u32, flow: Flow, device: &DeviceId) -> Result<(), AudioError>;
    fn clear_route(&self, pid: u32, flow: Flow) -> Result<(), AudioError>;
    fn get_route(&self, pid: u32, flow: Flow) -> Result<Option<DeviceId>, AudioError>;
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
