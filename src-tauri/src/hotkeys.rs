//! Atalhos globais: o registro por binding e o estado de cada atalho.
//!
//! O app guarda uma lista de bindings (combinação → ação) e registra cada um deles no
//! Windows. O que o Windows recusa fica **indisponível**: um atalho que falha não impede
//! os outros de valarem (RF-17).
//!
//! A forma do JSON é a fixa da Ruling 41 — uma entrada por binding, na ordem da lista:
//!
//! ```json
//! [{"accelerator":"Ctrl+Alt+M","status":{"type":"active"}}]
//! ```

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::Shortcut;

use crate::actions::Action;
use crate::bindings::Binding;
use crate::shell::{self, ShortcutRegistrar};
use crate::state::Settings;

/// Evento com o estado de registro de cada binding.
pub const BINDINGS_STATUS: &str = "bindings-status";

/// O estado de um binding: registrado ou indisponível.
///
/// A forma do JSON é a da Ruling 41: `{"type":"active"}` e
/// `{"type":"unavailable","message":"<motivo>"}`.
///
/// A variante `Unavailable` leva o motivo como **campo** (`{ message }`) e não como
/// valor solto: o serde não serializa uma variante *newtype* com tag interna e valor
/// simples — ele recusaria com "cannot serialize tagged newtype variant". O JSON é o
/// mesmo, que é o que a Task 16 espelha.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BindingStatus {
    /// Registrado: o atalho está valendo.
    Active,
    /// O Windows recusou o registro; a frase diz o motivo.
    Unavailable { message: String },
}

/// Uma entrada de estado por binding: o acelerador e o estado dele.
///
/// O acelerador viaja **com** o estado para o painel casar status × binding sem depender
/// da ordem (a ordem muda quando um binding sai do meio da lista).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BindingStatusEntry {
    /// A combinação de teclas do binding.
    pub accelerator: String,
    /// O estado do registro dela.
    pub status: BindingStatus,
}

/// O estado global dos atalhos: o que está registrado e o estado de cada binding.
#[derive(Default)]
struct Estado {
    /// Os bindings **efetivamente** registrados: é o que o handler do atalho consulta.
    registrados: Vec<Binding>,
    /// O estado de cada binding da lista salva, na ordem dela.
    status: Vec<BindingStatusEntry>,
}

static ESTADO: Mutex<Estado> = Mutex::new(Estado {
    registrados: Vec::new(),
    status: Vec::new(),
});

fn estado() -> std::sync::MutexGuard<'static, Estado> {
    ESTADO.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Os bindings efetivamente registrados agora.
pub fn registrados() -> Vec<Binding> {
    estado().registrados.clone()
}

/// O estado de cada binding da lista salva.
pub fn status() -> Vec<BindingStatusEntry> {
    estado().status.clone()
}

/// Guarda o resultado do último registro.
pub fn definir(registrados: Vec<Binding>, status: Vec<BindingStatusEntry>) {
    let mut estado = estado();
    estado.registrados = registrados;
    estado.status = status;
}

/// Uma entrada de estado por binding, na ordem da lista.
///
/// A lista de status tem sempre **uma entrada por binding** e na mesma ordem: é o que o
/// painel espera para casar status × binding (Ruling 41).
pub fn entradas(lista: &[Binding], status: &[BindingStatus]) -> Vec<BindingStatusEntry> {
    lista
        .iter()
        .enumerate()
        .map(|(indice, binding)| BindingStatusEntry {
            accelerator: binding.accelerator.clone(),
            status: status
                .get(indice)
                .cloned()
                .unwrap_or(BindingStatus::Active),
        })
        .collect()
}

/// Registra a lista nova de bindings e solta o que saiu dela.
///
/// Devolve o estado de cada binding da lista nova (na mesma ordem) e a lista que ficou
/// **efetivamente registrada** — a que o handler do atalho consulta.
///
/// As regras:
/// - o atalho que já está registrado **não** é tocado (a mesma tecla escrita de outro
///   jeito é a mesma tecla);
/// - a troca registra o novo **antes** de soltar o antigo (Ruling 18): se o novo for
///   recusado, o antigo continua valendo;
/// - a falha de um binding não impede os outros de serem registrados (RF-17).
pub fn apply_bindings(
    registrar: &dyn ShortcutRegistrar,
    atual: &[Binding],
    proxima: &[Binding],
) -> (Vec<BindingStatus>, Vec<Binding>) {
    let mut status: Vec<BindingStatus> = Vec::with_capacity(proxima.len());
    let mut registrados: Vec<Binding> = Vec::with_capacity(proxima.len());
    // Os aceleradores que a troca já soltou (o antigo sai depois do novo entrar).
    let mut soltos: Vec<String> = Vec::new();

    for binding in proxima {
        // A mesma tecla, escrita de outro jeito: já está registrada, não se mexe.
        if atual
            .iter()
            .any(|antigo| mesma_tecla(&antigo.accelerator, &binding.accelerator))
        {
            status.push(BindingStatus::Active);
            registrados.push(binding.clone());
            continue;
        }

        // Troca: o atalho antigo da mesma ação é solto depois do novo entrar.
        let troca = atual.iter().find(|antigo| {
            antigo.action == binding.action
                && !proxima
                    .iter()
                    .any(|outro| mesma_tecla(&outro.accelerator, &antigo.accelerator))
        });

        match troca {
            Some(antigo) => match shell::switch_shortcut(
                registrar,
                Some(&antigo.accelerator),
                &binding.accelerator,
            ) {
                Ok(()) => {
                    soltos.push(antigo.accelerator.clone());
                    status.push(BindingStatus::Active);
                    registrados.push(binding.clone());
                }
                Err(frase) => {
                    // O novo não entrou: o antigo continua valendo (e não é solto).
                    status.push(BindingStatus::Unavailable { message: frase });
                    registrados.push(antigo.clone());
                }
            },
            None => match registrar.register(&binding.accelerator) {
                Ok(()) => {
                    status.push(BindingStatus::Active);
                    registrados.push(binding.clone());
                }
                Err(frase) => {
                    status.push(BindingStatus::Unavailable { message: frase });
                }
            },
        }
    }

    // O que saiu da lista é solto — depois de registrar tudo (Ruling 18).
    for antigo in atual {
        if soltos
            .iter()
            .any(|solto| mesma_tecla(solto, &antigo.accelerator))
        {
            continue;
        }
        if proxima
            .iter()
            .any(|outro| mesma_tecla(&outro.accelerator, &antigo.accelerator))
        {
            continue;
        }
        if registrados
            .iter()
            .any(|r| mesma_tecla(&r.accelerator, &antigo.accelerator))
        {
            continue;
        }
        registrar.unregister(&antigo.accelerator);
    }

    (status, registrados)
}

/// Ajusta o atalho legado no binding `toggle_panel`.
///
/// O primeiro binding `toggle_panel` da lista recebe o atalho pedido; se a lista não tem
/// nenhum, o atalho entra como binding novo, no começo da lista.
pub fn unify_shortcut(lista: Vec<Binding>, accelerator: &str) -> Vec<Binding> {
    let mut lista = lista;
    match lista
        .iter_mut()
        .find(|binding| binding.action == Action::TogglePanel)
    {
        Some(binding) => binding.accelerator = accelerator.to_string(),
        None => lista.insert(
            0,
            Binding {
                accelerator: accelerator.to_string(),
                action: Action::TogglePanel,
            },
        ),
    }
    lista
}

/// O atalho legado é o espelho do acelerador do primeiro binding `toggle_panel`.
pub fn espelho_do_painel(lista: &[Binding]) -> String {
    lista
        .iter()
        .find(|binding| binding.action == Action::TogglePanel)
        .map(|binding| binding.accelerator.clone())
        .unwrap_or_default()
}

/// Registra os atalhos salvos na abertura do app e publica o estado de cada um.
pub fn apply_at_startup(app: &AppHandle, settings: &Settings) {
    let (status, registrados) = apply_bindings(&shell::registrador(app), &[], &settings.bindings);
    for (binding, estado) in settings.bindings.iter().zip(status.iter()) {
        if let BindingStatus::Unavailable { message: frase } = estado {
            eprintln!(
                "sonar: o atalho {} ficou indisponível: {frase}",
                binding.accelerator
            );
        }
    }
    let entradas = entradas(&settings.bindings, &status);
    definir(registrados, entradas.clone());
    let _ = app.emit(BINDINGS_STATUS, &entradas);
}

/// Duas escritas da mesma tecla?
///
/// A comparação é pela combinação **parseada**: o Windows devolve os modificadores em
/// outra ordem (`shift+control+alt+super+V`), e `ctrl+alt+m` é o mesmo que `Ctrl+Alt+M`.
fn mesma_tecla(a: &str, b: &str) -> bool {
    match (a.trim().parse::<Shortcut>(), b.trim().parse::<Shortcut>()) {
        (Ok(a), Ok(b)) => a == b,
        // Acelerador inválido: a comparação é só pelo texto, sem caixa.
        _ => a.trim().eq_ignore_ascii_case(b.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// O atalho que o Windows recusaria: a combinação está ocupada por outro programa.
    ///
    /// É uma combinação **válida** de propósito: um acelerador inválido seria recusado
    /// pela validação, antes do registro — e o teste é sobre o registro.
    const OCUPADO: &str = "Ctrl+Alt+O";

    /// A frase que o Windows devolve para o atalho ocupado.
    fn recusa(accelerator: &str) -> String {
        format!(
            "Não foi possível registrar o atalho {accelerator}. \
             Outro programa pode estar usando essa combinação."
        )
    }

    /// Registro falso: o atalho ocupado é recusado; o resto é aceito.
    #[derive(Default)]
    struct FakeRegistrar {
        calls: RefCell<Vec<String>>,
    }

    impl ShortcutRegistrar for FakeRegistrar {
        fn register(&self, accelerator: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("register {accelerator}"));
            if accelerator == OCUPADO {
                return Err(recusa(accelerator));
            }
            Ok(())
        }

        fn unregister(&self, accelerator: &str) {
            self.calls.borrow_mut().push(format!("unregister {accelerator}"));
        }
    }

    fn binding(accelerator: &str, action: Action) -> Binding {
        Binding {
            accelerator: accelerator.to_string(),
            action,
        }
    }

    // ---------- registro ----------

    #[test]
    fn unchanged_accelerators_are_not_touched() {
        // A lista é a mesma: nada é registrado nem solto.
        let registrar = FakeRegistrar::default();
        let lista = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding("Ctrl+Alt+Up", Action::StepOutputVolume { delta: 0.05 }),
        ];

        let (status, registrados) = apply_bindings(&registrar, &lista, &lista);

        assert!(
            registrar.calls.borrow().is_empty(),
            "atalho que não mudou não pode ser tocado: {:?}",
            registrar.calls.borrow()
        );
        assert_eq!(
            status,
            vec![BindingStatus::Active, BindingStatus::Active],
            "o atalho que já estava registrado está ativo"
        );
        assert_eq!(registrados, lista, "a lista registrada é a mesma");
    }

    #[test]
    fn failing_binding_is_unavailable_and_others_still_register() {
        // RF-17: a falha de um atalho não impede os outros de ficarem registrados.
        let registrar = FakeRegistrar::default();
        let lista = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding(OCUPADO, Action::TogglePanel),
            binding("Ctrl+Alt+Up", Action::StepOutputVolume { delta: 0.05 }),
        ];

        let (status, registrados) = apply_bindings(&registrar, &[], &lista);

        assert_eq!(
            status,
            vec![
                BindingStatus::Active,
                BindingStatus::Unavailable {
                    message: recusa(OCUPADO),
                },
                BindingStatus::Active,
            ],
            "o do meio falhou; os outros dois estão ativos"
        );
        assert_eq!(
            registrados.len(),
            2,
            "só os que entraram ficaram registrados: {registrados:?}"
        );
        let calls = registrar.calls.borrow().clone();
        assert!(
            calls.contains(&"register Ctrl+Alt+M".to_string()),
            "o primeiro atalho tem de ser registrado: {calls:?}"
        );
        assert!(
            calls.contains(&"register Ctrl+Alt+Up".to_string()),
            "o terceiro atalho tem de ser registrado: {calls:?}"
        );
    }

    #[test]
    fn failed_swap_keeps_previous_registered() {
        // Ruling 18: o novo entra antes do antigo sair. Se o novo for recusado, o antigo
        // continua valendo — e não é solto.
        let registrar = FakeRegistrar::default();
        let antes = vec![binding("Ctrl+Alt+K", Action::TogglePanel)];

        let (status, registrados) = apply_bindings(
            &registrar,
            &antes,
            &[binding(OCUPADO, Action::TogglePanel)],
        );

        assert_eq!(
            status,
            vec![BindingStatus::Unavailable {
                message: recusa(OCUPADO),
            }],
            "o atalho novo foi recusado"
        );
        assert_eq!(
            registrados, antes,
            "o atalho anterior continua valendo: {registrados:?}"
        );
        let calls = registrar.calls.borrow().clone();
        assert!(
            !calls.iter().any(|c| c.contains("unregister")),
            "o atalho anterior não pode ser solto: {calls:?}"
        );
    }

    #[test]
    fn moved_action_reuses_accelerator_without_double_register() {
        // Dois bindings trocam de ação: as teclas continuam as mesmas, então nenhum
        // registro é refeito (e nenhum atalho fica registrado duas vezes).
        let registrar = FakeRegistrar::default();
        let antes = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding("Ctrl+Alt+Up", Action::StepOutputVolume { delta: 0.05 }),
        ];
        let depois = vec![
            binding("Ctrl+Alt+M", Action::StepOutputVolume { delta: 0.05 }),
            binding("Ctrl+Alt+Up", Action::ToggleOutputMute),
        ];

        let (status, registrados) = apply_bindings(&registrar, &antes, &depois);

        assert!(
            registrar.calls.borrow().is_empty(),
            "as teclas não mudaram: {:?}",
            registrar.calls.borrow()
        );
        assert_eq!(
            status,
            vec![BindingStatus::Active, BindingStatus::Active],
            "as duas teclas continuam ativas"
        );
        assert_eq!(registrados, depois, "a ação de cada tecla é a nova");
    }

    #[test]
    fn swap_registers_the_new_before_releasing_the_old() {
        // A ordem é a da Ruling 18: registrar o novo, soltar o antigo.
        let registrar = FakeRegistrar::default();
        let antes = vec![binding("Ctrl+Alt+K", Action::TogglePanel)];

        apply_bindings(
            &registrar,
            &antes,
            &[binding("Ctrl+Alt+L", Action::TogglePanel)],
        );

        assert_eq!(
            *registrar.calls.borrow(),
            vec![
                "register Ctrl+Alt+L".to_string(),
                "unregister Ctrl+Alt+K".to_string(),
            ]
        );
    }

    #[test]
    fn removed_binding_is_released() {
        // O que sai da lista é solto do Windows.
        let registrar = FakeRegistrar::default();
        let antes = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding("Ctrl+Alt+K", Action::TogglePanel),
        ];

        let (_, registrados) = apply_bindings(
            &registrar,
            &antes,
            &[binding("Ctrl+Alt+M", Action::ToggleOutputMute)],
        );

        assert_eq!(registrados.len(), 1, "só o que ficou na lista segue registrado");
        let calls = registrar.calls.borrow().clone();
        assert!(
            calls.contains(&"unregister Ctrl+Alt+K".to_string()),
            "o atalho que saiu da lista tem de ser solto: {calls:?}"
        );
    }

    #[test]
    fn same_shortcut_written_differently_is_not_touched() {
        // `ctrl+alt+m` é a mesma tecla que `Ctrl+Alt+M`: registrar de novo seria recusado
        // pelo Windows.
        let registrar = FakeRegistrar::default();
        let antes = vec![binding("Ctrl+Alt+M", Action::ToggleOutputMute)];

        let (status, _) = apply_bindings(
            &registrar,
            &antes,
            &[binding("ctrl+alt+m", Action::ToggleOutputMute)],
        );

        assert!(
            registrar.calls.borrow().is_empty(),
            "a mesma tecla não pode ser registrada de novo: {:?}",
            registrar.calls.borrow()
        );
        assert_eq!(status, vec![BindingStatus::Active]);
    }

    // ---------- a forma do JSON (Ruling 41) ----------

    #[test]
    fn binding_status_json_shape_is_pinned() {
        // A forma é a que a Task 16 espelha em `types.ts`: duas chaves, e o status com a
        // tag `type` em snake_case.
        let ativo = serde_json::to_string(&BindingStatusEntry {
            accelerator: "Ctrl+Alt+M".to_string(),
            status: BindingStatus::Active,
        })
        .unwrap();
        assert_eq!(ativo, r#"{"accelerator":"Ctrl+Alt+M","status":{"type":"active"}}"#);

        let indisponivel = serde_json::to_string(&BindingStatusEntry {
            accelerator: "Ctrl+Alt+X".to_string(),
            status: BindingStatus::Unavailable {
                message: "Tecla já em uso por outro aplicativo.".to_string(),
            },
        })
        .unwrap();
        assert_eq!(
            indisponivel,
            r#"{"accelerator":"Ctrl+Alt+X","status":{"type":"unavailable","message":"Tecla já em uso por outro aplicativo."}}"#
        );
    }

    #[test]
    fn status_list_has_one_entry_per_binding_in_order() {
        let lista = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding("bad+Key", Action::TogglePanel),
            binding("Ctrl+Alt+Up", Action::StepOutputVolume { delta: 0.05 }),
        ];
        let status = vec![
            BindingStatus::Active,
            BindingStatus::Unavailable {
                message: "motivo".to_string(),
            },
            BindingStatus::Active,
        ];

        let entradas = entradas(&lista, &status);

        assert_eq!(entradas.len(), lista.len(), "uma entrada por binding");
        assert_eq!(entradas[0].accelerator, "Ctrl+Alt+M");
        assert_eq!(entradas[1].accelerator, "bad+Key");
        assert_eq!(entradas[2].accelerator, "Ctrl+Alt+Up");
        assert_eq!(
            entradas[1].status,
            BindingStatus::Unavailable {
                message: "motivo".to_string(),
            }
        );
    }

    // ---------- o atalho legado ----------

    #[test]
    fn unify_shortcut_changes_the_toggle_panel_binding() {
        let lista = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding("Super+Alt+V", Action::TogglePanel),
        ];

        let unificada = unify_shortcut(lista, "Ctrl+Alt+K");

        assert_eq!(unificada[0].accelerator, "Ctrl+Alt+M");
        assert_eq!(unificada[1].accelerator, "Ctrl+Alt+K");
        assert_eq!(unificada[1].action, Action::TogglePanel);
    }

    #[test]
    fn unify_shortcut_adds_the_binding_when_there_is_none() {
        let lista = vec![binding("Ctrl+Alt+M", Action::ToggleOutputMute)];

        let unificada = unify_shortcut(lista, "Ctrl+Alt+K");

        assert_eq!(unificada.len(), 2);
        assert_eq!(unificada[0].accelerator, "Ctrl+Alt+K");
        assert_eq!(unificada[0].action, Action::TogglePanel);
    }

    #[test]
    fn legacy_shortcut_mirrors_the_first_toggle_panel_binding() {
        let lista = vec![
            binding("Ctrl+Alt+M", Action::ToggleOutputMute),
            binding("Ctrl+Alt+K", Action::TogglePanel),
            binding("Super+Alt+V", Action::TogglePanel),
        ];

        assert_eq!(espelho_do_painel(&lista), "Ctrl+Alt+K");
    }

    #[test]
    fn legacy_shortcut_is_empty_without_a_toggle_panel_binding() {
        let lista = vec![binding("Ctrl+Alt+M", Action::ToggleOutputMute)];

        assert_eq!(espelho_do_painel(&lista), "");
    }
}
