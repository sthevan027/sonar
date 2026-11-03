// Funções puras que as telas de saída e microfone usam. Sem JSX: testáveis sem DOM.

import { isIconName, type IconName } from "../icons";
import type { AppState, AudioSession, DeviceView, Flow, Rule } from "../types";

export const DEFAULT_ROUTE_LABEL = "Padrão do Windows";

/** Um app na tela: todas as sessões de um mesmo exe (vários PIDs) viram uma linha. */
export interface AppRow {
  /** Exe em minúsculas: chave de agrupamento. */
  key: string;
  /** Exe como veio do Windows (é o que `set_app_route` recebe). */
  exe: string;
  /** Todos os PIDs deste exe no fluxo. O volume é aplicado a todos. */
  pids: number[];
  /** Algum PID está tocando agora. */
  active: boolean;
  /** Volume (0.0 a 1.0) da sessão que representa o app: a primeira ativa, ou a primeira. */
  volume: number;
}

/** Agrupa as sessões de um fluxo (padrão: saída) em linhas, separando as ativas das silenciosas. */
export function groupSessions(
  sessions: AudioSession[],
  flow: Flow = "render",
): { active: AppRow[]; silent: AppRow[] } {
  const groups = new Map<string, AudioSession[]>();
  for (const session of sessions) {
    if (session.flow !== flow) continue;
    const key = session.exe.toLowerCase();
    const list = groups.get(key);
    if (list) list.push(session);
    else groups.set(key, [session]);
  }

  const rows: AppRow[] = Array.from(groups, ([key, list]) => {
    const representative = list.find((s) => s.active) ?? list[0];
    return {
      key,
      exe: list[0].exe,
      pids: Array.from(new Set(list.map((s) => s.pid))),
      active: list.some((s) => s.active),
      volume: representative.volume,
    };
  }).sort((a, b) => (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));

  return {
    active: rows.filter((row) => row.active),
    silent: rows.filter((row) => !row.active),
  };
}

/** Nome para a tela: sem `.exe`, primeira letra em maiúscula (por ponto de código). */
export function appName(exe: string): string {
  const base = exe.trim().replace(/\.exe$/i, "");
  const [first = "", ...rest] = Array.from(base);
  return first.toUpperCase() + rest.join("");
}

export function findRule(rules: Rule[], exe: string, flow: Flow): Rule | undefined {
  const wanted = exe.toLowerCase();
  return rules.find((rule) => rule.flow === flow && rule.exe.toLowerCase() === wanted);
}

export interface RouteInfo {
  /** Texto do chip. */
  label: string;
  /** Id marcado no menu; `null` = Padrão do Windows. */
  selectedId: string | null;
  /** Aviso quando a regra aponta para um dispositivo que não está conectado. */
  warning: string | null;
}

/**
 * Rota que a tela mostra para um app. Sem regra: padrão do Windows. Regra para dispositivo
 * conectado: o dispositivo. Regra para dispositivo desconectado: padrão do Windows com aviso
 * (a regra continua guardada e volta sozinha quando o dispositivo reconectar).
 */
export function routeFor(state: AppState, exe: string, flow: Flow): RouteInfo {
  const rule = findRule(state.rules, exe, flow);
  if (!rule) return { label: DEFAULT_ROUTE_LABEL, selectedId: null, warning: null };

  const device = state.devices.find((d) => d.id === rule.device && d.flow === flow);
  if (device?.connected) {
    return { label: device.display_name, selectedId: device.id, warning: null };
  }
  const name = device?.display_name ?? "Dispositivo guardado";
  return {
    label: DEFAULT_ROUTE_LABEL,
    selectedId: null,
    warning: `${name} desconectado: usando o padrão do Windows`,
  };
}

/** Cor do chip: latão para fone (ícone "headphones"), azul-acinzentado para o resto. */
export function toneOf(device: DeviceView | undefined): "brass" | "sage" {
  return device?.icon === "headphones" ? "brass" : "sage";
}

/** Ícone escolhido para o dispositivo, se o nome existir no mapa de ícones. */
export function iconNameOf(device: DeviceView | undefined): IconName | undefined {
  return device && isIconName(device.icon) ? device.icon : undefined;
}

/** Roda um comando e chama `onFail` se ele falhar. O estado real volta pelo `state-changed`. */
export function attempt(call: Promise<unknown>, onFail: () => void): void {
  Promise.resolve(call).then(undefined, onFail);
}

/**
 * Dispositivos que o menu oferece para um fluxo: conectados e não ocultos, mais o dispositivo
 * da rota atual mesmo se estiver oculto (senão o chip mostraria um item que não está no menu).
 */
export function selectableDevices(
  devices: DeviceView[],
  flow: Flow,
  selectedId: string | null,
): DeviceView[] {
  return devices.filter(
    (d) => d.flow === flow && d.connected && (!d.hidden || d.id === selectedId),
  );
}

/** Escolha de rota que o usuário fez e que o sistema ainda não confirmou (ver `useRoutePending`). */
export interface PendingRoute {
  /** Sequência da escolha: uma falha antiga não apaga uma escolha mais nova do mesmo app. */
  id: number;
  exe: string;
  flow: Flow;
  /** Dispositivo escolhido; `null` = Padrão do Windows. */
  device: string | null;
}

/** Chave de um app num sentido: o exe em minúsculas, como o `findRule` compara. */
export function routeKey(exe: string, flow: Flow): string {
  return `${flow}|${exe.toLowerCase()}`;
}

/**
 * Rota mostrada enquanto a escolha está pendente: a escolhida, sem aviso de desconectado
 * (o aviso fala da regra salva, que ainda não foi trocada).
 */
export function pendingRouteInfo(
  state: AppState,
  flow: Flow,
  device: string | null,
): RouteInfo {
  if (device === null) return { label: DEFAULT_ROUTE_LABEL, selectedId: null, warning: null };
  const chosen = state.devices.find((d) => d.id === device && d.flow === flow);
  return { label: chosen?.display_name ?? device, selectedId: device, warning: null };
}

/** `true` quando a regra salva já é a escolha pendente (sem regra conta como `null`). */
export function isRouteSettled(
  state: AppState,
  exe: string,
  flow: Flow,
  device: string | null,
): boolean {
  return (findRule(state.rules, exe, flow)?.device ?? null) === device;
}
