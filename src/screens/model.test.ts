import { describe, expect, it } from "vitest";
import { routeFor, selectableDevices } from "./model";
import type { AppState, DeviceView } from "../types";

function dev(overrides: Partial<DeviceView>): DeviceView {
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

const headset = dev({ id: "h", display_name: "Headset" });
const hiddenHeadset = dev({ id: "hh", display_name: "Fone antigo", hidden: true });
const offMonitor = dev({ id: "old", display_name: "Monitor antigo", connected: false });

function state(rules: AppState["rules"], devices: DeviceView[]): AppState {
  return {
    devices,
    sessions: [],
    rules,
    profiles: [],
    settings: { shortcut: "Super+Alt+V", autostart: false },
    routing_available: true,
    last_profile: null,
    save_error: null,
  };
}

describe("routeFor", () => {
  it("route_without_rule_is_windows_default", () => {
    expect(routeFor(state([], [headset]), "spotify.exe", "render")).toEqual({
      label: "Padrão do Windows",
      selectedId: null,
      warning: null,
    });
  });

  it("route_to_connected_device_shows_it_checked", () => {
    const info = routeFor(
      state([{ exe: "Spotify.exe", flow: "render", device: "h" }], [headset]),
      "spotify.exe",
      "render",
    );
    expect(info).toEqual({ label: "Headset", selectedId: "h", warning: null });
  });

  it("route_to_disconnected_device_falls_back_with_warning", () => {
    const info = routeFor(
      state([{ exe: "discord.exe", flow: "render", device: "old" }], [offMonitor]),
      "discord.exe",
      "render",
    );
    expect(info.label).toBe("Padrão do Windows");
    expect(info.selectedId).toBeNull();
    expect(info.warning).toBe("Monitor antigo desconectado: usando o padrão do Windows");
  });

  it("route_to_hidden_connected_device_keeps_it_checked_and_listed", () => {
    const rules = [{ exe: "game.exe", flow: "render" as const, device: "hh" }];
    const devices = [headset, hiddenHeadset];
    const info = routeFor(state(rules, devices), "game.exe", "render");
    expect(info).toEqual({ label: "Fone antigo", selectedId: "hh", warning: null });

    const menu = selectableDevices(devices, "render", info.selectedId);
    expect(menu.map((d) => d.id)).toEqual(["h", "hh"]);
    // Sem rota para ele, o oculto some do menu.
    expect(selectableDevices(devices, "render", null).map((d) => d.id)).toEqual(["h"]);
  });
});
