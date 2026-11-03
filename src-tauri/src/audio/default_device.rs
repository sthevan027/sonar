//! Troca do dispositivo padrão do Windows nos três papéis, via `IPolicyConfig`.
//!
//! `IPolicyConfig` é uma interface COM não documentada. A vtable é declarada aqui à mão,
//! seguindo a referência do EarTrumpet (`IPolicyConfigWin7`, IID `F8679F50-...`, coclass
//! `PolicyConfigClient` `870AF99C-...`): IUnknown (3 slots), 8 métodos reservados,
//! `GetPropertyValue`, `SetPropertyValue`, `SetDefaultEndpoint` (slot 13), `SetEndpointVisibility`.
//! A referência do Earmark (`IPolicyConfigVista`, IID `568B9108-...`) tem outro layout e
//! coloca `SetDefaultEndpoint` no slot 12; ela não é usada aqui.
//! O QueryInterface pelo IID é a checagem de que o layout bate com o objeto.

#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
use windows::core::{IUnknown, Interface, GUID, HRESULT};
#[cfg(windows)]
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, ERole, IMMEndpoint,
};
#[cfg(windows)]
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

#[cfg(windows)]
use super::devices::{edata_flow, to_audio_error};
#[cfg(windows)]
use super::endpoint::device_by_id;
#[cfg(windows)]
use super::policy::{ensure_com, vtable_fn};
#[cfg(windows)]
use super::types::AudioError;
use super::types::{DeviceId, Flow};

/// Coclass `PolicyConfigClient` (referência EarTrumpet).
#[cfg(windows)]
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
/// IID de `IPolicyConfigWin7` (referência EarTrumpet, válida para Win10 RS1 até o presente).
#[cfg(windows)]
const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
/// Slot de `SetDefaultEndpoint`: 0..=2 IUnknown, 3..=10 oito reservados, 11 e 12 Get/SetPropertyValue, 13 este.
#[cfg(windows)]
const SET_DEFAULT_ENDPOINT_SLOT: usize = 13;

/// `SetDefaultEndpoint(this, LPWSTR deviceId, ERole role)`.
#[cfg(windows)]
type SetDefaultEndpointFn = unsafe extern "system" fn(*mut c_void, *const u16, i32) -> HRESULT;

/// Objeto `IPolicyConfig` com a referência própria (liberada no Drop de `IUnknown`).
#[cfg(windows)]
struct PolicyConfig {
    obj: IUnknown,
    set_default: SetDefaultEndpointFn,
}

#[cfg(windows)]
impl PolicyConfig {
    /// Cria a coclass e faz QueryInterface pelo IID do layout usado. Sem o IID, `Unsupported`.
    fn open() -> Result<Self, AudioError> {
        ensure_com()?;
        // SAFETY: criação de objeto COM padrão do sistema; sem agregação.
        let obj: IUnknown = unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
            .map_err(to_audio_error)?;
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: `raw` recebe uma referência nova (AddRef) quando o retorno é sucesso.
        unsafe { obj.query(&IID_POLICY_CONFIG, &mut raw) }
            .ok()
            .map_err(|e| AudioError::Unsupported(format!("IPolicyConfig indisponível: {e}")))?;
        // SAFETY: `raw` é a referência entregue por `query`; `IUnknown` assume a posse e libera no Drop.
        let obj = unsafe { IUnknown::from_raw(raw) };
        // SAFETY: o IID acima confirmou o layout; o slot está dentro da vtable de IPolicyConfigWin7.
        let set_default =
            unsafe { vtable_fn::<SetDefaultEndpointFn>(obj.as_raw(), SET_DEFAULT_ENDPOINT_SLOT) };
        Ok(Self { obj, set_default })
    }

    /// Grava o padrão de um papel. O id é o id puro do `MMDevice` (não a string SWD de rota).
    fn set_role(&self, id: &DeviceId, role: ERole) -> Result<(), AudioError> {
        // Id em UTF-16 terminado em NUL; o vetor vive até o fim da chamada.
        let wide: Vec<u16> = id.0.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: função da vtable validada por `open`; `wide` é válido durante a chamada.
        let hr = unsafe { (self.set_default)(self.obj.as_raw(), wide.as_ptr(), role.0) };
        if hr.is_err() {
            Err(AudioError::Os(hr.0))
        } else {
            Ok(())
        }
    }
}

/// Confere que o id é um endpoint do fluxo pedido. Sem isso, um id de captura com
/// `Flow::Render` seria gravado mesmo assim, sem o chamador perceber.
#[cfg(windows)]
fn check_flow(id: &DeviceId, flow: Flow) -> Result<(), AudioError> {
    let device = device_by_id(id)?;
    let endpoint: IMMEndpoint = device.cast().map_err(to_audio_error)?;
    // SAFETY: leitura simples do fluxo do endpoint.
    let actual = unsafe { endpoint.GetDataFlow() }.map_err(to_audio_error)?;
    if actual == edata_flow(flow) {
        Ok(())
    } else {
        Err(AudioError::Unsupported(
            "o dispositivo não é do fluxo pedido".into(),
        ))
    }
}

/// Troca do dispositivo padrão do Windows (ver `WindowsDefaultDevice`).
pub trait DefaultDevice: Send + Sync {
    fn set_default(&self, id: &DeviceId, flow: Flow) -> Result<(), AudioError>;
}

/// Backend real do Windows: grava o padrão nos três papéis (`eConsole`, `eMultimedia`,
/// `eCommunications`). Tenta os três mesmo que um falhe e devolve o primeiro erro.
#[cfg(windows)]
pub struct WindowsDefaultDevice;

#[cfg(windows)]
impl DefaultDevice for WindowsDefaultDevice {
    fn set_default(&self, id: &DeviceId, flow: Flow) -> Result<(), AudioError> {
        ensure_com()?;
        check_flow(id, flow)?;
        let policy = PolicyConfig::open()?;
        let mut first_err = None;
        for role in [eConsole, eMultimedia, eCommunications] {
            if let Err(e) = policy.set_role(id, role) {
                first_err.get_or_insert(e);
            }
        }
        first_err.map_or(Ok(()), Err)
    }
}

#[cfg(all(test, windows))]
mod manual_tests {
    use super::super::devices::{create_enumerator, device_id};
    use super::*;
    use windows::Win32::Media::Audio::IMMDeviceEnumerator;

    /// Padrão atual de um papel no fluxo, ou `None` se não houver.
    /// Roda em thread próprio: a leitura inicializa o COM ali, sem afetar o thread do
    /// `set_default`, que precisa começar sem COM inicializado.
    fn current_default(flow: Flow, role: ERole) -> Option<DeviceId> {
        std::thread::spawn(move || {
            ensure_com().ok()?;
            let enumerator: IMMDeviceEnumerator = create_enumerator().ok()?;
            // SAFETY: consulta ao enumerador, sem ponteiros próprios.
            let device =
                unsafe { enumerator.GetDefaultAudioEndpoint(edata_flow(flow), role) }.ok()?;
            device_id(&device).ok()
        })
        .join()
        .ok()
        .flatten()
    }

    /// Restaura os papéis originais no Drop, mesmo se uma asserção falhar.
    struct RestoreDefaults {
        originals: Vec<(ERole, DeviceId)>,
    }

    impl Drop for RestoreDefaults {
        fn drop(&mut self) {
            if let Ok(policy) = PolicyConfig::open() {
                for (role, id) in &self.originals {
                    // Erro ignorado: o Drop não tem como reportar falha.
                    let _ = policy.set_role(id, *role);
                }
            }
        }
    }

    /// Manual: troca o padrão para `SONAR_TEST_DEFAULT_DEVICE` no fluxo
    /// `SONAR_TEST_DEFAULT_FLOW` (`render` ou `capture`) e restaura os três papéis.
    /// Sem as variáveis, passa sem fazer nada.
    /// Rodar com `cargo test manual_default_device_roundtrip -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual: troca o padrão do Windows (exige SONAR_TEST_DEFAULT_DEVICE e SONAR_TEST_DEFAULT_FLOW)"]
    fn manual_default_device_roundtrip() {
        let (Ok(device), Ok(flow)) = (
            std::env::var("SONAR_TEST_DEFAULT_DEVICE"),
            std::env::var("SONAR_TEST_DEFAULT_FLOW"),
        ) else {
            println!("SONAR_TEST_DEFAULT_DEVICE/SONAR_TEST_DEFAULT_FLOW não definidos: teste manual pulado.");
            return;
        };
        let flow = match flow.to_ascii_lowercase().as_str() {
            "render" => Flow::Render,
            "capture" => Flow::Capture,
            other => panic!("SONAR_TEST_DEFAULT_FLOW inválido: {other} (use render ou capture)"),
        };
        // Sem `ensure_com` aqui: o `set_default` precisa ser o primeiro a usar COM neste thread.
        let target = DeviceId(device);
        let originals: Vec<(ERole, DeviceId)> = [eConsole, eMultimedia, eCommunications]
            .into_iter()
            .filter_map(|role| current_default(flow, role).map(|id| (role, id)))
            .collect();
        // Guard criado antes de qualquer escrita.
        let _restore = RestoreDefaults { originals };

        WindowsDefaultDevice
            .set_default(&target, flow)
            .expect("trocar o padrão");
        assert_eq!(current_default(flow, eMultimedia), Some(target));
    }
}
