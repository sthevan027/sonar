import { useEffect, useState } from "react";
import { appIcon } from "../api";

/**
 * Primeira letra do executável, sem `.exe`, em maiúscula. Conta pontos de código
 * (Array.from), para não partir letras acentuadas nem emoji.
 */
export function initialOf(exe: string): string {
  const name = exe.trim().replace(/\.exe$/i, "");
  const first = Array.from(name)[0];
  return first ? first.toUpperCase() : "?";
}

/**
 * Ícone do app (vindo de `api.appIcon`). Sem ícone, mostra só a inicial num bloco neutro:
 * nunca uma marca inventada.
 */
export function AppLogo({ exe }: { exe: string }) {
  const [src, setSrc] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setSrc(null);
    appIcon(exe)
      .then((icon) => {
        if (alive) setSrc(icon);
      })
      .catch(() => {
        if (alive) setSrc(null);
      });
    return () => {
      alive = false;
    };
  }, [exe]);

  if (src) {
    return <img className="logo" src={src} alt="" aria-hidden="true" />;
  }
  return (
    <span className="logo logo-init" aria-hidden="true">
      {initialOf(exe)}
    </span>
  );
}
