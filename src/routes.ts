/**
 * Telas do app. Cada tela nova entra como outro membro da união e um `case` em `Screen`
 * (App.tsx); as próximas, dispositivos e configurações.
 */
export type PanelTab = "output" | "mic";

/**
 * `returnTab` volta o painel para a aba de onde o editor foi aberto (Cancelar e Salvar).
 * `profileId` ausente = perfil novo.
 */
export type Route =
  | { name: "panel"; tab: PanelTab }
  | { name: "profileEditor"; profileId?: string; returnTab: PanelTab };
