// Estado do app para a interface. Assina `state-changed` ANTES de pedir `get_state`:
// um evento que chegue durante o pedido é mais novo que a resposta e vence.
// Antes do primeiro estado, `useAppState` suspende (lança a promessa). Se o carregamento
// falha, lança o erro (o ErrorBoundary mostra a mensagem) e `retry` tenta de novo: refaz
// só o `get_state`, sem reassinar o evento se a assinatura já deu certo.

import { useSyncExternalStore } from "react";
import * as api from "./api";
import type { AppState } from "./types";

export interface AppStateSource {
  onStateChanged: (callback: (state: AppState) => void) => Promise<unknown>;
  getState: () => Promise<AppState>;
}

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

export function createAppStore(source: AppStateSource) {
  let current: AppState | null = null;
  let failure: unknown = null;
  let hasFailure = false;
  let listening: Promise<void> | null = null;
  let loading = false;
  let started = false;
  let ready = deferred();
  const listeners = new Set<() => void>();

  function notify() {
    listeners.forEach((listener) => listener());
  }

  function listen(): Promise<void> {
    if (!listening) {
      listening = source
        .onStateChanged((state) => {
          current = state;
          hasFailure = false;
          failure = null;
          ready.resolve();
          notify();
        })
        .then(
          () => undefined,
          (error: unknown) => {
            // Sem assinatura, a próxima tentativa assina de novo.
            listening = null;
            throw error;
          },
        );
    }
    return listening;
  }

  function load() {
    if (loading) return;
    loading = true;
    failure = null;
    hasFailure = false;
    ready = deferred();
    listen()
      .then(() => source.getState())
      .then(
        (state) => {
          if (current === null) current = state;
        },
        (error: unknown) => {
          if (current === null) {
            failure = error;
            hasFailure = true;
          }
        },
      )
      .then(() => {
        loading = false;
        ready.resolve();
        notify();
      });
  }

  function start() {
    if (started) return;
    started = true;
    load();
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
      if (hasFailure) throw failure;
      throw ready.promise;
    },
    /** Tenta o carregamento de novo depois de uma falha. Sem efeito se já há estado ou carga em curso. */
    retry() {
      if (current) return;
      load();
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

/** Usado pelo botão "Tentar de novo" do ErrorBoundary. */
export function retryAppState() {
  appStore.retry();
}
