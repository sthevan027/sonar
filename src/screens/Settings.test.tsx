// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { ToastProvider } from "../components/Toast";
import type { AppState, Settings as AppSettings } from "../types";
import { Settings } from "./Settings";
import { shortcutFromKeys, shortcutKeys } from "./settingsModel";

const fx = vi.hoisted(() => ({ state: null as unknown as AppState }));

vi.mock("../store", () => ({
  useAppState: () => fx.state,
}));

vi.mock("../api", () => ({
  getSettings: vi.fn(),
  saveSettings: vi.fn(async () => undefined),
}));

function settings(overrides: Partial<AppSettings> = {}): AppSettings {
  return { shortcut: "Super+Alt+V", autostart: false, ...overrides };
}

function setup(initial: AppSettings = settings()) {
  vi.mocked(api.getSettings).mockResolvedValue(initial);
  fx.state = {
    devices: [],
    sessions: [],
    rules: [],
    profiles: [],
    settings: initial,
    routing_available: true,
    last_profile: null,
    save_error: null,
  };
  const handlers = { onBack: vi.fn(), onOpenDevices: vi.fn(), onOpenProfiles: vi.fn() };
  render(
    <ToastProvider>
      <Settings {...handlers} />
    </ToastProvider>,
  );
  return handlers;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("Settings", () => {
  it("toggle_autostart_calls_save_settings", async () => {
    setup();
    const toggle = await screen.findByRole("switch", { name: "Iniciar com o Windows" });
    expect(toggle.getAttribute("aria-checked")).toBe("false");

    fireEvent.click(toggle);

    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith({ shortcut: "Super+Alt+V", autostart: true }),
    );
    expect(
      screen.getByRole("switch", { name: "Iniciar com o Windows" }).getAttribute("aria-checked"),
    ).toBe("true");
  });

  it("shortcut_capture_rejects_key_without_modifier_then_saves_combo", async () => {
    setup();
    const keys = await screen.findByRole("button", { name: /Atalho global: Win Alt V/ });
    fireEvent.click(keys);

    fireEvent.keyDown(keys, { key: "k", code: "KeyK" });
    const hint = screen.getByText("Inclua Ctrl, Alt ou Win.");
    // A dica é anunciada por leitor de tela (região viva, educada).
    expect(hint.getAttribute("aria-live")).toBe("polite");
    expect(api.saveSettings).not.toHaveBeenCalled();

    // Shift sozinho não conta como modificador de atalho global.
    fireEvent.keyDown(keys, { key: "k", code: "KeyK", shiftKey: true });
    expect(screen.getByText("Inclua Ctrl, Alt ou Win.")).toBeTruthy();
    expect(api.saveSettings).not.toHaveBeenCalled();

    fireEvent.keyDown(keys, { key: "k", code: "KeyK", ctrlKey: true });
    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith({ shortcut: "Ctrl+K", autostart: false }),
    );
  });

  it("double_toggle_ends_with_last_value_in_order", async () => {
    setup();
    const toggle = await screen.findByRole("switch", { name: "Iniciar com o Windows" });
    fireEvent.click(toggle);
    fireEvent.click(toggle);

    await waitFor(() => expect(api.saveSettings).toHaveBeenCalledTimes(2));
    // Cada gravação leva o pedido vigente; a última fica com o valor final (desligado).
    const calls = vi.mocked(api.saveSettings).mock.calls;
    expect(calls[calls.length - 1][0].autostart).toBe(false);
    expect(
      screen.getByRole("switch", { name: "Iniciar com o Windows" }).getAttribute("aria-checked"),
    ).toBe("false");
  });

  it("failure_rolls_back_only_the_failed_field", async () => {
    setup();
    // A primeira gravação (ligar o autostart) falha; a segunda (atalho) passa.
    vi.mocked(api.saveSettings).mockRejectedValueOnce(new Error("disco cheio"));
    fireEvent.click(await screen.findByRole("switch", { name: "Iniciar com o Windows" }));
    const keys = screen.getByRole("button", { name: /Atalho global/ });
    fireEvent.click(keys);
    fireEvent.keyDown(keys, { key: "k", code: "KeyK", ctrlKey: true });

    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenLastCalledWith({ shortcut: "Ctrl+K", autostart: false }),
    );
    expect(await screen.findByText("Não foi possível salvar a configuração.")).toBeTruthy();
    expect(
      screen.getByRole("switch", { name: "Iniciar com o Windows" }).getAttribute("aria-checked"),
    ).toBe("false");
    expect(screen.getByRole("button", { name: "Atalho global: Ctrl K. Toque para trocar." })).toBeTruthy();
  });

  it("restore_default_shortcut_link_saves_super_alt_v", async () => {
    setup(settings({ shortcut: "Ctrl+K" }));
    fireEvent.click(await screen.findByRole("button", { name: "Restaurar atalho padrão (Win Alt V)" }));
    await waitFor(() =>
      expect(api.saveSettings).toHaveBeenCalledWith({ shortcut: "Super+Alt+V", autostart: false }),
    );
  });

  it("shows_toast_when_save_settings_fails", async () => {
    setup();
    vi.mocked(api.saveSettings).mockRejectedValueOnce(new Error("disco cheio"));
    fireEvent.click(await screen.findByRole("switch", { name: "Iniciar com o Windows" }));
    expect(await screen.findByText("Não foi possível salvar a configuração.")).toBeTruthy();
  });

  it("shortcut_helpers_map_keys_and_validate_combo", () => {
    expect(shortcutKeys("Super+Alt+V")).toEqual(["Win", "Alt", "V"]);
    const none = { altKey: false, shiftKey: false, metaKey: false, ctrlKey: false };
    expect(shortcutFromKeys({ code: "ControlLeft", ...none, ctrlKey: true })).toEqual({
      status: "wait",
    });
    expect(
      shortcutFromKeys({ code: "Digit7", ctrlKey: false, altKey: true, shiftKey: true, metaKey: true }),
    ).toEqual({ status: "ok", shortcut: "Alt+Shift+Super+7" });
    expect(shortcutFromKeys({ code: "ArrowLeft", ...none, ctrlKey: true })).toMatchObject({
      status: "invalid",
    });
  });
});
