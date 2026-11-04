//! Perfis: conjuntos nomeados de rotas por executável, ativados de uma vez.
//!
//! Lógica pura: `activate` só altera o `RuleSet` e devolve as mudanças; quem chama
//! aplica no sistema (`reconcile` e `clear_route`).

use crate::audio::types::{DeviceId, Flow};
use crate::rules::{Rule, RuleSet};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Destino de um sentido (saída ou entrada) de um item do perfil.
///
/// Na serialização, `Keep` é o campo ausente; `Default` é o texto `"default"`;
/// qualquer outro texto é o id de um dispositivo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Target {
    /// Não altera a rota atual do executável.
    #[default]
    Keep,
    /// Remove a regra e limpa a rota gravada pelo Windows.
    Default,
    /// Fixa o executável neste dispositivo.
    Device(DeviceId),
}

impl Target {
    pub fn is_keep(&self) -> bool {
        matches!(self, Target::Keep)
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Target::Keep => serializer.serialize_none(),
            Target::Default => serializer.serialize_str("default"),
            Target::Device(id) => serializer.serialize_str(&id.0),
        }
    }
}

impl<'de> Deserialize<'de> for Target {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<String>::deserialize(deserializer)? {
            None => Target::Keep,
            Some(text) if text == "default" => Target::Default,
            Some(text) => Target::Device(DeviceId(text)),
        })
    }
}

/// Uma linha do perfil: um executável e o destino de saída e de entrada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileItem {
    pub exe: String,
    #[serde(default, skip_serializing_if = "Target::is_keep")]
    pub output: Target,
    #[serde(default, skip_serializing_if = "Target::is_keep")]
    pub input: Target,
}

/// Perfil nomeado (o nome é opcional; o ícone identifica o perfil na interface).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub icon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub items: Vec<ProfileItem>,
}

/// Mudança feita por `activate`, para o chamador aplicar no sistema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// Gravar esta regra nas sessões do executável.
    Set(Rule),
    /// Limpar a rota do executável neste sentido. Sempre reportada, mesmo sem regra.
    Cleared { exe: String, flow: Flow },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    TooLong,
    ControlChar,
}

/// Valida o nome do perfil: apara espaços; vazio vira `None`.
pub fn validate_name(raw: &str) -> Result<Option<String>, NameError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(char::is_control) {
        return Err(NameError::ControlChar);
    }
    if trimmed.chars().count() > 20 {
        return Err(NameError::TooLong);
    }
    Ok(Some(trimmed.to_string()))
}

/// Aplica o perfil ao conjunto de regras e devolve as mudanças. Idempotente.
pub fn activate(profile: &Profile, rules: &mut RuleSet) -> Vec<Change> {
    let mut changes = Vec::new();
    for item in &profile.items {
        let exe = item.exe.to_lowercase();
        for (flow, target) in [(Flow::Render, &item.output), (Flow::Capture, &item.input)] {
            match target {
                Target::Keep => {}
                Target::Device(id) => {
                    let rule = Rule {
                        exe: exe.clone(),
                        flow,
                        device: id.clone(),
                    };
                    rules.upsert(rule.clone());
                    changes.push(Change::Set(rule));
                }
                Target::Default => {
                    rules.remove(&exe, flow);
                    changes.push(Change::Cleared {
                        exe: exe.clone(),
                        flow,
                    });
                }
            }
        }
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str) -> DeviceId {
        DeviceId(id.to_string())
    }

    fn rule(exe: &str, flow: Flow, device: &str) -> Rule {
        Rule {
            exe: exe.to_string(),
            flow,
            device: dev(device),
        }
    }

    fn rules_with(rules: &[Rule]) -> RuleSet {
        let mut set = RuleSet::default();
        for r in rules {
            set.upsert(r.clone());
        }
        set
    }

    fn item(exe: &str, output: Target, input: Target) -> ProfileItem {
        ProfileItem {
            exe: exe.to_string(),
            output,
            input,
        }
    }

    fn profile(items: Vec<ProfileItem>) -> Profile {
        Profile {
            id: "p1".to_string(),
            icon: "game".to_string(),
            name: None,
            items,
        }
    }

    #[test]
    fn keep_does_not_touch_existing_rule() {
        let mut rules = rules_with(&[
            rule("game.exe", Flow::Render, "fone"),
            rule("game.exe", Flow::Capture, "microfone"),
        ]);
        let before = rules.clone();
        let p = profile(vec![item("game.exe", Target::Keep, Target::Keep)]);

        let changes = activate(&p, &mut rules);

        assert!(changes.is_empty());
        assert_eq!(rules, before);
    }

    #[test]
    fn device_target_upserts_rule() {
        let mut rules = rules_with(&[rule("game.exe", Flow::Render, "alto-falante")]);
        let p = profile(vec![
            item("Game.EXE", Target::Device(dev("fone")), Target::Keep),
        ]);

        let changes = activate(&p, &mut rules);

        let expected = rule("game.exe", Flow::Render, "fone");
        assert_eq!(changes, vec![Change::Set(expected.clone())]);
        assert_eq!(rules.rules, vec![expected]);
    }

    #[test]
    fn default_target_removes_rule_and_reports_cleared() {
        let mut rules = rules_with(&[rule("game.exe", Flow::Render, "fone")]);
        let p = profile(vec![item("game.exe", Target::Default, Target::Keep)]);

        let changes = activate(&p, &mut rules);

        assert_eq!(
            changes,
            vec![Change::Cleared {
                exe: "game.exe".to_string(),
                flow: Flow::Render,
            }]
        );
        assert!(rules.get("game.exe", Flow::Render).is_none());
    }

    #[test]
    fn default_target_reports_cleared_even_without_rule() {
        let mut rules = RuleSet::default();
        let p = profile(vec![item("Game.exe", Target::Keep, Target::Default)]);

        let changes = activate(&p, &mut rules);

        assert_eq!(
            changes,
            vec![Change::Cleared {
                exe: "game.exe".to_string(),
                flow: Flow::Capture,
            }]
        );
        assert!(rules.rules.is_empty());
    }

    #[test]
    fn profile_only_changes_listed_apps() {
        let mut rules = rules_with(&[
            rule("spotify.exe", Flow::Render, "caixa"),
            rule("game.exe", Flow::Render, "fone"),
        ]);
        let p = profile(vec![item(
            "game.exe",
            Target::Device(dev("monitor")),
            Target::Keep,
        )]);

        let changes = activate(&p, &mut rules);

        assert_eq!(
            changes,
            vec![Change::Set(rule("game.exe", Flow::Render, "monitor"))]
        );
        assert_eq!(
            rules.get("spotify.exe", Flow::Render),
            Some(&rule("spotify.exe", Flow::Render, "caixa"))
        );
        assert_eq!(rules.rules.len(), 2);
    }

    #[test]
    fn activating_twice_is_idempotent() {
        let mut rules = rules_with(&[rule("outro.exe", Flow::Render, "caixa")]);
        let p = profile(vec![
            item("game.exe", Target::Device(dev("fone")), Target::Default),
            item("chat.exe", Target::Default, Target::Device(dev("microfone"))),
        ]);

        activate(&p, &mut rules);
        let after_first = rules.clone();
        let second = activate(&p, &mut rules);

        assert_eq!(rules, after_first);
        assert_eq!(
            second,
            vec![
                Change::Set(rule("game.exe", Flow::Render, "fone")),
                Change::Cleared {
                    exe: "game.exe".to_string(),
                    flow: Flow::Capture,
                },
                Change::Cleared {
                    exe: "chat.exe".to_string(),
                    flow: Flow::Render,
                },
                Change::Set(rule("chat.exe", Flow::Capture, "microfone")),
            ]
        );
    }

    #[test]
    fn validate_name_trims_and_limits() {
        assert_eq!(validate_name("  Jogo "), Ok(Some("Jogo".to_string())));
        assert_eq!(validate_name("   "), Ok(None));
        assert_eq!(validate_name(""), Ok(None));

        let twenty = "a".repeat(20);
        assert_eq!(validate_name(&twenty), Ok(Some(twenty.clone())));
        let twenty_one = "a".repeat(21);
        assert_eq!(validate_name(&twenty_one), Err(NameError::TooLong));

        assert_eq!(validate_name("\u{7}x"), Err(NameError::ControlChar));
    }

    #[test]
    fn target_serde_roundtrip() {
        let item_default = item("game.exe", Target::Default, Target::Keep);
        let json = serde_json::to_string(&item_default).unwrap();
        assert_eq!(json, r#"{"exe":"game.exe","output":"default"}"#);
        let back: ProfileItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back, item_default);

        let id = "{0.0.0.00000000}.{2329d0e9-306a-408e-9c4b-4061e553cdb4}";
        let item_device = item("chat.exe", Target::Device(dev(id)), Target::Keep);
        let json = serde_json::to_string(&item_device).unwrap();
        assert_eq!(
            json,
            format!(r#"{{"exe":"chat.exe","output":"{id}"}}"#)
        );
        let back: ProfileItem = serde_json::from_str(&json).unwrap();
        assert_eq!(back, item_device);

        let absent: ProfileItem =
            serde_json::from_str(r#"{"exe":"a.exe","output":"default"}"#).unwrap();
        assert_eq!(absent.input, Target::Keep);

        let p = profile(vec![item_default]);
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("\"name\""));
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
