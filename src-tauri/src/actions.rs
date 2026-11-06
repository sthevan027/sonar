//! Ações globais: o contrato único entre atalhos, bandeja, interface e a API local.
//! Uma ação entra como JSON (`{"type": "...", ...}`), é executada por um `ActionExecutor`
//! e devolve um `ActionResult` com a mesma forma para quem chamou.

use serde::{Deserialize, Serialize};

use crate::audio::types::AudioError;
use crate::engine::{Engine, MuteMode, VolumeMode};

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
    /// Falha interna do app: um bug no executor, não uma recusa do Windows.
    ///
    /// Serializa em `internal` — o espelho TypeScript precisa conhecer esse código.
    Internal,
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

    /// Ação que cumpriu só **parte** do pedido: o estado mudou, mas o pedido não foi
    /// cumprido inteiro.
    ///
    /// `ok` e `changed` são independentes: aqui `ok` é falso (não cumpriu tudo) e `changed`
    /// é o do resultado anterior (mudou de fato). O `resolved` é preservado — o que a
    /// ação resolveu continua valendo.
    ///
    /// Consequência para quem consome: **`ok == false` não implica `changed == false`**.
    /// O painel não pode assumir que uma falha significa "nada mudou".
    pub fn partial(previous: ActionResult, code: ErrorCode, message: &str) -> ActionResult {
        ActionResult {
            ok: false,
            changed: previous.changed,
            resolved: previous.resolved,
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
            "O dispositivo não está mais disponível.",
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

/// Executa as ações no `Engine`: o roteador que fala com o Windows.
///
/// O executor só despacha: cada `act_*` segura `apply` por conta própria. Aqui a trava
/// nunca é segurada — `Mutex` do std não é reentrante, e segurá-la nos dois níveis seria
/// um deadlock garantido.
impl ActionExecutor for Engine {
    fn execute(&self, action: Action) -> ActionResult {
        match action {
            Action::ActivateProfile { profile_id } => self.act_activate_profile(&profile_id),
            Action::SetOutputMute { muted } => self.act_output_mute(MuteMode::Set(muted)),
            Action::ToggleOutputMute => self.act_output_mute(MuteMode::Toggle),
            Action::StepOutputVolume { delta } => self.act_output_volume(VolumeMode::Step(delta)),
            Action::SetOutputVolume { value } => self.act_output_volume(VolumeMode::Set(value)),
            Action::SetAppMute { exe, muted } => self.act_app_mute(&exe, muted),
            // Mover app (Task 15), ciclar perfil (Task 18) e ciclar saída padrão (Task 19).
            Action::MoveApp { exe, device } => self.act_move_app(&exe, device),
            Action::CycleProfile { direction } => self.act_cycle_profile(direction),
            Action::CycleDefaultOutput { direction } => self.act_cycle_output(direction),
            // O painel não é do `Engine`: quem abre a janela é a `AppExecutor` (Task 8).
            // A Task 20 não implementa ação nenhuma.
            Action::TogglePanel => {
                ActionResult::failed(ErrorCode::Unsupported, "Ação ainda não disponível.")
            }
        }
    }
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
    fn failed_action_result_json_shape_is_pinned() {
        // A forma `failed` é a que a interface vai ler no IPC e no evento `action-result`:
        // as mesmas cinco chaves, na mesma ordem, com o código e a frase prontos.
        let json = serde_json::to_string(&ActionResult::failed(
            ErrorCode::OsError,
            "O Windows recusou a operação.",
        ))
        .unwrap();
        assert_eq!(
            json,
            r#"{"ok":false,"changed":false,"resolved":null,"error_code":"os_error","message":"O Windows recusou a operação."}"#
        );
    }

    #[test]
    fn partial_action_result_json_shape_is_pinned() {
        // A forma parcial — "falhou, mas mudou" — é a que o painel vai ler: as mesmas
        // cinco chaves, na mesma ordem, com `ok` falso e `changed` verdadeiro.
        let json = serde_json::to_string(&ActionResult::partial(
            ActionResult::done(true, Some("p1".to_string())),
            ErrorCode::Unsupported,
            "O perfil foi ativado, mas este Windows não permite trocar o dispositivo dos aplicativos.",
        ))
        .unwrap();
        assert_eq!(
            json,
            r#"{"ok":false,"changed":true,"resolved":"p1","error_code":"unsupported","message":"O perfil foi ativado, mas este Windows não permite trocar o dispositivo dos aplicativos."}"#
        );
    }

    #[test]
    fn partial_preserves_changed_and_resolved() {
        let partial = ActionResult::partial(
            ActionResult::done(true, Some("discord.exe".to_string())),
            ErrorCode::DeviceGone,
            "O mudo foi aplicado em parte.",
        );

        // `ok` falso: o pedido não foi cumprido inteiro.
        assert!(!partial.ok);
        // `changed` preservado: o estado mudou de fato.
        assert!(partial.changed);
        // `resolved` preservado: o que a ação resolveu continua valendo.
        assert_eq!(partial.resolved.as_deref(), Some("discord.exe"));
        assert_eq!(partial.error_code, Some(ErrorCode::DeviceGone));
    }

    #[test]
    fn internal_failure_serializes_as_internal() {
        // `internal` é o código novo: o espelho TypeScript da Task 16 tem de lê-lo
        // como uma falha interna do app — e não como `os_error`, que diria que foi
        // o Windows a recusar a operação.
        let json = serde_json::to_string(&ActionResult::failed(
            ErrorCode::Internal,
            "A ação falhou de forma inesperada.",
        ))
        .unwrap();
        assert_eq!(
            json,
            r#"{"ok":false,"changed":false,"resolved":null,"error_code":"internal","message":"A ação falhou de forma inesperada."}"#
        );
    }

    #[test]
    fn gone_failure_message_matches_the_spec() {
        // SPEC §3 (A4/A6/A7): o dispositivo que sumiu tem frase própria.
        let result = audio_failure(&AudioError::Gone);
        assert_eq!(result.error_code, Some(ErrorCode::DeviceGone));
        assert_eq!(
            result.message.as_deref(),
            Some("O dispositivo não está mais disponível.")
        );
    }

    #[test]
    fn os_failure_message_has_no_hex() {
        let result = audio_failure(&AudioError::Os(-2147024891));
        let message = result.message.expect("falha do Windows precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
    }
}
