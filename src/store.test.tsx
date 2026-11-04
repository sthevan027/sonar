import { describe, expect, it } from "vitest";
import { createAppStore } from "./store";
import type { AppState } from "./types";

function makeState(last: string | null): AppState {
  return {
    devices: [],
    sessions: [],
    rules: [],
    profiles: [],
    settings: { shortcut: "Super+Alt+V", autostart: false },
    routing_available: true,
    last_profile: last,
    save_error: null,
  };
}

function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

/** Espera até o store ficar pronto (o hook suspende jogando a promessa). */
async function waitReady(store: ReturnType<typeof createAppStore>): Promise<AppState> {
  for (;;) {
    try {
      return store.read();
    } catch (e) {
      if (e instanceof Promise) await e;
      else throw e;
    }
  }
}

describe("createAppStore", () => {
  it("store_subscribes_to_state_changed_before_get_state", async () => {
    const order: string[] = [];
    const store = createAppStore({
      onStateChanged: async () => {
        order.push("listen");
        return () => {};
      },
      getState: async () => {
        order.push("get_state");
        return makeState("A");
      },
    });
    await waitReady(store);
    expect(order).toEqual(["listen", "get_state"]);
  });

  it("store_event_received_during_get_state_wins_over_late_snapshot", async () => {
    let emit!: (s: AppState) => void;
    const pending = deferred<AppState>();
    const store = createAppStore({
      onStateChanged: async (cb) => {
        emit = cb;
        return () => {};
      },
      getState: () => pending.promise,
    });

    // Primeira leitura dispara o start; o evento chega antes do get_state responder.
    expect(() => store.read()).toThrow();
    await Promise.resolve();
    await Promise.resolve();
    emit(makeState("evento"));
    pending.resolve(makeState("snapshot antigo"));

    expect((await waitReady(store)).last_profile).toBe("evento");
  });

  it("store_uses_get_state_when_no_event_arrived", async () => {
    const store = createAppStore({
      onStateChanged: async () => () => {},
      getState: async () => makeState("snapshot"),
    });
    expect((await waitReady(store)).last_profile).toBe("snapshot");
  });

  it("store_failure_then_retry_reloads_without_resubscribing", async () => {
    let listens = 0;
    let calls = 0;
    const store = createAppStore({
      onStateChanged: async () => {
        listens += 1;
        return () => {};
      },
      getState: async () => {
        calls += 1;
        if (calls === 1) throw new Error("sem backend");
        return makeState("depois do retry");
      },
    });

    await expect(waitReady(store)).rejects.toThrow("sem backend");

    store.retry();
    expect((await waitReady(store)).last_profile).toBe("depois do retry");
    expect(calls).toBe(2);
    expect(listens).toBe(1);
  });

  it("store_retries_subscription_when_listen_failed", async () => {
    let listens = 0;
    const store = createAppStore({
      onStateChanged: async () => {
        listens += 1;
        if (listens === 1) throw new Error("evento indisponível");
        return () => {};
      },
      getState: async () => makeState("ok"),
    });

    await expect(waitReady(store)).rejects.toThrow("evento indisponível");

    store.retry();
    expect((await waitReady(store)).last_profile).toBe("ok");
    expect(listens).toBe(2);
  });
});
