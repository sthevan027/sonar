// Ponto na engrenagem: acende quando há versão nova. Lê o que o Rust já guardou ao montar
// (a checagem automática pode ter rodado antes) e ouve o evento para as seguintes.

import { useEffect, useState } from "react";
import * as api from "../api";

export function useUpdateAvailable(): boolean {
  const [available, setAvailable] = useState(false);

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;

    api.pendingUpdate().then(
      (found) => {
        if (alive && found) setAvailable(true);
      },
      () => undefined,
    );
    api.onUpdateAvailable(() => {
      if (alive) setAvailable(true);
    }).then(
      (stop) => {
        if (alive) unlisten = stop;
        else stop();
      },
      () => undefined,
    );

    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  return available;
}
