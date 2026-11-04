// @vitest-environment jsdom
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { AppState, DeviceView, MicLevel } from "../types";
import { litSegments, MicTab, peakToDb } from "./MicTab";

vi.mock("../api", () => ({
  appIcon: vi.fn(async () => null),
  onMicLevel: vi.fn(),
  setAppRoute: vi.fn(async () => undefined),
  setDeviceVolume: vi.fn(async () => undefined),
  setDeviceMuted: vi.fn(async () => undefined),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

const mic: DeviceView = {
  id: "m",
  flow: "capture",
  windows_name: "Microfone (Redragon)",
  name: null,
  display_name: "Mic do headset",
  icon: "headphones",
  hidden: false,
  connected: true,
  is_default: true,
  has_rules: false,
  volume: 0.78,
  muted: false,
};

function makeState(): AppState {
  return {
    devices: [mic],
    sessions: [
      { pid: 60, exe: "discord.exe", flow: "capture", device: "m", active: true, volume: 1, muted: false },
    ],
    rules: [],
    profiles: [],
    settings: { shortcut: "Super+Alt+V", autostart: false },
    routing_available: true,
    last_profile: null,
    save_error: null,
  };
}

describe("MicTab", () => {
  it("mic_tab_unsubscribes_mic_level_on_unmount", async () => {
    const unlisten = vi.fn();
    // A assinatura só termina quando a promessa de `listen` resolve: esperamos ela antes de desmontar.
    let finishListen!: (stop: () => void) => void;
    vi.mocked(api.onMicLevel).mockReturnValue(new Promise((resolve) => (finishListen = resolve)));

    const { unmount } = render(<MicTab state={makeState()} />);
    await waitFor(() => expect(api.onMicLevel).toHaveBeenCalledTimes(1));
    await act(async () => finishListen(unlisten));
    expect(unlisten).not.toHaveBeenCalled();

    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("mic_tab_unlistens_when_unmounted_before_listen_resolves", async () => {
    const unlisten = vi.fn();
    let finishListen!: (stop: () => void) => void;
    vi.mocked(api.onMicLevel).mockReturnValue(new Promise((resolve) => (finishListen = resolve)));

    const { unmount } = render(<MicTab state={makeState()} />);
    unmount();
    await act(async () => finishListen(unlisten));
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("mic_tab_renders_default_mic_and_apps_using_it", async () => {
    vi.mocked(api.onMicLevel).mockResolvedValue(() => undefined);
    render(<MicTab state={makeState()} />);

    expect(screen.getByText("Mic do headset")).toBeTruthy();
    expect(screen.getByText("Discord")).toBeTruthy();
    expect(screen.getByText("1 agora")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Microfone ligado/ })).toBeTruthy();
  });

  it("mic_tab_meter_maps_peak_to_db_and_segments", () => {
    expect(peakToDb(0)).toBe(-60);
    expect(peakToDb(0.125)).toBeCloseTo(-18.06, 1);
    expect(litSegments(0)).toBe(24);
    expect(litSegments(-60)).toBe(0);
    expect(litSegments(-120)).toBe(0);
  });

  it("mic_tab_ignores_levels_from_other_devices", async () => {
    let emit: ((level: MicLevel) => void) | undefined;
    vi.mocked(api.onMicLevel).mockImplementation(async (cb) => {
      emit = cb;
      return () => undefined;
    });
    const { container } = render(<MicTab state={makeState()} />);
    await waitFor(() => expect(emit).toBeDefined());

    act(() => emit?.({ id: "outro", peak: 0.9 }));
    expect(container.querySelectorAll(".meter i.l").length).toBe(0);
    act(() => emit?.({ id: "m", peak: 0.9 }));
    expect(container.querySelectorAll(".meter i.l").length).toBeGreaterThan(0);
  });
});
