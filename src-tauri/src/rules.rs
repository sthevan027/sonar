//! Regras de rota por executável e reconciliação com o sistema.
//!
//! Lógica pura: toda leitura e gravação de rota passa pelo `RouteBackend`.

use crate::audio::types::{AudioError, AudioSession, Device, DeviceId, Flow, RouteBackend};
use serde::{Deserialize, Serialize};

/// Regra "este executável sempre sai por este dispositivo".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub exe: String,
    pub flow: Flow,
    pub device: DeviceId,
}

/// Conjunto de regras. O `version` do arquivo em disco é responsabilidade do `Store`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleSet {
    pub rules: Vec<Rule>,
}

/// Resultado de uma tentativa de aplicar uma regra a um processo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Applied { pid: u32 },
    AlreadyRouted { pid: u32 },
    /// Dispositivo da regra ausente ou desconectado. Reportado uma vez por regra.
    DeviceAbsent { exe: String },
    Skipped { pid: u32, reason: AudioError },
}

impl RuleSet {
    /// Insere ou substitui a regra de mesmo executável e sentido. O `exe` é normalizado para minúsculo.
    pub fn upsert(&mut self, mut rule: Rule) {
        rule.exe = rule.exe.to_lowercase();
        match self
            .rules
            .iter_mut()
            .find(|r| r.exe == rule.exe && r.flow == rule.flow)
        {
            Some(existing) => *existing = rule,
            None => self.rules.push(rule),
        }
    }

    pub fn remove(&mut self, exe: &str, flow: Flow) {
        let exe = exe.to_lowercase();
        self.rules.retain(|r| !(r.exe == exe && r.flow == flow));
    }

    pub fn get(&self, exe: &str, flow: Flow) -> Option<&Rule> {
        let exe = exe.to_lowercase();
        self.rules.iter().find(|r| r.exe == exe && r.flow == flow)
    }
}

/// Aplica cada regra a todas as sessões do executável no mesmo sentido.
///
/// Só o papel multimídia é lido por `get_route`; uma troca externa feita apenas no
/// papel de console não é detectada e não gera nova gravação.
pub fn reconcile(
    rules: &RuleSet,
    devices: &[Device],
    sessions: &[AudioSession],
    backend: &dyn RouteBackend,
) -> Vec<Outcome> {
    rules
        .rules
        .iter()
        .flat_map(|rule| reconcile_rule(rule, devices, sessions, backend))
        .collect()
}

/// Aplica uma regra às sessões do executável dela no sentido. Usada pela releitura completa
/// (`reconcile`) e pela escolha de um app, que não relê o sistema.
pub fn reconcile_rule(
    rule: &Rule,
    devices: &[Device],
    sessions: &[AudioSession],
    backend: &dyn RouteBackend,
) -> Vec<Outcome> {
    let matching: Vec<&AudioSession> = sessions
        .iter()
        .filter(|s| s.flow == rule.flow && s.exe.eq_ignore_ascii_case(&rule.exe))
        .collect();
    if matching.is_empty() {
        return Vec::new();
    }
    let device_ready = devices.iter().any(|d| d.id == rule.device && d.connected);
    if !device_ready {
        return vec![Outcome::DeviceAbsent { exe: rule.exe.clone() }];
    }
    matching
        .into_iter()
        .map(|session| apply_to_session(rule, session, backend))
        .collect()
}

fn apply_to_session(
    rule: &Rule,
    session: &AudioSession,
    backend: &dyn RouteBackend,
) -> Outcome {
    let pid = session.pid;
    // Erro de leitura não é fatal: o backend real falha em PIDs que não consegue ler.
    if let Ok(Some(current)) = backend.get_route(pid, rule.flow) {
        if current == rule.device {
            return Outcome::AlreadyRouted { pid };
        }
    }
    match backend.set_route(pid, rule.flow, &rule.device) {
        Ok(()) => Outcome::Applied { pid },
        Err(reason) => Outcome::Skipped { pid, reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    /// Backend de teste: guarda rotas, grava chamadas e pode falhar por PID.
    #[derive(Default)]
    struct FakeBackend {
        /// Rota atual de cada PID, devolvida por `get_route`.
        routes: Mutex<HashMap<u32, DeviceId>>,
        /// PIDs para os quais `get_route` devolve erro.
        get_errors: HashSet<u32>,
        /// Erro devolvido por `set_route` para cada PID.
        set_errors: HashMap<u32, AudioError>,
        /// Chamadas de `set_route`: (pid, flow, dispositivo).
        set_calls: Mutex<Vec<(u32, Flow, DeviceId)>>,
    }

    impl FakeBackend {
        fn with_route(self, pid: u32, device: &str) -> Self {
            self.routes.lock().unwrap().insert(pid, dev_id(device));
            self
        }

        fn set_calls(&self) -> Vec<(u32, Flow, DeviceId)> {
            self.set_calls.lock().unwrap().clone()
        }
    }

    impl RouteBackend for FakeBackend {
        fn set_route(&self, pid: u32, flow: Flow, device: &DeviceId) -> Result<(), AudioError> {
            self.set_calls.lock().unwrap().push((pid, flow, device.clone()));
            match self.set_errors.get(&pid) {
                Some(err) => Err(err.clone()),
                None => {
                    self.routes.lock().unwrap().insert(pid, device.clone());
                    Ok(())
                }
            }
        }

        fn clear_route(&self, pid: u32, _flow: Flow) -> Result<(), AudioError> {
            self.routes.lock().unwrap().remove(&pid);
            Ok(())
        }

        fn get_route(&self, pid: u32, _flow: Flow) -> Result<Option<DeviceId>, AudioError> {
            if self.get_errors.contains(&pid) {
                return Err(AudioError::Os(-2147467259));
            }
            Ok(self.routes.lock().unwrap().get(&pid).cloned())
        }
    }

    fn dev_id(id: &str) -> DeviceId {
        DeviceId(id.to_string())
    }

    fn device(id: &str, flow: Flow, connected: bool) -> Device {
        Device {
            id: dev_id(id),
            flow,
            name: format!("Dispositivo {id}"),
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
            volume: 1.0,
            muted: false,
        }
    }

    fn rule_set(exe: &str, flow: Flow, device: &str) -> RuleSet {
        let mut set = RuleSet::default();
        set.upsert(Rule {
            exe: exe.to_string(),
            flow,
            device: dev_id(device),
        });
        set
    }

    #[test]
    fn applies_rule_to_every_pid_of_the_exe() {
        let rules = rule_set("chrome.exe", Flow::Render, "fone");
        let devices = [device("fone", Flow::Render, true)];
        let sessions = [
            session(10, "chrome.exe", Flow::Render, "alto-falante"),
            session(11, "chrome.exe", Flow::Render, "alto-falante"),
        ];
        let backend = FakeBackend::default();

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert_eq!(
            outcomes,
            vec![Outcome::Applied { pid: 10 }, Outcome::Applied { pid: 11 }]
        );
        assert_eq!(
            backend.set_calls(),
            vec![
                (10, Flow::Render, dev_id("fone")),
                (11, Flow::Render, dev_id("fone")),
            ]
        );
    }

    #[test]
    fn does_not_rewrite_when_route_already_matches() {
        let rules = rule_set("chrome.exe", Flow::Render, "fone");
        let devices = [device("fone", Flow::Render, true)];
        let sessions = [session(10, "chrome.exe", Flow::Render, "fone")];
        let backend = FakeBackend::default().with_route(10, "fone");

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert_eq!(outcomes, vec![Outcome::AlreadyRouted { pid: 10 }]);
        assert!(backend.set_calls().is_empty());
    }

    #[test]
    fn exe_match_is_case_insensitive() {
        let mut rules = RuleSet::default();
        rules.upsert(Rule {
            exe: "Discord.EXE".to_string(),
            flow: Flow::Render,
            device: dev_id("fone"),
        });
        assert_eq!(
            rules.get("discord.exe", Flow::Render).map(|r| r.exe.as_str()),
            Some("discord.exe")
        );
        let devices = [device("fone", Flow::Render, true)];
        let sessions = [session(20, "discord.exe", Flow::Render, "alto-falante")];
        let backend = FakeBackend::default();

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert_eq!(outcomes, vec![Outcome::Applied { pid: 20 }]);
    }

    #[test]
    fn device_absent_keeps_rule_and_reports() {
        let rules = rule_set("spotify.exe", Flow::Render, "fone");
        let before = rules.clone();
        let devices = [device("fone", Flow::Render, false)];
        let sessions = [session(30, "spotify.exe", Flow::Render, "alto-falante")];
        let backend = FakeBackend::default();

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert_eq!(
            outcomes,
            vec![Outcome::DeviceAbsent {
                exe: "spotify.exe".to_string()
            }]
        );
        assert!(backend.set_calls().is_empty());
        assert_eq!(rules, before);
    }

    #[test]
    fn applies_again_when_device_returns() {
        let rules = rule_set("spotify.exe", Flow::Render, "fone");
        let sessions = [session(30, "spotify.exe", Flow::Render, "alto-falante")];
        let backend = FakeBackend::default();

        let absent = reconcile(
            &rules,
            &[device("fone", Flow::Render, false)],
            &sessions,
            &backend,
        );
        assert_eq!(
            absent,
            vec![Outcome::DeviceAbsent {
                exe: "spotify.exe".to_string()
            }]
        );

        let back = reconcile(
            &rules,
            &[device("fone", Flow::Render, true)],
            &sessions,
            &backend,
        );
        assert_eq!(back, vec![Outcome::Applied { pid: 30 }]);
        assert_eq!(backend.set_calls().len(), 1);
    }

    #[test]
    fn pid_gone_is_skipped_and_others_continue() {
        let rules = rule_set("chrome.exe", Flow::Render, "fone");
        let devices = [device("fone", Flow::Render, true)];
        let sessions = [
            session(40, "chrome.exe", Flow::Render, "alto-falante"),
            session(41, "chrome.exe", Flow::Render, "alto-falante"),
            session(42, "chrome.exe", Flow::Render, "alto-falante"),
        ];
        let mut backend = FakeBackend::default();
        backend.set_errors.insert(41, AudioError::Gone);

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert_eq!(
            outcomes,
            vec![
                Outcome::Applied { pid: 40 },
                Outcome::Skipped {
                    pid: 41,
                    reason: AudioError::Gone
                },
                Outcome::Applied { pid: 42 },
            ]
        );
    }

    #[test]
    fn rule_for_other_flow_is_not_applied() {
        let rules = rule_set("discord.exe", Flow::Render, "fone");
        let devices = [device("fone", Flow::Render, true)];
        let sessions = [session(50, "discord.exe", Flow::Capture, "microfone")];
        let backend = FakeBackend::default();

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert!(outcomes.is_empty());
        assert!(backend.set_calls().is_empty());
    }

    /// Ocultar dispositivo é decisão de interface (fora de `types.rs`), então a regra
    /// não consulta nenhum atributo além de `connected`: um dispositivo conectado roteia.
    #[test]
    fn hidden_device_still_routes() {
        let rules = rule_set("chrome.exe", Flow::Render, "fone");
        let devices = [Device {
            is_default: false,
            ..device("fone", Flow::Render, true)
        }];
        let sessions = [session(60, "chrome.exe", Flow::Render, "alto-falante")];
        let backend = FakeBackend::default();

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert_eq!(outcomes, vec![Outcome::Applied { pid: 60 }]);
    }

    #[test]
    fn get_route_error_still_attempts_set_route() {
        let rules = rule_set("chrome.exe", Flow::Render, "fone");
        let devices = [device("fone", Flow::Render, true)];
        let sessions = [session(70, "chrome.exe", Flow::Render, "alto-falante")];
        let mut backend = FakeBackend::default();
        backend.get_errors.insert(70);

        let outcomes = reconcile(&rules, &devices, &sessions, &backend);

        assert_eq!(outcomes, vec![Outcome::Applied { pid: 70 }]);
        assert_eq!(backend.set_calls().len(), 1);
    }
}
