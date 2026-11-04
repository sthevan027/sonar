import type { ReactNode } from "react";

/** `edit` = contorno dourado (perfil em edição); `off` = apagado (desligado). */
export function Card({
  tone = "default",
  children,
}: {
  tone?: "default" | "edit" | "off";
  children: ReactNode;
}) {
  return <div className={tone === "default" ? "card" : `card ${tone}`}>{children}</div>;
}
