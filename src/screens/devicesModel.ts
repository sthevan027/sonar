// Funções puras da tela de dispositivos de áudio. Sem JSX: testáveis sem DOM.

import type { IconName } from "../icons";
import type { DeviceView, Flow } from "../types";
import { iconNameOf } from "./model";

/** Ícones que o usuário pode escolher para um dispositivo (os de docs/mockups/D.html). */
export const DEVICE_ICONS: { name: IconName; label: string }[] = [
  { name: "speaker", label: "Alto-falante" },
  { name: "headphones", label: "Fone" },
  { name: "monitor", label: "Monitor" },
  { name: "tv", label: "TV" },
  { name: "bluetooth", label: "Bluetooth" },
  { name: "mic", label: "Microfone" },
];

/** Ícone que o dispositivo mostra: o escolhido, ou o padrão do fluxo quando não há escolha. */
export function iconOf(device: DeviceView): IconName {
  return iconNameOf(device) ?? (device.flow === "capture" ? "mic" : "speaker");
}

/** Só os dispositivos que o painel mostra nos seletores: conectados e não ocultos. */
export function visibleDevices(devices: DeviceView[]): DeviceView[] {
  return devices.filter((d) => d.connected && !d.hidden);
}

/** Dispositivos de um fluxo, conectados primeiro (a ordem do Windows dentro de cada grupo). */
export function devicesOfFlow(devices: DeviceView[], flow: Flow): DeviceView[] {
  return devices
    .filter((d) => d.flow === flow)
    .sort((a, b) => Number(b.connected) - Number(a.connected));
}

/**
 * Linha de estado do cartão, separada por " · ". Desconectado com regras guardadas mostra
 * "regras guardadas" (as regras voltam sozinhas quando o dispositivo reconectar).
 */
export function statusLine(device: DeviceView): string {
  const parts: string[] = [device.connected ? "Conectado" : "Desconectado"];
  if (device.is_default) parts.push("padrão do Windows");
  if (device.hidden) parts.push("oculto no painel");
  if (!device.connected && device.has_rules) parts.push("regras guardadas");
  return parts.join(" · ");
}

/** Tom do bloco do ícone: `hs` fone (dourado), `sg` demais (azul), `nt` apagado (oculto/desconectado). */
export function tileToneOf(device: DeviceView): "hs" | "sg" | "nt" {
  if (!device.connected || device.hidden) return "nt";
  return iconOf(device) === "headphones" ? "hs" : "sg";
}

export const DEVICE_FOOT: Record<Flow, string> = {
  render: "Os nomes só mudam aqui. O Windows mantém o original.",
  capture: "Cada app escolhe o seu microfone. Mute e volume são do dispositivo.",
};
