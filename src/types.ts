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
