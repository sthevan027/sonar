//! `Engine`: dono do estado do app. Junta fontes do sistema, regras, perfis e metadados,
//! aplica as regras no Windows e avisa a interface (`notify`) a cada mudança.
//!
//! Travamento: o estado fica num `Mutex` interno. O notificador é chamado sempre depois
//! de soltar o travamento, então ele pode ler o estado sem deadlock.

use crate::audio::default_device::DefaultDevice;
use crate::audio::endpoint::EndpointControl;
use crate::audio::types::{AudioError, AudioSession, AudioSource, Device, DeviceId, Flow, RouteBackend};
use crate::profiles::{self, Change, NameError, Profile, Target};
use crate::rules::{reconcile, Outcome, Rule, RuleSet};
use crate::state::{AppState, DeviceMeta, DeviceView, Settings};
use crate::store::Store;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Estado mutável, sempre acessado sob o travamento do `Engine`.
struct Inner {
    devices: Vec<Device>,
    sessions: Vec<AudioSession>,
    rules: RuleSet,
    profiles: Vec<Profile>,
    settings: Settings,
    /// Metadados por id de endpoint; só entradas com algum campo preenchido.
    meta: BTreeMap<String, DeviceMeta>,
    routing_available: bool,
    last_profile: Option<String>,
    /// Mensagem da última gravação que falhou; `None` quando a última gravação deu certo.
    save_error: Option<String>,
}

pub struct Engine {
    source: Arc<dyn AudioSource>,
    backend: Arc<dyn RouteBackend>,
    ctl: Arc<dyn EndpointControl>,
    defaults: Arc<dyn DefaultDevice>,
    store: Store,
    notify: Arc<dyn Fn(&AppState) + Send + Sync>,
    inner: Mutex<Inner>,
}

impl Engine {
    /// Carrega tudo do disco antes de qualquer gravação: nada é salvo antes de ser lido.
    pub fn new(
        source: Arc<dyn AudioSource>,
        backend: Arc<dyn RouteBackend>,
        ctl: Arc<dyn EndpointControl>,
        defaults: Arc<dyn DefaultDevice>,
        store: Store,
        notify: Arc<dyn Fn(&AppState) + Send + Sync>,
    ) -> Engine {
        let settings: Settings = store.load("settings");
        let meta: BTreeMap<String, DeviceMeta> = store.load("devices");
        let rules: RuleSet = store.load("rules");
        let profiles: Vec<Profile> = store.load("profiles");
        let inner = Inner {
            devices: Vec::new(),
            sessions: Vec::new(),
            rules: normalize_rules(rules),
            profiles: profiles.into_iter().map(normalize_profile).collect(),
            settings,
            meta: meta.into_iter().filter(|(_, m)| m.is_meaningful()).collect(),
            routing_available: true,
            last_profile: None,
            save_error: None,
        };
        Engine {
            source,
            backend,
            ctl,
            defaults,
            store,
            notify,
            inner: Mutex::new(inner),
        }
    }

    /// Relê dispositivos e sessões, aplica as regras e avisa a interface.
    pub fn refresh(&self) {
        let state = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            self.sync(inner);
            build_state(inner, self.ctl.as_ref())
        };
        (self.notify)(&state);
    }

    /// Grava (ou, com `None`, remove) a regra do executável e aplica nas sessões abertas.
    pub fn set_app_route(&self, exe: &str, flow: Flow, device: Option<DeviceId>) {
        let exe = exe.to_lowercase();
        let clearing = device.is_none();
        let state = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            match device {
                Some(device) => inner.rules.upsert(Rule {
                    exe: exe.clone(),
                    flow,
                    device,
                }),
                None => inner.rules.remove(&exe, flow),
            }
            persist(&self.store, &mut inner.save_error, "rules", &inner.rules);
            self.sync(inner);
            if clearing {
                clear_sessions(self.backend.as_ref(), &inner.sessions, &exe, flow);
                self.reread_sessions(inner);
            }
            build_state(inner, self.ctl.as_ref())
        };
        (self.notify)(&state);
    }

    /// Ativa um perfil. `false` quando o id não existe (nada muda e nada é avisado).
    pub fn activate_profile(&self, id: &str) -> bool {
        let state = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            let Some(profile) = inner.profiles.iter().find(|p| p.id == id).cloned() else {
                return false;
            };
            let mut rules = inner.rules.clone();
            let changes = profiles::activate(&profile, &mut rules);
            inner.rules = rules;
            persist(&self.store, &mut inner.save_error, "rules", &inner.rules);
            // As regras novas (`Change::Set`) entram pelo `sync`; as limpezas são feitas aqui.
            self.sync(inner);
            let mut cleared = false;
            for change in changes {
                if let Change::Cleared { exe, flow } = change {
                    clear_sessions(self.backend.as_ref(), &inner.sessions, &exe, flow);
                    cleared = true;
                }
            }
            if cleared {
                self.reread_sessions(inner);
            }
            inner.last_profile = Some(id.to_string());
            build_state(inner, self.ctl.as_ref())
        };
        (self.notify)(&state);
        true
    }

    /// Cria ou substitui um perfil pelo id. Nome inválido é recusado sem gravar nada.
    pub fn save_profile(&self, mut p: Profile) -> Result<(), NameError> {
        p.name = profiles::validate_name(p.name.as_deref().unwrap_or(""))?;
        let p = normalize_profile(p);
        let state = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            match inner.profiles.iter_mut().find(|existing| existing.id == p.id) {
                Some(existing) => *existing = p,
                None => inner.profiles.push(p),
            }
            persist(&self.store, &mut inner.save_error, "profiles", &inner.profiles);
            build_state(inner, self.ctl.as_ref())
        };
        (self.notify)(&state);
        Ok(())
    }

    pub fn delete_profile(&self, id: &str) {
        let state = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            inner.profiles.retain(|p| p.id != id);
            if inner.last_profile.as_deref() == Some(id) {
                inner.last_profile = None;
            }
            persist(&self.store, &mut inner.save_error, "profiles", &inner.profiles);
            build_state(inner, self.ctl.as_ref())
        };
        (self.notify)(&state);
    }

    /// Nome vazio (ou só espaços) restaura o nome do Windows. Nome inválido devolve erro
    /// e o metadado anterior fica como está.
    pub fn update_device_meta(
        &self,
        id: &DeviceId,
        name: &str,
        icon: Option<String>,
        hidden: bool,
    ) -> Result<(), NameError> {
        let name = profiles::validate_name(name)?;
        let icon = icon
            .map(|i| i.trim().to_string())
            .filter(|i| !i.is_empty());
        let meta = DeviceMeta { name, icon, hidden };
        let state = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            // Metadado vazio equivale a não ter metadado: a entrada some do arquivo.
            if meta.is_meaningful() {
                inner.meta.insert(id.0.clone(), meta);
            } else {
                inner.meta.remove(&id.0);
            }
            persist(&self.store, &mut inner.save_error, "devices", &inner.meta);
            build_state(inner, self.ctl.as_ref())
        };
        (self.notify)(&state);
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    pub fn save_settings(&self, s: Settings) {
        let state = {
            let mut guard = self.lock();
            let inner = &mut *guard;
            inner.settings = s;
            persist(&self.store, &mut inner.save_error, "settings", &inner.settings);
            build_state(inner, self.ctl.as_ref())
        };
        (self.notify)(&state);
    }

    /// Snapshot do estado atual. Relê só volume e mudo dos dispositivos conectados.
    pub fn state(&self) -> AppState {
        build_state(&self.lock(), self.ctl.as_ref())
    }

    /// Volume de um app no sentido. Grava no sistema e, se der certo, avisa a interface.
    pub fn set_session_volume(&self, pid: u32, flow: Flow, v: f32) -> Result<(), AudioError> {
        self.ctl.set_session_volume(pid, flow, v)?;
        self.refresh();
        Ok(())
    }

    /// Mudo de um app no sentido. Mesmo fluxo de `set_session_volume`.
    pub fn set_session_muted(&self, pid: u32, flow: Flow, m: bool) -> Result<(), AudioError> {
        self.ctl.set_session_muted(pid, flow, m)?;
        self.refresh();
        Ok(())
    }

    pub fn set_device_volume(&self, id: &DeviceId, v: f32) -> Result<(), AudioError> {
        self.ctl.set_volume(id, v)?;
        self.refresh();
        Ok(())
    }

    pub fn set_device_muted(&self, id: &DeviceId, m: bool) -> Result<(), AudioError> {
        self.ctl.set_muted(id, m)?;
        self.refresh();
        Ok(())
    }

    /// Troca o padrão do Windows no sentido.
    pub fn set_default_device(&self, id: &DeviceId, flow: Flow) -> Result<(), AudioError> {
        self.defaults.set_default(id, flow)?;
        self.refresh();
        Ok(())
    }

    /// Pico atual do endpoint para o medidor. Não avisa a interface (roda ~15 vezes por
    /// segundo). Erro de leitura vira `0.0`: o medidor fica parado.
    pub fn mic_peak(&self, id: &DeviceId) -> f32 {
        self.ctl.peak(id).unwrap_or(0.0)
    }

    /// Microfone padrão conectado, do último sincronismo. Não toca no sistema.
    pub fn default_capture(&self) -> Option<DeviceId> {
        self.lock()
            .devices
            .iter()
            .find(|d| d.flow == Flow::Capture && d.is_default && d.connected)
            .map(|d| d.id.clone())
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Veneno do travamento: o estado continua válido o bastante para seguir.
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Relê dispositivos e sessões (mantém o último valor bom se a leitura falhar), aplica
    /// as regras e atualiza `routing_available`. Não notifica: quem chama decide quando avisar.
    fn sync(&self, inner: &mut Inner) {
        if let Ok(devices) = self.source.devices() {
            inner.devices = devices;
        }
        if let Ok(sessions) = self.source.sessions() {
            inner.sessions = sessions;
        }
        let outcomes = reconcile(
            &inner.rules,
            &inner.devices,
            &inner.sessions,
            self.backend.as_ref(),
        );
        if outcomes.iter().any(|o| matches!(o, Outcome::Applied { .. })) {
            // Uma rota recém-gravada muda o dispositivo da sessão: relê para não mostrar o antigo.
            self.reread_sessions(inner);
        }
        // Trava: cai no primeiro Unsupported e só volta com um roteamento bem-sucedido.
        if outcomes.iter().any(|o| {
            matches!(
                o,
                Outcome::Skipped {
                    reason: AudioError::Unsupported(_),
                    ..
                }
            )
        }) {
            inner.routing_available = false;
        } else if outcomes.iter().any(|o| {
            matches!(o, Outcome::Applied { .. } | Outcome::AlreadyRouted { .. })
        }) {
            inner.routing_available = true;
        }
    }

    /// Relê só as sessões (dispositivo atual de cada uma). Erro de leitura mantém o último valor.
    fn reread_sessions(&self, inner: &mut Inner) {
        if let Ok(sessions) = self.source.sessions() {
            inner.sessions = sessions;
        }
    }
}

/// Limpa a rota de cada sessão do executável no sentido. Falha numa sessão (ex.: já
/// encerrada) não impede as outras.
fn clear_sessions(
    backend: &dyn RouteBackend,
    sessions: &[AudioSession],
    exe: &str,
    flow: Flow,
) {
    for session in sessions
        .iter()
        .filter(|s| s.flow == flow && s.exe.eq_ignore_ascii_case(exe))
    {
        let _ = backend.clear_route(session.pid, flow);
    }
}

/// Grava um documento e atualiza `save_error`: mensagem quando a gravação falha, `None`
/// quando dá certo (a próxima gravação bem-sucedida limpa o erro). A falha também vai para
/// o stderr. O estado em memória segue válido e a próxima mudança tenta de novo.
fn persist<T: Serialize>(store: &Store, save_error: &mut Option<String>, name: &str, value: &T) {
    match store.save(name, value) {
        Ok(()) => *save_error = None,
        Err(err) => {
            eprintln!("sonar: não foi possível gravar {name}.json: {err}");
            *save_error = Some(format!("Não foi possível gravar {name}.json: {err}"));
        }
    }
}

/// Regras sem dispositivo são descartadas; o `upsert` deixa o executável em minúsculo.
fn normalize_rules(raw: RuleSet) -> RuleSet {
    let mut set = RuleSet::default();
    for rule in raw.rules {
        if !rule.device.0.is_empty() {
            set.upsert(rule);
        }
    }
    set
}

/// Descarta itens do perfil que apontam para um dispositivo sem id.
fn normalize_profile(mut profile: Profile) -> Profile {
    profile.items.retain(|item| {
        !has_empty_device(&item.output) && !has_empty_device(&item.input)
    });
    profile
}

fn has_empty_device(target: &Target) -> bool {
    matches!(target, Target::Device(id) if id.0.is_empty())
}

/// Monta o snapshot para a interface. Um dispositivo aparece se estiver conectado, se
/// alguma regra apontar para ele ou se tiver metadado; o resto (Windows lembra de
/// muitos dispositivos antigos) fica de fora.
fn build_state(inner: &Inner, ctl: &dyn EndpointControl) -> AppState {
    let devices = inner
        .devices
        .iter()
        .filter_map(|d| {
            let has_rules = inner.rules.rules.iter().any(|r| r.device == d.id);
            let has_meta = inner.meta.contains_key(&d.id.0);
            if !(d.connected || has_rules || has_meta) {
                return None;
            }
            let meta = inner.meta.get(&d.id.0).cloned().unwrap_or_default();
            // Endpoint desconectado não tem volume para ler: mostra o padrão.
            let (volume, muted) = if d.connected {
                (
                    ctl.volume(&d.id).unwrap_or(1.0),
                    ctl.muted(&d.id).unwrap_or(false),
                )
            } else {
                (1.0, false)
            };
            Some(DeviceView {
                id: d.id.clone(),
                flow: d.flow,
                windows_name: d.name.clone(),
                display_name: meta.name.clone().unwrap_or_else(|| d.name.clone()),
                name: meta.name,
                icon: meta.icon,
                hidden: meta.hidden,
                connected: d.connected,
                is_default: d.is_default,
                has_rules,
                volume,
                muted,
            })
        })
        .collect();
    AppState {
        devices,
        sessions: inner.sessions.clone(),
        rules: inner.rules.rules.clone(),
        profiles: inner.profiles.clone(),
        settings: inner.settings.clone(),
        routing_available: inner.routing_available,
        last_profile: inner.last_profile.clone(),
        save_error: inner.save_error.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::default_device::DefaultDevice;
    use crate::audio::endpoint::EndpointControl;
    use crate::audio::types::{AudioError, AudioSession, Device};
    use crate::profiles::{Profile, ProfileItem, Target};
    use crate::rules::Rule;
    use crate::state::DeviceMeta;
    use std::collections::{BTreeMap, HashMap};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    // ---------- fakes ----------

    /// Fonte de dispositivos e sessões controlada pelo teste.
    #[derive(Default)]
    struct FakeSource {
        devices: Mutex<Vec<Device>>,
        sessions: Mutex<Vec<AudioSession>>,
        fail_reads: AtomicBool,
    }

    impl FakeSource {
        fn set_devices(&self, devices: Vec<Device>) {
            *self.devices.lock().unwrap() = devices;
        }

        fn set_sessions(&self, sessions: Vec<AudioSession>) {
            *self.sessions.lock().unwrap() = sessions;
        }

        fn move_session(&self, pid: u32, flow: Flow, device: &DeviceId) {
            for session in self
                .sessions
                .lock()
                .unwrap()
                .iter_mut()
                .filter(|s| s.pid == pid && s.flow == flow)
            {
                session.device = device.clone();
            }
        }
    }

    impl AudioSource for FakeSource {
        fn devices(&self) -> Result<Vec<Device>, AudioError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(AudioError::Os(-1));
            }
            Ok(self.devices.lock().unwrap().clone())
        }

        fn sessions(&self) -> Result<Vec<AudioSession>, AudioError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(AudioError::Os(-1));
            }
            Ok(self.sessions.lock().unwrap().clone())
        }
    }

    /// Backend que grava chamadas. `get_route` reflete os `set_route` anteriores.
    #[derive(Default)]
    struct FakeBackend {
        routes: Mutex<HashMap<u32, DeviceId>>,
        set_calls: Mutex<Vec<(u32, Flow, DeviceId)>>,
        clear_calls: Mutex<Vec<(u32, Flow)>>,
        /// Quando verdadeiro, `set_route` devolve `Unsupported`.
        unsupported: AtomicBool,
        /// Fonte espelhada: após gravar ou limpar uma rota, a sessão do PID passa para o
        /// dispositivo novo (limpar leva ao padrão `padrao`), como o Windows faria.
        mirror: Mutex<Option<Arc<FakeSource>>>,
    }

    impl FakeBackend {
        fn set_calls(&self) -> Vec<(u32, Flow, DeviceId)> {
            self.set_calls.lock().unwrap().clone()
        }

        fn clear_calls(&self) -> Vec<(u32, Flow)> {
            self.clear_calls.lock().unwrap().clone()
        }

        fn preset_route(&self, pid: u32, device: &DeviceId) {
            self.routes.lock().unwrap().insert(pid, device.clone());
        }

        fn mirror_to(&self, pid: u32, flow: Flow, device: &DeviceId) {
            if let Some(source) = self.mirror.lock().unwrap().as_ref() {
                source.move_session(pid, flow, device);
            }
        }
    }

    impl RouteBackend for FakeBackend {
        fn set_route(&self, pid: u32, flow: Flow, device: &DeviceId) -> Result<(), AudioError> {
            self.set_calls
                .lock()
                .unwrap()
                .push((pid, flow, device.clone()));
            if self.unsupported.load(Ordering::SeqCst) {
                return Err(AudioError::Unsupported("teste".into()));
            }
            self.routes.lock().unwrap().insert(pid, device.clone());
            self.mirror_to(pid, flow, device);
            Ok(())
        }

        fn clear_route(&self, pid: u32, flow: Flow) -> Result<(), AudioError> {
            self.clear_calls.lock().unwrap().push((pid, flow));
            self.routes.lock().unwrap().remove(&pid);
            self.mirror_to(pid, flow, &dev_id("padrao"));
            Ok(())
        }

        fn get_route(&self, pid: u32, _flow: Flow) -> Result<Option<DeviceId>, AudioError> {
            Ok(self.routes.lock().unwrap().get(&pid).cloned())
        }
    }

    /// Chamada gravada no `EndpointControl` falso.
    #[derive(Debug, Clone, PartialEq)]
    enum CtlCall {
        SetVolume(String, f32),
        SetMuted(String, bool),
        SetSessionVolume(u32, Flow, f32),
        SetSessionMuted(u32, Flow, bool),
    }

    /// Controle de endpoints falso: grava as escritas e devolve os valores preparados
    /// pelo teste (padrão: volume 1.0, sem mudo, pico 0.0).
    #[derive(Default)]
    struct FakeCtl {
        volumes: Mutex<HashMap<String, f32>>,
        mutes: Mutex<HashMap<String, bool>>,
        peaks: Mutex<HashMap<String, f32>>,
        calls: Mutex<Vec<CtlCall>>,
        /// Leituras (volume, mudo, pico) falham quando verdadeiro.
        fail_reads: AtomicBool,
        /// Escritas falham quando verdadeiro; a chamada continua gravada.
        fail_writes: AtomicBool,
    }

    impl FakeCtl {
        fn calls(&self) -> Vec<CtlCall> {
            self.calls.lock().unwrap().clone()
        }

        fn set_volume_of(&self, id: &str, v: f32) {
            self.volumes.lock().unwrap().insert(id.to_string(), v);
        }

        fn set_mute_of(&self, id: &str, m: bool) {
            self.mutes.lock().unwrap().insert(id.to_string(), m);
        }

        fn set_peak_of(&self, id: &str, p: f32) {
            self.peaks.lock().unwrap().insert(id.to_string(), p);
        }

        fn record(&self, call: CtlCall) -> Result<(), AudioError> {
            self.calls.lock().unwrap().push(call);
            if self.fail_writes.load(Ordering::SeqCst) {
                return Err(AudioError::Os(-3));
            }
            Ok(())
        }

        fn read_guard(&self) -> Result<(), AudioError> {
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(AudioError::Os(-2));
            }
            Ok(())
        }
    }

    impl EndpointControl for FakeCtl {
        fn volume(&self, id: &DeviceId) -> Result<f32, AudioError> {
            self.read_guard()?;
            Ok(self.volumes.lock().unwrap().get(&id.0).copied().unwrap_or(1.0))
        }
        fn set_volume(&self, id: &DeviceId, v: f32) -> Result<(), AudioError> {
            self.record(CtlCall::SetVolume(id.0.clone(), v))?;
            self.volumes.lock().unwrap().insert(id.0.clone(), v);
            Ok(())
        }
        fn muted(&self, id: &DeviceId) -> Result<bool, AudioError> {
            self.read_guard()?;
            Ok(self.mutes.lock().unwrap().get(&id.0).copied().unwrap_or(false))
        }
        fn set_muted(&self, id: &DeviceId, m: bool) -> Result<(), AudioError> {
            self.record(CtlCall::SetMuted(id.0.clone(), m))?;
            self.mutes.lock().unwrap().insert(id.0.clone(), m);
            Ok(())
        }
        fn peak(&self, id: &DeviceId) -> Result<f32, AudioError> {
            self.read_guard()?;
            Ok(self.peaks.lock().unwrap().get(&id.0).copied().unwrap_or(0.0))
        }
        fn set_session_volume(&self, pid: u32, flow: Flow, v: f32) -> Result<(), AudioError> {
            self.record(CtlCall::SetSessionVolume(pid, flow, v))
        }
        fn set_session_muted(&self, pid: u32, flow: Flow, m: bool) -> Result<(), AudioError> {
            self.record(CtlCall::SetSessionMuted(pid, flow, m))
        }
    }

    /// Troca de padrão falsa: grava cada pedido; `fail` faz a troca falhar.
    #[derive(Default)]
    struct FakeDefaults {
        calls: Mutex<Vec<(DeviceId, Flow)>>,
        fail: AtomicBool,
    }

    impl FakeDefaults {
        fn calls(&self) -> Vec<(DeviceId, Flow)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl DefaultDevice for FakeDefaults {
        fn set_default(&self, id: &DeviceId, flow: Flow) -> Result<(), AudioError> {
            self.calls.lock().unwrap().push((id.clone(), flow));
            if self.fail.load(Ordering::SeqCst) {
                return Err(AudioError::Os(-4));
            }
            Ok(())
        }
    }

    // ---------- montagem ----------

    /// Fontes e contador de notificações compartilhados entre motores de um mesmo teste.
    struct World {
        source: Arc<FakeSource>,
        backend: Arc<FakeBackend>,
        ctl: Arc<FakeCtl>,
        defaults: Arc<FakeDefaults>,
        notes: Arc<AtomicUsize>,
    }

    impl World {
        fn new() -> World {
            let source = Arc::new(FakeSource::default());
            let backend = Arc::new(FakeBackend::default());
            *backend.mirror.lock().unwrap() = Some(source.clone());
            World {
                source,
                backend,
                ctl: Arc::new(FakeCtl::default()),
                defaults: Arc::new(FakeDefaults::default()),
                notes: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn engine_at(&self, dir: &Path) -> Engine {
            let notes = self.notes.clone();
            Engine::new(
                self.source.clone(),
                self.backend.clone(),
                self.ctl.clone(),
                self.defaults.clone(),
                Store::open(dir.to_path_buf()),
                Arc::new(move |_state: &AppState| {
                    notes.fetch_add(1, Ordering::SeqCst);
                }),
            )
        }

        fn notes(&self) -> usize {
            self.notes.load(Ordering::SeqCst)
        }
    }

    fn dev_id(id: &str) -> DeviceId {
        DeviceId(id.to_string())
    }

    fn device(id: &str, flow: Flow, connected: bool, name: &str) -> Device {
        Device {
            id: dev_id(id),
            flow,
            name: name.to_string(),
            connected,
            is_default: false,
        }
    }

    fn session(pid: u32, exe: &str, flow: Flow, device: &str) -> AudioSession {
        AudioSession {
            pid,
            exe: exe.to_string(),
            flow,
            device: dev_id(device),
            active: true,
            volume: 0.8,
            muted: false,
        }
    }

    fn rule(exe: &str, flow: Flow, device: &str) -> Rule {
        Rule {
            exe: exe.to_string(),
            flow,
            device: dev_id(device),
        }
    }

    fn profile(id: &str, items: Vec<ProfileItem>) -> Profile {
        Profile {
            id: id.to_string(),
            icon: "game".to_string(),
            name: None,
            items,
        }
    }

    fn save_rules(dir: &Path, rules: Vec<Rule>) {
        Store::open(dir.to_path_buf())
            .save("rules", &RuleSet { rules })
            .unwrap();
    }

    fn device_ids(state: &AppState) -> Vec<String> {
        state.devices.iter().map(|d| d.id.0.clone()).collect()
    }

    // ---------- testes ----------

    #[test]
    fn engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Engine>();
    }

    #[test]
    fn refresh_applies_rules_to_new_session() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(dir.path(), vec![rule("chrome.exe", Flow::Render, "fone")]);
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "alto")]);
        let before = world.notes();

        engine.refresh();

        assert_eq!(
            world.backend.set_calls(),
            vec![(10, Flow::Render, dev_id("fone"))]
        );
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn set_app_route_creates_rule_and_routes_existing_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world.source.set_sessions(vec![
            session(10, "chrome.exe", Flow::Render, "alto"),
            session(11, "chrome.exe", Flow::Render, "alto"),
        ]);
        let before = world.notes();

        engine.set_app_route("Chrome.EXE", Flow::Render, Some(dev_id("fone")));

        let expected = rule("chrome.exe", Flow::Render, "fone");
        assert_eq!(engine.state().rules, vec![expected.clone()]);
        assert_eq!(
            world.backend.set_calls(),
            vec![
                (10, Flow::Render, dev_id("fone")),
                (11, Flow::Render, dev_id("fone")),
            ]
        );
        let on_disk: RuleSet = Store::open(dir.path().to_path_buf()).load("rules");
        assert_eq!(on_disk.rules, vec![expected]);
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn set_app_route_none_clears_rule_and_routes() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(dir.path(), vec![rule("chrome.exe", Flow::Render, "fone")]);
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "fone")]);
        world.backend.preset_route(10, &dev_id("fone"));
        let before = world.notes();

        engine.set_app_route("chrome.exe", Flow::Render, None);

        assert!(engine.state().rules.is_empty());
        assert_eq!(world.backend.clear_calls(), vec![(10, Flow::Render)]);
        // A sessão já aparece no padrão do Windows, não no dispositivo antigo.
        assert_eq!(engine.state().sessions[0].device, dev_id("padrao"));
        assert!(world.backend.set_calls().is_empty());
        let on_disk: RuleSet = Store::open(dir.path().to_path_buf()).load("rules");
        assert!(on_disk.rules.is_empty());
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn activate_profile_updates_rules_and_applies() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            device("mic", Flow::Capture, true, "Microfone"),
        ]);
        world.source.set_sessions(vec![
            session(20, "game.exe", Flow::Render, "alto"),
            session(21, "game.exe", Flow::Capture, "mic"),
        ]);
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Default,
                }],
            ))
            .unwrap();
        let before = world.notes();

        assert!(engine.activate_profile("p1"));

        assert_eq!(
            engine.state().rules,
            vec![rule("game.exe", Flow::Render, "fone")]
        );
        assert_eq!(
            world.backend.set_calls(),
            vec![(20, Flow::Render, dev_id("fone"))]
        );
        assert_eq!(world.backend.clear_calls(), vec![(21, Flow::Capture)]);
        let capture = engine
            .state()
            .sessions
            .into_iter()
            .find(|s| s.pid == 21)
            .unwrap();
        assert_eq!(capture.device, dev_id("padrao"));
        assert_eq!(engine.state().last_profile.as_deref(), Some("p1"));
        let on_disk: RuleSet = Store::open(dir.path().to_path_buf()).load("rules");
        assert_eq!(on_disk.rules, vec![rule("game.exe", Flow::Render, "fone")]);
        assert_eq!(world.notes(), before + 1);

        let before = world.notes();
        assert!(!engine.activate_profile("nao-existe"));
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn update_device_meta_rejects_21_chars_and_keeps_old_name() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Alto")]);
        engine.refresh();
        let fone = dev_id("fone");
        engine
            .update_device_meta(&fone, "Monitor", None, false)
            .unwrap();
        assert_eq!(engine.state().devices[0].display_name, "Monitor");
        let devices_file = dir.path().join("devices.json");
        let bytes_before = std::fs::read(&devices_file).unwrap();
        let before = world.notes();

        let too_long = "a".repeat(21);
        let result = engine.update_device_meta(&fone, &too_long, Some("speaker".into()), true);

        assert_eq!(result, Err(NameError::TooLong));
        assert_eq!(engine.state().devices[0].display_name, "Monitor");
        assert!(!engine.state().devices[0].hidden);
        assert_eq!(std::fs::read(&devices_file).unwrap(), bytes_before);
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn update_device_meta_empty_name_restores_windows_name() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Alto")]);
        engine.refresh();
        let fone = dev_id("fone");
        engine
            .update_device_meta(&fone, "Monitor", None, false)
            .unwrap();
        let before = world.notes();

        engine
            .update_device_meta(&fone, "   ", None, false)
            .unwrap();

        let view = &engine.state().devices[0];
        assert_eq!(view.name, None);
        assert_eq!(view.display_name, "Alto");
        assert_eq!(view.windows_name, "Alto");
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn routing_unavailable_when_backend_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(dir.path(), vec![rule("game.exe", Flow::Render, "fone")]);
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(30, "game.exe", Flow::Render, "alto")]);
        world.backend.unsupported.store(true, Ordering::SeqCst);

        engine.refresh();

        let state = engine.state();
        assert!(!state.routing_available);
        // Volumes não dependem do roteamento: a sessão e o dispositivo seguem no estado.
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].pid, 30);
        assert_eq!(state.sessions[0].volume, 0.8);
        assert!(!state.sessions[0].muted);
        assert_eq!(device_ids(&state), vec!["fone".to_string()]);
    }

    #[test]
    fn disconnected_device_without_rule_or_meta_is_not_listed() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(dir.path(), vec![rule("spotify.exe", Flow::Render, "monitor")]);
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            device("antigo", Flow::Render, false, "Antigo"),
            device("monitor", Flow::Render, false, "Monitor"),
            device("velho", Flow::Render, false, "Velho"),
        ]);
        engine.refresh();
        engine
            .update_device_meta(&dev_id("velho"), "", None, true)
            .unwrap();

        let state = engine.state();
        assert_eq!(
            device_ids(&state),
            vec!["fone".to_string(), "monitor".to_string(), "velho".to_string()]
        );
        let monitor = state.devices.iter().find(|d| d.id.0 == "monitor").unwrap();
        assert!(monitor.has_rules);
        let velho = state.devices.iter().find(|d| d.id.0 == "velho").unwrap();
        assert!(velho.hidden);

        // Dar nome e depois apagar o nome deixa o dispositivo sem metadado: some da lista.
        let antigo = dev_id("antigo");
        engine
            .update_device_meta(&antigo, "Nome", None, false)
            .unwrap();
        assert!(device_ids(&engine.state()).contains(&"antigo".to_string()));
        engine.update_device_meta(&antigo, "", None, false).unwrap();
        assert!(!device_ids(&engine.state()).contains(&"antigo".to_string()));
        let meta: BTreeMap<String, DeviceMeta> =
            Store::open(dir.path().to_path_buf()).load("devices");
        assert!(!meta.contains_key("antigo"));
    }

    #[test]
    fn rules_persist_across_engine_restart() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        {
            let engine = world.engine_at(dir.path());
            engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));
        }

        let second = world.engine_at(dir.path());

        assert_eq!(
            second.state().rules,
            vec![rule("chrome.exe", Flow::Render, "fone")]
        );
    }

    #[test]
    fn load_normalizes_uppercase_exe_and_drops_empty_device_ids() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        store
            .save(
                "rules",
                &RuleSet {
                    rules: vec![
                        rule("Chrome.EXE", Flow::Render, "fone"),
                        rule("spotify.exe", Flow::Capture, ""),
                    ],
                },
            )
            .unwrap();
        store
            .save(
                "profiles",
                &vec![profile(
                    "p1",
                    vec![
                        ProfileItem {
                            exe: "game.exe".to_string(),
                            output: Target::Device(dev_id("")),
                            input: Target::Keep,
                        },
                        ProfileItem {
                            exe: "chat.exe".to_string(),
                            output: Target::Device(dev_id("fone")),
                            input: Target::Default,
                        },
                    ],
                )],
            )
            .unwrap();
        let mut meta = BTreeMap::new();
        meta.insert("antigo".to_string(), DeviceMeta::default());
        store.save("devices", &meta).unwrap();
        let world = World::new();
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            device("antigo", Flow::Render, false, "Antigo"),
        ]);

        let engine = world.engine_at(dir.path());
        engine.refresh();
        let state = engine.state();

        assert_eq!(
            state.rules,
            vec![rule("chrome.exe", Flow::Render, "fone")]
        );
        assert_eq!(state.profiles.len(), 1);
        assert_eq!(
            state.profiles[0].items,
            vec![ProfileItem {
                exe: "chat.exe".to_string(),
                output: Target::Device(dev_id("fone")),
                input: Target::Default,
            }]
        );
        assert_eq!(device_ids(&state), vec!["fone".to_string()]);
    }

    #[test]
    fn refresh_keeps_snapshot_when_source_fails() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "fone")]);
        engine.refresh();

        world.source.fail_reads.store(true, Ordering::SeqCst);
        engine.refresh();

        let state = engine.state();
        assert_eq!(device_ids(&state), vec!["fone".to_string()]);
        assert_eq!(state.sessions.len(), 1);
    }

    #[test]
    fn routing_unavailable_latches_until_a_route_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(dir.path(), vec![rule("game.exe", Flow::Render, "fone")]);
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(30, "game.exe", Flow::Render, "alto")]);
        world.backend.unsupported.store(true, Ordering::SeqCst);
        engine.refresh();
        assert!(!engine.state().routing_available);

        // Sem sessão casando com regra não há resultados: continua falso.
        world.source.set_sessions(vec![]);
        engine.refresh();
        assert!(!engine.state().routing_available);

        // Só um roteamento bem-sucedido religa.
        world.backend.unsupported.store(false, Ordering::SeqCst);
        world
            .source
            .set_sessions(vec![session(30, "game.exe", Flow::Render, "alto")]);
        engine.refresh();
        assert!(engine.state().routing_available);
    }

    #[test]
    fn rule_for_absent_device_is_kept_and_reapplied_when_it_returns() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(dir.path(), vec![rule("chrome.exe", Flow::Render, "monitor")]);
        let world = World::new();
        let engine = world.engine_at(dir.path());
        // Monitor ausente da lista do Windows.
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "alto")]);
        engine.refresh();

        assert!(world.backend.set_calls().is_empty());
        let state = engine.state();
        assert_eq!(state.rules, vec![rule("chrome.exe", Flow::Render, "monitor")]);
        assert_eq!(state.save_error, None);

        // Presente, mas desconectado: continua sem aplicar e sem erro.
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            device("monitor", Flow::Render, false, "Monitor"),
        ]);
        engine.refresh();
        assert!(world.backend.set_calls().is_empty());
        assert_eq!(engine.state().rules.len(), 1);
        assert_eq!(engine.state().save_error, None);

        // Volta conectado: o próximo refresh aplica a regra.
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            device("monitor", Flow::Render, true, "Monitor"),
        ]);
        engine.refresh();
        assert_eq!(
            world.backend.set_calls(),
            vec![(10, Flow::Render, dev_id("monitor"))]
        );
        assert_eq!(engine.state().save_error, None);
    }

    #[test]
    fn app_state_json_shape_is_pinned() {
        let state = AppState {
            devices: vec![DeviceView {
                id: dev_id("fone"),
                flow: Flow::Render,
                windows_name: "Alto".into(),
                name: Some("Monitor".into()),
                display_name: "Monitor".into(),
                icon: None,
                hidden: false,
                connected: true,
                is_default: true,
                has_rules: true,
                volume: 0.75,
                muted: true,
            }],
            sessions: vec![session(10, "chrome.exe", Flow::Render, "fone")],
            rules: vec![rule("chrome.exe", Flow::Render, "fone")],
            profiles: vec![profile("p1", vec![])],
            settings: Settings::default(),
            routing_available: true,
            last_profile: Some("p1".into()),
            save_error: None,
        };

        let json = serde_json::to_value(&state).unwrap();
        let keys = |v: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(
            keys(&json),
            vec![
                "devices",
                "last_profile",
                "profiles",
                "routing_available",
                "rules",
                "save_error",
                "sessions",
                "settings",
            ]
        );
        assert_eq!(json["save_error"], serde_json::Value::Null);
        assert_eq!(json["routing_available"], true);
        assert_eq!(json["last_profile"], "p1");

        let device_json = &json["devices"][0];
        assert_eq!(
            keys(device_json),
            vec![
                "connected",
                "display_name",
                "flow",
                "has_rules",
                "hidden",
                "icon",
                "id",
                "is_default",
                "muted",
                "name",
                "volume",
                "windows_name",
            ]
        );
        assert_eq!(device_json["flow"], "render");
        assert_eq!(device_json["display_name"], "Monitor");
        assert_eq!(device_json["volume"], 0.75);
        assert_eq!(device_json["muted"], true);

        let session_json = &json["sessions"][0];
        assert_eq!(
            keys(session_json),
            vec!["active", "device", "exe", "flow", "muted", "pid", "volume"]
        );
        assert_eq!(session_json["flow"], "render");
        assert_eq!(json["rules"][0]["flow"], "render");

        assert_eq!(json["settings"]["shortcut"], "Super+Alt+V");
        assert_eq!(json["settings"]["autostart"], false);

        let mut with_error = state.clone();
        with_error.save_error = Some("falhou".into());
        assert_eq!(
            serde_json::to_value(&with_error).unwrap()["save_error"],
            "falhou"
        );
    }

    #[test]
    fn set_session_volume_forwards_to_ctl_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let before = world.notes();

        engine.set_session_volume(10, Flow::Render, 0.4).unwrap();

        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetSessionVolume(10, Flow::Render, 0.4)]
        );
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn set_session_muted_forwards_to_ctl_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let before = world.notes();

        engine.set_session_muted(11, Flow::Capture, true).unwrap();

        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetSessionMuted(11, Flow::Capture, true)]
        );
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn set_device_volume_forwards_and_shows_new_value() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        engine.refresh();
        let before = world.notes();

        engine.set_device_volume(&dev_id("fone"), 0.3).unwrap();

        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetVolume("fone".to_string(), 0.3)]
        );
        assert_eq!(engine.state().devices[0].volume, 0.3);
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn set_device_muted_forwards_and_shows_new_value() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        engine.refresh();
        let before = world.notes();

        engine.set_device_muted(&dev_id("fone"), true).unwrap();

        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetMuted("fone".to_string(), true)]
        );
        assert!(engine.state().devices[0].muted);
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn set_default_device_forwards_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let before = world.notes();

        engine
            .set_default_device(&dev_id("mic"), Flow::Capture)
            .unwrap();

        assert_eq!(
            world.defaults.calls(),
            vec![(dev_id("mic"), Flow::Capture)]
        );
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn failed_endpoint_write_is_returned_and_does_not_notify() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.ctl.fail_writes.store(true, Ordering::SeqCst);
        world.defaults.fail.store(true, Ordering::SeqCst);
        let before = world.notes();

        assert!(engine.set_session_volume(10, Flow::Render, 0.4).is_err());
        assert!(engine.set_device_muted(&dev_id("fone"), true).is_err());
        assert!(engine
            .set_default_device(&dev_id("fone"), Flow::Render)
            .is_err());
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn mic_peak_reads_ctl_without_notifying() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.ctl.set_peak_of("mic", 0.42);
        let before = world.notes();

        assert_eq!(engine.mic_peak(&dev_id("mic")), 0.42);
        assert_eq!(world.notes(), before);

        // Leitura que falha vira 0.0: o medidor fica parado, sem erro.
        world.ctl.fail_reads.store(true, Ordering::SeqCst);
        assert_eq!(engine.mic_peak(&dev_id("mic")), 0.0);
    }

    #[test]
    fn device_view_shows_volume_and_mute_only_for_connected_devices() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            device("antigo", Flow::Render, false, "Antigo"),
        ]);
        world.ctl.set_volume_of("fone", 0.25);
        world.ctl.set_mute_of("fone", true);
        // Desconectado: mesmo que o controle tivesse valores, a visão mostra o padrão.
        world.ctl.set_volume_of("antigo", 0.2);
        world.ctl.set_mute_of("antigo", true);
        engine.refresh();
        engine
            .update_device_meta(&dev_id("antigo"), "Nome", None, false)
            .unwrap();

        let state = engine.state();
        let fone = state.devices.iter().find(|d| d.id.0 == "fone").unwrap();
        assert_eq!(fone.volume, 0.25);
        assert!(fone.muted);
        let antigo = state.devices.iter().find(|d| d.id.0 == "antigo").unwrap();
        assert_eq!(antigo.volume, 1.0);
        assert!(!antigo.muted);
    }

    #[test]
    fn device_view_falls_back_to_defaults_when_ctl_read_fails() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world.ctl.set_volume_of("fone", 0.25);
        world.ctl.set_mute_of("fone", true);
        world.ctl.fail_reads.store(true, Ordering::SeqCst);

        engine.refresh();

        let fone = &engine.state().devices[0];
        assert_eq!(fone.volume, 1.0);
        assert!(!fone.muted);
    }

    #[test]
    fn default_capture_is_the_connected_default_microphone() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let mic = Device {
            is_default: true,
            ..device("mic", Flow::Capture, true, "Microfone")
        };
        world.source.set_devices(vec![
            Device {
                is_default: false,
                ..device("outro", Flow::Capture, true, "Outro")
            },
            mic.clone(),
            Device {
                is_default: true,
                ..device("fone", Flow::Render, true, "Fone")
            },
        ]);
        engine.refresh();
        assert_eq!(engine.default_capture(), Some(dev_id("mic")));

        // Padrão desconectado não conta: não há microfone padrão ativo.
        world.source.set_devices(vec![Device {
            connected: false,
            ..mic
        }]);
        engine.refresh();
        assert_eq!(engine.default_capture(), None);
    }

    #[test]
    fn failed_save_is_reported_and_cleared_by_next_success() {
        let dir = tempfile::tempdir().unwrap();
        // Um arquivo no lugar do diretório de dados: toda gravação falha.
        let blocked = dir.path().join("dados");
        std::fs::write(&blocked, b"arquivo no lugar do diretorio").unwrap();
        let world = World::new();
        let engine = world.engine_at(&blocked);
        let before = world.notes();

        engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));

        let error = engine.state().save_error;
        assert!(error.as_deref().unwrap_or("").contains("rules.json"));
        assert_eq!(world.notes(), before + 1);

        // O diretório passa a existir: a próxima gravação funciona e limpa o erro.
        std::fs::remove_file(&blocked).unwrap();
        std::fs::create_dir(&blocked).unwrap();
        engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));

        assert_eq!(engine.state().save_error, None);
        let on_disk: RuleSet = Store::open(blocked.clone()).load("rules");
        assert_eq!(on_disk.rules, vec![rule("chrome.exe", Flow::Render, "fone")]);
    }
}
