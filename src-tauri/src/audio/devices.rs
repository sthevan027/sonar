//! Dispositivos de áudio (endpoints de saída e entrada), fonte de sessões e notificações.
//!
//! O backend Windows implementa `AudioSource` e `watch`; o resto é código puro de tipos.

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::sync::mpsc::{channel, Receiver, Sender};
#[cfg(windows)]
use std::thread::JoinHandle;

#[cfg(windows)]
use windows::core::{implement, PCWSTR};
#[cfg(windows)]
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
#[cfg(windows)]
use windows::Win32::Foundation::PROPERTYKEY;
#[cfg(windows)]
use windows::Win32::Media::Audio::{
    eCapture, eMultimedia, eRender, EDataFlow, ERole, IAudioSessionManager2,
    IAudioSessionNotification, IMMDevice, IMMDeviceEnumerator, IMMNotificationClient,
    IMMNotificationClient_Impl, MMDeviceEnumerator, DEVICE_STATE, DEVICE_STATEMASK_ALL,
    DEVICE_STATE_ACTIVE,
};
#[cfg(windows)]
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED, STGM_READ,
};

#[cfg(windows)]
use super::policy::ensure_com;
#[cfg(windows)]
use super::sessions::{keep_real_sessions, register_session_notifier, sessions_on, SessionNotifier};
#[cfg(windows)]
use super::types::AudioError;
use super::types::{AudioEvent, AudioSession, AudioSource, Device, DeviceId, Flow};

/// Mantém só os `Ok` de uma enumeração. Um item que falha (ex.: sumiu entre `GetCount` e
/// `Item`) é descartado sozinho, sem esconder os outros.
pub(super) fn collect_ok<T, E, I>(items: I) -> Vec<T>
where
    I: IntoIterator<Item = Result<T, E>>,
{
    items.into_iter().filter_map(Result::ok).collect()
}

/// Erro do Windows vira `AudioError::Os` com o HRESULT.
#[cfg(windows)]
pub(super) fn to_audio_error(e: windows::core::Error) -> AudioError {
    AudioError::Os(e.code().0)
}

#[cfg(windows)]
pub(super) fn edata_flow(flow: Flow) -> EDataFlow {
    match flow {
        Flow::Render => eRender,
        Flow::Capture => eCapture,
    }
}

/// Cria o `IMMDeviceEnumerator` do thread chamador. O chamador já inicializou o COM.
#[cfg(windows)]
pub(super) fn create_enumerator() -> Result<IMMDeviceEnumerator, AudioError> {
    // SAFETY: criação de objeto COM padrão do sistema; sem agregação.
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(to_audio_error)
}

/// Id puro do endpoint (`GetId`). Libera a memória que o Windows alocou para a string.
#[cfg(windows)]
pub(super) fn device_id(device: &IMMDevice) -> Result<DeviceId, AudioError> {
    // SAFETY: GetId devolve uma string alocada com CoTaskMemAlloc, liberada logo abaixo.
    let pwstr = unsafe { device.GetId() }.map_err(to_audio_error)?;
    // SAFETY: ponteiro válido devolvido por GetId, lido antes de ser liberado.
    let text = unsafe { pwstr.to_string() };
    // SAFETY: liberação única da memória devolvida por GetId.
    unsafe { CoTaskMemFree(Some(pwstr.0 as *const c_void)) };
    text.map(DeviceId)
        .map_err(|_| AudioError::Unsupported("id de dispositivo inválido".into()))
}

/// Nome amigável (`PKEY_Device_FriendlyName`). `None` quando o Windows não devolve.
#[cfg(windows)]
fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: propriedades de leitura do endpoint.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        Some(value.to_string())
    }
}

/// Id do padrão do Windows para o papel multimídia no fluxo dado. `None` se não houver.
#[cfg(windows)]
fn default_id(enumerator: &IMMDeviceEnumerator, flow: Flow) -> Option<DeviceId> {
    // SAFETY: consulta ao enumerador, sem ponteiros próprios.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(edata_flow(flow), eMultimedia) }.ok()?;
    device_id(&device).ok()
}

/// Endpoints ativos (conectados) de um fluxo, com o id já lido.
#[cfg(windows)]
pub(super) fn active_endpoints(
    enumerator: &IMMDeviceEnumerator,
    flow: Flow,
) -> Result<Vec<(IMMDevice, DeviceId)>, AudioError> {
    // SAFETY: enumeração simples com máscara de estado ACTIVE.
    let collection = unsafe { enumerator.EnumAudioEndpoints(edata_flow(flow), DEVICE_STATE_ACTIVE) }
        .map_err(to_audio_error)?;
    let count = unsafe { collection.GetCount() }.map_err(to_audio_error)?;
    Ok(collect_ok((0..count).map(|index| -> Result<(IMMDevice, DeviceId), AudioError> {
        let device = unsafe { collection.Item(index) }.map_err(to_audio_error)?;
        let id = device_id(&device)?;
        Ok((device, id))
    })))
}

/// Todos os endpoints, inclusive desconectados e não presentes (`DEVICE_STATEMASK_ALL`),
/// porque regras guardadas podem apontar para um dispositivo que está desligado agora.
#[cfg(windows)]
fn list_devices(enumerator: &IMMDeviceEnumerator) -> Result<Vec<Device>, AudioError> {
    let mut out = Vec::new();
    for flow in [Flow::Render, Flow::Capture] {
        let default = default_id(enumerator, flow);
        // SAFETY: enumeração com máscara "todos os estados".
        let collection = unsafe {
            enumerator.EnumAudioEndpoints(edata_flow(flow), DEVICE_STATE(DEVICE_STATEMASK_ALL))
        }
        .map_err(to_audio_error)?;
        let count = unsafe { collection.GetCount() }.map_err(to_audio_error)?;
        out.extend(collect_ok((0..count).map(|index| -> Result<Device, AudioError> {
            let device = unsafe { collection.Item(index) }.map_err(to_audio_error)?;
            let id = device_id(&device)?;
            let state = unsafe { device.GetState() }.map_err(to_audio_error)?;
            let name = friendly_name(&device)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| id.0.clone());
            Ok(Device {
                is_default: default.as_ref() == Some(&id),
                connected: state == DEVICE_STATE_ACTIVE,
                id,
                flow,
                name,
            })
        })));
    }
    Ok(out)
}

/// Backend real do Windows: `IMMDeviceEnumerator` e `IAudioSessionManager2`.
#[cfg(windows)]
pub struct WindowsAudioSource;

#[cfg(windows)]
impl AudioSource for WindowsAudioSource {
    fn devices(&self) -> Result<Vec<Device>, AudioError> {
        ensure_com()?;
        list_devices(&create_enumerator()?)
    }

    /// Sessões de todos os endpoints ativos, saída e entrada. Falha num dispositivo
    /// (ex.: foi removido agora) não derruba a lista dos outros.
    fn sessions(&self) -> Result<Vec<AudioSession>, AudioError> {
        ensure_com()?;
        let enumerator = create_enumerator()?;
        let mut out = Vec::new();
        for flow in [Flow::Render, Flow::Capture] {
            for (device, id) in active_endpoints(&enumerator, flow)? {
                if let Ok(found) = sessions_on(&device, &id, flow) {
                    out.extend(found);
                }
            }
        }
        Ok(keep_real_sessions(out))
    }
}

/// Callback de `IMMNotificationClient`: mudança de dispositivo ou do padrão do Windows.
/// Nunca entra em pânico: erro de envio (receptor descartado) é ignorado.
#[cfg(windows)]
#[implement(IMMNotificationClient)]
struct DeviceNotifier {
    tx: Sender<AudioEvent>,
}

#[cfg(windows)]
impl DeviceNotifier_Impl {
    fn notify(&self) -> windows::core::Result<()> {
        let _ = self.tx.send(AudioEvent::DevicesChanged);
        Ok(())
    }
}

#[cfg(windows)]
impl IMMNotificationClient_Impl for DeviceNotifier_Impl {
    fn OnDeviceStateChanged(&self, _pwstrdeviceid: &PCWSTR, _dwnewstate: DEVICE_STATE) -> windows::core::Result<()> {
        self.notify()
    }
    fn OnDeviceAdded(&self, _pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        self.notify()
    }
    fn OnDeviceRemoved(&self, _pwstrdeviceid: &PCWSTR) -> windows::core::Result<()> {
        self.notify()
    }
    fn OnDefaultDeviceChanged(&self, _flow: EDataFlow, _role: ERole, _pwstrdefaultdeviceid: &PCWSTR) -> windows::core::Result<()> {
        self.notify()
    }
    /// Mudança de propriedade (volume, formato, nome) chega em rajadas; não é avisada.
    fn OnPropertyValueChanged(&self, _pwstrdeviceid: &PCWSTR, _key: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

/// Assinaturas ativas de `watch`. Vivem no thread de notificações; ao cair, cancelam tudo.
#[cfg(windows)]
struct Registrations {
    enumerator: IMMDeviceEnumerator,
    client: IMMNotificationClient,
    sessions: Vec<(IAudioSessionManager2, IAudioSessionNotification)>,
}

#[cfg(windows)]
impl Drop for Registrations {
    fn drop(&mut self) {
        // SAFETY: cancela registros feitos por `register_all` com os mesmos objetos; erro ignorado.
        unsafe {
            let _ = self.enumerator.UnregisterEndpointNotificationCallback(&self.client);
        }
        for (manager, notifier) in &self.sessions {
            // SAFETY: mesmo gerenciador e notificador usados no registro; erro ignorado.
            unsafe {
                let _ = manager.UnregisterSessionNotification(notifier);
            }
        }
    }
}

/// Registra o callback de dispositivos e o de sessões nos endpoints ativos agora.
/// Falha ao registrar um endpoint específico é ignorada (os demais continuam cobertos).
#[cfg(windows)]
fn register_all(sender: &Sender<AudioEvent>) -> Result<Registrations, AudioError> {
    let enumerator = create_enumerator()?;
    let client: IMMNotificationClient = DeviceNotifier { tx: sender.clone() }.into();
    // SAFETY: `client` é um objeto COM válido; `Registrations` o mantém vivo até cancelar.
    unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }.map_err(to_audio_error)?;
    let mut registrations = Registrations {
        enumerator,
        client,
        sessions: Vec::new(),
    };
    for flow in [Flow::Render, Flow::Capture] {
        for (device, _id) in active_endpoints(&registrations.enumerator, flow).unwrap_or_default() {
            // SAFETY: ativação padrão do gerenciador de sessões do endpoint.
            let Ok(manager) = (unsafe { device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) })
            else {
                continue;
            };
            let notifier: IAudioSessionNotification = SessionNotifier { tx: sender.clone() }.into();
            if register_session_notifier(&manager, &notifier).is_ok() {
                registrations.sessions.push((manager, notifier));
            }
        }
    }
    Ok(registrations)
}

/// Corpo do thread de notificações. COM em MTA: os callbacks chegam em threads RPC do
/// próprio MTA. Um thread STA sem bomba de mensagens nunca receberia esses avisos.
#[cfg(windows)]
fn run_watch(
    sender: Sender<AudioEvent>,
    stop: Receiver<()>,
    ready: Sender<Result<(), AudioError>>,
) {
    // SAFETY: inicialização de COM de um thread novo, dedicado a este loop.
    let init = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if init.is_err() {
        let _ = ready.send(Err(AudioError::Os(init.0)));
        return;
    }
    match register_all(&sender) {
        Ok(registrations) => {
            let _ = ready.send(Ok(()));
            // Bloqueia até o WatchHandle cair e soltar o Sender de `stop`.
            let _ = stop.recv();
            drop(registrations);
        }
        Err(e) => {
            let _ = ready.send(Err(e));
        }
    }
    // SAFETY: par do CoInitializeEx acima; os objetos COM já foram soltos.
    unsafe { CoUninitialize() };
}

/// Mantém as assinaturas de `watch` vivas. Ao ser descartado, cancela os callbacks de
/// dispositivo e de sessão e espera o thread de notificações encerrar.
#[cfg(windows)]
pub struct WatchHandle {
    stop: Option<Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(windows)]
impl Drop for WatchHandle {
    fn drop(&mut self) {
        // Soltar o Sender faz o `recv` do thread voltar com erro: ele cancela e sai.
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Avisa por `sender` quando dispositivos mudam (`DevicesChanged`) ou quando surge
/// sessão nova (`SessionsChanged`). Enquanto o `WatchHandle` existir, as assinaturas valem.
///
/// As assinaturas vivem num thread próprio em COM MTA, então o `WatchHandle` pode ser
/// descartado em qualquer thread.
///
/// Limitação: as notificações de sessão são registradas só nos endpoints ativos no
/// momento desta chamada. Um dispositivo que fica ativo depois não recebe esse aviso;
/// quem consome deve reenumerar em `DevicesChanged`, e um refresh periódico virá depois.
#[cfg(windows)]
pub fn watch(sender: Sender<AudioEvent>) -> Result<WatchHandle, AudioError> {
    let (stop_tx, stop_rx) = channel::<()>();
    let (ready_tx, ready_rx) = channel::<Result<(), AudioError>>();
    let thread = std::thread::Builder::new()
        .name("sonar-audio-watch".into())
        .spawn(move || run_watch(sender, stop_rx, ready_tx))
        .map_err(|e| AudioError::Unsupported(format!("thread de notificações: {e}")))?;
    match ready_rx.recv() {
        Ok(Ok(())) => Ok(WatchHandle {
            stop: Some(stop_tx),
            thread: Some(thread),
        }),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            let _ = thread.join();
            Err(AudioError::Unsupported("thread de notificações encerrou".into()))
        }
    }
}

#[cfg(test)]
mod skip_tests {
    use super::collect_ok;

    #[test]
    fn failing_item_does_not_hide_others() {
        let items: Vec<Result<u32, &str>> = vec![Ok(1), Err("sumiu"), Ok(3), Err("sumiu"), Ok(5)];
        assert_eq!(collect_ok(items), vec![1, 3, 5]);
    }

    #[test]
    fn all_failing_items_give_empty_list() {
        let items: Vec<Result<u32, &str>> = vec![Err("a"), Err("b")];
        assert!(collect_ok(items).is_empty());
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Manual e somente leitura: imprime dispositivos e sessões. Não altera rotas nem volumes.
    /// Rodar com `cargo test manual_list_devices_and_sessions -- --ignored --nocapture`.
    #[test]
    #[ignore = "manual: imprime dispositivos e sessões (somente leitura)"]
    fn manual_list_devices_and_sessions() {
        let source = WindowsAudioSource;
        let devices = source.devices().expect("listar dispositivos");
        println!("Dispositivos ({}):", devices.len());
        for d in &devices {
            println!(
                "  {:?} \"{}\" conectado={} padrao={} [{}]",
                d.flow, d.name, d.connected, d.is_default, d.id.0
            );
        }
        let sessions = source.sessions().expect("listar sessões");
        println!("Sessões ({}):", sessions.len());
        for s in &sessions {
            println!(
                "  pid={} {} {:?} ativa={} volume={:.2} mudo={} [{}]",
                s.pid, s.exe, s.flow, s.active, s.volume, s.muted, s.device.0
            );
        }
    }
}
