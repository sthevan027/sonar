// Funções puras da tela de atalhos globais: catálogo de ações, rótulos, validação e padrões.
// Sem JSX: testável sem DOM.

import type { IconName } from "../icons";
import type {
  Action,
  Binding,
  DeviceView,
  Direction,
  Profile,
  Settings,
  SettingsWithBindings,
} from "../types";
import { appName } from "./model";
import { normalizeAccelerator, SHORTCUT_MODIFIER_MESSAGE } from "./settingsModel";

/** Família da ação: o `type` do enum `Action` do Rust. */
export type ActionKind = Action["type"];

export interface ActionChoice {
  kind: ActionKind;
  /** Nome no seletor de ação. */
  label: string;
  /** Diga curta do parâmetro ("perfil", "exe", "±5%"). */
  hint: string;
  /** Ícone da família da ação. */
  icon: IconName;
  /** Ações de perfil usam o latão na lista. */
  brass: boolean;
}

/**
 * Lista fechada de ações (SPEC §5 do rascunho). O parâmetro de cada uma é escolhido logo
 * abaixo, na seção "Parâmetros" do editor.
 */
export const ACTION_CHOICES: ActionChoice[] = [
  {
    kind: "toggle_panel",
    label: "Mostrar ou esconder o painel",
    hint: "",
    icon: "keyboard",
    brass: false,
  },
  {
    kind: "activate_profile",
    label: "Ativar um perfil",
    hint: "perfil",
    icon: "layers",
    brass: true,
  },
  {
    kind: "cycle_profile",
    label: "Próximo / Perfil anterior",
    hint: "↻",
    icon: "layers",
    brass: true,
  },
  {
    kind: "toggle_output_mute",
    label: "Alternar o mudo da saída padrão",
    hint: "",
    icon: "volume-x",
    brass: false,
  },
  {
    kind: "set_output_mute",
    label: "Silenciar / Ativar o som da saída padrão",
    hint: "mudo",
    icon: "volume-x",
    brass: false,
  },
  {
    kind: "step_output_volume",
    label: "Volume da saída padrão (passo)",
    hint: "±5%",
    icon: "volume-2",
    brass: false,
  },
  {
    kind: "set_output_volume",
    label: "Volume da saída padrão (valor)",
    hint: "fixo",
    icon: "volume-2",
    brass: false,
  },
  {
    kind: "set_app_mute",
    label: "Mudo de um app",
    hint: "exe",
    icon: "audio-lines",
    brass: false,
  },
  {
    kind: "move_app",
    label: "Mover um app para outra saída",
    hint: "exe",
    icon: "headphones",
    brass: false,
  },
  {
    kind: "cycle_default_output",
    label: "Próxima saída padrão",
    hint: "↻",
    icon: "speaker",
    brass: false,
  },
];

/**
 * Lista de bindings das configurações. `null` quando o Rust ainda não manda o campo (a
 * migração da Task 12 não chegou): aí a tela não pode afirmar que não existe atalho.
 */
export function bindingsOf(settings: Settings | undefined | null): Binding[] | null {
  if (!settings) return null;
  const list = (settings as SettingsWithBindings).bindings;
  return Array.isArray(list) ? list : null;
}

/** Duas listas com o mesmo conteúdo (para não recriar a lista a cada evento). */
export function sameBindings(a: Binding[], b: Binding[]): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Ação inicial de cada família, para quando o atalho é novo. */
export function actionOfKind(kind: ActionKind): Action {
  switch (kind) {
    case "toggle_panel":
      return { type: "toggle_panel" };
    case "activate_profile":
      return { type: "activate_profile", profile_id: "" };
    case "cycle_profile":
      return { type: "cycle_profile", direction: "next" };
    case "toggle_output_mute":
      return { type: "toggle_output_mute" };
    case "set_output_mute":
      return { type: "set_output_mute", muted: true };
    case "step_output_volume":
      return { type: "step_output_volume", delta: 0.05 };
    case "set_output_volume":
      return { type: "set_output_volume", value: 0.5 };
    case "set_app_mute":
      return { type: "set_app_mute", exe: "", muted: true };
    case "move_app":
      return { type: "move_app", exe: "", device: null };
    case "cycle_default_output":
      return { type: "cycle_default_output", direction: "next" };
  }
}

/** Troca o parâmetro de uma ação, mantendo a família. */
export function withProfile(action: Action, profileId: string): Action {
  return action.type === "activate_profile" ? { ...action, profile_id: profileId } : action;
}

export function withDirection(action: Action, direction: Direction): Action {
  if (action.type === "cycle_profile" || action.type === "cycle_default_output") {
    return { ...action, direction };
  }
  return action;
}

export function withMuted(action: Action, muted: boolean): Action {
  if (action.type === "set_output_mute" || action.type === "set_app_mute") {
    return { ...action, muted };
  }
  return action;
}

export function withDelta(action: Action, delta: number): Action {
  return action.type === "step_output_volume" ? { ...action, delta } : action;
}

export function withValue(action: Action, value: number): Action {
  return action.type === "set_output_volume" ? { ...action, value } : action;
}

export function withExe(action: Action, exe: string): Action {
  if (action.type === "set_app_mute" || action.type === "move_app") return { ...action, exe };
  return action;
}

export function withDevice(action: Action, device: string | null): Action {
  return action.type === "move_app" ? { ...action, device } : action;
}

/** A ação precisa de um parâmetro que ainda não foi escolhido. */
export function missingParameter(action: Action): string | null {
  switch (action.type) {
    case "activate_profile":
      return action.profile_id ? null : "Escolha o perfil.";
    case "set_app_mute":
    case "move_app":
      return action.exe ? null : "Escolha o app.";
    default:
      return null;
  }
}

/** A ação tem seção de parâmetros no editor. */
export function hasParameters(kind: ActionKind): boolean {
  return (
    kind === "activate_profile" ||
    kind === "cycle_profile" ||
    kind === "set_output_mute" ||
    kind === "step_output_volume" ||
    kind === "set_output_volume" ||
    kind === "set_app_mute" ||
    kind === "move_app" ||
    kind === "cycle_default_output"
  );
}

/** Contexto para escrever os rótulos: quem existe no sistema agora. */
export interface LabelContext {
  profiles: Profile[];
  devices: DeviceView[];
}

function profileOf(ctx: LabelContext, id: string): Profile | undefined {
  return ctx.profiles.find((p) => p.id === id);
}

function deviceOf(ctx: LabelContext, id: string): DeviceView | undefined {
  return ctx.devices.find((d) => d.id === id);
}

/** Título da linha: o alvo do atalho. */
export function bindingTitle(action: Action, ctx: LabelContext): string {
  switch (action.type) {
    case "toggle_panel":
      return "Atalho global";
    case "toggle_output_mute":
    case "set_output_mute":
      return "Mudo da saída padrão";
    case "step_output_volume":
    case "set_output_volume":
      return "Volume da saída padrão";
    case "activate_profile": {
      const profile = profileOf(ctx, action.profile_id);
      if (!profile) return "Perfil apagado";
      return `Perfil "${profile.name ?? profile.id}"`;
    }
    case "cycle_profile":
      return action.direction === "next" ? "Próximo perfil" : "Perfil anterior";
    case "set_app_mute":
      return action.muted ? `Mudo do "${appName(action.exe)}"` : `Som do "${appName(action.exe)}"`;
    case "move_app":
      return `Mover "${appName(action.exe)}"`;
    case "cycle_default_output":
      return "Próxima saída padrão";
  }
}

/** Detalhe da linha: o que o atalho faz. */
export function bindingDetail(action: Action, ctx: LabelContext): string {
  switch (action.type) {
    case "toggle_panel":
      return "Abre o painel na hora";
    case "toggle_output_mute":
      return "Alternar o mudo";
    case "set_output_mute":
      return action.muted ? "Silenciar" : "Ativar o som";
    case "step_output_volume":
      if (action.delta > 0) return "Subir 5%";
      if (action.delta < 0) return "Baixar 5%";
      return "Ajustar o volume";
    case "set_output_volume":
      return `Definir em ${Math.round(action.value * 100)}%`;
    case "activate_profile":
      return profileOf(ctx, action.profile_id) ? "Ativar este perfil" : "Perfil não encontrado";
    case "cycle_profile":
      return action.direction === "next"
        ? "Avançar na lista de perfis"
        : "Voltar na lista de perfis";
    case "set_app_mute":
      return action.muted ? "Silenciar o app" : "Ativar o som do app";
    case "move_app": {
      if (action.device === null) return "Para: Padrão";
      const device = deviceOf(ctx, action.device);
      return device ? `Para: ${device.display_name}` : "Saída não encontrada";
    }
    case "cycle_default_output":
      return "Alternar a saída do Windows";
  }
}

/** Ícone da família da ação. */
export function bindingIcon(action: Action): IconName {
  const choice = ACTION_CHOICES.find((c) => c.kind === action.type);
  return choice?.icon ?? "keyboard";
}

/** Ações de perfil são as de latão na lista. */
export function bindingIsBrass(action: Action): boolean {
  return ACTION_CHOICES.find((c) => c.kind === action.type)?.brass ?? false;
}

export const DUPLICATE_MESSAGE = "Este atalho já está em uso por outro atalho.";
export const EMPTY_ACCELERATOR_MESSAGE = "Escolha uma combinação de teclas.";
export const TOO_LONG_MESSAGE = "Atalho muito longo (máximo 64 caracteres).";
export const SYNTAX_MESSAGE = "Use letras, números ou F1 a F12.";

/** Tecla principal aceita: letra, número, F1 a F12 e as duas setas de volume. */
const MAIN_KEY = /^(?:[A-Za-z0-9]|F(?:[1-9]|1[0-2])|Up|Down)$/;

/**
 * Valida o acelerador digitado (RF-16): vazio, tamanho, sintaxe e modificador forte.
 * Devolve a frase do erro ou `null` quando o atalho serve.
 */
export function validateAccelerator(accelerator: string): string | null {
  const trimmed = accelerator.trim();
  if (!trimmed) return EMPTY_ACCELERATOR_MESSAGE;
  if (trimmed.length > 64) return TOO_LONG_MESSAGE;
  const parts = trimmed
    .split("+")
    .map((part) => part.trim())
    .filter(Boolean);
  const main = parts[parts.length - 1] ?? "";
  if (!MAIN_KEY.test(main)) return SYNTAX_MESSAGE;
  const modifiers = parts.slice(0, -1).map((part) => part.toLowerCase());
  if (!modifiers.some((m) => m === "ctrl" || m === "alt" || m === "super")) {
    return SHORTCUT_MODIFIER_MESSAGE;
  }
  return null;
}

/**
 * O acelerador já está em uso por outro binding da lista? O Rust compara os atalhos
 * parseados, então a comparação aqui também ignora a caixa.
 */
export function duplicateMessage(accelerator: string, others: Binding[]): string | null {
  const wanted = normalizeAccelerator(accelerator);
  if (!wanted) return null;
  const used = others.some((b) => normalizeAccelerator(b.accelerator) === wanted);
  return used ? DUPLICATE_MESSAGE : null;
}
