// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(async (_command: string, _args?: unknown) => undefined),
  open: vi.fn(async (_options: unknown): Promise<string | null> => null),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => undefined) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));

import { pickExecutable } from "./api";

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
