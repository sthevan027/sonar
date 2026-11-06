// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type { AppState } from "./types";

// Rotas de Configurações e Dispositivos: a aba do painel sobrevive ao caminho de volta.
const state: AppState = {
  devices: [],
  sessions: [],
  rules: [],
  profiles: [],
  settings: { shortcut: "Super+Alt+V", autostart: false },
  routing_available: true,
  last_profile: null,
  save_error: null,
};

vi.mock("./api", () => ({
  getState: vi.fn(async () => state),
  onStateChanged: vi.fn(async () => () => {}),
  getSettings: vi.fn(async () => ({ shortcut: "Super+Alt+V", autostart: false })),
  getDefaultBindings: vi.fn(async () => []),
  saveSettings: vi.fn(async () => undefined),
  onMicLevel: vi.fn(async () => () => {}),
  appIcon: vi.fn(async () => null),
  pendingUpdate: vi.fn(async () => null),
  onUpdateAvailable: vi.fn(async () => () => {}),
  onUpdateProgress: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(async () => null),
  installUpdate: vi.fn(async () => undefined),
}));

afterEach(() => cleanup());

describe("navegação do painel", () => {
  it("voltar de dispositivos e de configurações mantém a aba Microfone", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("tab", { name: "Microfone" }));

    fireEvent.click(screen.getByRole("button", { name: "Configurações" }));
    fireEvent.click(await screen.findByRole("button", { name: /Dispositivos de áudio/ }));
    expect(await screen.findByText("Dispositivos de áudio", { selector: "b" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Voltar" }));
    expect(await screen.findByRole("button", { name: /Dispositivos de áudio/ })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Voltar" }));
    const mic = await screen.findByRole("tab", { name: "Microfone" });
    expect(mic.getAttribute("aria-selected")).toBe("true");
  });
});
