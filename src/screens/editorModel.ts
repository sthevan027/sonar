// Funções puras do editor de perfil e do seletor de apps. Sem JSX: testáveis sem DOM.
// Regras de nome e de destino espelham o Rust (`profiles.rs`): o nome é aparado antes de
// contar; o destino `undefined` some do JSON e mantém a rota atual.

import type {
  AppState,
  DeviceView,
  Flow,
  Profile,
  ProfileItem,
  RunningApp,
  Target,
} from "../types";

export const NAME_MAX = 20;

/** Os 8 ícones de perfil do mockup C. Os nomes são os de `icons.ts`. */
export const PROFILE_ICONS: { name: string; label: string }[] = [
  { name: "gamepad-2", label: "Jogos" },
  { name: "briefcase", label: "Trabalho" },
  { name: "music", label: "Música" },
  { name: "headphones", label: "Fone" },
  { name: "tv", label: "Vídeo" },
  { name: "book-open", label: "Estudo" },
  { name: "coffee", label: "Pausa" },
  { name: "rocket", label: "Projeto" },
];

export const DEFAULT_PROFILE_ICON = "gamepad-2";

/** Tamanho do nome por ponto de código, depois de aparar (como o Rust). */
export function nameLength(name: string): number {
  return Array.from(name.trim()).length;
}

/** Erro do nome, ou `null` quando vale. Mesmas regras de `validate_name` no Rust. */
export function nameError(name: string): string | null {
  if (/\p{Cc}/u.test(name.trim())) return "O nome não pode ter caracteres de controle.";
  if (nameLength(name) > NAME_MAX) return `O nome pode ter no máximo ${NAME_MAX} caracteres.`;
  return null;
}

/** Escolha de um destino: `"keep"` (manter), `"default"` (padrão do Windows) ou id do dispositivo. */
export type Choice = "keep" | "default" | string;

export function choiceOf(target: Target | undefined): Choice {
  return target ?? "keep";
}

/** Id marcado no `DeviceMenu`: `null` = Padrão do Windows; `"keep"` = manter. */
export function menuIdOf(choice: Choice): string | null {
  return choice === "default" ? null : choice;
}

export function choiceFromMenu(id: string | null): Choice {
  return id === null ? "default" : id;
}

/** Texto do chip de um destino. Dispositivo guardado mas desconectado mostra o nome dele. */
export function choiceLabel(choice: Choice, devices: DeviceView[], flow: Flow): string {
  if (choice === "keep") return "Manter atual";
  if (choice === "default") return "Padrão do Windows";
  const device = devices.find((d) => d.id === choice && d.flow === flow);
  return device ? device.display_name : "Dispositivo desconectado";
}

/** Dispositivo escolhido para um destino (para a cor do chip). */
export function deviceOfChoice(
  choice: Choice,
  devices: DeviceView[],
  flow: Flow,
): DeviceView | undefined {
  if (choice === "keep" || choice === "default") return undefined;
  return devices.find((d) => d.id === choice && d.flow === flow);
}

/**
 * O chip de mic aparece para o app que tem sessão de captura, e também quando o item já
 * tem destino de mic guardado (assim um destino salvo nunca fica escondido sem aviso).
 */
export function usesMic(state: AppState, item: ProfileItem): boolean {
  if (item.input !== undefined) return true;
  const exe = item.exe.toLowerCase();
  return state.sessions.some((s) => s.flow === "capture" && s.exe.toLowerCase() === exe);
}

export interface Draft {
  id: string;
  icon: string;
  name: string;
  items: ProfileItem[];
}

/** Rascunho do editor: do perfil existente, ou novo com id gerado por `newId`. */
export function draftFrom(profile: Profile | undefined, newId: () => string): Draft {
  if (!profile) {
    return { id: newId(), icon: DEFAULT_PROFILE_ICON, name: "", items: [] };
  }
  return {
    id: profile.id,
    icon: profile.icon,
    name: profile.name ?? "",
    items: profile.items.map((item) => ({ ...item })),
  };
}

/** Monta o perfil para `save_profile`. Campos `undefined` não entram; nome vazio some. */
export function buildProfile(draft: Draft): Profile {
  const profile: Profile = {
    id: draft.id,
    icon: draft.icon,
    items: draft.items.map(itemOf),
  };
  const name = draft.name.trim();
  if (name) profile.name = name;
  return profile;
}

function itemOf(item: ProfileItem): ProfileItem {
  const out: ProfileItem = { exe: item.exe };
  if (item.output !== undefined) out.output = item.output;
  if (item.input !== undefined) out.input = item.input;
  return out;
}

/** Adiciona o app com destinos "manter". Não repete um exe já presente. */
export function addApp(items: ProfileItem[], exe: string): ProfileItem[] {
  const key = exe.toLowerCase();
  if (items.some((item) => item.exe.toLowerCase() === key)) return items;
  return [...items, { exe: key }];
}

/** Filtra apps por nome ou exe, sem diferenciar maiúsculas. Consulta vazia devolve tudo. */
export function filterApps(apps: RunningApp[], query: string): RunningApp[] {
  const wanted = query.trim().toLowerCase();
  if (!wanted) return apps;
  return apps.filter(
    (app) => app.name.toLowerCase().includes(wanted) || app.exe.toLowerCase().includes(wanted),
  );
}

/** App a partir de um caminho escolhido em "Procurar .exe". `null` se o caminho não tem nome. */
export function appFromPath(path: string): RunningApp | null {
  const base = path.trim().split(/[\\/]/).pop() ?? "";
  if (!base) return null;
  return { exe: base.toLowerCase(), name: base.replace(/\.exe$/i, "") };
}
