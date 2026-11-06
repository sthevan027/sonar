//! Atalhos globais: a tabela que liga uma combinação de teclas a uma ação.
//!
//! A tabela é a mesma que a interface já espera (o `Vec<Binding>` das configurações entra
//! na Task 14); aqui ela já existe como lista de partidas.

use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::Shortcut;

use crate::actions::Action;

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
            action: Action::ToggleOutputMute,
        },
        Binding {
            accelerator: "Ctrl+Alt+Up".to_string(),
            action: Action::StepOutputVolume { delta: 0.05 },
        },
        Binding {
            accelerator: "Ctrl+Alt+Down".to_string(),
            action: Action::StepOutputVolume { delta: -0.05 },
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

        assert_eq!(bindings.len(), 4);
        assert_eq!(bindings[0].accelerator, "Super+Alt+V");
        assert_eq!(bindings[0].action, Action::TogglePanel);
        assert_eq!(bindings[1].accelerator, "Ctrl+Alt+M");
        assert_eq!(bindings[1].action, Action::ToggleOutputMute);
        assert_eq!(bindings[2].accelerator, "Ctrl+Alt+Up");
        assert_eq!(bindings[2].action, Action::StepOutputVolume { delta: 0.05 });
        assert_eq!(bindings[3].accelerator, "Ctrl+Alt+Down");
        assert_eq!(bindings[3].action, Action::StepOutputVolume { delta: -0.05 });
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
}
