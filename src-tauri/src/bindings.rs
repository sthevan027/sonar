//! Atalhos globais: a tabela que liga uma combinação de teclas a uma ação.
//!
//! A tabela é a mesma que a interface já espera (o `Vec<Binding>` das configurações entra
//! na Task 14); aqui ela já existe como lista de partidas.

use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::Shortcut;

use crate::actions::{Action, Direction};
use crate::hotkeys::espelho_do_painel;
use crate::state::Settings;
use crate::store::CURRENT_VERSION;

/// Um atalho global e a ação que ele dispara.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Binding {
    /// A combinação de teclas, no formato do plugin de atalho global (ex.: `Ctrl+Alt+M`).
    pub accelerator: String,
    /// A ação disparada quando a combinação é pressionada.
    pub action: Action,
}

/// Os atalhos padrão do app.
///
/// **Provisório:** as combinações valem até o resultado da sonda S4, que só o autor
/// consegue rodar — no Windows real, com o plugin global de atalho. Se alguma combinação
/// estiver ocupada por outro programa, ela fica indisponível (a falha vai para o log) e
/// os demais atalhos continuam valendo: um atalho que falha não impede os outros.
pub fn default_bindings() -> Vec<Binding> {
    vec![
        Binding {
            accelerator: "Super+Alt+V".to_string(),
            action: Action::TogglePanel,
        },
        Binding {
            accelerator: "Ctrl+Alt+M".to_string(),
            action: Action::ToggleMicMute,
        },
        Binding {
            accelerator: "Ctrl+Alt+Up".to_string(),
            action: Action::StepOutputVolume { delta: 0.05 },
        },
        Binding {
            accelerator: "Ctrl+Alt+Down".to_string(),
            action: Action::StepOutputVolume { delta: -0.05 },
        },
        Binding {
            accelerator: "Ctrl+Alt+P".to_string(),
            action: Action::CycleProfile {
                direction: Direction::Next,
            },
        },
    ]
}

/// A ação disparada por um atalho pressionado.
///
/// A comparação é pela combinação **parseada**, não pelo texto: o plugin devolve o atalho
/// como o Windows montou (`shift+control+alt+super+V`), e o texto da tabela pode ter os
/// modificadores em outra ordem (`Super+Alt+V`). Comparando as combinações, as duas
/// formas batem.
pub fn action_for(bindings: &[Binding], pressed: &Shortcut) -> Option<Action> {
    bindings.iter().find_map(|binding| {
        let combination: Shortcut = binding.accelerator.parse().ok()?;
        (combination == *pressed).then(|| binding.action.clone())
    })
}

/// Tamanho máximo do texto do acelerador.
pub const MAX_ACCELERATOR_CHARS: usize = 64;

/// O motivo pelo qual uma lista de atalhos foi recusada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingError {
    /// O acelerador está vazio.
    Empty,
    /// O acelerador passou do limite de caracteres.
    TooLong,
    /// O acelerador não é uma combinação que o plugin entende.
    Syntax,
    /// O mesmo atalho já está na lista.
    Duplicate,
}

impl BindingError {
    /// A frase para o usuário, em português.
    pub fn message(&self) -> &'static str {
        match self {
            BindingError::Empty => "O atalho está vazio.",
            BindingError::TooLong => "O atalho é longo demais (máximo de 64 caracteres).",
            BindingError::Syntax => "O atalho não é uma combinação válida.",
            BindingError::Duplicate => "Esse atalho já está em uso.",
        }
    }
}

/// Confere uma lista de atalhos. Devolve o **índice** do primeiro item com problema e o
/// motivo; `Ok(())` quando a lista inteira presta.
///
/// A comparação de duplicatas é pela combinação **parseada**: `ctrl+alt+m` e
/// `Ctrl+Alt+M` são a mesma tecla, e `Ctrl+Alt+M` e `Alt+Ctrl+M` também.
pub fn validate_bindings(list: &[Binding]) -> Result<(), (usize, BindingError)> {
    let mut vistas: Vec<Shortcut> = Vec::new();
    for (indice, binding) in list.iter().enumerate() {
        let acelerador = binding.accelerator.trim();
        if acelerador.is_empty() {
            return Err((indice, BindingError::Empty));
        }
        if acelerador.chars().count() > MAX_ACCELERATOR_CHARS {
            return Err((indice, BindingError::TooLong));
        }
        let combinacao = acelerador
            .parse::<Shortcut>()
            .map_err(|_| (indice, BindingError::Syntax))?;
        if vistas.contains(&combinacao) {
            return Err((indice, BindingError::Duplicate));
        }
        vistas.push(combinacao);
    }
    Ok(())
}

/// Leva as configurações de um formato antigo para o atual.
///
/// O `bool` diz se houve migração (isto é, se o chamador precisa gravar).
///
/// O que a migração faz, para `version < 2`:
/// - o atalho legado (`settings.shortcut`) vira um binding que alterna o painel;
/// - os padrões do app são semeados junto, para o autor não precisar recriá-los;
/// - o atalho legado **vence** qualquer padrão que use a mesma tecla, e o `toggle_panel`
///   padrão não é semeado (o painel já tem o atalho do usuário);
/// - `settings.shortcut` passa a ser o espelho do painel da lista nova.
///
/// Duas garantias: uma lista de bindings já existente e não vazia **nunca** é
/// sobrescrita (Ruling 14), e um atalho legado vazio ou inválido não gera binding
/// nenhum — a migração nunca inventa um atalho que o Windows não entende.
///
/// A versão do formato é a do `Store` (`store::CURRENT_VERSION`): uma fonte só — um
/// número de versão em dois arquivos diverge em silêncio.
pub fn migrate_settings(version: u32, mut settings: Settings) -> (Settings, bool) {
    if version >= CURRENT_VERSION {
        return (settings, false);
    }
    // Ruling 14: lista já existente e não vazia fica como está.
    if !settings.bindings.is_empty() {
        return (settings, false);
    }

    // O atalho legado válido **vence**: ele é o painel do usuário. O padrão com a mesma
    // tecla (de qualquer ação) e o `toggle_panel` padrão saem da semeadura — senão a tecla
    // do usuário mudaria de dono em silêncio, ou o painel ganharia dois atalhos.
    let legado = settings.shortcut.trim().to_string();
    let combinacao = if legado.is_empty() {
        None
    } else {
        legado.parse::<Shortcut>().ok()
    };
    let mut bindings: Vec<Binding> = Vec::new();
    let mut padroes = default_bindings();
    if let Some(combinacao) = combinacao {
        padroes.retain(|padrao| {
            padrao.action != Action::TogglePanel
                && padrao
                    .accelerator
                    .parse::<Shortcut>()
                    .map(|c| c != combinacao)
                    .unwrap_or(true)
        });
        bindings.push(Binding {
            accelerator: legado,
            action: Action::TogglePanel,
        });
    }
    bindings.extend(padroes);
    // O campo antigo espelha o painel que de fato vale (nunca fica com lixo).
    settings.shortcut = espelho_do_painel(&bindings);
    settings.bindings = bindings;
    (settings, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_have_no_duplicate_accelerators() {
        let bindings = default_bindings();

        for (indice, binding) in bindings.iter().enumerate() {
            for outra in bindings.iter().skip(indice + 1) {
                assert!(
                    !binding.accelerator.eq_ignore_ascii_case(&outra.accelerator),
                    "atalho repetido: {} e {}",
                    binding.accelerator,
                    outra.accelerator
                );
            }
        }
    }

    #[test]
    fn default_bindings_are_the_expected_ones() {
        let bindings = default_bindings();

        assert_eq!(bindings.len(), 5);
        assert_eq!(bindings[0].accelerator, "Super+Alt+V");
        assert_eq!(bindings[0].action, Action::TogglePanel);
        assert_eq!(bindings[1].accelerator, "Ctrl+Alt+M");
        assert_eq!(bindings[1].action, Action::ToggleMicMute);
        assert_eq!(bindings[2].accelerator, "Ctrl+Alt+Up");
        assert_eq!(bindings[2].action, Action::StepOutputVolume { delta: 0.05 });
        assert_eq!(bindings[3].accelerator, "Ctrl+Alt+Down");
        assert_eq!(bindings[3].action, Action::StepOutputVolume { delta: -0.05 });
        assert_eq!(bindings[4].accelerator, "Ctrl+Alt+P");
    }

    #[test]
    fn default_bindings_include_cycle_profile_on_ctrl_alt_p() {
        let padrao = default_bindings()
            .into_iter()
            .find(|b| b.accelerator == "Ctrl+Alt+P")
            .expect("Ctrl+Alt+P está nos padrões");

        assert_eq!(
            padrao.action,
            Action::CycleProfile {
                direction: crate::actions::Direction::Next
            }
        );
    }

    #[test]
    fn every_default_accelerator_parses() {
        for binding in default_bindings() {
            assert!(
                binding.accelerator.parse::<Shortcut>().is_ok(),
                "atalho inválido para o plugin: {}",
                binding.accelerator
            );
        }
    }

    #[test]
    fn action_for_matches_the_pressed_combination() {
        let bindings = default_bindings();

        let pressed: Shortcut = "Ctrl+Alt+Up".parse().unwrap();
        assert_eq!(
            action_for(&bindings, &pressed),
            Some(Action::StepOutputVolume { delta: 0.05 })
        );
    }

    #[test]
    fn action_for_ignores_the_order_of_the_modifiers() {
        let bindings = default_bindings();

        // O Windows devolve os modificadores em outra ordem que a da tabela.
        let pressed: Shortcut = "Alt+Ctrl+Up".parse().unwrap();
        assert_eq!(
            action_for(&bindings, &pressed),
            Some(Action::StepOutputVolume { delta: 0.05 })
        );
    }

    #[test]
    fn action_for_returns_none_for_an_unknown_combination() {
        let bindings = default_bindings();

        let pressed: Shortcut = "Ctrl+Alt+K".parse().unwrap();
        assert_eq!(action_for(&bindings, &pressed), None);
    }

    #[test]
    fn binding_json_shape_is_pinned() {
        // A forma é a que a interface vai ler quando `bindings` entrar nas configurações.
        let json = serde_json::to_string(&Binding {
            accelerator: "Ctrl+Alt+M".to_string(),
            action: Action::ToggleOutputMute,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"accelerator":"Ctrl+Alt+M","action":{"type":"toggle_output_mute"}}"#
        );
    }

    // ---------- validação ----------

    fn binding(accelerator: &str) -> Binding {
        Binding {
            accelerator: accelerator.to_string(),
            action: Action::TogglePanel,
        }
    }

    #[test]
    fn accepts_the_default_bindings() {
        assert_eq!(validate_bindings(&default_bindings()), Ok(()));
    }

    #[test]
    fn rejects_an_empty_accelerator() {
        let lista = vec![binding("Ctrl+Alt+M"), binding("   ")];
        assert_eq!(validate_bindings(&lista), Err((1, BindingError::Empty)));
    }

    #[test]
    fn rejects_a_bad_syntax() {
        for invalido in ["Ctrl+", "Banana"] {
            let lista = vec![binding(invalido)];
            assert_eq!(
                validate_bindings(&lista),
                Err((0, BindingError::Syntax)),
                "acelerador: {invalido}"
            );
        }
    }

    #[test]
    fn rejects_an_accelerator_over_64_chars() {
        let lista = vec![binding(&"A".repeat(65))];
        assert_eq!(validate_bindings(&lista), Err((0, BindingError::TooLong)));
    }

    #[test]
    fn rejects_duplicate_accelerators_ignoring_case() {
        let lista = vec![binding("ctrl+alt+m"), binding("Ctrl+Alt+M")];
        assert_eq!(validate_bindings(&lista), Err((1, BindingError::Duplicate)));
    }

    #[test]
    fn rejects_duplicate_accelerators_ignoring_the_order_of_the_modifiers() {
        let lista = vec![binding("Ctrl+Alt+M"), binding("Alt+Ctrl+M")];
        assert_eq!(validate_bindings(&lista), Err((1, BindingError::Duplicate)));
    }

    #[test]
    fn every_binding_error_has_a_message_in_portuguese() {
        // A frase exata de cada variante: o teste confere o que o nome promete (a frase
        // que o usuário lê), e não só que existe texto.
        let esperado = [
            (BindingError::Empty, "O atalho está vazio."),
            (
                BindingError::TooLong,
                "O atalho é longo demais (máximo de 64 caracteres).",
            ),
            (BindingError::Syntax, "O atalho não é uma combinação válida."),
            (BindingError::Duplicate, "Esse atalho já está em uso."),
        ];

        for (erro, frase) in esperado {
            assert_eq!(erro.message(), frase, "frase errada para {erro:?}");
        }
    }

    // ---------- migração ----------

    fn settings_com(shortcut: &str, bindings: Vec<Binding>) -> Settings {
        Settings {
            shortcut: shortcut.to_string(),
            autostart: false,
            bindings,
        }
    }

    /// Quantos bindings `toggle_panel` a lista tem.
    fn paineis(lista: &[Binding]) -> Vec<&Binding> {
        lista
            .iter()
            .filter(|b| b.action == Action::TogglePanel)
            .collect()
    }

    #[test]
    fn migration_seeds_the_legacy_shortcut_and_the_defaults() {
        let (migrada, migrou) = migrate_settings(1, settings_com("Super+Alt+P", Vec::new()));

        assert!(migrou);
        assert_eq!(
            migrada.bindings[0],
            Binding {
                accelerator: "Super+Alt+P".to_string(),
                action: Action::TogglePanel,
            }
        );
        // Os padrões entram, menos o `toggle_panel` deles: o legado do usuário é o painel.
        for padrao in default_bindings()
            .into_iter()
            .filter(|b| b.action != Action::TogglePanel)
        {
            assert!(
                migrada.bindings.contains(&padrao),
                "padrão ausente: {padrao:?}"
            );
        }
        assert_eq!(paineis(&migrada.bindings).len(), 1, "um painel só");
        // O campo antigo continua no lugar e espelha o painel.
        assert_eq!(migrada.shortcut, "Super+Alt+P");
    }

    #[test]
    fn migration_lets_the_legacy_shortcut_win_over_a_colliding_default() {
        // I1: o legado coincide com o padrão de OUTRA ação (silenciar a saída). O usuário
        // venceu: o atalho dele abre o painel e o padrão colidente é que sai.
        for legado in ["Ctrl+Alt+M", "Ctrl+Alt+P", "ctrl+alt+m", "Alt+Ctrl+M"] {
            let (migrada, migrou) = migrate_settings(1, settings_com(legado, Vec::new()));

            assert!(migrou, "legado: {legado}");
            assert_eq!(
                migrada.bindings[0],
                Binding {
                    accelerator: legado.to_string(),
                    action: Action::TogglePanel,
                },
                "legado: {legado}"
            );
            assert_eq!(paineis(&migrada.bindings).len(), 1, "legado: {legado}");
            // Nenhum acelerador repetido: a lista passa na validação do `save_bindings`.
            assert_eq!(validate_bindings(&migrada.bindings), Ok(()), "legado: {legado}");
            // Invariante: o campo antigo é o espelho do painel.
            assert_eq!(migrada.shortcut, legado, "legado: {legado}");
        }
        // Os padrões que não colidem continuam lá.
        let (migrada, _) = migrate_settings(1, settings_com("Ctrl+Alt+M", Vec::new()));
        assert!(!migrada
            .bindings
            .iter()
            .any(|b| b.action == Action::ToggleMicMute));
        assert!(migrada
            .bindings
            .iter()
            .any(|b| b.accelerator == "Ctrl+Alt+Up"));
    }

    #[test]
    fn migration_does_not_duplicate_the_legacy_shortcut() {
        // O legado é exatamente o painel padrão: a lista sai igual aos padrões.
        let (migrada, migrou) = migrate_settings(1, settings_com("Super+Alt+V", Vec::new()));

        assert!(migrou);
        assert_eq!(migrada.bindings, default_bindings());
        assert_eq!(paineis(&migrada.bindings).len(), 1);
        assert_eq!(migrada.shortcut, "Super+Alt+V");
    }

    #[test]
    fn migration_keeps_a_single_panel_binding_for_a_custom_legacy_shortcut() {
        // Agravante do I1: legado personalizado sem colisão não pode semear também o
        // `Super+Alt+V` do painel (o usuário pode ter largado essa tecla de propósito).
        let (migrada, _) = migrate_settings(1, settings_com("Super+Alt+P", Vec::new()));

        assert_eq!(paineis(&migrada.bindings).len(), 1);
        assert!(!migrada
            .bindings
            .iter()
            .any(|b| b.accelerator == "Super+Alt+V"));
    }

    #[test]
    fn migration_of_a_garbage_shortcut_creates_no_binding() {
        for atalho in ["", "???"] {
            let (migrada, migrou) = migrate_settings(1, settings_com(atalho, Vec::new()));
            assert!(migrou, "atalho: {atalho:?}");
            assert_eq!(
                migrada.bindings,
                default_bindings(),
                "atalho: {atalho:?}"
            );
            // O campo antigo nunca fica com lixo: espelha o painel que de fato vale.
            assert_eq!(migrada.shortcut, "Super+Alt+V", "atalho: {atalho:?}");
        }
    }

    #[test]
    fn migration_keeps_a_binding_list_that_is_already_there() {
        // Ruling 14: uma lista já existente e não vazia nunca é sobrescrita.
        let lista = vec![Binding {
            accelerator: "Ctrl+Alt+K".to_string(),
            action: Action::TogglePanel,
        }];
        let (migrada, migrou) = migrate_settings(1, settings_com("Super+Alt+P", lista.clone()));

        assert_eq!(migrada.bindings, lista);
        assert!(!migrou, "não há o que migrar: a lista já está lá");
    }

    #[test]
    fn migration_of_the_current_version_changes_nothing() {
        let antes = settings_com("Super+Alt+P", Vec::new());
        let (migrada, migrou) = migrate_settings(2, antes.clone());

        assert_eq!(migrada, antes);
        assert!(!migrou);
    }
}
