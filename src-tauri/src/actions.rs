//! Ações globais: o contrato único entre atalhos, bandeja, interface e a API local.
//! Uma ação entra como JSON (`{"type": "...", ...}`), é executada por um `ActionExecutor`
//! e devolve um `ActionResult` com a mesma forma para quem chamou.

use serde::{Deserialize, Serialize};

use crate::audio::types::AudioError;

/// Sentido de uma troca circular (próximo ou anterior).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Next,
    Prev,
}

/// Ação pedida por um atalho, pela bandeja ou pela API local.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Mostra ou esconde o painel.
    TogglePanel,
    /// Aplica o perfil salvo com este id.
    ActivateProfile { profile_id: String },
    /// Passa para o perfil seguinte ou anterior.
    CycleProfile { direction: Direction },
    /// Liga ou desliga o mudo da saída padrão.
    SetOutputMute { muted: bool },
    /// Inverte o mudo da saída padrão.
    ToggleOutputMute,
    /// Soma `delta` ao volume da saída padrão (de 0.0 a 1.0, o resultado é limitado).
    StepOutputVolume { delta: f32 },
    /// Define o volume da saída padrão (de 0.0 a 1.0).
    SetOutputVolume { value: f32 },
    /// Liga ou desliga o mudo de um app, pelo nome do executável (ex.: `spotify.exe`).
    SetAppMute { exe: String, muted: bool },
    /// Manda um app para um dispositivo; `None` volta ao padrão do sistema.
    MoveApp { exe: String, device: Option<String> },
    /// Troca a saída padrão do Windows pela seguinte ou anterior.
    CycleDefaultOutput { direction: Direction },
}

/// Motivo de uma falha, em forma de código estável para quem chamou.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ProfileNotFound,
    NoProfiles,
    NoOutputDevice,
    NotEnoughOutputs,
    DeviceGone,
    Unsupported,
    OsError,
    InvalidExe,
    InvalidValue,
    QueueFull,
}

/// Resultado de uma ação. Sempre serializa com as cinco chaves, mesmo quando vazias.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionResult {
    pub ok: bool,
    /// Verdadeiro quando o estado mudou de fato (um mudo já ligado não muda nada).
    pub changed: bool,
    /// O que a ação resolveu, quando faz sentido (ex.: id do perfil ou do dispositivo escolhido).
    pub resolved: Option<String>,
    pub error_code: Option<ErrorCode>,
    /// Frase curta, em português, para mostrar ao usuário.
    pub message: Option<String>,
}

impl ActionResult {
    /// Ação concluída. `changed` diz se o estado mudou.
    pub fn done(changed: bool, resolved: Option<String>) -> ActionResult {
        ActionResult {
            ok: true,
            changed,
            resolved,
            error_code: None,
            message: None,
        }
    }

    /// Ação recusada ou falha, com o código e a frase para o usuário.
    pub fn failed(code: ErrorCode, message: &str) -> ActionResult {
        ActionResult {
            ok: false,
            changed: false,
            resolved: None,
            error_code: Some(code),
            message: Some(message.to_string()),
        }
    }
}

/// Converte uma falha de áudio em resultado. Nunca expõe o HRESULT em hexadecimal.
pub fn audio_failure(err: &AudioError) -> ActionResult {
    match err {
        AudioError::Gone => ActionResult::failed(
            ErrorCode::DeviceGone,
            "O dispositivo ou o app não está mais disponível.",
        ),
        AudioError::Unsupported(_) => ActionResult::failed(
            ErrorCode::Unsupported,
            "Esta versão do Windows não suporta a operação.",
        ),
        AudioError::Os(_) => {
            ActionResult::failed(ErrorCode::OsError, "O Windows recusou a operação.")
        }
    }
}

/// Executa ações. Implementado pelo roteador que fala com o sistema de áudio.
pub trait ActionExecutor: Send + Sync {
    fn execute(&self, action: Action) -> ActionResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_action_variant() {
        let cases: Vec<(&str, Action)> = vec![
            (r#"{"type":"toggle_panel"}"#, Action::TogglePanel),
            (
                r#"{"type":"activate_profile","profile_id":"jogos"}"#,
                Action::ActivateProfile {
                    profile_id: "jogos".to_string(),
                },
            ),
            (
                r#"{"type":"cycle_profile","direction":"next"}"#,
                Action::CycleProfile {
                    direction: Direction::Next,
                },
            ),
            (
                r#"{"type":"set_output_mute","muted":true}"#,
                Action::SetOutputMute { muted: true },
            ),
            (r#"{"type":"toggle_output_mute"}"#, Action::ToggleOutputMute),
            (
                r#"{"type":"step_output_volume","delta":0.05}"#,
                Action::StepOutputVolume { delta: 0.05 },
            ),
            (
                r#"{"type":"set_output_volume","value":0.5}"#,
                Action::SetOutputVolume { value: 0.5 },
            ),
            (
                r#"{"type":"set_app_mute","exe":"spotify.exe","muted":false}"#,
                Action::SetAppMute {
                    exe: "spotify.exe".to_string(),
                    muted: false,
                },
            ),
            (
                r#"{"type":"move_app","exe":"chrome.exe","device":null}"#,
                Action::MoveApp {
                    exe: "chrome.exe".to_string(),
                    device: None,
                },
            ),
            (
                r#"{"type":"move_app","exe":"chrome.exe","device":"{0.0.0.00000000}.{abc}"}"#,
                Action::MoveApp {
                    exe: "chrome.exe".to_string(),
                    device: Some("{0.0.0.00000000}.{abc}".to_string()),
                },
            ),
            (
                r#"{"type":"cycle_default_output","direction":"prev"}"#,
                Action::CycleDefaultOutput {
                    direction: Direction::Prev,
                },
            ),
        ];

        for (json, expected) in cases {
            let parsed: Action = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("falhou ao ler {json}: {e}"));
            assert_eq!(parsed, expected, "JSON: {json}");
        }
    }

    #[test]
    fn unknown_action_type_is_rejected() {
        let parsed = serde_json::from_str::<Action>(r#"{"type":"mute_everything"}"#);
        assert!(parsed.is_err());
    }

    #[test]
    fn action_result_json_shape_is_pinned() {
        let json = serde_json::to_string(&ActionResult::done(false, None)).unwrap();
        assert_eq!(
            json,
            r#"{"ok":true,"changed":false,"resolved":null,"error_code":null,"message":null}"#
        );
    }

    #[test]
    fn os_failure_message_has_no_hex() {
        let result = audio_failure(&AudioError::Os(-2147024891));
        let message = result.message.expect("falha do Windows precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
    }
}
