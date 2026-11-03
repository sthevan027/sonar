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
}

pub struct Engine {
    source: Arc<dyn AudioSource>,
    backend: Arc<dyn RouteBackend>,
    // Usados pelos comandos de volume e de padrão (Task 9b).
    #[allow(dead_code)]
    ctl: Arc<dyn EndpointControl>,
    #[allow(dead_code)]
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
            build_state(inner)
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
            self.persist("rules", &inner.rules);
            self.sync(inner);
            if clearing {
                clear_sessions(self.backend.as_ref(), &inner.sessions, &exe, flow);
            }
            build_state(inner)
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
            self.persist("rules", &inner.rules);
            // As regras novas (`Change::Set`) entram pelo `sync`; as limpezas são feitas aqui.
            self.sync(inner);
            for change in changes {
                if let Change::Cleared { exe, flow } = change {
                    clear_sessions(self.backend.as_ref(), &inner.sessions, &exe, flow);
                }
            }
            inner.last_profile = Some(id.to_string());
            build_state(inner)
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
            self.persist("profiles", &inner.profiles);
            build_state(inner)
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
            self.persist("profiles", &inner.profiles);
            build_state(inner)
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
            self.persist("devices", &inner.meta);
            build_state(inner)
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
            self.persist("settings", &inner.settings);
            build_state(inner)
        };
        (self.notify)(&state);
    }

    /// Snapshot do estado atual, sem reler o sistema.
    pub fn state(&self) -> AppState {
        build_state(&self.lock())
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
            if let Ok(sessions) = self.source.sessions() {
                inner.sessions = sessions;
            }
        }
        inner.routing_available = !outcomes.iter().any(|o| {
            matches!(
                o,
                Outcome::Skipped {
                    reason: AudioError::Unsupported(_),
                    ..
                }
            )
        });
    }

    /// Grava no disco. Falha não derruba o app: o estado em memória segue válido e a
    /// próxima mudança tenta gravar de novo.
    fn persist<T: Serialize>(&self, name: &str, value: &T) {
        if let Err(err) = self.store.save(name, value) {
            eprintln!("sonar: não foi possível gravar {name}.json: {err}");
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
fn build_state(inner: &Inner) -> AppState {
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
            Ok(())
        }

        fn clear_route(&self, pid: u32, flow: Flow) -> Result<(), AudioError> {
            self.clear_calls.lock().unwrap().push((pid, flow));
            self.routes.lock().unwrap().remove(&pid);
            Ok(())
        }

        fn get_route(&self, pid: u32, _flow: Flow) -> Result<Option<DeviceId>, AudioError> {
            Ok(self.routes.lock().unwrap().get(&pid).cloned())
        }
    }

    struct FakeCtl;

    impl EndpointControl for FakeCtl {
        fn volume(&self, _id: &DeviceId) -> Result<f32, AudioError> {
            Ok(1.0)
        }
        fn set_volume(&self, _id: &DeviceId, _v: f32) -> Result<(), AudioError> {
            Ok(())
        }
        fn muted(&self, _id: &DeviceId) -> Result<bool, AudioError> {
            Ok(false)
        }
        fn set_muted(&self, _id: &DeviceId, _m: bool) -> Result<(), AudioError> {
            Ok(())
        }
        fn peak(&self, _id: &DeviceId) -> Result<f32, AudioError> {
            Ok(0.0)
        }
        fn set_session_volume(&self, _pid: u32, _flow: Flow, _v: f32) -> Result<(), AudioError> {
            Ok(())
        }
        fn set_session_muted(&self, _pid: u32, _flow: Flow, _m: bool) -> Result<(), AudioError> {
            Ok(())
        }
    }

    struct FakeDefaults;

    impl DefaultDevice for FakeDefaults {
        fn set_default(&self, _id: &DeviceId, _flow: Flow) -> Result<(), AudioError> {
            Ok(())
        }
    }

    // ---------- montagem ----------

    /// Fontes e contador de notificações compartilhados entre motores de um mesmo teste.
    struct World {
        source: Arc<FakeSource>,
        backend: Arc<FakeBackend>,
        notes: Arc<AtomicUsize>,
    }

    impl World {
        fn new() -> World {
            World {
                source: Arc::new(FakeSource::default()),
                backend: Arc::new(FakeBackend::default()),
                notes: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn engine_at(&self, dir: &Path) -> Engine {
            let notes = self.notes.clone();
            Engine::new(
                self.source.clone(),
                self.backend.clone(),
                Arc::new(FakeCtl),
                Arc::new(FakeDefaults),
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
}
