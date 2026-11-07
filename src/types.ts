// Espelha a serialização do Rust: `src-tauri/src/state.rs`, `audio/types.rs`, `rules.rs`
// e `profiles.rs`. Chaves em snake_case; `Flow` vira texto minúsculo; `DeviceId` vira texto.

export type Flow = "render" | "capture";

/**
 * Destino de um item de perfil. `"default"` = padrão do Windows; qualquer outro texto =
 * id do dispositivo. No item, o campo ausente quer dizer "manter a rota atual".
 */
export type Target = string;

export interface DeviceView {
  id: string;
  flow: Flow;
  windows_name: string;
  /** Nome personalizado, ou `null` para mostrar o nome do Windows. */
  name: string | null;
  /** Nome personalizado ou, na falta dele, o nome do Windows. */
  display_name: string;
  icon: string | null;
  hidden: boolean;
  connected: boolean;
  is_default: boolean;
  has_rules: boolean;
  /** Volume mestre do endpoint, 0.0 a 1.0. `1` quando desconectado ou sem leitura. */
  volume: number;
  /** `false` quando desconectado ou sem leitura. */
  muted: boolean;
}

export interface AudioSession {
  pid: number;
  exe: string;
  flow: Flow;
  device: string;
  active: boolean;
  volume: number;
  muted: boolean;
}

export interface Rule {
  exe: string;
  flow: Flow;
  device: string;
}

export interface ProfileItem {
  exe: string;
  /** Ausente = manter; `"default"` = padrão do Windows; outro texto = id do dispositivo. */
  output?: Target;
  /** Mesma regra de `output`. */
  input?: Target;
}

export interface Profile {
  id: string;
  icon: string;
  /** Ausente quando o perfil não tem nome. */
  name?: string;
  items: ProfileItem[];
}

/** App com janela aberta (`list_running_apps`): exe em minúsculas e nome amigável. */
export interface RunningApp {
  exe: string;
  name: string;
}

export interface Settings {
  shortcut: string;
  autostart: boolean;
  /**
   * Tabela de atalhos globais (`Settings.bindings` do Rust). Opcional: ausente = "o
   * frontend não mexe", e o Rust mantém a lista que já tem.
   */
  bindings?: Binding[];
}

/**
 * Ações globais: espelha `Action` de `src-tauri/src/actions.rs`. Enum com tag `type` em
 * snake_case; `Direction` vira `"next"` / `"prev"`.
 */
export type Direction = "next" | "prev";

export type Action =
  | { type: "toggle_panel" }
  | { type: "activate_profile"; profile_id: string }
  | { type: "cycle_profile"; direction: Direction }
  | { type: "set_mic_mute"; muted: boolean }
  | { type: "toggle_mic_mute" }
  | { type: "set_output_mute"; muted: boolean }
  | { type: "toggle_output_mute" }
  | { type: "step_output_volume"; delta: number }
  | { type: "set_output_volume"; value: number }
  | { type: "set_app_mute"; exe: string; muted: boolean }
  | { type: "move_app"; exe: string; device: string | null }
  | { type: "cycle_default_output"; direction: Direction };

/** Um atalho: a combinação de teclas e a ação que ela dispara. */
export interface Binding {
  accelerator: string;
  action: Action;
}

/**
 * Estado de um binding: `active` (registrado) ou `unavailable` com o motivo. A forma é
 * fixa pela Ruling 41 — enum com tag `type` em snake_case.
 */
export type BindingStatus =
  | { type: "active" }
  | { type: "unavailable"; message: string };

/**
 * Uma entrada de estado por binding. O `accelerator` viaja junto do estado para a tela casar
 * status x binding sem depender da ordem (a ordem muda quando um binding sai do meio).
 */
export type BindingStatusEntry = {
  accelerator: string;
  status: BindingStatus;
};

/** Motivo de uma falha, em código estável (`ErrorCode` do Rust, em snake_case). */
export type ErrorCode =
  | "profile_not_found"
  | "no_profiles"
  | "no_output_device"
  | "no_input_device"
  | "not_enough_outputs"
  | "device_gone"
  | "unsupported"
  | "os_error"
  | "invalid_exe"
  | "invalid_value"
  | "queue_full"
  | "internal";

/**
 * Resultado de uma ação: as mesmas cinco chaves do Rust, sempre presentes.
 *
 * `ok == false` **não** implica `changed == false`: existe a forma parcial (o estado mudou,
 * mas o pedido não foi cumprido inteiro) e a `message` diz as duas coisas.
 */
export interface ActionResult {
  ok: boolean;
  changed: boolean;
  resolved: string | null;
  error_code: ErrorCode | null;
  message: string | null;
}

export interface AppState {
  devices: DeviceView[];
  sessions: AudioSession[];
  rules: Rule[];
  profiles: Profile[];
  settings: Settings;
  /** Falso quando o roteamento por app não é suportado neste Windows. */
  routing_available: boolean;
  /** Último perfil ativado nesta execução (não persistido). */
  last_profile: string | null;
  /** Mensagem quando a última gravação em disco falhou; `null` quando está tudo gravado. */
  save_error: string | null;
}

/** Versão nova do Sonar (`check_for_update`, `pending_update`, evento `update-available`). */
export interface UpdateInfo {
  version: string;
  /** Notas da versão em texto puro; pode vir vazio. */
  notes: string;
  /** Publicação em segundos desde 1970 (UTC), ou `null`. */
  date: number | null;
}

/** Payload do evento `update-progress`. `total` é `null` quando o servidor não informa. */
export interface UpdateProgress {
  downloaded: number;
  total: number | null;
}

/** Payload do evento `mic-level`. */
export interface MicLevel {
  id: string;
  peak: number;
}

/**
 * Converte a escolha da interface para o destino do item de perfil. `"keep"` vira
 * `undefined`, que some do JSON e faz o Rust manter a rota atual.
 */
export function toTarget(choice: "keep" | "default" | string): Target | undefined {
  return choice === "keep" ? undefined : choice;
}
