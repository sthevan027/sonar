// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(async (_command: string, _args?: unknown): Promise<unknown> => undefined),
  open: vi.fn(async (_options: unknown): Promise<string | null> => null),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));

import {
  dispatchAction,
  getBindingStatus,
  getDefaultBindings,
  pickExecutable,
  saveBindings,
} from "./api";

afterEach(() => {
  vi.clearAllMocks();
});

function suspendCalls() {
  return mocks.invoke.mock.calls.filter(([command]) => command === "set_blur_hide_suspended");
}

function lastSuspendCall() {
  const calls = suspendCalls();
  return calls[calls.length - 1];
}

describe("pickExecutable", () => {
  it("suspende o esconder por foco antes do diálogo e retoma depois", async () => {
    mocks.open.mockResolvedValueOnce("C:\\Jogos\\game.exe");

    await expect(pickExecutable()).resolves.toBe("C:\\Jogos\\game.exe");

    expect(suspendCalls()).toEqual([
      ["set_blur_hide_suspended", { suspended: true }],
      ["set_blur_hide_suspended", { suspended: false }],
    ]);
    const [on, off] = mocks.invoke.mock.invocationCallOrder.filter((_, i) =>
      mocks.invoke.mock.calls[i][0] === "set_blur_hide_suspended",
    );
    const opened = mocks.open.mock.invocationCallOrder[0];
    expect(on).toBeLessThan(opened);
    expect(opened).toBeLessThan(off);
  });

  it("retoma a suspensão mesmo quando o diálogo falha", async () => {
    mocks.open.mockRejectedValueOnce(new Error("diálogo quebrou"));

    await expect(pickExecutable()).rejects.toThrow("diálogo quebrou");

    expect(lastSuspendCall()).toEqual(["set_blur_hide_suspended", { suspended: false }]);
  });

  it("devolve null quando o usuário cancela e ainda retoma a suspensão", async () => {
    mocks.open.mockResolvedValueOnce(null);

    await expect(pickExecutable()).resolves.toBeNull();

    expect(lastSuspendCall()).toEqual(["set_blur_hide_suspended", { suspended: false }]);
  });
});

describe("getBindingStatus", () => {
  it("chama o comando get_binding_status sem argumentos", async () => {
    const status = [{ accelerator: "Ctrl+Alt+M", status: { type: "active" as const } }];
    mocks.invoke.mockResolvedValueOnce(status);

    await expect(getBindingStatus()).resolves.toEqual(status);
    expect(mocks.invoke).toHaveBeenCalledWith("get_binding_status");
  });
});

describe("getDefaultBindings", () => {
  it("chama o comando get_default_bindings sem argumentos", async () => {
    const bindings = [{ accelerator: "Super+Alt+V", action: { type: "toggle_panel" as const } }];
    mocks.invoke.mockResolvedValueOnce(bindings);

    await expect(getDefaultBindings()).resolves.toEqual(bindings);
    expect(mocks.invoke).toHaveBeenCalledWith("get_default_bindings");
  });
});

describe("saveBindings", () => {
  it("manda a lista inteira no campo bindings", async () => {
    const bindings = [
      { accelerator: "Super+Alt+V", action: { type: "toggle_panel" as const } },
      {
        accelerator: "Ctrl+Alt+1",
        action: { type: "activate_profile" as const, profile_id: "p1" },
      },
    ];
    const status = [{ accelerator: "Ctrl+Alt+1", status: { type: "active" as const } }];
    mocks.invoke.mockResolvedValueOnce(status);

    await expect(saveBindings(bindings)).resolves.toEqual(status);
    expect(mocks.invoke).toHaveBeenCalledWith("save_bindings", { bindings });
  });
});

describe("dispatchAction", () => {
  it("manda a ação no campo action", async () => {
    const result = {
      ok: true,
      changed: true,
      resolved: null,
      error_code: null,
      message: null,
    };
    mocks.invoke.mockResolvedValueOnce(result);

    await expect(dispatchAction({ type: "toggle_panel" })).resolves.toEqual(result);
    expect(mocks.invoke).toHaveBeenCalledWith("dispatch_action", {
      action: { type: "toggle_panel" },
    });
  });
});
