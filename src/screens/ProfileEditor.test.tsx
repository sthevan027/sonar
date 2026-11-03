// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { AppState, AudioSession, DeviceView, Profile, RunningApp } from "../types";
import { AppPicker } from "./AppPicker";
import { ProfileEditor } from "./ProfileEditor";

// Estado controlado pelo teste: o store real assina eventos do Tauri.
const fx = vi.hoisted(() => ({ state: null as unknown as AppState }));

vi.mock("../store", () => ({
  useAppState: () => fx.state,
}));

vi.mock("../api", () => ({
  appIcon: vi.fn(async () => null),
  saveProfile: vi.fn(async () => undefined),
  deleteProfile: vi.fn(async () => undefined),
  listRunningApps: vi.fn(async (): Promise<RunningApp[]> => []),
  pickExecutable: vi.fn(async () => null),
}));

const apps: RunningApp[] = [
  { exe: "spotify.exe", name: "Spotify" },
  { exe: "discord.exe", name: "Discord" },
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

function session(exe: string, flow: "render" | "capture", pid: number): AudioSession {
  return { pid, exe, flow, device: "x", active: true, volume: 1, muted: false };
}

const jogo: Profile = {
  id: "jogo",
  icon: "gamepad-2",
  name: "Jogo",
  items: [
    { exe: "spotify.exe" },
    { exe: "discord.exe", output: "out-headset", input: "default" },
    { exe: "chrome.exe", output: "default" },
  ],
};

function stateWith(overrides: Partial<AppState> = {}): AppState {
  return {
    devices: [
      device({ id: "out-caixa", windows_name: "Alto-falantes", display_name: "Caixa", is_default: true }),
      device({ id: "out-headset", windows_name: "Fone", display_name: "Headset", icon: "headphones" }),
      device({
        id: "in-headset",
        flow: "capture",
        windows_name: "Microfone do headset",
        display_name: "Mic do headset",
        is_default: true,
      }),
    ],
    sessions: [
      session("spotify.exe", "render", 10),
      session("discord.exe", "render", 20),
      session("discord.exe", "capture", 20),
    ],
    rules: [],
    profiles: [jogo],
    settings: { shortcut: "Super+Alt+V", autostart: false },
    routing_available: true,
    last_profile: null,
    save_error: null,
    ...overrides,
  };
}

beforeEach(() => {
  fx.state = stateWith();
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function nameField(): HTMLInputElement {
  return screen.getByLabelText("Nome (opcional)") as HTMLInputElement;
}

describe("ProfileEditor", () => {
  it("editor_rejects_name_over_20_chars", () => {
    render(<ProfileEditor onDone={vi.fn()} />);
    const save = screen.getByRole("button", { name: "Salvar perfil" }) as HTMLButtonElement;

    fireEvent.change(nameField(), { target: { value: "a".repeat(20) } });
    expect(screen.getByText("20 / 20")).toBeTruthy();
    expect(save.disabled).toBe(false);

    fireEvent.change(nameField(), { target: { value: "a".repeat(21) } });
    const counter = screen.getByText("21 / 20");
    expect(counter.className).toContain("over");
    expect(save.disabled).toBe(true);
  });

  it("editor_rejects_name_over_20_chars_counting_code_points", () => {
    render(<ProfileEditor onDone={vi.fn()} />);
    // 20 emoji são 20 pontos de código (Array.from), mesmo com 40 unidades UTF-16.
    fireEvent.change(nameField(), { target: { value: "😀".repeat(20) } });
    expect(screen.getByText("20 / 20")).toBeTruthy();
    expect(
      (screen.getByRole("button", { name: "Salvar perfil" }) as HTMLButtonElement).disabled,
    ).toBe(false);
  });

  it("editor_item_target_mapping", () => {
    render(<ProfileEditor profileId="jogo" onDone={vi.fn()} />);

    // Chips de saída e de mic, na ordem dos itens: spotify (saída), discord (saída), discord (mic), chrome (saída).
    const chips = () =>
      Array.from(document.querySelectorAll<HTMLButtonElement>('button[aria-haspopup="menu"]'));

    // Spotify: manter -> Caixa.
    fireEvent.click(chips()[0]);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Caixa" }));

    // Discord: Headset -> manter atual (campo some do JSON).
    fireEvent.click(chips()[1]);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Manter atual" }));

    fireEvent.click(screen.getByRole("button", { name: "Salvar perfil" }));

    expect(api.saveProfile).toHaveBeenCalledTimes(1);
    const saved = JSON.parse(JSON.stringify(vi.mocked(api.saveProfile).mock.calls[0][0]));
    expect(saved).toEqual({
      id: "jogo",
      icon: "gamepad-2",
      name: "Jogo",
      items: [
        { exe: "spotify.exe", output: "out-caixa" },
        { exe: "discord.exe", input: "default" },
        { exe: "chrome.exe", output: "default" },
      ],
    });
    expect(saved.items[1]).not.toHaveProperty("output");
  });

  it("editor_hides_mic_chip_for_apps_without_capture_session", () => {
    // Spotify só tem sessão de saída; Discord tem sessão de captura.
    fx.state = stateWith({
      profiles: [{ id: "chat", icon: "music", items: [{ exe: "spotify.exe" }, { exe: "discord.exe" }] }],
    });
    const { container } = render(<ProfileEditor profileId="chat" onDone={vi.fn()} />);

    const rows = container.querySelectorAll(".ed-pr");
    expect(rows).toHaveLength(2);
    expect(rows[0].querySelectorAll('[data-kind="mic"]')).toHaveLength(0);
    expect(rows[1].querySelectorAll('[data-kind="mic"]')).toHaveLength(1);
  });

  it("create_path_adds_apps_routes_output_and_mic_and_saves", async () => {
    fx.state = stateWith({ profiles: [] });
    vi.mocked(api.listRunningApps)
      .mockResolvedValueOnce(apps)
      .mockResolvedValueOnce(apps);
    const onDone = vi.fn();
    const { container } = render(<ProfileEditor onDone={onDone} />);

    // Spotify: só saída. Sem sessão de mic, o chip de mic não aparece.
    fireEvent.click(screen.getByRole("button", { name: "Adicionar app" }));
    fireEvent.click(await screen.findByRole("button", { name: "Spotify" }));
    const chips = () =>
      Array.from(container.querySelectorAll<HTMLButtonElement>('button[aria-haspopup="menu"]'));
    expect(chips()).toHaveLength(1);
    fireEvent.click(chips()[0]);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Caixa" }));

    // Discord: tem sessão de captura, então ganha o chip de mic.
    fireEvent.click(screen.getByRole("button", { name: "Adicionar app" }));
    fireEvent.click(await screen.findByRole("button", { name: "Discord" }));
    expect(container.querySelectorAll('[data-kind="mic"]')).toHaveLength(1);
    fireEvent.click(chips()[1]);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Headset" }));
    fireEvent.click(chips()[2]);
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Mic do headset" }));

    fireEvent.click(screen.getByRole("button", { name: "Salvar perfil" }));
    await vi.waitFor(() => expect(onDone).toHaveBeenCalledTimes(1));

    const saved = JSON.parse(JSON.stringify(vi.mocked(api.saveProfile).mock.calls[0][0]));
    expect(saved).toEqual({
      id: expect.any(String),
      icon: "gamepad-2",
      items: [
        { exe: "spotify.exe", output: "out-caixa" },
        { exe: "discord.exe", output: "out-headset", input: "in-headset" },
      ],
    });
  });

  it("delete_needs_inline_confirmation_before_calling_api", () => {
    render(<ProfileEditor profileId="jogo" onDone={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Excluir perfil" }));
    expect(api.deleteProfile).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Excluir" }));
    expect(api.deleteProfile).toHaveBeenCalledWith("jogo");
  });
});

describe("AppPicker", () => {
  it("filters the running apps by name or exe as the user types", async () => {
    vi.mocked(api.listRunningApps).mockResolvedValueOnce(apps);
    render(<AppPicker exclude={[]} onPick={vi.fn()} onCancel={vi.fn()} />);

    expect(await screen.findByRole("button", { name: "Spotify" })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Buscar app"), { target: { value: "dis" } });

    const list = screen.getByRole("list", { name: "Apps abertos" });
    expect(within(list).queryByRole("button", { name: "Spotify" })).toBeNull();
    expect(within(list).getByRole("button", { name: "Discord" })).toBeTruthy();
  });
});
