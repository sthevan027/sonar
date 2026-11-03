//! Sessões de áudio por processo. As funções puras (normalização do nome do executável
//! e filtro de sessões do sistema) ficam fora do bloco `cfg(windows)` para ter teste direto.

#[cfg(windows)]
use std::sync::mpsc::Sender;

#[cfg(windows)]
use windows::core::{implement, Interface, Ref};
#[cfg(windows)]
use windows::Win32::Foundation::CloseHandle;
#[cfg(windows)]
use windows::Win32::Media::Audio::{
    IAudioSessionControl, IAudioSessionControl2, IAudioSessionEnumerator, IAudioSessionManager2,
    IAudioSessionNotification,
    IAudioSessionNotification_Impl, IMMDevice, ISimpleAudioVolume, AudioSessionStateActive,
};
#[cfg(windows)]
use windows::Win32::System::Com::CLSCTX_ALL;
#[cfg(windows)]
use windows::core::PWSTR;
#[cfg(windows)]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

#[cfg(windows)]
use super::devices::{collect_ok, to_audio_error};
#[cfg(windows)]
use super::types::AudioError;
use super::types::{AudioEvent, AudioSession, DeviceId, Flow};

/// Nome usado quando o Windows não deixa ler o caminho do processo (ex.: acesso negado).
pub const UNKNOWN_EXE: &str = "desconhecido.exe";

/// Só o nome do arquivo, em minúsculo: `C:\Program Files\Spotify\Spotify.exe` -> `spotify.exe`.
pub fn normalize_exe(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or("").to_lowercase()
}

/// Remove a sessão de PID 0 (sons do sistema do Windows), que não é um app de verdade.
pub fn keep_real_sessions(sessions: Vec<AudioSession>) -> Vec<AudioSession> {
    sessions.into_iter().filter(|s| s.pid != 0).collect()
}

/// Nome do executável de um processo, normalizado. `UNKNOWN_EXE` quando não dá para ler.
#[cfg(windows)]
pub(super) fn process_exe(pid: u32) -> String {
    if pid == 0 {
        return UNKNOWN_EXE.to_string();
    }
    // SAFETY: OpenProcess só devolve um handle; ele é fechado logo abaixo.
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return UNKNOWN_EXE.to_string();
    };
    // Caminhos longos (até 32767 caracteres) cabem no buffer; o Windows devolve o tamanho usado.
    let mut buf = vec![0u16; 32768];
    let mut len = buf.len() as u32;
    // SAFETY: `buf` tem `len` posições válidas para escrita; `handle` é válido até CloseHandle.
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len)
    }
    .is_ok();
    // SAFETY: handle obtido de OpenProcess acima, fechado uma vez.
    let _ = unsafe { CloseHandle(handle) };
    if !ok {
        return UNKNOWN_EXE.to_string();
    }
    normalize_exe(&String::from_utf16_lossy(&buf[..len as usize]))
}

/// Sessões de um dispositivo (saída ou entrada) via `IAudioSessionManager2`.
/// Inclui sessões inativas; o campo `active` diz o estado de cada uma.
#[cfg(windows)]
pub(super) fn sessions_on(
    device: &IMMDevice,
    device_id: &DeviceId,
    flow: Flow,
) -> Result<Vec<AudioSession>, AudioError> {
    // SAFETY: ativação de interface COM padrão do endpoint; sem parâmetros de ativação.
    let manager: IAudioSessionManager2 =
        unsafe { device.Activate(CLSCTX_ALL, None) }.map_err(to_audio_error)?;
    // SAFETY: chamadas sem ponteiros próprios; o gerenciador está vivo neste escopo.
    let list = unsafe { manager.GetSessionEnumerator() }.map_err(to_audio_error)?;
    let count = unsafe { list.GetCount() }.map_err(to_audio_error)?;
    Ok(collect_ok(
        (0..count).map(|index| session_at(&list, index, device_id, flow)),
    ))
}

/// Uma sessão. Erro em controle, pid ou estado descarta só esta sessão (ver `collect_ok`).
/// Volume ou mudo que não puderem ser lidos caem para 1.0 e falso.
#[cfg(windows)]
fn session_at(
    list: &IAudioSessionEnumerator,
    index: i32,
    device_id: &DeviceId,
    flow: Flow,
) -> Result<AudioSession, AudioError> {
    let control: IAudioSessionControl = unsafe { list.GetSession(index) }.map_err(to_audio_error)?;
    let control2: IAudioSessionControl2 = control.cast().map_err(to_audio_error)?;
    // SAFETY: consultas simples sobre a sessão recém-obtida.
    let pid = unsafe { control2.GetProcessId() }.map_err(to_audio_error)?;
    let state = unsafe { control2.GetState() }.map_err(to_audio_error)?;
    let (level, muted) = volume_of(&control);
    Ok(AudioSession {
        pid,
        exe: process_exe(pid),
        flow,
        device: device_id.clone(),
        active: state == AudioSessionStateActive,
        volume: level,
        muted,
    })
}

/// Volume mestre (0.0..=1.0) e mudo. Falha em qualquer leitura usa 1.0 e falso.
#[cfg(windows)]
fn volume_of(control: &IAudioSessionControl) -> (f32, bool) {
    let Ok(volume) = control.cast::<ISimpleAudioVolume>() else {
        return (1.0, false);
    };
    // SAFETY: consultas simples sobre o controle de volume da sessão.
    let level = unsafe { volume.GetMasterVolume() }.unwrap_or(1.0);
    let muted = unsafe { volume.GetMute() }.map(|m| m.as_bool()).unwrap_or(false);
    (level.clamp(0.0, 1.0), muted)
}

/// Registra o callback de sessões criadas no gerenciador de um dispositivo.
#[cfg(windows)]
pub(super) fn register_session_notifier(
    manager: &IAudioSessionManager2,
    notifier: &IAudioSessionNotification,
) -> Result<(), AudioError> {
    // SAFETY: `notifier` é um objeto COM válido, referenciado pelo chamador enquanto registrado.
    unsafe { manager.RegisterSessionNotification(notifier) }.map_err(to_audio_error)
}

/// Callback de `IAudioSessionNotification`: avisa que surgiu sessão nova.
/// Nunca entra em pânico: erro de envio (receptor descartado) é ignorado.
#[cfg(windows)]
#[implement(IAudioSessionNotification)]
pub(super) struct SessionNotifier {
    pub(super) tx: Sender<AudioEvent>,
}

#[cfg(windows)]
impl IAudioSessionNotification_Impl for SessionNotifier_Impl {
    fn OnSessionCreated(&self, _newsession: Ref<'_, IAudioSessionControl>) -> windows::core::Result<()> {
        let _ = self.tx.send(AudioEvent::SessionsChanged);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(pid: u32) -> AudioSession {
        AudioSession {
            pid,
            exe: "app.exe".into(),
            flow: Flow::Render,
            device: DeviceId("{0.0.0.00000000}.{2329d0e9-306a-408e-9c4b-4061e553cdb4}".into()),
            active: true,
            volume: 1.0,
            muted: false,
        }
    }

    #[test]
    fn exe_name_is_lowercase_file_name_only() {
        assert_eq!(normalize_exe(r"C:\Program Files\Spotify\Spotify.exe"), "spotify.exe");
        assert_eq!(normalize_exe(r"C:\Users\x\AppData\Local\Discord\app-1.0\DISCORD.EXE"), "discord.exe");
        assert_eq!(normalize_exe("notepad.exe"), "notepad.exe");
        assert_eq!(normalize_exe(r"D:/apps/Teams.exe"), "teams.exe");
    }

    #[test]
    fn sessions_with_pid_zero_are_dropped() {
        let kept = keep_real_sessions(vec![session(0), session(1234), session(0), session(42)]);
        let pids: Vec<u32> = kept.iter().map(|s| s.pid).collect();
        assert_eq!(pids, vec![1234, 42]);
    }
}
