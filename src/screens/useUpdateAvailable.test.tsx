// @vitest-environment jsdom
import { cleanup, renderHook, waitFor, act } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { UpdateInfo } from "../types";
import { useUpdateAvailable } from "./useUpdateAvailable";

const listeners = vi.hoisted(() => ({ onFound: null as null | ((info: UpdateInfo) => void) }));

vi.mock("../api", () => ({
  pendingUpdate: vi.fn(async () => null),
  onUpdateAvailable: vi.fn(async (callback: (info: UpdateInfo) => void) => {
    listeners.onFound = callback;
    return () => {};
  }),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  listeners.onFound = null;
});

describe("useUpdateAvailable", () => {
  it("fica apagado quando não há versão nova", async () => {
    const { result } = renderHook(() => useUpdateAvailable());
    await waitFor(() => expect(api.onUpdateAvailable).toHaveBeenCalled());
    expect(result.current).toBe(false);
  });

  it("acende quando a checagem automática já achou algo antes de montar", async () => {
    vi.mocked(api.pendingUpdate).mockResolvedValueOnce({
      version: "0.2.0",
      notes: "",
      date: null,
    });
    const { result } = renderHook(() => useUpdateAvailable());
    await waitFor(() => expect(result.current).toBe(true));
  });

  it("acende quando o evento update-available chega depois", async () => {
    const { result } = renderHook(() => useUpdateAvailable());
    await waitFor(() => expect(listeners.onFound).not.toBeNull());
    act(() => listeners.onFound?.({ version: "0.2.0", notes: "", date: null }));
    expect(result.current).toBe(true);
  });
});
