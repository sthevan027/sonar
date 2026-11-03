// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as api from "./api";
import type { AppState } from "./types";

const state: AppState = {
  devices: [
    {
      id: "out-1",
      flow: "render",
      windows_name: "Alto-falantes",
      name: null,
      display_name: "Caixa",
      icon: null,
      hidden: false,
      connected: true,
      is_default: true,
      has_rules: false,
      volume: 1,
      muted: false,
    },
  ],
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
  pendingUpdate: vi.fn(async () => null),
  onUpdateAvailable: vi.fn(async () => () => {}),
  onUpdateProgress: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(async () => null),
  installUpdate: vi.fn(async () => undefined),
}));

afterEach(() => cleanup());

describe("App", () => {
  it("mostra a falha de get_state e carrega ao tentar de novo", async () => {
    vi.mocked(api.getState).mockRejectedValueOnce(new Error("sem backend"));
    render(<App />);
    expect(await screen.findByText("Não foi possível carregar")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Tentar de novo" }));
    expect(await screen.findByText("Caixa")).toBeTruthy();
    expect(screen.getByText("Saída padrão")).toBeTruthy();
  });

  it("renderiza a saída padrão no painel", async () => {
    render(<App />);
    expect(await screen.findByText("Caixa")).toBeTruthy();
    expect(screen.getByText("Saída padrão")).toBeTruthy();
  });

  it("cancelar no editor volta para a aba de onde ele abriu", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("tab", { name: "Microfone" }));

    fireEvent.click(screen.getByRole("button", { name: "Novo perfil" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancelar" }));

    const mic = await screen.findByRole("tab", { name: "Microfone" });
    expect(mic.getAttribute("aria-selected")).toBe("true");
  });
});
