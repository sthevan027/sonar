//! Volume, mudo e nível de endpoints; volume e mudo por sessão (por PID).
//! A função pura (`clamp_volume`) fica fora do bloco `cfg(windows)` para ter teste direto.

#[cfg(windows)]
use windows::core::{Interface, PCWSTR};
#[cfg(windows)]
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
#[cfg(windows)]
use windows::Win32::Media::Audio::{
    IAudioSessionControl2, IAudioSessionManager2, IMMDevice, ISimpleAudioVolume,
};
#[cfg(windows)]
use windows::Win32::System::Com::CLSCTX_ALL;

#[cfg(windows)]
use super::devices::{active_endpoints, create_enumerator, to_audio_error};
#[cfg(windows)]
use super::policy::ensure_com;
#[cfg(windows)]
use super::types::AudioError;
use super::types::{DeviceId, Flow};

/// Limita a 0.0..=1.0. NaN vira 0.0 (`f32::clamp` deixaria NaN passar).
pub fn clamp_volume(v: f32) -> f32 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

/// Volume, mudo e nível de endpoints e volume/mudo de sessões. Volumes em 0.0..=1.0;
/// valores fora do intervalo são limitados.
pub trait EndpointControl: Send + Sync {
    fn volume(&self, id: &DeviceId) -> Result<f32, AudioError>;
    fn set_volume(&self, id: &DeviceId, v: f32) -> Result<(), AudioError>;
    fn muted(&self, id: &DeviceId) -> Result<bool, AudioError>;
    fn set_muted(&self, id: &DeviceId, m: bool) -> Result<(), AudioError>;
    fn peak(&self, id: &DeviceId) -> Result<f32, AudioError>;
    fn set_session_volume(&self, pid: u32, flow: Flow, v: f32) -> Result<(), AudioError>;
    fn set_session_muted(&self, pid: u32, flow: Flow, m: bool) -> Result<(), AudioError>;
}

/// `IMMDevice` de um endpoint pelo id puro (`IMMDeviceEnumerator::GetDevice`).
#[cfg(windows)]
pub(super) fn device_by_id(id: &DeviceId) -> Result<IMMDevice, AudioError> {
    let enumerator = create_enumerator()?;
    // Id em UTF-16 terminado em NUL; o vetor vive até o fim da chamada.
    let wide: Vec<u16> = id.0.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` é uma string terminada em NUL, válida durante a chamada.
    unsafe { enumerator.GetDevice(PCWSTR(wide.as_ptr())) }.map_err(to_audio_error)
}

/// Interface de volume do endpoint (`IAudioEndpointVolume`).
#[cfg(windows)]
fn endpoint_volume(id: &DeviceId) -> Result<IAudioEndpointVolume, AudioError> {
    let device = device_by_id(id)?;
    // SAFETY: ativação padrão de interface do endpoint; sem parâmetros de ativação.
    unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.map_err(to_audio_error)
}

/// Aplica `apply` a cada sessão do `pid` nos endpoints ativos do fluxo. Tenta todas;
/// devolve o primeiro erro de aplicação, ou `Gone` se nenhuma sessão do PID existir.
#[cfg(windows)]
fn for_each_session(
    pid: u32,
    flow: Flow,
    apply: impl Fn(&ISimpleAudioVolume) -> windows::core::Result<()>,
) -> Result<(), AudioError> {
    ensure_com()?;
    let enumerator = create_enumerator()?;
    let mut matched = 0usize;
    let mut first_err: Option<AudioError> = None;
    for (device, _id) in active_endpoints(&enumerator, flow)? {
        // SAFETY: ativação padrão do gerenciador de sessões do endpoint.
        let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
        else {
            continue;
        };
        // SAFETY: enumeração simples do gerenciador recém-ativado.
        let Ok(list) = (unsafe { manager.GetSessionEnumerator() }) else {
            continue;
        };
        let Ok(count) = (unsafe { list.GetCount() }) else {
            continue;
        };
        for index in 0..count {
            let Ok(control) = (unsafe { list.GetSession(index) }) else {
                continue;
            };
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            // SAFETY: consulta simples sobre a sessão recém-obtida.
            if unsafe { control2.GetProcessId() }.ok() != Some(pid) {
                continue;
            }
            matched += 1;
            let volume = match control.cast::<ISimpleAudioVolume>() {
                Ok(volume) => volume,
                Err(e) => {
                    first_err.get_or_insert(to_audio_error(e));
                    continue;
                }
            };
            if let Err(e) = apply(&volume) {
                first_err.get_or_insert(to_audio_error(e));
            }
        }
    }
    if let Some(e) = first_err {
        return Err(e);
    }
    if matched == 0 {
        return Err(AudioError::Gone);
    }
    Ok(())
}

/// Backend real do Windows: `IAudioEndpointVolume`, `IAudioMeterInformation` e
/// `ISimpleAudioVolume`. Sem estado; cada chamada obtém as interfaces de novo.
#[cfg(windows)]
pub struct WindowsEndpoint;

#[cfg(windows)]
impl EndpointControl for WindowsEndpoint {
    fn volume(&self, id: &DeviceId) -> Result<f32, AudioError> {
        ensure_com()?;
        let volume = endpoint_volume(id)?;
        // SAFETY: leitura simples da interface recém-ativada.
        unsafe { volume.GetMasterVolumeLevelScalar() }
            .map(clamp_volume)
            .map_err(to_audio_error)
    }

    fn set_volume(&self, id: &DeviceId, v: f32) -> Result<(), AudioError> {
        ensure_com()?;
        let volume = endpoint_volume(id)?;
        // SAFETY: escrita de volume escalar; contexto de evento nulo (sem notificação própria).
        unsafe { volume.SetMasterVolumeLevelScalar(clamp_volume(v), std::ptr::null()) }
            .map_err(to_audio_error)
    }

    fn muted(&self, id: &DeviceId) -> Result<bool, AudioError> {
        ensure_com()?;
        let volume = endpoint_volume(id)?;
        // SAFETY: leitura simples da interface recém-ativada.
        unsafe { volume.GetMute() }
            .map(|m| m.as_bool())
            .map_err(to_audio_error)
    }

    fn set_muted(&self, id: &DeviceId, m: bool) -> Result<(), AudioError> {
        ensure_com()?;
        let volume = endpoint_volume(id)?;
        // SAFETY: escrita de mudo; contexto de evento nulo.
        unsafe { volume.SetMute(m, std::ptr::null()) }.map_err(to_audio_error)
    }

    fn peak(&self, id: &DeviceId) -> Result<f32, AudioError> {
        ensure_com()?;
        let device = device_by_id(id)?;
        // SAFETY: ativação padrão do medidor de pico do endpoint.
        let meter = unsafe { device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) }
            .map_err(to_audio_error)?;
        // SAFETY: leitura simples do pico atual.
        unsafe { meter.GetPeakValue() }
            .map(clamp_volume)
            .map_err(to_audio_error)
    }

    fn set_session_volume(&self, pid: u32, flow: Flow, v: f32) -> Result<(), AudioError> {
        let v = clamp_volume(v);
        for_each_session(pid, flow, |volume| {
            // SAFETY: escrita de volume na sessão obtida por `for_each_session`.
            unsafe { volume.SetMasterVolume(v, std::ptr::null()) }
        })
    }

    fn set_session_muted(&self, pid: u32, flow: Flow, m: bool) -> Result<(), AudioError> {
        for_each_session(pid, flow, |volume| {
            // SAFETY: escrita de mudo na sessão obtida por `for_each_session`.
            unsafe { volume.SetMute(m, std::ptr::null()) }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_volume;

    #[test]
    fn clamp_volume_limits_to_unit_range() {
        assert_eq!(clamp_volume(-0.2), 0.0);
        assert_eq!(clamp_volume(1.7), 1.0);
        assert_eq!(clamp_volume(f32::NAN), 0.0);
        assert_eq!(clamp_volume(0.5), 0.5);
    }
}

/// Manual: altera o volume do endpoint `SONAR_TEST_DEVICE` e restaura o valor original,
/// mesmo se uma asserção falhar. Sem a variável, passa sem fazer nada.
/// Rodar com `cargo test manual_endpoint_volume_roundtrip -- --ignored --nocapture`.
#[cfg(all(test, windows))]
mod manual_tests {
    use super::*;

    struct RestoreVolume {
        id: DeviceId,
        volume: f32,
    }

    impl Drop for RestoreVolume {
        fn drop(&mut self) {
            // Erro ignorado: o Drop não tem como reportar falha.
            let _ = WindowsEndpoint.set_volume(&self.id, self.volume);
        }
    }

    #[test]
    #[ignore = "manual: altera o volume de SONAR_TEST_DEVICE e restaura (exige a variável)"]
    fn manual_endpoint_volume_roundtrip() {
        let Ok(device) = std::env::var("SONAR_TEST_DEVICE") else {
            println!("SONAR_TEST_DEVICE não definido: teste manual pulado.");
            return;
        };
        let id = DeviceId(device);
        let control = WindowsEndpoint;
        let original = control.volume(&id).expect("ler volume original");
        // Guard criado antes de qualquer escrita: o valor original volta mesmo em falha.
        let _restore = RestoreVolume {
            id: id.clone(),
            volume: original,
        };
        let target = if original > 0.5 {
            original - 0.1
        } else {
            original + 0.1
        };
        control.set_volume(&id, target).expect("gravar volume de teste");
        let read = control.volume(&id).expect("ler volume gravado");
        assert!(
            (read - clamp_volume(target)).abs() < 0.02,
            "esperado ~{}, lido {read}",
            clamp_volume(target)
        );
    }
}
