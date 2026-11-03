// Wrappers tipados dos comandos do Tauri (`src-tauri/src/commands.rs`) e dos eventos.
// Os nomes dos argumentos são os do Rust; o Tauri converte camelCase para snake_case.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppState, Flow, MicLevel, Profile, Settings } from "./types";

export function getState(): Promise<AppState> {
  return invoke<AppState>("get_state");
}

/** `device` = `null` remove a regra e limpa as rotas do executável. */
export function setAppRoute(exe: string, flow: Flow, device: string | null): Promise<void> {
  return invoke<void>("set_app_route", { exe, flow, device });
}

export function setSessionVolume(pid: number, flow: Flow, volume: number): Promise<void> {
  return invoke<void>("set_session_volume", { pid, flow, volume });
}

export function setSessionMuted(pid: number, flow: Flow, muted: boolean): Promise<void> {
  return invoke<void>("set_session_muted", { pid, flow, muted });
}

export function setDeviceVolume(id: string, volume: number): Promise<void> {
  return invoke<void>("set_device_volume", { id, volume });
}

export function setDeviceMuted(id: string, muted: boolean): Promise<void> {
  return invoke<void>("set_device_muted", { id, muted });
}

export function setDefaultDevice(id: string, flow: Flow): Promise<void> {
  return invoke<void>("set_default_device", { id, flow });
}

/** Nome vazio restaura o nome do Windows. */
export function updateDeviceMeta(
  id: string,
  name: string,
  icon: string | null,
  hidden: boolean,
): Promise<void> {
  return invoke<void>("update_device_meta", { id, name, icon, hidden });
}

export function saveProfile(profile: Profile): Promise<void> {
  return invoke<void>("save_profile", { profile });
}

export function deleteProfile(id: string): Promise<void> {
  return invoke<void>("delete_profile", { id });
}

export function activateProfile(id: string): Promise<void> {
  return invoke<void>("activate_profile", { id });
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function saveSettings(settings: Settings): Promise<void> {
  return invoke<void>("save_settings", { settings });
}

/** Assina `state-changed`. Devolve a função que cancela a assinatura. */
export function onStateChanged(callback: (state: AppState) => void): Promise<UnlistenFn> {
  return listen<AppState>("state-changed", (event) => callback(event.payload));
}

/** Assina `mic-level` (cerca de 15 vezes por segundo com o painel aberto). */
export function onMicLevel(callback: (level: MicLevel) => void): Promise<UnlistenFn> {
  return listen<MicLevel>("mic-level", (event) => callback(event.payload));
}

/** Stub até a Task 12: ainda não há ícone de app; devolve `null`. */
export async function appIcon(_exe: string): Promise<string | null> {
  return null;
}
