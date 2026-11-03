// Estado do app para a interface. Assina `state-changed` ANTES de pedir `get_state`:
// um evento que chegue durante o pedido é mais novo que a resposta e vence.
// Antes do primeiro estado, `useAppState` suspende (lança a promessa), então nunca há um
// estado inventado na tela.

import { useSyncExternalStore } from "react";
import * as api from "./api";
import type { AppState } from "./types";

export interface AppStateSource {
  onStateChanged: (callback: (state: AppState) => void) => Promise<unknown>;
  getState: () => Promise<AppState>;
}

export function createAppStore(source: AppStateSource) {
  let current: AppState | null = null;
  let failure: unknown = null;
  let started = false;
  const listeners = new Set<() => void>();

  let resolveReady!: () => void;
  const ready = new Promise<void>((resolve) => {
    resolveReady = resolve;
  });

  function notify() {
    listeners.forEach((listener) => listener());
  }

  function start() {
    if (started) return;
    started = true;
    source
      .onStateChanged((state) => {
        current = state;
        resolveReady();
        notify();
      })
      .then(() => source.getState())
      .then(
        (state) => {
          // Se um evento já trouxe estado, a resposta de `get_state` pode ser mais velha.
          if (current === null) current = state;
          resolveReady();
          notify();
        },
        (error: unknown) => {
          if (current === null) failure = error;
          resolveReady();
          notify();
        },
      );
  }

  return {
    subscribe(listener: () => void) {
      start();
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getSnapshot(): AppState | null {
      return current;
    },
    /** Devolve o estado ou lança: a promessa (suspense) enquanto carrega, o erro se falhar. */
    read(): AppState {
      start();
      if (current) return current;
      if (failure) throw failure;
      throw ready;
    },
  };
}

const appStore = createAppStore({
  onStateChanged: api.onStateChanged,
  getState: api.getState,
});

export function useAppState(): AppState {
  useSyncExternalStore(appStore.subscribe, appStore.getSnapshot);
  return appStore.read();
}
