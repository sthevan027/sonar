//! `Engine`: dono do estado do app. Junta fontes do sistema, regras, perfis e metadados,
//! aplica as regras no Windows e avisa a interface (`notify`) a cada mudança.
//!
//! Travamentos (sempre nesta ordem, nunca o contrário):
//! - `apply`: quem chama o backend de roteamento (ou relê e reaplica regras) segura este.
//!   Assim uma releitura nunca reaplica uma regra antiga por cima de uma escolha nova.
//! - `persist`: uma gravação em disco por vez; cada uma copia o estado atual ao começar.
//! - `emit`: monta o `AppState` e avisa a interface. Estados saem na ordem em que foram montados.
//! - `inner`: os dados. Segurado só por instantes, nunca durante chamada COM ou disco.
//!
//! A leitura do sistema (dispositivos, sessões, volumes) acontece sem nenhum desses travamentos
//! de dados: é a parte lenta. Escolher a saída de um app não relê nada: aplica a regra às
//! sessões já conhecidas daquele app e atualiza só elas.

use crate::actions::{ActionResult, ErrorCode, audio_failure};
use crate::audio::default_device::DefaultDevice;
use crate::audio::endpoint::{clamp_volume, EndpointControl};
use crate::audio::types::{AudioError, AudioSession, AudioSource, Device, DeviceId, Flow, RouteBackend};
use crate::profiles::{self, Change, NameError, Profile, Target};
use crate::rules::{reconcile, reconcile_rule, Outcome, Rule, RuleSet};
use crate::state::{AppState, DeviceMeta, DeviceView, Settings};
use crate::store::Store;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Volume e mudo de um endpoint conectado, como o sistema os informou.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Level {
    volume: f32,
    muted: bool,
}

impl Default for Level {
    /// O que a tela mostra quando não há leitura (endpoint desconectado ou leitura que falhou).
    fn default() -> Self {
        Level {
            volume: 1.0,
            muted: false,
        }
    }
}

/// Como o mudo da saída padrão deve ser aplicado.
#[allow(dead_code)] // Tarefas seguintes (F1) consomem esta ação; até lá só os testes a usam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteMode {
    /// Deixa a saída no estado pedido.
    Set(bool),
    /// Inverte o estado atual da saída.
    Toggle,
}

/// Diferença mínima para o volume contar como mudado. Abaixo disso, o endpoint já está
/// no valor pedido e a escrita é desnecessária.
const VOLUME_EPSILON: f32 = 0.0001;

/// Como o volume da saída padrão deve ser aplicado.
#[allow(dead_code)] // Tarefas seguintes (F1) consomem esta ação; até lá só os testes a usam.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VolumeMode {
    /// Soma um valor ao volume atual da saída.
    Step(f32),
    /// Define o volume da saída de forma absoluta.
    Set(f32),
}

/// Estado mutável, sempre acessado sob o travamento `inner` do `Engine`.
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
    /// Volume e mudo dos endpoints conectados. Lidos no `refresh`; os comandos de volume os
    /// atualizam, para que montar o estado nunca precise falar com o Windows.
    levels: BTreeMap<String, Level>,
    /// Sobe a cada mudança feita pelo usuário (rota, volume, mudo). Uma leitura do sistema que
    /// começou antes de uma mudança não sobrescreve o que o usuário acabou de fazer.
    generation: u64,
}

/// Leitura do sistema feita sem travamento. `None` quando aquela parte falhou: o último valor bom fica.
struct SystemRead {
    devices: Option<Vec<Device>>,
    sessions: Option<Vec<AudioSession>>,
    levels: Option<BTreeMap<String, Level>>,
}

pub struct Engine {
    source: Arc<dyn AudioSource>,
    backend: Arc<dyn RouteBackend>,
    ctl: Arc<dyn EndpointControl>,
    defaults: Arc<dyn DefaultDevice>,
    store: Store,
    notify: Arc<dyn Fn(&AppState) + Send + Sync>,
    inner: Mutex<Inner>,
    apply: Mutex<()>,
    persist: Mutex<()>,
    emit: Mutex<()>,
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
            levels: BTreeMap::new(),
            generation: 0,
        };
        Engine {
            source,
            backend,
            ctl,
            defaults,
            store,
            notify,
            inner: Mutex::new(inner),
            apply: Mutex::new(()),
            persist: Mutex::new(()),
            emit: Mutex::new(()),
        }
    }

    /// Relê dispositivos, sessões e volumes, aplica as regras e avisa a interface.
    pub fn refresh(&self) {
        let generation = self.lock().generation;
        let read = self.read_system();
        {
            let _apply = guard(&self.apply);
            self.store_system(generation, read);
            self.reconcile_cached();
        }
        self.emit();
    }

    /// Grava (ou, com `None`, remove) a regra do executável e aplica só às sessões desse app.
    /// Não relê o sistema: a sessão ganha o dispositivo novo na hora e a próxima releitura
    /// confirma. Se o app ainda não tiver sessão conhecida, a regra vale quando ela aparecer.
    pub fn set_app_route(&self, exe: &str, flow: Flow, device: Option<DeviceId>) {
        let exe = exe.to_lowercase();
        {
            let _apply = guard(&self.apply);
            let (devices, sessions) = {
                let mut inner = self.lock();
                inner.generation += 1;
                match &device {
                    Some(device) => inner.rules.upsert(Rule {
                        exe: exe.clone(),
                        flow,
                        device: device.clone(),
                    }),
                    None => inner.rules.remove(&exe, flow),
                }
                (inner.devices.clone(), inner.sessions.clone())
            };
            match device {
                Some(device) => {
                    let rule = Rule {
                        exe: exe.clone(),
                        flow,
                        device: device.clone(),
                    };
                    let outcomes =
                        reconcile_rule(&rule, &devices, &sessions, self.backend.as_ref());
                    let routed: Vec<u32> = outcomes
                        .iter()
                        .filter_map(|o| match o {
                            Outcome::Applied { pid } | Outcome::AlreadyRouted { pid } => Some(*pid),
                            _ => None,
                        })
                        .collect();
                    self.patch_sessions(&routed, flow, &device);
                    self.latch_routing(&outcomes);
                }
                None => {
                    let pids: Vec<u32> = sessions
                        .iter()
                        .filter(|s| s.flow == flow && s.exe.eq_ignore_ascii_case(&exe))
                        .map(|s| s.pid)
                        .collect();
                    clear_sessions(self.backend.as_ref(), &sessions, &exe, flow);
                    // Sem rota, a sessão segue o padrão do Windows para o sentido.
                    if let Some(default) = devices
                        .iter()
                        .find(|d| d.flow == flow && d.is_default && d.connected)
                    {
                        self.patch_sessions(&pids, flow, &default.id);
                    }
                }
            }
        }
        self.persist("rules", |inner| inner.rules.clone());
        self.emit();
    }

    /// Ativa um perfil. `false` quando o id não existe (nada muda e nada é avisado).
    pub fn activate_profile(&self, id: &str) -> bool {
        {
            let _apply = guard(&self.apply);
            let (profile, mut rules) = {
                let inner = self.lock();
                let Some(profile) = inner.profiles.iter().find(|p| p.id == id).cloned() else {
                    return false;
                };
                (profile, inner.rules.clone())
            };
            let changes = profiles::activate(&profile, &mut rules);
            let generation = {
                let mut inner = self.lock();
                inner.rules = rules;
                inner.generation += 1;
                inner.generation
            };
            self.persist("rules", |inner| inner.rules.clone());
            // Troca de perfil relê tudo: as regras novas entram pela releitura.
            let read = self.read_system();
            self.store_system(generation, read);
            self.reconcile_cached();
            let cleared: Vec<(String, Flow)> = changes
                .into_iter()
                .filter_map(|change| match change {
                    Change::Cleared { exe, flow } => Some((exe, flow)),
                    Change::Set(_) => None,
                })
                .collect();
            if !cleared.is_empty() {
                let sessions = self.lock().sessions.clone();
                for (exe, flow) in &cleared {
                    clear_sessions(self.backend.as_ref(), &sessions, exe, *flow);
                }
                // Sem a leitura, a sessão limpa mostraria o dispositivo antigo.
                if let Ok(fresh) = self.source.sessions() {
                    self.lock().sessions = fresh;
                }
            }
            self.lock().last_profile = Some(id.to_string());
        }
        self.emit();
        true
    }

    /// Cria ou substitui um perfil pelo id. Nome inválido é recusado sem gravar nada.
    pub fn save_profile(&self, mut p: Profile) -> Result<(), NameError> {
        p.name = profiles::validate_name(p.name.as_deref().unwrap_or(""))?;
        let p = normalize_profile(p);
        {
            let mut inner = self.lock();
            match inner.profiles.iter_mut().find(|existing| existing.id == p.id) {
                Some(existing) => *existing = p,
                None => inner.profiles.push(p),
            }
        }
        self.persist("profiles", |inner| inner.profiles.clone());
        self.emit();
        Ok(())
    }

    pub fn delete_profile(&self, id: &str) {
        {
            let mut inner = self.lock();
            inner.profiles.retain(|p| p.id != id);
            if inner.last_profile.as_deref() == Some(id) {
                inner.last_profile = None;
            }
        }
        self.persist("profiles", |inner| inner.profiles.clone());
        self.emit();
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
        {
            let mut inner = self.lock();
            // Metadado vazio equivale a não ter metadado: a entrada some do arquivo.
            if meta.is_meaningful() {
                inner.meta.insert(id.0.clone(), meta);
            } else {
                inner.meta.remove(&id.0);
            }
        }
        self.persist("devices", |inner| inner.meta.clone());
        self.emit();
        Ok(())
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    pub fn save_settings(&self, s: Settings) {
        self.lock().settings = s;
        self.persist("settings", |inner| inner.settings.clone());
        self.emit();
    }

    /// Snapshot do estado atual. Não fala com o sistema: só lê o que já foi lido.
    pub fn state(&self) -> AppState {
        build_state(&self.lock())
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

    /// Volume do endpoint. Não relê o sistema: atualiza o valor guardado e avisa a interface.
    pub fn set_device_volume(&self, id: &DeviceId, v: f32) -> Result<(), AudioError> {
        self.write_device_volume(id, v)
    }

    /// Mudo do endpoint. Mesmo fluxo de `set_device_volume`.
    pub fn set_device_muted(&self, id: &DeviceId, m: bool) -> Result<(), AudioError> {
        self.write_device_muted(id, m)
    }

    /// Liga, desliga ou inverte o mudo da saída padrão. Segura `apply` durante todo o
    /// método: duas ações de mudo não podem se cruzar. O estado do mudo é lido na hora (o
    /// valor guardado pode ser velho) e a escrita só acontece quando o valor muda.
    #[allow(dead_code)] // Tarefas seguintes (F1) consomem esta ação; até lá só os testes a usam.
    pub fn act_output_mute(&self, mode: MuteMode) -> ActionResult {
        let _apply = guard(&self.apply);
        let Some(id) = self.default_render() else {
            return ActionResult::failed(
                ErrorCode::NoOutputDevice,
                "Não há saída padrão conectada.",
            );
        };
        let current = match self.ctl.muted(&id) {
            Ok(muted) => muted,
            Err(err) => return audio_failure(&err),
        };
        let target = match mode {
            MuteMode::Set(muted) => muted,
            MuteMode::Toggle => !current,
        };
        if target == current {
            // Já está do jeito pedido: nada muda e ninguém é avisado.
            return ActionResult::done(false, Some(id.0));
        }
        if let Err(err) = self.write_device_muted(&id, target) {
            return audio_failure(&err);
        }
        ActionResult::done(true, Some(id.0))
    }

    /// Soma ou define o volume da saída padrão. Segura `apply` durante todo o método: duas
    /// ações de volume não podem se cruzar. O volume é lido na hora (o valor guardado pode
    /// ser velho) e a escrita só acontece quando o valor muda de verdade.
    #[allow(dead_code)] // Tarefas seguintes (F1) consomem esta ação; até lá só os testes a usam.
    pub fn act_output_volume(&self, mode: VolumeMode) -> ActionResult {
        let _apply = guard(&self.apply);
        // Valor não finito é recusado antes de olhar o sistema: nada é tocado.
        let requested = match mode {
            VolumeMode::Step(delta) => delta,
            VolumeMode::Set(value) => value,
        };
        if !requested.is_finite() {
            return ActionResult::failed(
                ErrorCode::InvalidValue,
                "O valor de volume pedido não é válido.",
            );
        }
        let Some(id) = self.default_render() else {
            return ActionResult::failed(
                ErrorCode::NoOutputDevice,
                "Não há saída padrão conectada.",
            );
        };
        let current = match self.ctl.volume(&id) {
            Ok(volume) => volume,
            Err(err) => return audio_failure(&err),
        };
        let target = match mode {
            VolumeMode::Step(delta) => clamp_volume(current + delta),
            VolumeMode::Set(value) => clamp_volume(value),
        };
        if (target - current).abs() <= VOLUME_EPSILON {
            // Já está do jeito pedido: nada muda e ninguém é avisado.
            return ActionResult::done(false, Some(id.0));
        }
        if let Err(err) = self.write_device_volume(&id, target) {
            return audio_failure(&err);
        }
        ActionResult::done(true, Some(id.0))
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

    /// Saída padrão conectada, do último sincronismo. Não toca no sistema.
    pub fn default_render(&self) -> Option<DeviceId> {
        self.lock()
            .devices
            .iter()
            .find(|d| d.flow == Flow::Render && d.is_default && d.connected)
            .map(|d| d.id.clone())
    }

    /// Pids das sessões de um app no sentido, do último sincronismo. O nome do
    /// executável é comparado sem diferenciar caixa.
    #[allow(dead_code)] // Tarefas seguintes (F1) consomem este resolvedor; até lá só os testes o usam.
    pub fn session_pids(&self, exe: &str, flow: Flow) -> Vec<u32> {
        self.lock()
            .sessions
            .iter()
            .filter(|s| s.flow == flow && s.exe.eq_ignore_ascii_case(exe))
            .map(|s| s.pid)
            .collect()
    }

    /// Saídas conectadas e visíveis, na ordem dos dispositivos. É a ordem do painel.
    #[allow(dead_code)] // Tarefas seguintes (F1) consomem este resolvedor; até lá só os testes o usam.
    pub fn visible_outputs(&self) -> Vec<DeviceId> {
        let inner = self.lock();
        inner
            .devices
            .iter()
            .filter(|d| d.flow == Flow::Render && d.connected)
            .filter(|d| !inner.meta.get(&d.id.0).map(|m| m.hidden).unwrap_or(false))
            .map(|d| d.id.clone())
            .collect()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        guard(&self.inner)
    }

    /// Lê o sistema. Não segura nenhum travamento do `Engine`.
    fn read_system(&self) -> SystemRead {
        let devices = self.source.devices().ok();
        let sessions = self.source.sessions().ok();
        let levels = devices.as_ref().map(|devices| {
            let levels: BTreeMap<String, Level> = devices
                .iter()
                .filter(|d| d.connected)
                .map(|d| {
                    let level = Level {
                        volume: self.ctl.volume(&d.id).unwrap_or(1.0),
                        muted: self.ctl.muted(&d.id).unwrap_or(false),
                    };
                    (d.id.0.clone(), level)
                })
                .collect();
            levels
        });
        SystemRead {
            devices,
            sessions,
            levels,
        }
    }

    /// Guarda a leitura (quem chama segura `apply`). Dispositivos sempre entram; sessões e
    /// volumes só se o usuário não mudou nada enquanto a leitura era feita. Nesse caso o valor
    /// do usuário fica, e a próxima releitura traz o resto.
    fn store_system(&self, generation: u64, read: SystemRead) {
        let mut inner = self.lock();
        if let Some(devices) = read.devices {
            inner.devices = devices;
        }
        if inner.generation == generation {
            if let Some(sessions) = read.sessions {
                inner.sessions = sessions;
            }
            if let Some(levels) = read.levels {
                inner.levels = levels;
            }
        }
    }

    /// Aplica as regras às sessões conhecidas e atualiza `routing_available` (quem chama segura `apply`).
    fn reconcile_cached(&self) {
        let (rules, devices, sessions) = {
            let inner = self.lock();
            (inner.rules.clone(), inner.devices.clone(), inner.sessions.clone())
        };
        let outcomes = reconcile(&rules, &devices, &sessions, self.backend.as_ref());
        if outcomes.iter().any(|o| matches!(o, Outcome::Applied { .. })) {
            // Uma rota recém-gravada muda o dispositivo da sessão: relê para não mostrar o antigo.
            if let Ok(fresh) = self.source.sessions() {
                self.lock().sessions = fresh;
            }
        }
        self.latch_routing(&outcomes);
    }

    /// Trava: cai no primeiro Unsupported e só volta com um roteamento bem-sucedido.
    fn latch_routing(&self, outcomes: &[Outcome]) {
        let mut inner = self.lock();
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
        } else if outcomes
            .iter()
            .any(|o| matches!(o, Outcome::Applied { .. } | Outcome::AlreadyRouted { .. }))
        {
            inner.routing_available = true;
        }
    }

    /// Põe o dispositivo novo nas sessões (dos `pids`, no sentido) que o sistema acabou de rotear.
    fn patch_sessions(&self, pids: &[u32], flow: Flow, device: &DeviceId) {
        let mut inner = self.lock();
        for session in inner
            .sessions
            .iter_mut()
            .filter(|s| s.flow == flow && pids.contains(&s.pid))
        {
            session.device = device.clone();
        }
    }

    /// Escreve o mudo no endpoint e guarda o valor novo. Não segura `apply`: quem chama
    /// decide se segura (`act_output_mute` segura; `set_device_muted` não).
    fn write_device_muted(&self, id: &DeviceId, m: bool) -> Result<(), AudioError> {
        self.ctl.set_muted(id, m)?;
        {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.levels.entry(id.0.clone()).or_default().muted = m;
        }
        self.emit();
        Ok(())
    }

    /// Escreve o volume no endpoint e guarda o valor novo. Não segura `apply`: quem chama
    /// decide se segura (`act_output_volume` segura; `set_device_volume` não).
    fn write_device_volume(&self, id: &DeviceId, v: f32) -> Result<(), AudioError> {
        self.ctl.set_volume(id, v)?;
        {
            let mut inner = self.lock();
            inner.generation += 1;
            inner.levels.entry(id.0.clone()).or_default().volume = clamp_volume(v);
        }
        self.emit();
        Ok(())
    }

    /// Monta o estado e avisa a interface. Só o `emit` é segurado: estados saem na ordem em que foram montados.
    fn emit(&self) {
        let _emit = guard(&self.emit);
        let state = build_state(&self.lock());
        (self.notify)(&state);
    }

    /// Grava uma seção do estado. O valor é copiado sob `inner` e a escrita roda fora dele.
    /// `persist` serializa as escritas: a última a acontecer leva o estado mais novo.
    fn persist<T: Serialize>(&self, name: &str, snapshot: impl FnOnce(&Inner) -> T) {
        let _write = guard(&self.persist);
        let value = snapshot(&self.lock());
        let error = write_document(&self.store, name, &value);
        self.lock().save_error = error;
    }
}

fn guard<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    // Veneno do travamento: o estado continua válido o bastante para seguir.
    lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
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

/// Grava um documento. Devolve a mensagem quando a gravação falha (vai para o stderr também);
/// `None` quando dá certo. A próxima gravação bem-sucedida limpa o erro. O estado em memória
/// segue válido e a próxima mudança tenta de novo.
fn write_document<T: Serialize>(store: &Store, name: &str, value: &T) -> Option<String> {
    match store.save(name, value) {
        Ok(()) => None,
        Err(err) => {
            eprintln!("sonar: não foi possível gravar {name}.json: {err}");
            Some(format!("Não foi possível gravar {name}.json: {err}"))
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
/// muitos dispositivos antigos) fica de fora. Não fala com o sistema.
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
            // Endpoint desconectado não tem volume para mostrar: mostra o padrão.
            let level = if d.connected {
                inner.levels.get(&d.id.0).copied().unwrap_or_default()
            } else {
                Level::default()
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
                volume: level.volume,
                muted: level.muted,
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
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::Duration;

    // ---------- fakes ----------

    /// Fonte de dispositivos e sessões controlada pelo teste.
    #[derive(Default)]
    struct FakeSource {
        devices: Mutex<Vec<Device>>,
        sessions: Mutex<Vec<AudioSession>>,
        fail_reads: AtomicBool,
        /// Quantas vezes `devices()` e `sessions()` foram chamados (releituras completas).
        device_reads: AtomicUsize,
        session_reads: AtomicUsize,
    }

    impl FakeSource {
        fn set_devices(&self, devices: Vec<Device>) {
            *self.devices.lock().unwrap() = devices;
        }

        fn device_reads(&self) -> usize {
            self.device_reads.load(Ordering::SeqCst)
        }

        fn session_reads(&self) -> usize {
            self.session_reads.load(Ordering::SeqCst)
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
            self.device_reads.fetch_add(1, Ordering::SeqCst);
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(AudioError::Os(-1));
            }
            Ok(self.devices.lock().unwrap().clone())
        }

        fn sessions(&self) -> Result<Vec<AudioSession>, AudioError> {
            self.session_reads.fetch_add(1, Ordering::SeqCst);
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
        /// O endpoint sumiu: ler o mudo dele devolve `Gone` e escrever o mudo nele também falha.
        gone: AtomicBool,
        /// Porta só de teste: avisa que a chamada chegou no backend e a segura parada até o
        /// teste liberar. Vazia (padrão) = a chamada passa direto.
        gate_chegou: Mutex<Option<mpsc::Sender<()>>>,
        gate_liberar: Mutex<Option<mpsc::Receiver<()>>>,
        /// Leituras feitas (volume, mudo, pico), para provar que a escolha de rota não as repete.
        reads: AtomicUsize,
    }

    impl FakeCtl {
        fn calls(&self) -> Vec<CtlCall> {
            self.calls.lock().unwrap().clone()
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::SeqCst)
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
            self.reads.fetch_add(1, Ordering::SeqCst);
            if self.fail_reads.load(Ordering::SeqCst) {
                return Err(AudioError::Os(-2));
            }
            Ok(())
        }

        /// Posta a porta de teste: a próxima leitura avisa em `chegou` e fica parada
        /// esperando a permissão de `liberar`.
        fn set_gate(&self, chegou: mpsc::Sender<()>, liberar: mpsc::Receiver<()>) {
            *self.gate_chegou.lock().unwrap() = Some(chegou);
            *self.gate_liberar.lock().unwrap() = Some(liberar);
        }

        /// Estaciona a chamada dentro da porta, se houver uma posta. Sem porta, passa direto.
        /// A porta é de uso único: a primeira chamada que chega é a que para.
        fn wait_gate(&self) {
            let chegou = self.gate_chegou.lock().unwrap().take();
            let liberar = self.gate_liberar.lock().unwrap().take();
            let (Some(chegou), Some(liberar)) = (chegou, liberar) else {
                return;
            };
            // Avisa que chegou e espera a permissão do teste.
            let _ = chegou.send(());
            let _ = liberar.recv();
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
            // A porta de teste (quando posta) segura a chamada aqui, com a trava `apply` do
            // chamador ainda na mão: é o ponto exato em que a exclusão mútua pode ser provada.
            self.wait_gate();
            self.read_guard()?;
            if self.gone.load(Ordering::SeqCst) {
                return Err(AudioError::Gone);
            }
            Ok(self.mutes.lock().unwrap().get(&id.0).copied().unwrap_or(false))
        }
        fn set_muted(&self, id: &DeviceId, m: bool) -> Result<(), AudioError> {
            self.record(CtlCall::SetMuted(id.0.clone(), m))?;
            if self.gone.load(Ordering::SeqCst) {
                return Err(AudioError::Gone);
            }
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
        // Estado de regime: a interface já leu o sistema antes de o usuário escolher.
        engine.refresh();
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
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            Device {
                is_default: true,
                ..device("padrao", Flow::Render, true, "Padrao")
            },
        ]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "fone")]);
        world.backend.preset_route(10, &dev_id("fone"));
        engine.refresh();
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
    fn set_mute_twice_changes_once() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        engine.refresh();
        let before = world.notes();

        let first = engine.act_output_mute(MuteMode::Set(true));
        let second = engine.act_output_mute(MuteMode::Set(true));

        assert!(first.ok);
        assert!(first.changed);
        assert_eq!(first.resolved.as_deref(), Some("fone"));
        assert!(second.ok);
        assert!(!second.changed);
        // O mudo já estava ligado: a segunda chamada não escreve nada.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetMuted("fone".to_string(), true)]
        );
        assert!(engine.state().devices[0].muted);
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn toggle_reads_fresh_state() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        // O endpoint chega mudo de fora do app.
        world.ctl.set_mute_of("fone", true);
        engine.refresh();
        let before = world.notes();

        let first = engine.act_output_mute(MuteMode::Toggle);

        assert!(first.ok);
        assert!(first.changed);
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetMuted("fone".to_string(), false)]
        );

        // Mudança por fora, entre as duas chamadas: o toggle relê o endpoint, não o estado guardado.
        world.ctl.set_mute_of("fone", true);
        let second = engine.act_output_mute(MuteMode::Toggle);

        assert!(second.ok);
        assert!(second.changed);
        assert_eq!(
            world.ctl.calls(),
            vec![
                CtlCall::SetMuted("fone".to_string(), false),
                CtlCall::SetMuted("fone".to_string(), false),
            ]
        );
        assert_eq!(world.notes(), before + 2);
    }

    #[test]
    fn mute_without_default_output_fails_without_emit() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        // Nenhum dispositivo é o padrão do Windows.
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        engine.refresh();
        let before = world.notes();
        let reads_before = world.ctl.reads();

        let result = engine.act_output_mute(MuteMode::Set(true));

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::NoOutputDevice));
        assert!(!result.changed);
        assert_eq!(result.resolved, None);
        assert!(world.ctl.calls().is_empty());
        // Nem escrita nem leitura: sem saída padrão não se fala com a API do Windows.
        assert_eq!(world.ctl.reads(), reads_before);
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn mute_on_gone_device_returns_device_gone() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        engine.refresh();
        let before = world.notes();
        let state_before = engine.state();
        world.ctl.gone.store(true, Ordering::SeqCst);

        let result = engine.act_output_mute(MuteMode::Set(true));

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::DeviceGone));
        assert!(!result.changed);
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        assert_eq!(
            engine.state().devices[0].muted,
            state_before.devices[0].muted
        );
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn mute_write_failure_keeps_state_and_returns_os_error() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        engine.refresh();
        let state_before = engine.state();
        let gen_before = engine.lock().generation;
        let before = world.notes();
        // A leitura do mudo funciona; a escrita é que o Windows recusa.
        world.ctl.fail_writes.store(true, Ordering::SeqCst);

        let result = engine.act_output_mute(MuteMode::Set(true));

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::OsError));
        assert!(!result.changed);
        assert_eq!(result.resolved, None);
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // A escrita foi tentada e falhou: nada de estado guardado, nada de aviso.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetMuted("fone".to_string(), true)]
        );
        assert_eq!(
            engine.state().devices[0].muted,
            state_before.devices[0].muted
        );
        // A geração não sobe: nada foi mudado, então nenhum cache de leitura pode ser invalidado.
        assert_eq!(engine.lock().generation, gen_before);
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn act_output_mute_holds_apply_while_running() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = Arc::new(world.engine_at(dir.path()));
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        engine.refresh();
        let before = world.notes();

        // Porta do backend: a ação estaciona dentro de `ctl.muted()`, ou seja, parada com a
        // trava `apply` na mão. Nada de janela de tempo: a prova é a posse da trava.
        let (chegou_tx, chegou_rx) = mpsc::channel();
        let (liberar_tx, liberar_rx) = mpsc::channel();
        world.ctl.set_gate(chegou_tx, liberar_rx);

        let (tx, rx) = mpsc::channel();
        let worker = Arc::clone(&engine);
        let handle = std::thread::spawn(move || {
            let _ = tx.send(worker.act_output_mute(MuteMode::Set(true)));
        });

        // Espera positiva: se a ação não chegar no backend, o teste falha alto em vez de
        // passar calado por falta de prova.
        chegou_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a ação não chegou no backend");

        // A ação está parada dentro da leitura do mudo: ela tem que estar segurando `apply`.
        assert!(
            engine.apply.try_lock().is_err(),
            "a ação não segurou `apply` enquanto lia o mudo"
        );

        // Libera a ação: ela termina e devolve o resultado.
        liberar_tx
            .send(())
            .expect("a porta de teste já estava fechada");
        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a ação não terminou depois de a porta ser liberada");
        handle.join().unwrap();
        assert!(result.ok);
        assert!(result.changed);
        assert_eq!(result.resolved.as_deref(), Some("fone"));
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn step_volume_clamps_and_reports_change() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.ctl.set_volume_of("fone", 0.98);
        engine.refresh();
        let before = world.notes();

        let result = engine.act_output_volume(VolumeMode::Step(0.05));

        assert!(result.ok);
        assert!(result.changed);
        assert_eq!(result.resolved.as_deref(), Some("fone"));
        // 0,98 + 0,05 passa de 1: o valor gravado é o limite.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetVolume("fone".to_string(), 1.0)]
        );
        assert_eq!(engine.state().devices[0].volume, 1.0);
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn volume_reads_fresh_state_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.ctl.set_volume_of("fone", 0.98);
        engine.refresh();
        // Mudança por fora, depois do refresh: o endpoint passa a estar em 0,50, mas o
        // estado guardado continua dizendo 0,98.
        world.ctl.set_volume_of("fone", 0.50);
        let stale = engine.state();
        assert_eq!(stale.devices[0].volume, 0.98);
        let reads_before = world.ctl.reads();

        let result = engine.act_output_volume(VolumeMode::Step(0.05));

        assert!(result.ok);
        assert!(result.changed);
        assert_eq!(result.resolved.as_deref(), Some("fone"));
        // A soma parte do valor lido na hora (0,50 + 0,05). Quem usasse o estado
        // guardado somaria 0,98 + 0,05 e gravaria 1,0.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetVolume("fone".to_string(), 0.55)]
        );
        assert_eq!(engine.state().devices[0].volume, 0.55);
        // Uma única leitura: a do próprio endpoint, no momento da ação.
        assert_eq!(world.ctl.reads(), reads_before + 1);
    }

    #[test]
    fn step_at_limit_is_not_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.ctl.set_volume_of("fone", 1.0);
        engine.refresh();
        let before = world.notes();
        let reads_before = world.ctl.reads();

        let high = engine.act_output_volume(VolumeMode::Step(0.05));

        assert!(high.ok);
        assert!(!high.changed);
        assert_eq!(world.notes(), before);
        // A leitura fresca acontece (é ela que diz que o valor já está no limite),
        // mas é uma só: nada releu o endpoint uma segunda vez.
        assert_eq!(world.ctl.reads(), reads_before + 1);

        world.ctl.set_volume_of("fone", 0.0);
        engine.refresh();
        let before_low = world.notes();
        let reads_before_low = world.ctl.reads();

        let low = engine.act_output_volume(VolumeMode::Step(-0.05));

        assert!(low.ok);
        assert!(!low.changed);
        // Nos dois limites o endpoint não foi escrito e ninguém foi avisado.
        assert!(world.ctl.calls().is_empty());
        assert_eq!(world.notes(), before_low);
        assert_eq!(world.ctl.reads(), reads_before_low + 1);
    }

    #[test]
    fn set_volume_is_absolute_and_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.ctl.set_volume_of("fone", 0.5);
        engine.refresh();
        let before = world.notes();

        // O valor pedido é absoluto: o que está gravado não entra na conta.
        // Quem tratasse `Set` como soma gravaria 0,5 + 0,3 = 0,8.
        let absolute = engine.act_output_volume(VolumeMode::Set(0.3));

        assert!(absolute.ok);
        assert!(absolute.changed);
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetVolume("fone".to_string(), 0.3)]
        );

        let high = engine.act_output_volume(VolumeMode::Set(1.7));

        assert!(high.ok);
        assert!(high.changed);

        let low = engine.act_output_volume(VolumeMode::Set(-1.0));

        assert!(low.ok);
        assert!(low.changed);
        // Os três pedidos: o absoluto de 0,3 e os dois limites.
        assert_eq!(
            world.ctl.calls(),
            vec![
                CtlCall::SetVolume("fone".to_string(), 0.3),
                CtlCall::SetVolume("fone".to_string(), 1.0),
                CtlCall::SetVolume("fone".to_string(), 0.0),
            ]
        );
        assert_eq!(engine.state().devices[0].volume, 0.0);
        assert_eq!(world.notes(), before + 3);
    }

    #[test]
    fn set_volume_to_current_value_is_not_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.ctl.set_volume_of("fone", 0.5);
        engine.refresh();
        let before = world.notes();

        // Decisão, não acidente: pedir o volume que já está não muda nada e não
        // grava (mesma regra do mudo, RF-06). O epsilon absorve o arredondamento.
        let same = engine.act_output_volume(VolumeMode::Set(0.5));

        assert!(same.ok);
        assert!(!same.changed);
        assert_eq!(same.resolved.as_deref(), Some("fone"));
        assert!(world.ctl.calls().is_empty());
        assert_eq!(world.notes(), before);

        // Dentro do epsilon (0,0001): a mesma decisão.
        let within = engine.act_output_volume(VolumeMode::Set(0.50004));

        assert!(within.ok);
        assert!(!within.changed);
        assert!(world.ctl.calls().is_empty());
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn non_finite_volume_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        engine.refresh();
        let before = world.notes();
        let reads_before = world.ctl.reads();

        let nan = engine.act_output_volume(VolumeMode::Step(f32::NAN));
        let infinite = engine.act_output_volume(VolumeMode::Set(f32::INFINITY));

        assert!(!nan.ok);
        assert_eq!(nan.error_code, Some(ErrorCode::InvalidValue));
        assert!(!nan.changed);
        assert!(!infinite.ok);
        assert_eq!(infinite.error_code, Some(ErrorCode::InvalidValue));
        assert!(!infinite.changed);
        let message = nan.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // O valor inválido é recusado antes de falar com o endpoint.
        assert!(world.ctl.calls().is_empty());
        assert_eq!(world.ctl.reads(), reads_before);
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn volume_without_default_output_fails() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        // Nenhum dispositivo é o padrão do Windows.
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        engine.refresh();
        let before = world.notes();

        let result = engine.act_output_volume(VolumeMode::Step(0.05));

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::NoOutputDevice));
        assert!(!result.changed);
        assert_eq!(result.resolved, None);
        assert!(world.ctl.calls().is_empty());
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn volume_read_failure_returns_os_error_without_change() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.ctl.set_volume_of("fone", 0.6);
        engine.refresh();
        let state_before = engine.state();
        let before = world.notes();
        // A leitura é que falha: ligada depois do refresh, senão o próprio refresh falha.
        world.ctl.fail_reads.store(true, Ordering::SeqCst);

        let result = engine.act_output_volume(VolumeMode::Step(0.05));

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::OsError));
        assert!(!result.changed);
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // Sem leitura do volume não há escrita: o endpoint não foi tocado.
        assert!(world.ctl.calls().is_empty());
        assert_eq!(
            engine.state().devices[0].volume,
            state_before.devices[0].volume
        );
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn volume_write_failure_keeps_state_and_returns_os_error() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.ctl.set_volume_of("fone", 0.5);
        engine.refresh();
        let state_before = engine.state();
        let before = world.notes();
        // A leitura do volume funciona; a escrita é que o Windows recusa.
        world.ctl.fail_writes.store(true, Ordering::SeqCst);

        let result = engine.act_output_volume(VolumeMode::Step(0.125));

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::OsError));
        assert!(!result.changed);
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // A escrita foi tentada e falhou: nada de estado guardado, nada de aviso.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetVolume("fone".to_string(), 0.625)]
        );
        assert_eq!(
            engine.state().devices[0].volume,
            state_before.devices[0].volume
        );
        assert_eq!(world.notes(), before);
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
    fn default_render_is_the_connected_default_output() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let fone = Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        };
        world.source.set_devices(vec![
            device("alto", Flow::Render, true, "Alto"),
            fone.clone(),
            Device {
                is_default: true,
                ..device("mic", Flow::Capture, true, "Microfone")
            },
        ]);
        engine.refresh();
        assert_eq!(engine.default_render(), Some(dev_id("fone")));

        // Padrão desconectado não conta: não há saída padrão ativa.
        world.source.set_devices(vec![Device {
            connected: false,
            ..fone
        }]);
        engine.refresh();
        assert_eq!(engine.default_render(), None);
    }

    #[test]
    fn session_pids_matches_exe_case_insensitively_per_flow() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world.source.set_sessions(vec![
            session(10, "discord.exe", Flow::Render, "fone"),
            session(11, "Discord.EXE", Flow::Render, "fone"),
            session(12, "discord.exe", Flow::Capture, "mic"),
        ]);
        engine.refresh();

        assert_eq!(engine.session_pids("Discord.exe", Flow::Render), vec![10, 11]);
        assert_eq!(engine.session_pids("discord.exe", Flow::Capture), vec![12]);
    }

    #[test]
    fn visible_outputs_skips_hidden_and_disconnected_keeping_order() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![
            device("um", Flow::Render, true, "Um"),
            device("dois", Flow::Render, true, "Dois"),
            device("tres", Flow::Render, true, "Tres"),
        ]);
        engine.refresh();
        assert_eq!(
            engine.visible_outputs(),
            vec![dev_id("um"), dev_id("dois"), dev_id("tres")]
        );

        // "Dois" some por estar oculto; "Tres" some por estar desconectado.
        engine
            .update_device_meta(&dev_id("dois"), "Dois", None, true)
            .unwrap();
        world.source.set_devices(vec![
            device("um", Flow::Render, true, "Um"),
            device("dois", Flow::Render, true, "Dois"),
            device("tres", Flow::Render, false, "Tres"),
        ]);
        engine.refresh();

        assert_eq!(engine.visible_outputs(), vec![dev_id("um")]);
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

    #[test]
    fn set_app_route_does_not_reenumerate_devices_or_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "alto")]);
        engine.refresh();
        let devices_before = world.source.device_reads();
        let sessions_before = world.source.session_reads();

        engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));

        assert_eq!(world.source.device_reads(), devices_before);
        assert_eq!(world.source.session_reads(), sessions_before);
        assert_eq!(
            world.backend.set_calls(),
            vec![(10, Flow::Render, dev_id("fone"))]
        );
    }

    #[test]
    fn set_app_route_does_not_read_endpoint_volume_or_mute() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![
            device("fone", Flow::Render, true, "Fone"),
            device("alto", Flow::Render, true, "Alto"),
        ]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "alto")]);
        engine.refresh();
        let reads_before = world.ctl.reads();

        engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));
        let _ = engine.state();

        assert_eq!(world.ctl.reads(), reads_before);
    }

    #[test]
    fn set_app_route_only_routes_sessions_of_that_app() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world.source.set_sessions(vec![
            session(10, "chrome.exe", Flow::Render, "alto"),
            session(11, "spotify.exe", Flow::Render, "alto"),
        ]);
        engine.refresh();

        engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));

        assert_eq!(
            world.backend.set_calls(),
            vec![(10, Flow::Render, dev_id("fone"))]
        );
        let spotify = engine
            .state()
            .sessions
            .into_iter()
            .find(|s| s.pid == 11)
            .unwrap();
        assert_eq!(spotify.device, dev_id("alto"));
    }

    #[test]
    fn set_app_route_patches_the_cached_session_device_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "alto")]);
        engine.refresh();

        engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));

        let chrome = engine.state().sessions.into_iter().find(|s| s.pid == 10).unwrap();
        assert_eq!(chrome.device, dev_id("fone"));
    }

    #[test]
    fn rule_set_before_its_session_exists_is_applied_by_next_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        engine.refresh();

        // Ainda não há sessão do app em cache: a regra é gravada e nada é roteado.
        engine.set_app_route("chrome.exe", Flow::Render, Some(dev_id("fone")));
        assert!(world.backend.set_calls().is_empty());

        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "alto")]);
        engine.refresh();

        assert_eq!(
            world.backend.set_calls(),
            vec![(10, Flow::Render, dev_id("fone"))]
        );
    }

    #[test]
    fn set_device_volume_does_not_read_the_endpoint_back() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        engine.refresh();
        let reads_before = world.ctl.reads();

        engine.set_device_volume(&dev_id("fone"), 0.3).unwrap();

        assert_eq!(world.ctl.reads(), reads_before);
        assert_eq!(engine.state().devices[0].volume, 0.3);
    }
}
