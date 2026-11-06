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

use crate::actions::{ActionResult, Direction, ErrorCode, audio_failure};
use crate::apps::{ExeRequest, parse_exe_request};
use crate::audio::default_device::DefaultDevice;
use crate::audio::endpoint::{clamp_volume, EndpointControl};
use crate::audio::types::{AudioError, AudioSession, AudioSource, Device, DeviceId, Flow, RouteBackend};
use crate::bindings::migrate_settings;
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

/// Relato de uma ativação de perfil: os fatos que a ação precisa saber depois de ativar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProfileActivation {
    /// O perfil existe.
    found: bool,
    /// As regras ou o perfil ativo mudaram de fato (comparação antes/depois).
    changed: bool,
    /// O roteamento esbarrou em `Unsupported` **nesta** ativação.
    routing_unsupported: bool,
}

impl ProfileActivation {
    /// Perfil inexistente: nada muda e nada é avisado.
    fn not_found() -> ProfileActivation {
        ProfileActivation {
            found: false,
            changed: false,
            routing_unsupported: false,
        }
    }
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
        let (version, settings) = store.load_versioned("settings");
        // Formato antigo: o atalho legado vira um binding e os padrões são semeados.
        // A gravação só acontece quando a migração muda alguma coisa — assim o
        // arquivo bom não é reescrito a cada abertura.
        let (settings, migrou) = migrate_settings(version, settings);
        let save_error = if migrou {
            write_document(&store, "settings", &settings)
        } else {
            None
        };
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
            save_error,
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
        {
            let _apply = guard(&self.apply);
            self.route_locked(exe, flow, device);
        }
        self.persist("rules", |inner| inner.rules.clone());
        self.emit();
    }

    /// Corpo de `set_app_route`: **quem chama já segura `apply`** (o `Mutex` não é
    /// reentrante). Não grava em disco nem avisa a interface. Devolve se o conjunto de
    /// regras mudou de fato (comparação antes/depois).
    fn route_locked(&self, exe: &str, flow: Flow, device: Option<DeviceId>) -> bool {
        let exe = exe.to_lowercase();
        let rules_changed;
        {
            let (devices, sessions) = {
                let mut inner = self.lock();
                inner.generation += 1;
                let before = inner.rules.clone();
                match &device {
                    Some(device) => inner.rules.upsert(Rule {
                        exe: exe.clone(),
                        flow,
                        device: device.clone(),
                    }),
                    None => inner.rules.remove(&exe, flow),
                }
                rules_changed = inner.rules != before;
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
        rules_changed
    }

    /// Ativa um perfil. `false` quando o id não existe (nada muda e nada é avisado).
    pub fn activate_profile(&self, id: &str) -> bool {
        self.activate_profile_report(id).found
    }

    /// Ativa um perfil e relata o que aconteceu de fato.
    ///
    /// A ativação inteira roda sob a trava `apply`. O relato traz três fatos:
    /// - `found`: o perfil existe;
    /// - `changed`: as regras ou o perfil ativo mudaram (comparação antes/depois);
    /// - `routing_unsupported`: o roteamento esbarrou em `Unsupported` **nesta** ativação.
    ///
    /// O escopo do `changed` são as **regras e o perfil ativo**. O dispositivo das
    /// sessões fica de fora: ele muda por re-leitura do sistema e por sessão que entra
    /// ou sai — não é a ação que mudou.
    fn activate_profile_report(&self, id: &str) -> ProfileActivation {
        let report = {
            let _apply = guard(&self.apply);
            let (profile, mut rules, rules_before, profile_before) = {
                let inner = self.lock();
                let Some(profile) = inner.profiles.iter().find(|p| p.id == id).cloned() else {
                    return ProfileActivation::not_found();
                };
                (
                    profile,
                    inner.rules.clone(),
                    inner.rules.clone(),
                    inner.last_profile.clone(),
                )
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
            // O veredito do roteamento é o **desta** rodada, não o latch global: uma
            // falha antiga não pode fazer esta ativação parecer um `Unsupported`.
            let routing_unsupported = self.reconcile_cached();
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
            // `changed` é comparação antes/depois, não constante: reativar o perfil já
            // ativo não muda nada. `RuleSet::upsert`/`remove` preservam a ordem e o
            // `RuleSet` é `PartialEq`, então comparar o conjunto é confiável.
            let changed = {
                let inner = self.lock();
                inner.rules != rules_before || inner.last_profile != profile_before
            };
            ProfileActivation {
                found: true,
                changed,
                routing_unsupported,
            }
        };
        if report.found {
            self.emit();
        }
        report
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

    /// Liga ou desliga o mudo de um app, pelo nome do executável (ex.: `spotify.exe`).
    /// Vale para todas as sessões de saída desse app: uma sessão que já encerrou não
    /// interrompe as outras. Sem sessão do app, a ação dá certo sem mudar nada.
    ///
    /// Só as sessões que estão fora do estado pedido são escritas. `changed` diz se o
    /// estado mudou de fato: uma sessão que já estava no estado pedido não conta, e uma
    /// escrita que o Windows recusou também não.
    ///
    /// As sessões são lidas na hora (`source.sessions()`), não do snapshot guardado: um app
    /// que acabou de abrir ainda não está no cache, e mudá-lo pelo pid errado é o pior erro
    /// possível aqui.
    pub fn act_app_mute(&self, exe: &str, muted: bool) -> ActionResult {
        // Pedido inválido é recusado antes de qualquer leitura: nada é tocado.
        let Some(request) = parse_exe_request(exe) else {
            return ActionResult::failed(
                ErrorCode::InvalidExe,
                "O nome do aplicativo não é válido.",
            );
        };
        let key = exe_key(request);
        let sessions = match self.source.sessions() {
            Ok(sessions) => sessions,
            Err(err) => return audio_failure(&err),
        };
        // Só a saída do app: o microfone está fora de escopo.
        // E só as sessões que estão fora do estado pedido: uma sessão já no estado pedido
        // não é escrita (seria desperdício) nem conta como mudança (seria mentira).
        let targets: Vec<&AudioSession> = sessions
            .iter()
            .filter(|s| s.flow == Flow::Render && s.exe.eq_ignore_ascii_case(&key))
            .filter(|s| s.muted != muted)
            .collect();
        // Sem sessão do app, ou todas já no estado pedido: não é erro, é só não ter o que
        // mudar. Nenhuma escrita, nenhum notificador, nenhuma releitura.
        if targets.is_empty() {
            return ActionResult::done(false, Some(key));
        }
        let mut first_error: Option<AudioError> = None;
        let mut changed = false;
        {
            // `apply` é segurado só enquanto as sessões são mudadas. O `refresh()` adquire
            // `apply` por conta própria: chamá-lo com a trava na mão seria um deadlock.
            let _apply = guard(&self.apply);
            for session in &targets {
                match self.ctl.set_session_muted(session.pid, Flow::Render, muted) {
                    Ok(()) => changed = true,
                    // Uma sessão que sumiu não impede as outras.
                    Err(err) => {
                        if first_error.is_none() {
                            first_error = Some(err);
                        }
                    }
                }
            }
            if changed {
                // Mudança do usuário: invalida a leitura que estiver em andamento.
                self.lock().generation += 1;
            }
        }
        if changed {
            // Fora do escopo da trava: o refresh já avisa a interface.
            self.refresh();
            if first_error.is_none() {
                // Todo mundo foi mudado: o pedido foi cumprido inteiro.
                return ActionResult::done(true, Some(key));
            }
        }
        // O branch de erro se keya pelo erro existir, não por `changed` ser falso: o caso
        // "não havia nada a mudar" já saiu antes, e um `OsError` aqui seria erro falso.
        match first_error {
            // Metade do pedido cumprida: o estado mudou, mas o pedido não foi cumprido
            // inteiro. `ok` falso e `changed` verdadeiro ao mesmo tempo — a forma honesta.
            Some(err) if changed => {
                let falha = audio_failure(&err);
                let code = falha
                    .error_code
                    .expect("falha de áudio sempre tem código");
                let motivo = falha.message.expect("falha de áudio sempre tem frase");
                let message = format!(
                    "O mudo foi aplicado em parte, mas uma sessão não pôde ser mudada: {}",
                    lowercase_first(&motivo)
                );
                ActionResult::partial(ActionResult::done(true, Some(key)), code, &message)
            }
            Some(err) => audio_failure(&err),
            // Inalcançável hoje: o laço só deixa `changed == false` quando toda escrita
            // deu `Err`, o que garante `first_error.is_some()`. Fica `done` (e não um erro
            // inventado) para que um refactor futuro não passe a mentir com um erro falso.
            None => ActionResult::done(false, Some(key)),
        }
    }

    /// Ativa um perfil como ação (A2). Id que não existe vira `profile_not_found` e nada
    /// muda no estado.
    ///
    /// Não segura `apply`: `activate_profile_report` adquire a trava por conta própria, e o
    /// `Mutex` do std não é reentrante.
    pub fn act_activate_profile(&self, id: &str) -> ActionResult {
        let report = self.activate_profile_report(id);
        if !report.found {
            return ActionResult::failed(ErrorCode::ProfileNotFound, "Perfil não encontrado.");
        }
        if report.routing_unsupported {
            return ActionResult::partial(
                ActionResult::done(report.changed, Some(id.to_string())),
                ErrorCode::Unsupported,
                "O perfil foi ativado, mas este Windows não permite trocar o dispositivo dos aplicativos.",
            );
        }
        ActionResult::done(report.changed, Some(id.to_string()))
    }

    /// Manda um app para um dispositivo (A9); `None` — ou o texto `Padrão`/`Default` — volta
    /// ao padrão do Windows. Dispositivo ausente **não** é erro: a regra fica guardada e vale
    /// quando ele voltar.
    ///
    /// `changed` compara as regras antes e depois, dentro de `apply`: gravar a regra que já
    /// existia não muda nada. `apply` é segurado em um nível só e solto antes de gravar em
    /// disco e avisar a interface.
    pub fn act_move_app(&self, exe: &str, device: Option<String>) -> ActionResult {
        let Some(request) = parse_exe_request(exe) else {
            return ActionResult::failed(
                ErrorCode::InvalidExe,
                "O nome do aplicativo não é válido.",
            );
        };
        let key = exe_key(request);
        // A normalização vem antes de resolver o id: só o rótulo vira `None`.
        let device = device.filter(|d| !is_default_label(d)).map(DeviceId);
        let (changed, routing_available) = {
            let _apply = guard(&self.apply);
            let changed = self.route_locked(&key, Flow::Render, device);
            (changed, self.lock().routing_available)
        };
        if changed {
            self.persist("rules", |inner| inner.rules.clone());
        }
        self.emit();
        if !routing_available {
            let message = "Este Windows não permite essa operação.";
            // A regra pode ter sido gravada mesmo sem o roteamento: sucesso parcial.
            return if changed {
                ActionResult::partial(
                    ActionResult::done(true, Some(key)),
                    ErrorCode::Unsupported,
                    message,
                )
            } else {
                ActionResult::failed(ErrorCode::Unsupported, message)
            };
        }
        ActionResult::done(changed, Some(key))
    }

    /// Passa para o perfil seguinte ou anterior (A3). A ordem é a de `profiles`; o cursor é
    /// `last_profile`, só em memória. Sem cursor (ou com o perfil já apagado), `Next` vai ao
    /// primeiro e `Prev` ao último.
    ///
    /// Não segura `apply`: a ativação adquire a trava por conta própria.
    pub fn act_cycle_profile(&self, dir: Direction) -> ActionResult {
        let target = {
            let inner = self.lock();
            let count = inner.profiles.len();
            if count == 0 {
                return ActionResult::failed(ErrorCode::NoProfiles, "Nenhum perfil criado.");
            }
            let current = inner
                .last_profile
                .as_deref()
                .and_then(|id| inner.profiles.iter().position(|p| p.id == id));
            let next = cycle_index(current, count, dir);
            if current == Some(next) {
                // Um perfil só, e já é o ativo: o ciclo não tem para onde ir.
                return ActionResult::failed(
                    ErrorCode::NoProfiles,
                    "É preciso ao menos dois perfis para alternar.",
                );
            }
            inner.profiles[next].id.clone()
        };
        self.act_activate_profile(&target)
    }

    /// Troca a saída padrão do Windows pela seguinte ou anterior entre as visíveis (A10).
    /// Se a padrão estiver oculta, `Next` vai à primeira e `Prev` à última.
    ///
    /// A escolha e a troca acontecem sob `apply` (dois atalhos seguidos não escolhem a mesma
    /// saída); a releitura vem depois, fora da trava, porque `refresh()` adquire `apply`.
    pub fn act_cycle_output(&self, dir: Direction) -> ActionResult {
        let target = {
            let _apply = guard(&self.apply);
            let outputs = self.visible_outputs();
            if outputs.len() < 2 {
                return ActionResult::failed(
                    ErrorCode::NotEnoughOutputs,
                    "É preciso ao menos duas saídas.",
                );
            }
            let current = self
                .default_render()
                .and_then(|id| outputs.iter().position(|o| *o == id));
            let target = outputs[cycle_index(current, outputs.len(), dir)].clone();
            if let Err(err) = self.defaults.set_default(&target, Flow::Render) {
                return audio_failure(&err);
            }
            target
        };
        self.refresh();
        ActionResult::done(true, Some(target.0))
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
    ///
    /// Devolve `true` quando **esta** rodada esbarrou em `Unsupported`. O latch global
    /// (`routing_available`) continua sendo atualizado, mas ele é histórico: quem precisa
    /// saber se o roteamento funcionou *nesta* ativação usa o valor devolvido aqui.
    fn reconcile_cached(&self) -> bool {
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
        let unsupported = outcomes.iter().any(|o| {
            matches!(
                o,
                Outcome::Skipped {
                    reason: AudioError::Unsupported(_),
                    ..
                }
            )
        });
        self.latch_routing(&outcomes);
        unsupported
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

/// Chave de comparação do executável: o nome do arquivo, em minúsculas. Um caminho
/// absoluto vira o nome do arquivo; um nome puro fica como está.
fn exe_key(request: ExeRequest<'_>) -> String {
    match request {
        ExeRequest::Name(name) => name.to_lowercase(),
        ExeRequest::Path(path) => exe_file_name(path).to_lowercase(),
    }
}

/// Rótulo que a interface usa para "voltar ao padrão do Windows" no lugar de um id.
fn is_default_label(text: &str) -> bool {
    matches!(text.trim().to_lowercase().as_str(), "padrão" | "default")
}

/// Próxima posição de um ciclo de `count` itens (`count > 0`). Sem posição atual, `Next`
/// vai ao primeiro e `Prev` ao último; com ela, dá a volta nas pontas.
fn cycle_index(current: Option<usize>, count: usize, dir: Direction) -> usize {
    match (current, dir) {
        (None, Direction::Next) => 0,
        (None, Direction::Prev) => count - 1,
        (Some(at), Direction::Next) => (at + 1) % count,
        (Some(at), Direction::Prev) => (at + count - 1) % count,
    }
}

/// Nome do arquivo de um caminho: o que vem depois do último `\` ou `/`.
fn exe_file_name(path: &str) -> &str {
    let cut = path.rfind(['\\', '/']).map(|at| at + 1).unwrap_or(0);
    &path[cut..]
}

/// Primeira letra em minúscula, para encaixar uma frase dentro de outra.
fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
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
    use crate::actions::{Action, ActionExecutor, Direction};
    use crate::audio::default_device::DefaultDevice;
    use crate::audio::endpoint::EndpointControl;
    use crate::audio::types::{AudioError, AudioSession, Device};
    use crate::bindings::Binding;
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

    /// Porta só de teste: a chamada avisa que chegou e fica parada até o teste liberar.
    /// Uso único: a primeira chamada que chega é a que para. Sem porta postada, a chamada
    /// passa direto.
    #[derive(Default)]
    struct Gate {
        chegou: Mutex<Option<mpsc::Sender<()>>>,
        liberar: Mutex<Option<mpsc::Receiver<()>>>,
    }

    impl Gate {
        /// Posta a porta: a próxima chamada avisa em `chegou` e espera a permissão de
        /// `liberar`.
        fn post(&self, chegou: mpsc::Sender<()>, liberar: mpsc::Receiver<()>) {
            *self.chegou.lock().unwrap() = Some(chegou);
            *self.liberar.lock().unwrap() = Some(liberar);
        }

        /// Estaciona a chamada dentro da porta, se houver uma posta.
        fn wait(&self) {
            let chegou = self.chegou.lock().unwrap().take();
            let liberar = self.liberar.lock().unwrap().take();
            let (Some(chegou), Some(liberar)) = (chegou, liberar) else {
                return;
            };
            // Avisa que chegou e espera a permissão do teste.
            let _ = chegou.send(());
            let _ = liberar.recv();
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
        /// Porta só de teste: a gravação de rota avisa que chegou e fica parada até o
        /// teste liberar. Serve para provar a posse da trava `apply` no caminho da
        /// ativação de perfil.
        gate: Gate,
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

        /// Posta a porta de teste da gravação de rota.
        fn set_gate(&self, chegou: mpsc::Sender<()>, liberar: mpsc::Receiver<()>) {
            self.gate.post(chegou, liberar);
        }
    }

    impl RouteBackend for FakeBackend {
        fn set_route(&self, pid: u32, flow: Flow, device: &DeviceId) -> Result<(), AudioError> {
            // A porta de teste segura a gravação aqui, com a trava `apply` do chamador
            // ainda na mão: é o ponto exato em que a exclusão mútua pode ser provada.
            self.gate.wait();
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
        /// Pids de sessão que já sumiram do Windows: escrever neles devolve `Gone`.
        gone_sessions: Mutex<Vec<u32>>,
        /// Porta só de teste da leitura de mudo: avisa que a chamada chegou no backend e a
        /// segura parada até o teste liberar. Vazia (padrão) = a chamada passa direto.
        gate: Gate,
        /// Porta só de teste das escritas de sessão: mesma ideia da de cima, mas separada
        /// para não colidir com a porta da leitura de mudo.
        gate_sessao: Gate,
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

        /// Marca uma sessão como encerrada: escrever o mudo nela devolve `Gone`.
        fn gone_session(&self, pid: u32) {
            self.gone_sessions.lock().unwrap().push(pid);
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
            self.gate.post(chegou, liberar);
        }

        /// Estaciona a chamada dentro da porta, se houver uma posta. Sem porta, passa direto.
        /// A porta é de uso único: a primeira chamada que chega é a que para.
        fn wait_gate(&self) {
            self.gate.wait();
        }

        /// Posta a porta de teste das escritas de sessão: a próxima escrita de mudo de
        /// sessão avisa em `chegou` e fica parada esperando a permissão de `liberar`.
        fn set_gate_sessao(&self, chegou: mpsc::Sender<()>, liberar: mpsc::Receiver<()>) {
            self.gate_sessao.post(chegou, liberar);
        }

        /// Estaciona a escrita de sessão dentro da porta, se houver uma posta.
        fn wait_gate_sessao(&self) {
            self.gate_sessao.wait();
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
            // A porta de teste segura a escrita aqui, com a trava `apply` do chamador
            // ainda na mão: é o ponto exato em que a exclusão mútua pode ser provada.
            self.wait_gate_sessao();
            self.record(CtlCall::SetSessionMuted(pid, flow, m))?;
            if self.gone_sessions.lock().unwrap().contains(&pid) {
                return Err(AudioError::Gone);
            }
            Ok(())
        }
    }

    /// Troca de padrão falsa: grava cada pedido; `fail` faz a troca falhar.
    #[derive(Default)]
    struct FakeDefaults {
        calls: Mutex<Vec<(DeviceId, Flow)>>,
        fail: AtomicBool,
        /// O dispositivo sumiu: a troca devolve `Gone`.
        gone: AtomicBool,
    }

    impl FakeDefaults {
        fn calls(&self) -> Vec<(DeviceId, Flow)> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl DefaultDevice for FakeDefaults {
        fn set_default(&self, id: &DeviceId, flow: Flow) -> Result<(), AudioError> {
            self.calls.lock().unwrap().push((id.clone(), flow));
            if self.gone.load(Ordering::SeqCst) {
                return Err(AudioError::Gone);
            }
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
        // A forma do estado que a interface lê.
        //
        // **Mudança de propósito (Ruling 20 / RNF-06):** `settings` ganhou o campo
        // `bindings` — a tabela de atalhos globais (combinação → ação). O `AppState`
        // carrega `Settings` como está, então o campo novo aparece dentro de
        // `settings` e é essa a forma que o JSON passa a ter. Nada mais do estado
        // mudou: as chaves de cima continuam as mesmas.
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
            settings: Settings {
                shortcut: "Super+Alt+V".into(),
                autostart: false,
                bindings: vec![Binding {
                    accelerator: "Ctrl+Alt+M".into(),
                    action: Action::ToggleOutputMute,
                }],
            },
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
        // Campo novo (Ruling 20): a tabela de atalhos dentro das configurações.
        let bindings_json = json["settings"]["bindings"].as_array().unwrap();
        assert_eq!(bindings_json.len(), 1);
        assert_eq!(bindings_json[0]["accelerator"], "Ctrl+Alt+M");
        assert_eq!(bindings_json[0]["action"]["type"], "toggle_output_mute");
        let settings_keys = |v: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            keys.sort();
            keys
        };
        assert_eq!(
            settings_keys(&json["settings"]),
            vec!["autostart", "bindings", "shortcut"]
        );

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
            .recv_timeout(Duration::from_secs(15))
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
            .recv_timeout(Duration::from_secs(15))
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

    // ---------- ativação de perfil por ação (A2) ----------

    #[test]
    fn activate_unknown_profile_fails_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(40, "game.exe", Flow::Render, "fone")]);
        engine.refresh();
        let before = engine.state();
        let notes_before = world.notes();

        let result = engine.act_activate_profile("nao-existe");

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::ProfileNotFound));
        assert!(!result.changed);
        assert_eq!(result.resolved, None);
        assert_eq!(result.message.as_deref(), Some("Perfil não encontrado."));
        // Estado igual ao anterior: nenhuma regra gravada, nenhum aviso.
        assert_eq!(engine.state(), before);
        assert_eq!(world.notes(), notes_before);
    }

    #[test]
    fn activate_profile_action_applies_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(40, "game.exe", Flow::Render, "fone")]);
        engine.refresh();

        let result = engine.act_activate_profile("p1");

        assert!(result.ok);
        assert!(result.changed);
        assert_eq!(result.resolved.as_deref(), Some("p1"));
        assert_eq!(
            engine.state().rules,
            vec![rule("game.exe", Flow::Render, "fone")]
        );
        assert_eq!(
            world.backend.set_calls(),
            vec![(40, Flow::Render, dev_id("fone"))]
        );
        assert_eq!(engine.state().last_profile.as_deref(), Some("p1"));
    }

    #[test]
    fn activate_profile_action_fails_when_routing_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(40, "game.exe", Flow::Render, "fone")]);
        // Este Windows não deixa trocar a rota dos aplicativos.
        world.backend.unsupported.store(true, Ordering::SeqCst);
        engine.refresh();

        let result = engine.act_activate_profile("p1");

        // O perfil foi ativado (as regras foram gravadas), mas o roteamento falhou:
        // sucesso parcial. `ok` falso e `changed` verdadeiro ao mesmo tempo — a resposta
        // honesta. O veredito vem **desta** ativação, não do latch global.
        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::Unsupported));
        assert!(result.changed, "o perfil foi ativado: o estado mudou de fato");
        assert_eq!(result.resolved.as_deref(), Some("p1"));
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // A frase diz as duas coisas: o que mudou e o que não deu.
        assert_eq!(
            message,
            "O perfil foi ativado, mas este Windows não permite trocar o dispositivo dos aplicativos."
        );
        // As regras foram persistidas mesmo com o roteamento indisponível.
        assert_eq!(
            engine.state().rules,
            vec![rule("game.exe", Flow::Render, "fone")]
        );
        assert_eq!(engine.state().last_profile.as_deref(), Some("p1"));
    }

    #[test]
    fn reactivating_the_current_profile_reports_no_change() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(40, "game.exe", Flow::Render, "fone")]);
        engine.refresh();

        let first = engine.act_activate_profile("p1");
        assert!(first.ok);
        assert!(first.changed);

        // O mesmo perfil de novo: as regras são as mesmas e o perfil ativo já era este.
        let second = engine.act_activate_profile("p1");

        assert!(second.ok);
        assert!(
            !second.changed,
            "a ação disse que mudou quando nada mudou"
        );
        assert_eq!(second.resolved.as_deref(), Some("p1"));
    }

    #[test]
    fn switching_profile_reports_change() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        // Mesmo conteúdo de regras do `p1`: o que muda é o perfil ativo.
        engine
            .save_profile(profile(
                "p2",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        // Regras diferentes: agora o conjunto de regras muda também.
        engine
            .save_profile(profile(
                "p3",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("monitor")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        world
            .source
            .set_devices(vec![
                device("fone", Flow::Render, true, "Fone"),
                device("monitor", Flow::Render, true, "Monitor"),
            ]);
        world
            .source
            .set_sessions(vec![session(40, "game.exe", Flow::Render, "fone")]);
        engine.refresh();

        let first = engine.act_activate_profile("p1");
        assert!(first.ok);
        assert!(first.changed);

        // Outro perfil, mesmas regras: o perfil ativo mudou.
        let second = engine.act_activate_profile("p2");
        assert!(second.ok);
        assert!(
            second.changed,
            "trocar de perfil muda o perfil ativo, mesmo com as mesmas regras"
        );
        assert_eq!(second.resolved.as_deref(), Some("p2"));
        assert_eq!(engine.state().last_profile.as_deref(), Some("p2"));

        // Perfil com regras diferentes: o conjunto de regras muda também.
        let third = engine.act_activate_profile("p3");
        assert!(third.ok);
        assert!(third.changed, "trocar de perfil com regras diferentes muda o estado");
        assert_eq!(third.resolved.as_deref(), Some("p3"));
        assert_eq!(
            engine.state().rules,
            vec![rule("game.exe", Flow::Render, "monitor")]
        );
        assert_eq!(engine.state().last_profile.as_deref(), Some("p3"));
    }

    #[test]
    fn activating_profile_without_routing_ignores_stale_unsupported_latch() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(40, "game.exe", Flow::Render, "alto")]);
        engine.refresh();

        // Uma reconciliação antiga falhou: o latch global ficou em `false`.
        world.backend.unsupported.store(true, Ordering::SeqCst);
        engine.set_app_route("game.exe", Flow::Render, Some(dev_id("fone")));
        assert!(!engine.state().routing_available);

        // Agora o Windows volta a aceitar o roteamento, mas a sessão do app some.
        world.backend.unsupported.store(false, Ordering::SeqCst);
        world.source.set_sessions(vec![]);
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "outro.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();

        let result = engine.act_activate_profile("p1");

        // A ativação não esbarrou em `Unsupported`: o veredito vem da ativação, não do
        // latch global (que continua em `false`, porque só um roteamento bem-sucedido o
        // religa).
        assert!(result.ok, "a ativação herdou o `false` de uma falha antiga");
        assert!(result.changed);
        assert_eq!(result.resolved.as_deref(), Some("p1"));
        assert_eq!(result.error_code, None);
        assert!(!engine.state().routing_available);
    }

    #[test]
    fn activate_profile_action_does_not_deadlock_on_apply() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = Arc::new(world.engine_at(dir.path()));
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(40, "game.exe", Flow::Render, "fone")]);
        engine.refresh();

        // Porta do backend: a ativação estaciona dentro de `set_route()`, ou seja, parada
        // com a trava `apply` na mão.
        let (chegou_tx, chegou_rx) = mpsc::channel();
        let (liberar_tx, liberar_rx) = mpsc::channel();
        world.backend.set_gate(chegou_tx, liberar_rx);

        let (tx, rx) = mpsc::channel();
        let worker = Arc::clone(&engine);
        let handle = std::thread::spawn(move || {
            let _ = tx.send(worker.act_activate_profile("p1"));
        });

        // Espera positiva: se a ação pegar `apply` duas vezes, ela nunca chega no backend
        // e o teste falha alto (com prazo) em vez de travar a suíte.
        chegou_rx
            .recv_timeout(Duration::from_secs(15))
            .expect("a ativação não chegou no backend de roteamento");

        // A ação está parada dentro do backend de rota: ela tem que estar segurando `apply`.
        assert!(
            engine.apply.try_lock().is_err(),
            "a ativação não segurou `apply` enquanto roteava"
        );

        // Libera: a ação termina.
        liberar_tx
            .send(())
            .expect("a porta de teste já estava fechada");
        let result = rx
            .recv_timeout(Duration::from_secs(15))
            .expect("a ativação não terminou depois de a porta ser liberada");
        handle.join().unwrap();

        assert!(result.ok);
        assert!(result.changed);
        assert_eq!(result.resolved.as_deref(), Some("p1"));
    }

    // ---------- A9: mover app ----------

    /// Motor com o fone conhecido e uma sessão do Chrome tocando nele.
    fn move_app_setup(dir: &Path) -> (World, Engine) {
        let world = World::new();
        let engine = world.engine_at(dir);
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "chrome.exe", Flow::Render, "fone")]);
        engine.refresh();
        (world, engine)
    }

    #[test]
    fn move_app_stores_rule_for_absent_device() {
        let dir = tempfile::tempdir().unwrap();
        let (world, engine) = move_app_setup(dir.path());

        let result = engine.act_move_app("Chrome.EXE", Some("fantasma".to_string()));

        // Dispositivo ausente não é erro: a regra fica guardada para quando ele voltar.
        assert!(result.ok, "{result:?}");
        assert!(result.changed);
        assert_eq!(result.error_code, None);
        assert_eq!(result.resolved.as_deref(), Some("chrome.exe"));
        assert_eq!(
            engine.state().rules,
            vec![rule("chrome.exe", Flow::Render, "fantasma")]
        );
        assert!(world.backend.set_calls().is_empty());
    }

    #[test]
    fn move_app_with_none_clears_the_rule() {
        let dir = tempfile::tempdir().unwrap();
        save_rules(dir.path(), vec![rule("chrome.exe", Flow::Render, "fone")]);
        let (_world, engine) = move_app_setup(dir.path());

        let result = engine.act_move_app("chrome.exe", None);

        assert!(result.ok, "{result:?}");
        assert!(result.changed);
        assert!(engine.state().rules.is_empty());
        // Limpar uma regra que já não existe não muda nada.
        let again = engine.act_move_app("chrome.exe", None);
        assert!(again.ok);
        assert!(!again.changed);
    }

    #[test]
    fn move_app_reports_unsupported_when_backend_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let (world, engine) = move_app_setup(dir.path());
        world.backend.unsupported.store(true, Ordering::SeqCst);

        let result = engine.act_move_app("chrome.exe", Some("fone".to_string()));

        // A regra foi gravada, mas o roteamento não deu: sucesso parcial (Ruling 35).
        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::Unsupported));
        assert!(result.changed, "a regra foi gravada: o estado mudou");
        assert_eq!(
            result.message.as_deref(),
            Some("Este Windows não permite essa operação.")
        );
        assert_eq!(
            engine.state().rules,
            vec![rule("chrome.exe", Flow::Render, "fone")]
        );
    }

    #[test]
    fn move_app_rejects_invalid_exe() {
        let dir = tempfile::tempdir().unwrap();
        let (world, engine) = move_app_setup(dir.path());
        let notes = world.notes();

        for bad in ["", "chrome", "..\\chrome.exe", "\\\\srv\\x\\chrome.exe"] {
            let result = engine.act_move_app(bad, Some("fone".to_string()));
            assert!(!result.ok, "{bad:?}");
            assert!(!result.changed);
            assert_eq!(result.error_code, Some(ErrorCode::InvalidExe), "{bad:?}");
        }
        assert!(engine.state().rules.is_empty());
        assert_eq!(world.notes(), notes);
    }

    #[test]
    fn move_app_treats_padrao_string_as_absent_device() {
        for label in ["Padrão", "Default"] {
            let dir = tempfile::tempdir().unwrap();
            save_rules(dir.path(), vec![rule("chrome.exe", Flow::Render, "fone")]);
            let (world, engine) = move_app_setup(dir.path());
            let calls_before = world.backend.set_calls().len();

            let result = engine.act_move_app("chrome.exe", Some(label.to_string()));

            // Mesma semântica de `None`: a regra some, sem erro e sem rota nova.
            assert!(result.ok, "{label}: {result:?}");
            assert!(result.changed, "{label}");
            assert_eq!(result.error_code, None);
            assert!(engine.state().rules.is_empty(), "{label}");
            assert_eq!(world.backend.set_calls().len(), calls_before);
        }
    }

    #[test]
    fn move_app_does_not_touch_a_real_device_id() {
        let dir = tempfile::tempdir().unwrap();
        let (_world, engine) = move_app_setup(dir.path());

        let result = engine.act_move_app("chrome.exe", Some("fone".to_string()));

        assert!(result.ok, "{result:?}");
        assert_eq!(
            engine.state().rules,
            vec![rule("chrome.exe", Flow::Render, "fone")]
        );
    }

    #[test]
    fn move_app_reports_change_only_when_the_rule_differs() {
        let dir = tempfile::tempdir().unwrap();
        let (_world, engine) = move_app_setup(dir.path());

        let first = engine.act_move_app("chrome.exe", Some("fone".to_string()));
        let same = engine.act_move_app("CHROME.exe", Some("fone".to_string()));
        let other = engine.act_move_app("chrome.exe", Some("outro".to_string()));

        assert!(first.changed);
        assert!(same.ok && !same.changed, "regra idêntica não muda nada");
        assert!(other.ok && other.changed, "trocar o dispositivo muda");
    }

    // ---------- A3: ciclar perfil ----------

    fn three_profiles(engine: &Engine) {
        for id in ["p1", "p2", "p3"] {
            engine.save_profile(profile(id, vec![])).unwrap();
        }
    }

    fn cycled(engine: &Engine, dir: Direction) -> Option<String> {
        let result = engine.act_cycle_profile(dir);
        assert!(result.ok, "{result:?}");
        assert!(result.changed, "{result:?}");
        result.resolved
    }

    #[test]
    fn cycle_wraps_after_last() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        three_profiles(&engine);

        let ids: Vec<Option<String>> = (0..4).map(|_| cycled(&engine, Direction::Next)).collect();

        let expected: Vec<Option<String>> = ["p1", "p2", "p3", "p1"]
            .iter()
            .map(|s| Some(s.to_string()))
            .collect();
        assert_eq!(ids, expected);
        assert_eq!(engine.state().last_profile.as_deref(), Some("p1"));
    }

    #[test]
    fn cycle_prev_wraps_to_last() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        three_profiles(&engine);

        // Sem cursor, `Prev` vai ao último; depois recua e dá a volta.
        assert_eq!(cycled(&engine, Direction::Prev).as_deref(), Some("p3"));
        assert_eq!(cycled(&engine, Direction::Prev).as_deref(), Some("p2"));
        assert_eq!(cycled(&engine, Direction::Prev).as_deref(), Some("p1"));
        assert_eq!(cycled(&engine, Direction::Prev).as_deref(), Some("p3"));
    }

    #[test]
    fn deleted_current_profile_goes_to_first() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        three_profiles(&engine);
        assert!(engine.act_activate_profile("p2").ok);

        engine.delete_profile("p2");

        assert_eq!(cycled(&engine, Direction::Next).as_deref(), Some("p1"));
    }

    #[test]
    fn no_profiles_is_a_failure_that_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let before = engine.state();
        let notes = world.notes();

        let result = engine.act_cycle_profile(Direction::Next);

        assert!(!result.ok);
        assert!(!result.changed);
        assert_eq!(result.error_code, Some(ErrorCode::NoProfiles));
        assert_eq!(result.message.as_deref(), Some("Nenhum perfil criado."));
        assert_eq!(engine.state(), before);
        assert_eq!(world.notes(), notes);
    }

    #[test]
    fn cycle_does_not_persist_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        three_profiles(&engine);
        assert_eq!(cycled(&engine, Direction::Next).as_deref(), Some("p1"));
        assert_eq!(cycled(&engine, Direction::Next).as_deref(), Some("p2"));

        // Um motor novo sobre o mesmo diretório começa do primeiro perfil.
        let reopened = world.engine_at(dir.path());

        assert_eq!(reopened.state().last_profile, None);
        assert_eq!(cycled(&reopened, Direction::Next).as_deref(), Some("p1"));
    }

    #[test]
    fn cycle_with_a_single_current_profile_does_nothing_with_a_warning() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        engine.save_profile(profile("p1", vec![])).unwrap();
        assert_eq!(cycled(&engine, Direction::Next).as_deref(), Some("p1"));

        let result = engine.act_cycle_profile(Direction::Next);

        // O ciclo não tem para onde avançar: `failed` com `changed` falso (Ruling 17).
        assert!(!result.ok);
        assert!(!result.changed);
        assert!(result.error_code.is_some());
        assert!(result.message.is_some());
    }

    // ---------- A10: ciclar saída padrão ----------

    /// Saídas conectadas, na ordem dada, com `default` como padrão do Windows.
    fn outputs(ids: &[&str], default: &str) -> Vec<Device> {
        ids.iter()
            .map(|id| Device {
                is_default: *id == default,
                ..device(id, Flow::Render, true, id)
            })
            .collect()
    }

    /// Simula o Windows aceitando a troca: o padrão da fonte muda e o motor relê.
    fn windows_switches_default(world: &World, engine: &Engine, ids: &[&str], default: &str) {
        world.source.set_devices(outputs(ids, default));
        engine.refresh();
    }

    #[test]
    fn cycle_output_skips_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let ids = ["a", "b", "c"];
        world.source.set_devices(outputs(&ids, "a"));
        engine.refresh();
        engine
            .update_device_meta(&dev_id("b"), "b", None, true)
            .unwrap();

        let first = engine.act_cycle_output(Direction::Next);
        assert!(first.ok, "{first:?}");
        assert!(first.changed);
        assert_eq!(first.resolved.as_deref(), Some("c"));
        windows_switches_default(&world, &engine, &ids, "c");

        let second = engine.act_cycle_output(Direction::Next);
        assert_eq!(second.resolved.as_deref(), Some("a"));
        assert!(second.ok && second.changed);
        assert_eq!(
            world.defaults.calls(),
            vec![(dev_id("c"), Flow::Render), (dev_id("a"), Flow::Render)]
        );
    }

    #[test]
    fn single_output_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(outputs(&["a"], "a"));
        engine.refresh();

        let result = engine.act_cycle_output(Direction::Next);

        assert!(!result.ok);
        assert!(!result.changed);
        assert_eq!(result.error_code, Some(ErrorCode::NotEnoughOutputs));
        assert_eq!(
            result.message.as_deref(),
            Some("É preciso ao menos duas saídas.")
        );
        assert!(world.defaults.calls().is_empty());
    }

    #[test]
    fn cycle_output_when_default_is_hidden_goes_to_first() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(outputs(&["a", "b", "c"], "b"));
        engine.refresh();
        engine
            .update_device_meta(&dev_id("b"), "b", None, true)
            .unwrap();

        let result = engine.act_cycle_output(Direction::Next);

        assert!(result.ok, "{result:?}");
        assert_eq!(result.resolved.as_deref(), Some("a"));
        assert_eq!(world.defaults.calls(), vec![(dev_id("a"), Flow::Render)]);
    }

    #[test]
    fn cycle_output_survives_gone_device() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(outputs(&["a", "b"], "a"));
        engine.refresh();
        world.defaults.gone.store(true, Ordering::SeqCst);

        let result = engine.act_cycle_output(Direction::Next);

        assert!(!result.ok);
        assert!(!result.changed);
        assert_eq!(result.error_code, Some(ErrorCode::DeviceGone));
        // O motor segue de pé: a próxima tentativa, com o dispositivo de volta, funciona.
        world.defaults.gone.store(false, Ordering::SeqCst);
        let retry = engine.act_cycle_output(Direction::Next);
        assert!(retry.ok, "{retry:?}");
        assert_eq!(retry.resolved.as_deref(), Some("b"));
    }

    #[test]
    fn executor_maps_move_app_and_both_cycles() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(outputs(&["a", "b"], "a"));
        engine.refresh();
        engine.save_profile(profile("p1", vec![])).unwrap();

        let moved = engine.execute(Action::MoveApp {
            exe: "chrome.exe".to_string(),
            device: Some("a".to_string()),
        });
        let profile_cycled = engine.execute(Action::CycleProfile {
            direction: Direction::Next,
        });
        let output_cycled = engine.execute(Action::CycleDefaultOutput {
            direction: Direction::Next,
        });

        assert!(moved.ok && moved.changed, "{moved:?}");
        assert_eq!(profile_cycled.resolved.as_deref(), Some("p1"));
        assert_eq!(output_cycled.resolved.as_deref(), Some("b"));
    }

    // ---------- executor de ações ----------

    #[test]
    fn executor_maps_each_implemented_action() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_devices(vec![Device {
            is_default: true,
            ..device("fone", Flow::Render, true, "Fone")
        }]);
        world.source.set_sessions(vec![
            session(30, "discord.exe", Flow::Render, "fone"),
            session(40, "game.exe", Flow::Render, "fone"),
        ]);
        world.ctl.set_volume_of("fone", 0.5);
        engine
            .save_profile(profile(
                "p1",
                vec![ProfileItem {
                    exe: "game.exe".to_string(),
                    output: Target::Device(dev_id("fone")),
                    input: Target::Keep,
                }],
            ))
            .unwrap();
        engine.refresh();

        let mute = engine.execute(Action::SetOutputMute { muted: true });
        assert!(mute.ok);
        assert!(mute.changed);
        assert_eq!(mute.resolved.as_deref(), Some("fone"));

        let toggle = engine.execute(Action::ToggleOutputMute);
        assert!(toggle.ok);
        assert!(toggle.changed);

        let step = engine.execute(Action::StepOutputVolume { delta: 0.1 });
        assert!(step.ok);
        assert!(step.changed);

        let set = engine.execute(Action::SetOutputVolume { value: 0.25 });
        assert!(set.ok);
        assert!(set.changed);

        let app = engine.execute(Action::SetAppMute {
            exe: "discord.exe".to_string(),
            muted: true,
        });
        assert!(app.ok);
        assert!(app.changed);
        assert_eq!(app.resolved.as_deref(), Some("discord.exe"));

        let activate = engine.execute(Action::ActivateProfile {
            profile_id: "p1".to_string(),
        });
        assert!(activate.ok);
        assert!(activate.changed);
        assert_eq!(activate.resolved.as_deref(), Some("p1"));

        // Cada ação foi para o `act_*` correspondente: o executor não reimplementa nada.
        assert_eq!(
            world.ctl.calls(),
            vec![
                CtlCall::SetMuted("fone".to_string(), true),
                CtlCall::SetMuted("fone".to_string(), false),
                CtlCall::SetVolume("fone".to_string(), 0.6),
                CtlCall::SetVolume("fone".to_string(), 0.25),
                CtlCall::SetSessionMuted(30, Flow::Render, true),
            ]
        );
        assert_eq!(
            world.backend.set_calls(),
            vec![(40, Flow::Render, dev_id("fone"))]
        );
    }

    #[test]
    fn executor_refuses_actions_not_implemented_yet() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());

        // Só o painel segue sem ação no `Engine`: ele é da `AppExecutor` (Task 8).
        for action in [Action::TogglePanel] {
            let result = engine.execute(action);
            assert!(!result.ok);
            assert_eq!(result.error_code, Some(ErrorCode::Unsupported));
            assert!(!result.changed);
            assert_eq!(result.resolved, None);
            assert_eq!(
                result.message.as_deref(),
                Some("Ação ainda não disponível.")
            );
        }

        // Nenhuma ação recusada chegou ao Windows.
        assert!(world.ctl.calls().is_empty());
        assert!(world.backend.set_calls().is_empty());
    }

    // ---------- mudo de app (A8) ----------

    #[test]
    fn app_mute_affects_all_pids_of_the_exe_only() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_sessions(vec![
            session(10, "discord.exe", Flow::Render, "fone"),
            session(11, "Discord.EXE", Flow::Render, "fone"),
            session(12, "spotify.exe", Flow::Render, "fone"),
            // Captura do mesmo app: o microfone está fora de escopo.
            session(13, "discord.exe", Flow::Capture, "mic"),
        ]);

        let result = engine.act_app_mute("discord.exe", true);

        assert!(result.ok);
        assert!(result.changed);
        assert_eq!(result.resolved.as_deref(), Some("discord.exe"));
        // Só as sessões de saída do Discord: as outras duas nem são tocadas.
        assert_eq!(
            world.ctl.calls(),
            vec![
                CtlCall::SetSessionMuted(10, Flow::Render, true),
                CtlCall::SetSessionMuted(11, Flow::Render, true),
            ]
        );
    }

    #[test]
    fn app_mute_without_session_is_ok_and_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_sessions(vec![session(12, "spotify.exe", Flow::Render, "fone")]);
        let before = world.notes();

        let result = engine.act_app_mute("discord.exe", true);

        assert!(result.ok);
        assert!(!result.changed);
        assert_eq!(result.resolved.as_deref(), Some("discord.exe"));
        assert!(world.ctl.calls().is_empty());
        // Nada mudou: ninguém é avisado.
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn app_mute_rejects_unc_path() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_sessions(vec![session(10, "discord.exe", Flow::Render, "fone")]);
        let before = world.notes();

        let result = engine.act_app_mute("\\\\servidor\\share\\x.exe", true);

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::InvalidExe));
        assert!(!result.changed);
        assert_eq!(result.resolved, None);
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // Recusado antes de olhar o sistema: nada foi lido nem escrito.
        assert!(world.ctl.calls().is_empty());
        assert_eq!(world.source.session_reads(), 0);
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn app_mute_survives_one_gone_session() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_sessions(vec![
            session(10, "discord.exe", Flow::Render, "fone"),
            session(11, "discord.exe", Flow::Render, "fone"),
        ]);
        // A primeira sessão já sumiu do Windows.
        world.ctl.gone_session(10);

        let result = engine.act_app_mute("discord.exe", true);

        // Uma sessão mudou, a outra falhou: o pedido não foi cumprido inteiro.
        assert!(!result.ok, "a ação escondeu a falha da sessão que sumiu");
        assert!(result.changed, "a outra sessão foi muda de fato");
        // A sessão que sumiu não interrompe a outra.
        assert_eq!(
            world.ctl.calls(),
            vec![
                CtlCall::SetSessionMuted(10, Flow::Render, true),
                CtlCall::SetSessionMuted(11, Flow::Render, true),
            ]
        );
    }

    #[test]
    fn app_mute_reports_partial_failure_when_one_session_fails() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_sessions(vec![
            session(10, "discord.exe", Flow::Render, "fone"),
            session(11, "discord.exe", Flow::Render, "fone"),
        ]);
        // A primeira sessão já sumiu do Windows: a escrita nela falha.
        world.ctl.gone_session(10);
        let before = world.notes();

        let result = engine.act_app_mute("discord.exe", true);

        // Metade do pedido cumprida: o estado mudou, mas o pedido não foi cumprido
        // inteiro. `ok` falso e `changed` verdadeiro ao mesmo tempo.
        assert!(!result.ok, "a ação disse que deu quando uma sessão falhou");
        assert!(result.changed, "a ação disse que nada mudou quando uma sessão mudou");
        assert_eq!(result.error_code, Some(ErrorCode::DeviceGone));
        assert_eq!(result.resolved.as_deref(), Some("discord.exe"));
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // A frase diz as duas coisas: o que mudou e o que falhou.
        assert!(
            message.contains("em parte"),
            "a mensagem não diz que o mudo foi parcial: {message}"
        );
        assert!(
            message.contains("dispositivo"),
            "a mensagem não diz o que falhou: {message}"
        );
        // O estado mudou: o `refresh()` da ação aconteceu e a interface foi avisada.
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn app_mute_fails_when_every_session_fails() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_sessions(vec![session(10, "discord.exe", Flow::Render, "fone")]);
        world.ctl.gone_session(10);
        let before = world.notes();

        let result = engine.act_app_mute("discord.exe", true);

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::DeviceGone));
        assert!(!result.changed);
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn app_mute_reports_unchanged_when_already_muted() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let mut ja_mudo = session(10, "discord.exe", Flow::Render, "fone");
        ja_mudo.muted = true;
        world.source.set_sessions(vec![ja_mudo]);
        let before = world.notes();

        let result = engine.act_app_mute("discord.exe", true);

        assert!(result.ok);
        assert!(!result.changed);
        assert_eq!(result.resolved.as_deref(), Some("discord.exe"));
        // Já estava no estado pedido: nenhuma escrita.
        assert!(world.ctl.calls().is_empty());
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn app_mute_uses_fresh_sessions_not_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_sessions(vec![session(10, "discord.exe", Flow::Render, "fone")]);
        engine.refresh();
        // App aberto depois do refresh: a sessão nova não está no snapshot guardado.
        world.source.set_sessions(vec![
            session(10, "discord.exe", Flow::Render, "fone"),
            session(20, "discord.exe", Flow::Render, "fone"),
        ]);

        let result = engine.act_app_mute("discord.exe", true);

        assert!(result.ok);
        assert!(result.changed);
        // A sessão que acabou de aparecer também é mudada.
        assert_eq!(
            world.ctl.calls(),
            vec![
                CtlCall::SetSessionMuted(10, Flow::Render, true),
                CtlCall::SetSessionMuted(20, Flow::Render, true),
            ]
        );
    }

    #[test]
    fn app_mute_accepts_an_exe_path() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world.source.set_sessions(vec![session(10, "discord.exe", Flow::Render, "fone")]);

        let result = engine.act_app_mute("C:\\jogos\\discord.exe", true);

        assert!(result.ok);
        assert!(result.changed);
        // A chave comparada é o nome do arquivo, não o caminho.
        assert_eq!(result.resolved.as_deref(), Some("discord.exe"));
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetSessionMuted(10, Flow::Render, true)]
        );
    }

    // ---------- mudo de app: `changed` honesto ----------

    #[test]
    fn app_mute_reports_no_change_when_the_only_session_to_change_is_gone() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let mut ja_mudo = session(11, "discord.exe", Flow::Render, "fone");
        ja_mudo.muted = true;
        world.source.set_sessions(vec![
            session(10, "discord.exe", Flow::Render, "fone"),
            ja_mudo,
        ]);
        // A sessão que precisava mudar já sumiu do Windows.
        world.ctl.gone_session(10);
        let before = world.notes();

        let result = engine.act_app_mute("discord.exe", true);

        // Nada mudou de fato: a escrita falhou e a outra sessão já estava no estado pedido.
        assert!(!result.changed, "a ação mudou o estado sem mudar nada");
        // A única sessão que precisava mudar estava morta: isso é erro, não no-op.
        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::DeviceGone));
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // A sessão 11 já estava no estado pedido: ela nem entra no alvo, nem é escrita.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetSessionMuted(10, Flow::Render, true)]
        );
        // Nada mudou: ninguém é avisado.
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn app_mute_skips_sessions_already_in_the_requested_state() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let mut ja_mudo = session(10, "discord.exe", Flow::Render, "fone");
        ja_mudo.muted = true;
        world
            .source
            .set_sessions(vec![ja_mudo, session(11, "discord.exe", Flow::Render, "fone")]);

        let result = engine.act_app_mute("discord.exe", true);

        assert!(result.ok);
        assert!(result.changed);
        // A sessão 10 já estava muda: reescrevê-la seria desperdício.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetSessionMuted(11, Flow::Render, true)]
        );
    }

    #[test]
    fn app_mute_without_change_does_not_refresh_or_notify() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        let mut ja_mudo = session(10, "discord.exe", Flow::Render, "fone");
        ja_mudo.muted = true;
        world.source.set_sessions(vec![ja_mudo]);
        let before = world.notes();
        let device_reads_before = world.source.device_reads();
        let session_reads_before = world.source.session_reads();

        let result = engine.act_app_mute("discord.exe", true);

        assert!(result.ok);
        assert!(!result.changed);
        assert_eq!(result.resolved.as_deref(), Some("discord.exe"));
        // Já estava do jeito pedido: nenhuma escrita.
        assert!(world.ctl.calls().is_empty());
        // Nenhuma releitura completa do sistema: só a leitura de sessões da própria ação.
        assert_eq!(world.source.device_reads(), device_reads_before);
        assert_eq!(world.source.session_reads(), session_reads_before + 1);
        // Nenhum notificador: nada mudou, ninguém precisa saber.
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn app_mute_write_failure_other_than_gone_returns_os_error() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "discord.exe", Flow::Render, "fone")]);
        engine.refresh();
        let state_before = engine.state();
        let gen_before = engine.lock().generation;
        let before = world.notes();
        // A leitura das sessões funciona; a escrita é que o Windows recusa.
        // Ligado depois do refresh, para o refresh não ser o que falha.
        world.ctl.fail_writes.store(true, Ordering::SeqCst);

        let result = engine.act_app_mute("discord.exe", true);

        assert!(!result.ok);
        assert_eq!(result.error_code, Some(ErrorCode::OsError));
        assert!(!result.changed);
        let message = result.message.expect("falha precisa de mensagem");
        assert!(!message.contains("0x"), "mensagem com hexadecimal: {message}");
        // A escrita foi tentada e falhou: o estado da sessão fica como estava.
        assert_eq!(
            world.ctl.calls(),
            vec![CtlCall::SetSessionMuted(10, Flow::Render, true)]
        );
        assert_eq!(
            engine.state().sessions[0].muted,
            state_before.sessions[0].muted
        );
        // A geração não sobe: nada mudou, então nenhum cache de leitura é invalidado.
        assert_eq!(engine.lock().generation, gen_before);
        assert_eq!(world.notes(), before);
    }

    #[test]
    fn app_mute_emits_a_single_state_change() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = world.engine_at(dir.path());
        world
            .source
            .set_devices(vec![device("fone", Flow::Render, true, "Fone")]);
        world
            .source
            .set_sessions(vec![session(10, "discord.exe", Flow::Render, "fone")]);
        engine.refresh();
        let before = world.notes();

        let result = engine.act_app_mute("discord.exe", true);

        assert!(result.ok);
        assert!(result.changed);
        // Exatamente um aviso: o do `refresh()` da ação. Nem um `emit()` a mais, nem dois
        // refreshes.
        assert_eq!(world.notes(), before + 1);
    }

    #[test]
    fn app_mute_holds_apply_while_writing_the_session() {
        let dir = tempfile::tempdir().unwrap();
        let world = World::new();
        let engine = Arc::new(world.engine_at(dir.path()));
        world
            .source
            .set_sessions(vec![session(10, "discord.exe", Flow::Render, "fone")]);

        // Porta do backend: a ação estaciona dentro de `set_session_muted()`, ou seja, parada
        // com a trava `apply` na mão. Nada de janela de tempo: a prova é a posse da trava.
        let (chegou_tx, chegou_rx) = mpsc::channel();
        let (liberar_tx, liberar_rx) = mpsc::channel();
        world.ctl.set_gate_sessao(chegou_tx, liberar_rx);

        let (tx, rx) = mpsc::channel();
        let worker = Arc::clone(&engine);
        let handle = std::thread::spawn(move || {
            let _ = tx.send(worker.act_app_mute("discord.exe", true));
        });

        // Espera positiva: se a ação não chegar no backend, o teste falha alto em vez de
        // passar calado por falta de prova.
        chegou_rx
            .recv_timeout(Duration::from_secs(15))
            .expect("a ação não chegou no backend");

        // A escrita está em curso: a ação tem que estar segurando `apply`.
        assert!(
            engine.apply.try_lock().is_err(),
            "a ação não segurou `apply` enquanto escrevia a sessão"
        );

        // Um `refresh()` concorrente não passa enquanto a escrita está em curso.
        let releitura_pronta = Arc::new(AtomicBool::new(false));
        let flag = releitura_pronta.clone();
        let outro = Arc::clone(&engine);
        let devices_before = world.source.device_reads();
        let releitura = std::thread::spawn(move || {
            outro.refresh();
            flag.store(true, Ordering::SeqCst);
        });
        // A releitura lê o sistema antes de pedir `apply`: quando a contagem de leitura de
        // dispositivos subir, ela já está em curso e esperando a trava.
        let prazo = std::time::Instant::now() + Duration::from_secs(15);
        while world.source.device_reads() == devices_before {
            assert!(
                std::time::Instant::now() < prazo,
                "a releitura concorrente não começou"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !releitura_pronta.load(Ordering::SeqCst),
            "o refresh passou enquanto a escrita ainda estava em curso"
        );

        // Libera a escrita: a ação termina. O `refresh()` dela fica fora do escopo da trava
        // — se ficasse dentro, esta espera estouraria.
        liberar_tx
            .send(())
            .expect("a porta de teste já estava fechada");
        let result = rx
            .recv_timeout(Duration::from_secs(15))
            .expect("a ação não terminou depois de a porta ser liberada");
        handle.join().unwrap();
        releitura.join().unwrap();

        assert!(result.ok);
        assert!(result.changed);
        assert!(releitura_pronta.load(Ordering::SeqCst));
    }

    // ---------- migração das configurações ----------

    /// Grava um `settings.json` de verdade (o formato da v1) na pasta do teste.
    fn escrever_settings_v1(dir: &Path, shortcut: &str, autostart: bool) -> String {
        let conteudo = format!(
            r#"{{"version":1,"data":{{"shortcut":"{shortcut}","autostart":{autostart}}}}}"#
        );
        std::fs::write(dir.join("settings.json"), &conteudo).unwrap();
        conteudo
    }

    #[test]
    fn v1_settings_file_migrates_and_keeps_bak() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = escrever_settings_v1(dir.path(), "Super+Alt+P", true);
        let world = World::new();

        let engine = world.engine_at(dir.path());

        let settings = engine.settings();
        // O atalho legado virou um binding de alternar o painel.
        assert!(
            settings.bindings.contains(&Binding {
                accelerator: "Super+Alt+P".to_string(),
                action: Action::TogglePanel,
            }),
            "o atalho legado não virou binding: {:?}",
            settings.bindings
        );
        // E os padrões foram semeados, para o autor não precisar recriá-los.
        for padrao in crate::bindings::default_bindings() {
            assert!(
                settings.bindings.contains(&padrao),
                "padrão ausente: {padrao:?}"
            );
        }
        // O resto das configurações foi preservado.
        assert!(settings.autostart);
        assert_eq!(settings.shortcut, "Super+Alt+P");
        // O arquivo da v1 ficou no `.bak`.
        let bak = std::fs::read_to_string(dir.path().join("settings.json.bak")).unwrap();
        assert_eq!(bak, v1);
    }

    #[test]
    fn garbage_legacy_shortcut_creates_no_binding() {
        for atalho in ["", "???"] {
            let dir = tempfile::tempdir().unwrap();
            escrever_settings_v1(dir.path(), atalho, false);
            let world = World::new();

            let engine = world.engine_at(dir.path());

            assert_eq!(
                engine.settings().bindings,
                crate::bindings::default_bindings(),
                "atalho: {atalho:?}"
            );
        }
    }

    #[test]
    fn current_version_file_is_not_migrated_again() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = escrever_settings_v1(dir.path(), "Super+Alt+P", true);
        let world = World::new();

        let primeiro = world.engine_at(dir.path());
        let bindings = primeiro.settings().bindings;
        let main_depois = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let bak_depois = std::fs::read_to_string(dir.path().join("settings.json.bak")).unwrap();

        // Abrir de novo não pode duplicar nem regravar o arquivo.
        let segundo = world.engine_at(dir.path());

        assert_eq!(segundo.settings().bindings, bindings);
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json")).unwrap(),
            main_depois
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json.bak")).unwrap(),
            bak_depois
        );
        assert_eq!(bak_depois, v1);
    }
}
