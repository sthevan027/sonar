// Escolha de saída/mic por app com resposta imediata. Ao escolher, o chip já mostra o
// dispositivo novo (sem esperar o `state-changed`). A escolha some quando um estado chega
// com a regra salva igual a ela; se o comando falhar, volta ao que era e mostra o aviso.
// Escolha que chega depois de outra do mesmo app vale mais: a falha da antiga não mexe nela.

import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../api";
import type { AppState, Flow } from "../types";
import { isRouteSettled, routeKey, type PendingRoute } from "./model";

export type PendingRoutes = ReadonlyMap<string, PendingRoute>;

export interface RoutePending {
  pending: PendingRoutes;
  /** Mostra a escolha já, envia o comando e, se ele falhar, desfaz e chama `onFail`. */
  choose: (exe: string, flow: Flow, device: string | null, onFail: () => void) => void;
}

/** Tira as escolhas que o estado já reflete. Devolve o mesmo mapa se nada mudou. */
export function settlePending(prev: PendingRoutes, state: AppState): PendingRoutes {
  let next: Map<string, PendingRoute> | null = null;
  prev.forEach((entry, key) => {
    if (!isRouteSettled(state, entry.exe, entry.flow, entry.device)) return;
    next ??= new Map(prev);
    next.delete(key);
  });
  return next ?? prev;
}

export function useRoutePending(state: AppState): RoutePending {
  const [pending, setPending] = useState<PendingRoutes>(() => new Map());
  const sequence = useRef(0);

  // Cada estado que chega confirma as escolhas que ele já reflete.
  useEffect(() => {
    setPending((prev) => settlePending(prev, state));
  }, [state]);

  const choose = useCallback(
    (exe: string, flow: Flow, device: string | null, onFail: () => void) => {
      sequence.current += 1;
      const id = sequence.current;
      const key = routeKey(exe, flow);
      setPending((prev) => new Map(prev).set(key, { id, exe, flow, device }));
      Promise.resolve(api.setAppRoute(exe, flow, device)).then(undefined, () => {
        setPending((prev) => {
          if (prev.get(key)?.id !== id) return prev;
          const next = new Map(prev);
          next.delete(key);
          return next;
        });
        onFail();
      });
    },
    [],
  );

  return { pending, choose };
}
