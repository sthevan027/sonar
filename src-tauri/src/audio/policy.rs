//! Política de roteamento por app: string do dispositivo, variante da API por build
//! e, no Windows, a chamada à interface `AudioPolicyConfig` (porta do spike).

#[cfg(windows)]
use std::ffi::c_void;

#[cfg(windows)]
use windows::core::{IInspectable, Interface, IUnknown, GUID, HRESULT, HSTRING};
#[cfg(windows)]
use windows::Wdk::System::SystemServices::RtlGetVersion;
#[cfg(windows)]
use windows::Win32::Media::Audio::{eCapture, eConsole, eMultimedia, eRender};
#[cfg(windows)]
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
#[cfg(windows)]
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
#[cfg(windows)]
use windows::Win32::System::WinRT::{RoGetActivationFactory, WindowsCreateString};

#[cfg(windows)]
use super::types::{AudioError, RouteBackend};
use super::types::{DeviceId, Flow};

const MMDEVAPI_PREFIX: &str = r"\\?\SWD#MMDEVAPI#";
const RENDER_SUFFIX: &str = "#{e6327cad-dcec-4949-ae8a-991e976a79d2}";
const CAPTURE_SUFFIX: &str = "#{2eef81be-33fa-4800-9670-1cd474972c3f}";

/// Primeira build com a interface `AudioPolicyConfig` da variante Win11 (21H2).
const WIN11_MIN_BUILD: u32 = 21390;
/// Primeira build do Windows 10 com a API de roteamento (abaixo disso, sem suporte).
const DOWNLEVEL_MIN_BUILD: u32 = 19041;

/// IIDs da interface `AudioPolicyConfig` por variante (valores do spike).
#[cfg(windows)]
const IID_WIN11: GUID = GUID::from_u128(0xab3d4648_e242_459f_b02f_541c70306324);
#[cfg(windows)]
const IID_DOWNLEVEL: GUID = GUID::from_u128(0x2a59116d_6c4f_45e0_a74f_707e3fef9258);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Win11,
    Downlevel,
}

/// String que o Windows espera para um endpoint: prefixo `\\?\SWD#MMDEVAPI#`
/// (duas barras invertidas), o id do endpoint e o sufixo do fluxo.
pub fn policy_string(id: &DeviceId, flow: Flow) -> String {
    let suffix = match flow {
        Flow::Render => RENDER_SUFFIX,
        Flow::Capture => CAPTURE_SUFFIX,
    };
    format!("{MMDEVAPI_PREFIX}{}{suffix}", id.0)
}

/// Inverso de `policy_string`: remove prefixo e sufixo e devolve o id puro.
/// `None` quando a string não tem o formato esperado.
pub fn parse_policy_string(s: &str) -> Option<DeviceId> {
    let rest = s.strip_prefix(MMDEVAPI_PREFIX)?;
    let id = rest
        .strip_suffix(RENDER_SUFFIX)
        .or_else(|| rest.strip_suffix(CAPTURE_SUFFIX))?;
    (!id.is_empty()).then(|| DeviceId(id.to_string()))
}

/// `None` abaixo da build 19041, onde a API de roteamento não existe.
pub fn variant_for_build(build: u32) -> Option<Variant> {
    if build >= WIN11_MIN_BUILD {
        Some(Variant::Win11)
    } else if build >= DOWNLEVEL_MIN_BUILD {
        Some(Variant::Downlevel)
    } else {
        None
    }
}

/// Métodos "reservados" na vtable antes de `Set`, depois dos 6 de IUnknown+IInspectable.
pub fn reserved_slots(v: Variant) -> usize {
    match v {
        Variant::Win11 => 19,
        Variant::Downlevel => 17,
    }
}

#[cfg(windows)]
fn iid_for(v: Variant) -> GUID {
    match v {
        Variant::Win11 => IID_WIN11,
        Variant::Downlevel => IID_DOWNLEVEL,
    }
}

/// Índice da vtable de `Set`; `Get` é o slot seguinte.
#[cfg(windows)]
fn set_slot(v: Variant) -> usize {
    6 + reserved_slots(v)
}

/// `Set(pid, flow, role, hstring)`.
#[cfg(windows)]
type SetFn = unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut c_void) -> HRESULT;
/// `Get(pid, flow, role, *out hstring)`.
#[cfg(windows)]
type GetFn = unsafe extern "system" fn(*mut c_void, u32, i32, i32, *mut HSTRING) -> HRESULT;

/// HRESULT de "COM já inicializado com outro modelo de thread".
#[cfg(windows)]
const RPC_E_CHANGED_MODE: HRESULT = HRESULT(0x8001_0106_u32 as i32);

/// Número da build do Windows, via `RtlGetVersion` (não via `cmd /c ver`).
#[cfg(windows)]
fn windows_build() -> Option<u32> {
    // SAFETY: OSVERSIONINFOW é uma struct C só de inteiros; zerada e com o tamanho preenchido.
    let mut info: OSVERSIONINFOW = unsafe { std::mem::zeroed() };
    info.dwOSVersionInfoSize = std::mem::size_of::<OSVERSIONINFOW>() as u32;
    // SAFETY: ponteiro válido para uma struct do tamanho informado.
    let status = unsafe { RtlGetVersion(&mut info) };
    (status.0 == 0).then_some(info.dwBuildNumber)
}

#[cfg(windows)]
pub(super) fn ensure_com() -> Result<(), AudioError> {
    // SAFETY: inicialização de COM da thread chamadora; sem ponteiro reservado.
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    // is_ok cobre S_OK e S_FALSE (já inicializado no mesmo modelo).
    if hr.is_ok() || hr == RPC_E_CHANGED_MODE {
        Ok(())
    } else {
        Err(AudioError::Os(hr.0))
    }
}

#[cfg(windows)]
fn hstring_from(s: &str) -> Result<HSTRING, AudioError> {
    let utf16: Vec<u16> = s.encode_utf16().collect();
    // SAFETY: a string é copiada pelo combase; o HSTRING devolvido é dono do próprio buffer.
    unsafe { WindowsCreateString(Some(&utf16)) }.map_err(|e| AudioError::Os(e.code().0))
}

/// Lê o ponteiro da função `index` da vtable do objeto COM.
///
/// # Safety
/// `obj` precisa apontar para um objeto COM válido e `index` precisa existir na vtable
/// e ter a assinatura `F`.
#[cfg(windows)]
unsafe fn vtable_fn<F: Copy>(obj: *mut c_void, index: usize) -> F {
    let vtbl = *(obj as *const *const usize);
    let entry = *vtbl.add(index);
    std::mem::transmute_copy::<usize, F>(&entry)
}

/// Objeto `AudioPolicyConfig` da variante certa, com a referência própria (liberada no Drop).
#[cfg(windows)]
struct Policy {
    obj: IUnknown,
    set: SetFn,
    get: GetFn,
}

#[cfg(windows)]
impl Policy {
    fn activate(variant: Variant) -> Result<Policy, AudioError> {
        ensure_com()?;
        let class = HSTRING::from("Windows.Media.Internal.AudioPolicyConfig");
        // SAFETY: ativação de classe WinRT por nome; o retorno é um IInspectable válido.
        let factory: IInspectable = unsafe { RoGetActivationFactory(&class) }
            .map_err(|e| AudioError::Unsupported(format!("ativação falhou: {e}")))?;
        let iid = iid_for(variant);
        let mut raw: *mut c_void = std::ptr::null_mut();
        // SAFETY: `raw` recebe uma referência nova (AddRef) quando o retorno é sucesso.
        unsafe { factory.query(&iid, &mut raw) }
            .ok()
            .map_err(|e| AudioError::Unsupported(format!("variante {variant:?} indisponível: {e}")))?;
        // SAFETY: `raw` é a referência entregue por `query`; `IUnknown` assume a posse e libera no Drop.
        let obj = unsafe { IUnknown::from_raw(raw) };
        // SAFETY: slots calculados a partir da variante, conforme a porta do spike.
        let (set, get) = unsafe {
            (
                vtable_fn::<SetFn>(obj.as_raw(), set_slot(variant)),
                vtable_fn::<GetFn>(obj.as_raw(), set_slot(variant) + 1),
            )
        };
        Ok(Policy { obj, set, get })
    }
}

#[cfg(windows)]
fn flow_raw(flow: Flow) -> i32 {
    match flow {
        Flow::Render => eRender.0,
        Flow::Capture => eCapture.0,
    }
}

/// Backend real do Windows (`AudioPolicyConfig`). Lê a build uma vez, na criação.
#[cfg(windows)]
pub struct WindowsRouteBackend {
    build: Option<u32>,
}

#[cfg(windows)]
impl WindowsRouteBackend {
    pub fn new() -> Self {
        Self { build: windows_build() }
    }

    fn variant(&self) -> Result<Variant, AudioError> {
        let build = self
            .build
            .ok_or_else(|| AudioError::Unsupported("não foi possível ler a build do Windows".into()))?;
        variant_for_build(build)
            .ok_or_else(|| AudioError::Unsupported(format!("build {build} abaixo de 19041")))
    }

    /// Grava o mesmo valor nos dois papéis: eMultimedia (1) e eConsole (0).
    /// Tenta os dois mesmo que o primeiro falhe e devolve o primeiro erro.
    fn write(&self, pid: u32, flow: Flow, value: &HSTRING) -> Result<(), AudioError> {
        let policy = Policy::activate(self.variant()?)?;
        let raw: *mut c_void = unsafe { std::mem::transmute_copy(value) };
        let mut first_err = None;
        for role in [eMultimedia.0, eConsole.0] {
            // SAFETY: função da vtable validada na ativação; `value` vive até o fim da chamada.
            let hr = unsafe { (policy.set)(policy.obj.as_raw(), pid, flow_raw(flow), role, raw) };
            if hr.is_err() && first_err.is_none() {
                first_err = Some(AudioError::Os(hr.0));
            }
        }
        first_err.map_or(Ok(()), Err)
    }
}

#[cfg(windows)]
impl RouteBackend for WindowsRouteBackend {
    fn set_route(&self, pid: u32, flow: Flow, device: &DeviceId) -> Result<(), AudioError> {
        let value = hstring_from(&policy_string(device, flow))?;
        self.write(pid, flow, &value)
    }

    /// HSTRING vazio limpa a rota: o processo volta ao padrão do Windows.
    fn clear_route(&self, pid: u32, flow: Flow) -> Result<(), AudioError> {
        self.write(pid, flow, &HSTRING::new())
    }

    fn get_route(&self, pid: u32, flow: Flow) -> Result<Option<DeviceId>, AudioError> {
        let policy = Policy::activate(self.variant()?)?;
        let mut out = HSTRING::new();
        // SAFETY: função da vtable validada na ativação; `out` é um HSTRING válido para escrita.
        let hr = unsafe {
            (policy.get)(policy.obj.as_raw(), pid, flow_raw(flow), eMultimedia.0, &mut out)
        };
        if hr.is_err() {
            return Err(AudioError::Os(hr.0));
        }
        let text = out.to_string();
        if text.is_empty() {
            return Ok(None);
        }
        parse_policy_string(&text)
            .map(Some)
            .ok_or_else(|| AudioError::Unsupported("formato de rota desconhecido".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDPOINT: &str = "{0.0.0.00000000}.{2329d0e9-306a-408e-9c4b-4061e553cdb4}";

    fn device() -> DeviceId {
        DeviceId(ENDPOINT.to_string())
    }

    #[test]
    fn policy_string_render_has_double_backslash_and_111_chars() {
        let s = policy_string(&device(), Flow::Render);
        assert!(s.starts_with(r"\\?\SWD#MMDEVAPI#"));
        assert!(s.ends_with("#{e6327cad-dcec-4949-ae8a-991e976a79d2}"));
        assert_eq!(s.len(), 111);
    }

    #[test]
    fn policy_string_capture_uses_capture_suffix() {
        let s = policy_string(&device(), Flow::Capture);
        assert!(s.starts_with(r"\\?\SWD#MMDEVAPI#"));
        assert!(s.ends_with("#{2eef81be-33fa-4800-9670-1cd474972c3f}"));
    }

    #[test]
    fn parse_policy_string_roundtrip() {
        for flow in [Flow::Render, Flow::Capture] {
            let s = policy_string(&device(), flow);
            assert_eq!(parse_policy_string(&s), Some(device()));
        }
    }

    #[test]
    fn parse_policy_string_rejects_garbage() {
        assert_eq!(parse_policy_string(""), None);
        assert_eq!(parse_policy_string("garbage"), None);
        // Prefixo sem sufixo, e prefixo com uma barra só (formato errado).
        assert_eq!(parse_policy_string(&format!(r"\\?\SWD#MMDEVAPI#{ENDPOINT}")), None);
        assert_eq!(parse_policy_string(&format!(r"\?\SWD#MMDEVAPI#{ENDPOINT}#{{e6327cad-dcec-4949-ae8a-991e976a79d2}}")), None);
        // Prefixo e sufixo sem id no meio.
        assert_eq!(parse_policy_string(r"\\?\SWD#MMDEVAPI##{e6327cad-dcec-4949-ae8a-991e976a79d2}"), None);
    }

    #[test]
    fn variant_for_build_boundaries() {
        assert_eq!(variant_for_build(21389), Some(Variant::Downlevel));
        assert_eq!(variant_for_build(21390), Some(Variant::Win11));
        assert_eq!(variant_for_build(26300), Some(Variant::Win11));
        assert_eq!(variant_for_build(19040), None);
    }

    #[test]
    fn reserved_slots_per_variant() {
        assert_eq!(reserved_slots(Variant::Win11), 19);
        assert_eq!(reserved_slots(Variant::Downlevel), 17);
    }

    /// Manual: só roda com `cargo test -- --ignored`, com `SONAR_TEST_PID` (processo com áudio,
    /// ex.: Spotify tocando) e `SONAR_TEST_DEVICE` (id puro do endpoint de destino).
    /// Sem as variáveis, passa sem fazer nada. Restaura a rota anterior no fim.
    #[cfg(windows)]
    #[test]
    #[ignore = "manual: exige SONAR_TEST_PID e SONAR_TEST_DEVICE e um app tocando áudio"]
    fn manual_route_roundtrip() {
        let (Ok(pid), Ok(device)) = (
            std::env::var("SONAR_TEST_PID"),
            std::env::var("SONAR_TEST_DEVICE"),
        ) else {
            println!("SONAR_TEST_PID/SONAR_TEST_DEVICE não definidos: teste manual pulado.");
            return;
        };
        let pid: u32 = pid.parse().expect("SONAR_TEST_PID precisa ser um número");
        let device = DeviceId(device);
        let backend = WindowsRouteBackend::new();

        let before = backend.get_route(pid, Flow::Render).expect("ler rota atual");
        backend
            .set_route(pid, Flow::Render, &device)
            .expect("gravar rota");
        let read = backend.get_route(pid, Flow::Render).expect("ler rota gravada");

        // Restaura antes de validar, para o app voltar ao estado original mesmo se o assert falhar.
        match &before {
            Some(prev) => backend.set_route(pid, Flow::Render, prev).expect("restaurar rota"),
            None => backend.clear_route(pid, Flow::Render).expect("limpar rota"),
        }
        assert_eq!(read, Some(device));
    }
}
