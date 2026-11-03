/**
 * Telas do app. Cada tela nova entra como outro membro da união e um `case` em `Screen`
 * (App.tsx).
 */
export type PanelTab = "output" | "mic";

/** Aba de Dispositivos de áudio: saída (`render`) ou entrada (`capture`). */
export type DeviceTab = "render" | "capture";

/**
 * `returnTab` volta o painel para a aba de onde a tela foi aberta (Cancelar, Voltar, Salvar).
 * `profileId` ausente = perfil novo. Dispositivos e Configurações voltam para a tela que as
 * abriu (Configurações, no caso de Dispositivos) e, no fim, para o painel.
 */
export type Route =
  | { name: "panel"; tab: PanelTab }
  | { name: "profileEditor"; profileId?: string; returnTab: PanelTab }
  | { name: "settings"; returnTab: PanelTab }
  | { name: "devices"; tab: DeviceTab; returnTab: PanelTab };
