// Funções puras da tela de configurações: atalho global e "Sobre". Sem JSX.

import pkg from "../../package.json";
import type { Binding, Profile } from "../types";

export const APP_NAME = "Sonar";
export const APP_VERSION = pkg.version;

/** Teclas de uma combinação para o desenho do atalho ("Super+Alt+V" -> Win, Alt, V). */
export function shortcutKeys(shortcut: string): string[] {
  return shortcut
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => (part === "Super" ? "Win" : part));
}

/** Campos de um evento de teclado que a captura usa. */
export interface KeyInput {
  code: string;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  metaKey: boolean;
}

export type ShortcutOutcome =
  /** Só uma tecla de modificação: continua esperando a tecla principal. */
  | { status: "wait" }
  | { status: "invalid"; message: string }
  | { status: "ok"; shortcut: string };

const MODIFIER_CODE = /^(Control|Shift|Alt|Meta|OS|AltGraph)(Left|Right)?$/;

/** Tecla principal pelo `code` (independe do layout): letras, números, F1 a F12 e as setas
 *  para cima e para baixo. */
function mainKeyOf(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  if (/^F([1-9]|1[0-2])$/.test(code)) return code;
  // Só as setas para cima e para baixo viram atalho: as laterais são navegação de menu.
  if (code === "ArrowUp") return "Up";
  if (code === "ArrowDown") return "Down";
  return null;
}

/** Mensagem quando a combinação não tem Ctrl, Alt nem Win (Shift sozinho não conta). */
export const SHORTCUT_MODIFIER_MESSAGE = "Inclua Ctrl, Alt ou Win.";

/**
 * Combinação pedida na captura do atalho. Exige uma tecla principal e pelo menos um dos
 * modificadores Ctrl, Alt ou Win. Shift sozinho não basta (digitar uma letra não pode virar
 * atalho global). Ordem fixa: Ctrl, Alt, Shift, Super.
 */
export function shortcutFromKeys(input: KeyInput): ShortcutOutcome {
  if (MODIFIER_CODE.test(input.code)) return { status: "wait" };
  const main = mainKeyOf(input.code);
  if (main === null) {
    return { status: "invalid", message: "Use letras, números, F1 a F12 ou as setas para cima e para baixo." };
  }
  const modifiers: string[] = [];
  if (input.ctrlKey) modifiers.push("Ctrl");
  if (input.altKey) modifiers.push("Alt");
  if (input.shiftKey) modifiers.push("Shift");
  if (input.metaKey) modifiers.push("Super");
  const strong = input.ctrlKey || input.altKey || input.metaKey;
  if (!strong) {
    return { status: "invalid", message: SHORTCUT_MODIFIER_MESSAGE };
  }
  return { status: "ok", shortcut: [...modifiers, main].join("+") };
}

/**
 * Comparação de aceleradores: sem caixa e sem espaços, com os modificadores em ordem
 * fixa. O Rust compara os atalhos *parseados*, então "ctrl+alt+m" e "Ctrl+Alt+M" são a
 * mesma tecla — e "Alt+Super+V" é a mesma que "Super+Alt+V".
 */
export function normalizeAccelerator(accelerator: string): string {
  const parts = accelerator.replace(/\s+/g, "").split("+").filter(Boolean);
  const main = parts.pop() ?? "";
  const modifiers = parts.map((part) => part.toLowerCase()).sort();
  return [...modifiers, main.toLowerCase()].join("+");
}

/**
 * Sugere atalhos `Ctrl+Alt+1` a `Ctrl+Alt+9` para os perfis que ainda não têm binding de
 * perfil. O número começa na posição do perfil na lista (`Ctrl+Alt+1` para o primeiro) e
 * pula para o próximo número livre quando o acelerador já está em uso por qualquer
 * binding. No máximo nove sugestões.
 */
export function suggestProfileBindings(profiles: Profile[], existing: Binding[]): Binding[] {
  const used = new Set(existing.map((binding) => normalizeAccelerator(binding.accelerator)));
  const alreadyBound = new Set(
    existing
      .filter((binding) => binding.action.type === "activate_profile")
      .map((binding) => (binding.action as { profile_id: string }).profile_id),
  );

  const suggestions: Binding[] = [];
  profiles.forEach((profile, index) => {
    if (suggestions.length >= 9) return;
    // Perfil que já tem atalho não entra na lista.
    if (alreadyBound.has(profile.id)) return;
    let number = index + 1;
    while (number <= 9 && used.has(normalizeAccelerator(`Ctrl+Alt+${number}`))) number += 1;
    if (number > 9) return;
    used.add(normalizeAccelerator(`Ctrl+Alt+${number}`));
    suggestions.push({
      accelerator: `Ctrl+Alt+${number}`,
      action: { type: "activate_profile", profile_id: profile.id },
    });
  });
  return suggestions;
}
