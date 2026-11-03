/**
 * Telas do app. Cada tela nova entra como outro membro da união e um `case` em `Screen`
 * (App.tsx): a Task 12 acrescenta o editor de perfil; as próximas, dispositivos e configurações.
 */
export type PanelTab = "output" | "mic";

export type Route = { name: "panel"; tab: PanelTab };
