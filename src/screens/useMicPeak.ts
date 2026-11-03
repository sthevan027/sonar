import { useEffect, useState } from "react";
import * as api from "../api";

/**
 * Pico (0 a 1) do microfone `micId` em tempo real. Assina `mic-level` só enquanto há um id e
 * cancela ao sair. A assinatura que chega depois da desmontagem é cancelada na hora (StrictMode
 * monta, desmonta e monta de novo, então isso importa).
 */
export function useMicPeak(micId: string | null): number {
  const [peak, setPeak] = useState(0);

  useEffect(() => {
    if (micId === null) return;
    let alive = true;
    let unlisten: (() => void) | undefined;
    api
      .onMicLevel((level) => {
        if (alive && level.id === micId) setPeak(level.peak);
      })
      .then((stop) => {
        if (alive) unlisten = stop;
        else stop();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
      setPeak(0);
    };
  }, [micId]);

  return peak;
}
