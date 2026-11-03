// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { AppState, AudioSession, DeviceView } from "../types";
import { groupSessions } from "./model";
import { OutputTab } from "./OutputTab";

vi.mock("../api", () => ({
  appIcon: vi.fn(async () => null),
  setAppRoute: vi.fn(async () => undefined),
  setSessionVolume: vi.fn(async () => undefined),
  setDeviceVolume: vi.fn(async () => undefined),
  setDefaultDevice: vi.fn(async () => undefined),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  vi.useRealTimers();
});

function session(
  pid: number,
  exe: string,
  overrides: Partial<AudioSession> = {},
): AudioSession {
  return { pid, exe, flow: "render", device: "c", active: true, volume: 0.5, muted: false, ...overrides };
}

describe("groupSessions", () => {
  it("groupSessions_merges_pids_of_same_exe", () => {
    const { active, silent } = groupSessions([
      session(30, "chrome.exe"),
      session(31, "chrome.exe", { volume: 0.2 }),
      session(32, "chrome.exe", { active: false }),
    ]);
    expect(silent).toEqual([]);
    expect(active).toHaveLength(1);
    expect(active[0].exe).toBe("chrome.exe");
    expect(active[0].pids).toEqual([30, 31, 32]);
    expect(active[0].active).toBe(true);
    expect(active[0].volume).toBe(0.5);
  });

  it("groupSessions_puts_inactive_in_silent", () => {
    const { active, silent } = groupSessions([
      session(10, "spotify.exe"),
      session(40, "obs.exe", { active: false }),
    ]);
    expect(active.map((r) => r.exe)).toEqual(["spotify.exe"]);
    expect(silent.map((r) => r.exe)).toEqual(["obs.exe"]);
    expect(silent[0].active).toBe(false);
  });

  it("groupSessions_ignores_capture_sessions_in_output_tab", () => {
    const { active, silent } = groupSessions([
      session(50, "discord.exe", { flow: "capture" }),
      session(20, "discord.exe"),
    ]);
    expect(active.map((r) => r.pids)).toEqual([[20]]);
    expect(silent).toEqual([]);
  });
});

const devices: DeviceView[] = [
  device({ id: "c", windows_name: "Alto-falantes", display_name: "Caixa", is_default: true, volume: 0.62 }),
  device({ id: "h", windows_name: "Fone", display_name: "Headset", icon: "headphones", volume: 0.8 }),
  device({
    id: "old",
    windows_name: "Monitor",
    display_name: "Monitor antigo",
    connected: false,
    has_rules: true,
  }),
  device({
    id: "m",
    flow: "capture",
    windows_name: "Microfone do headset",
    display_name: "Mic do headset",
    is_default: true,
  }),
];

function device(overrides: Partial<DeviceView>): DeviceView {
  return {
    id: "x",
    flow: "render",
    windows_name: "Dispositivo",
    name: null,
    display_name: "Dispositivo",
    icon: null,
    hidden: false,
    connected: true,
    is_default: false,
    has_rules: false,
    volume: 1,
    muted: false,
    ...overrides,
  };
}

function makeState(overrides: Partial<AppState> = {}): AppState {
  return {
    devices,
    sessions: [
      session(10, "spotify.exe", { device: "c", volume: 0.48 }),
      session(20, "discord.exe", { device: "h", volume: 0.8 }),
      session(30, "chrome.exe", { device: "c", volume: 0.35 }),
      session(31, "chrome.exe", { device: "c", volume: 0.35 }),
      session(40, "obs.exe", { active: false, volume: 1 }),
      session(50, "discord.exe", { flow: "capture" }),
    ],
    rules: [
      { exe: "spotify.exe", flow: "render", device: "h" },
      { exe: "discord.exe", flow: "render", device: "old" },
    ],
    profiles: [],
    settings: { shortcut: "Super+Alt+V", autostart: false },
    routing_available: true,
    last_profile: null,
    save_error: null,
    ...overrides,
  };
}

describe("OutputTab", () => {
  it("output_tab_renders_default_output_apps_and_collapsed_silent_row", () => {
    render(<OutputTab state={makeState()} />);

    expect(screen.getByText("Caixa")).toBeTruthy();
    expect(screen.getByText("Spotify")).toBeTruthy();
    expect(screen.getByText("Discord")).toBeTruthy();
    expect(screen.getAllByText("Chrome")).toHaveLength(1);
    expect(screen.getByText("3 tocando")).toBeTruthy();

    // Spotify tem regra para o Headset conectado; Discord aponta para um dispositivo desligado.
    expect(screen.getByRole("button", { name: "Headset" })).toBeTruthy();
    expect(screen.getByText("Monitor antigo desconectado: usando o padrão do Windows")).toBeTruthy();

    // Em silêncio começa recolhida.
    const toggle = screen.getByRole("button", { name: /Em silêncio/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByText("Obs")).toBeNull();
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("Obs")).toBeTruthy();
  });

  it("output_tab_routes_app_to_chosen_device", () => {
    render(<OutputTab state={makeState()} />);
    fireEvent.click(screen.getByRole("button", { name: "Headset" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Caixa" }));
    expect(api.setAppRoute).toHaveBeenCalledWith("spotify.exe", "render", "c");
  });

  it("output_tab_volume_is_debounced_and_applies_to_every_pid", () => {
    vi.useFakeTimers();
    render(<OutputTab state={makeState()} />);
    const slider = screen.getByRole("slider", { name: "Volume do Chrome" });

    fireEvent.change(slider, { target: { value: "40" } });
    vi.advanceTimersByTime(30);
    fireEvent.change(slider, { target: { value: "45" } });
    expect(api.setSessionVolume).not.toHaveBeenCalled();

    vi.advanceTimersByTime(60);
    expect(api.setSessionVolume).toHaveBeenCalledTimes(2);
    expect(api.setSessionVolume).toHaveBeenCalledWith(30, "render", 0.45);
    expect(api.setSessionVolume).toHaveBeenCalledWith(31, "render", 0.45);
  });

  it("output_tab_empty_states_without_apps_or_devices", () => {
    render(<OutputTab state={makeState({ sessions: [], devices: [] })} />);
    expect(screen.getByText(/Nenhum app com som agora/)).toBeTruthy();
    expect(screen.getByText("Nenhuma saída conectada")).toBeTruthy();
  });
});

