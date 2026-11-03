// @vitest-environment jsdom
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "./App";
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
}));

afterEach(() => cleanup());

describe("App", () => {
  it("renderiza o nome Sonar e a saída padrão", async () => {
    render(<App />);
    expect(await screen.findByText("Caixa")).toBeTruthy();
    expect(screen.getByText("Sonar")).toBeTruthy();
  });
});
