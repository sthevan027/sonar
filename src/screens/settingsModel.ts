// Funções puras da tela de configurações: atalho global e "Sobre". Sem JSX.

import pkg from "../../package.json";

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

/** Tecla principal pelo `code` (independe do layout): letras, números e F1 a F12. */
function mainKeyOf(code: string): string | null {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return digit[1];
  if (/^F([1-9]|1[0-2])$/.test(code)) return code;
  return null;
}

/**
 * Combinação pedida na captura do atalho. Exige uma tecla principal e pelo menos um
 * modificador (Ctrl, Alt, Shift ou Win). Ordem fixa: Ctrl, Alt, Shift, Super.
 */
export function shortcutFromKeys(input: KeyInput): ShortcutOutcome {
  if (MODIFIER_CODE.test(input.code)) return { status: "wait" };
  const main = mainKeyOf(input.code);
  if (main === null) {
    return { status: "invalid", message: "Use letras, números ou F1 a F12." };
  }
  const modifiers: string[] = [];
  if (input.ctrlKey) modifiers.push("Ctrl");
  if (input.altKey) modifiers.push("Alt");
  if (input.shiftKey) modifiers.push("Shift");
  if (input.metaKey) modifiers.push("Super");
  if (modifiers.length === 0) {
    return { status: "invalid", message: "Inclua Ctrl, Alt, Shift ou Win." };
  }
  return { status: "ok", shortcut: [...modifiers, main].join("+") };
}
