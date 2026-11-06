// Wrappers tipados dos comandos do Tauri (`src-tauri/src/commands.rs`) e dos eventos.
// Os nomes dos argumentos são os do Rust; o Tauri converte camelCase para snake_case.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  Action,
  ActionResult,
  AppState,
  Binding,
  BindingStatusEntry,
  Flow,
  MicLevel,
  Profile,
  RunningApp,
  Settings,
  UpdateInfo,
  UpdateProgress,
} from "./types";

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

/** Rejeita com a frase do Rust (em português) quando o atalho ou o "Iniciar com o Windows" falham; nada é gravado nesse caso. */
export function saveSettings(settings: Settings): Promise<void> {
  return invoke<void>("save_settings", { settings });
}

/** Assina `state-changed`. Devolve a função que cancela a assinatura. */
export function onStateChanged(callback: (state: AppState) => void): Promise<UnlistenFn> {
  return listen<AppState>("state-changed", (event) => callback(event.payload));
}

/**
 * Estado de registro dos bindings: uma entrada por binding, na mesma ordem de
 * `settings.bindings`. O acelerador viaja com o estado para o painel casar status x binding
 * sem depender da ordem (Ruling 41).
 */
export function getBindingStatus(): Promise<BindingStatusEntry[]> {
  return invoke<BindingStatusEntry[]>("get_binding_status");
}

/**
 * Grava a lista inteira de bindings. O Rust valida a lista toda, registra e só então
 * persiste; em erro, rejeita com a frase em português e não grava nada.
 */
export function saveBindings(bindings: Binding[]): Promise<BindingStatusEntry[]> {
  return invoke<BindingStatusEntry[]>("save_bindings", { bindings });
}

/** Executa uma ação na hora, sem passar pela fila de atalho. */
export function dispatchAction(action: Action): Promise<ActionResult> {
  return invoke<ActionResult>("dispatch_action", { action });
}

/** Assina `bindings-status`. Devolve a função que cancela a assinatura. */
export function onBindingsStatus(
  callback: (status: BindingStatusEntry[]) => void,
): Promise<UnlistenFn> {
  return listen<BindingStatusEntry[]>("bindings-status", (event) => callback(event.payload));
}

/** Assina `action-result`: o resultado de uma ação disparada por atalho. */
export function onActionResult(callback: (result: ActionResult) => void): Promise<UnlistenFn> {
  return listen<ActionResult>("action-result", (event) => callback(event.payload));
}

/** Assina `mic-level` (cerca de 15 vezes por segundo com o painel aberto). */
export function onMicLevel(callback: (level: MicLevel) => void): Promise<UnlistenFn> {
  return listen<MicLevel>("mic-level", (event) => callback(event.payload));
}

/** Apps com janela visível, um por executável. */
export function listRunningApps(): Promise<RunningApp[]> {
  return invoke<RunningApp[]>("list_running_apps");
}

/**
 * Ícone do app como data URL PNG, ou `null` quando não há. Cache em memória por exe:
 * resultado `null` não fica guardado (um app aberto depois ganha o ícone numa nova tentativa).
 */
const iconCache = new Map<string, Promise<string | null>>();

export function appIcon(exe: string): Promise<string | null> {
  const key = exe.trim().toLowerCase();
  if (!key) return Promise.resolve(null);
  let pending = iconCache.get(key);
  if (!pending) {
    pending = invoke<string | null>("app_icon", { exe: key }).catch(() => null);
    iconCache.set(key, pending);
    void pending.then((icon) => {
      if (icon === null) iconCache.delete(key);
    });
  }
  return pending;
}

/**
 * Procura versão nova no endpoint configurado. Devolve `null` quando já está na mais recente.
 * Rejeita com a frase do Rust (sem rede, sem release, assinatura inválida).
 */
export function checkForUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("check_for_update");
}

/** Versão já encontrada nesta execução, ou `null`. A engrenagem consulta ao montar. */
export function pendingUpdate(): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>("pending_update");
}

/**
 * Baixa e instala. No Windows o app fecha e reabre na versão nova, então a promessa pode nunca
 * resolver: quem chama deve manter "instalando" sem tempo limite. Rejeita só antes do instalador.
 */
export function installUpdate(): Promise<void> {
  return invoke<void>("install_update");
}

/** Assina `update-available` (checagem automática). Devolve a função que cancela a assinatura. */
export function onUpdateAvailable(callback: (info: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("update-available", (event) => callback(event.payload));
}

/** Assina `update-progress` durante o download. */
export function onUpdateProgress(callback: (progress: UpdateProgress) => void): Promise<UnlistenFn> {
  return listen<UpdateProgress>("update-progress", (event) => callback(event.payload));
}

/** Enquanto o diálogo de arquivo está aberto, o painel não se esconde por perda de foco. */
export function setBlurHideSuspended(suspended: boolean): Promise<void> {
  return invoke<void>("set_blur_hide_suspended", { suspended });
}

/** Diálogo "Procurar .exe". Devolve o caminho escolhido, ou `null` se cancelou. */
export async function pickExecutable(): Promise<string | null> {
  await setBlurHideSuspended(true);
  try {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Programas", extensions: ["exe"] }],
    });
    return typeof picked === "string" ? picked : null;
  } finally {
    await setBlurHideSuspended(false);
  }
}
