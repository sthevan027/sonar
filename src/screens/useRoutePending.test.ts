import { describe, expect, it, vi } from "vitest";
import type { AppState, DeviceView, Rule } from "../types";
import { pendingRouteInfo, isRouteSettled, routeKey, type PendingRoute } from "./model";
import { settlePending, type PendingRoutes } from "./useRoutePending";

vi.mock("../api", () => ({
  setAppRoute: vi.fn(async () => undefined),
}));

function device(id: string, overrides: Partial<DeviceView> = {}): DeviceView {
  return {
    id,
    flow: "render",
    windows_name: id,
    name: null,
    display_name: id.toUpperCase(),
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

function state(rules: Rule[], devices: DeviceView[] = [device("c"), device("h")]): AppState {
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

function pendingMap(...entries: PendingRoute[]): PendingRoutes {
  return new Map(entries.map((e) => [routeKey(e.exe, e.flow), e]));
}

describe("routeKey", () => {
  it("routeKey_lowercases_exe_and_keeps_flow", () => {
    expect(routeKey("Spotify.EXE", "render")).toBe("render|spotify.exe");
    expect(routeKey("Spotify.EXE", "capture")).not.toBe(routeKey("Spotify.EXE", "render"));
  });
});

describe("pendingRouteInfo", () => {
  it("pending_device_is_shown_without_disconnected_warning", () => {
    const info = pendingRouteInfo(state([]), "render", "h");
    expect(info).toEqual({ label: "H", selectedId: "h", warning: null });
  });

  it("pending_default_shows_windows_default", () => {
    expect(pendingRouteInfo(state([]), "render", null)).toEqual({
      label: "Padrão do Windows",
      selectedId: null,
      warning: null,
    });
  });
});

describe("isRouteSettled", () => {
  it("route_is_settled_when_rule_device_matches_and_null_means_no_rule", () => {
    const withRule = state([{ exe: "spotify.exe", flow: "render", device: "c" }]);
    expect(isRouteSettled(withRule, "Spotify.exe", "render", "c")).toBe(true);
    expect(isRouteSettled(withRule, "spotify.exe", "render", "h")).toBe(false);
    expect(isRouteSettled(withRule, "spotify.exe", "render", null)).toBe(false);
    expect(isRouteSettled(state([]), "spotify.exe", "render", null)).toBe(true);
  });
});

describe("settlePending", () => {
  const spotifyToCaixa: PendingRoute = { id: 1, exe: "spotify.exe", flow: "render", device: "c" };

  it("settle_removes_only_entries_the_state_already_reflects", () => {
    const pending = pendingMap(
      spotifyToCaixa,
      { id: 2, exe: "discord.exe", flow: "render", device: "h" },
    );
    const next = settlePending(
      pending,
      state([{ exe: "spotify.exe", flow: "render", device: "c" }]),
    );
    expect([...next.keys()]).toEqual(["render|discord.exe"]);
  });

  it("settle_keeps_the_same_map_when_nothing_matches", () => {
    const pending = pendingMap(spotifyToCaixa);
    expect(settlePending(pending, state([]))).toBe(pending);
  });
});
