// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { AppState, DeviceView } from "../types";
import { Devices } from "./Devices";
import { visibleDevices } from "./devicesModel";
import { OutputTab } from "./OutputTab";

// Estado controlado pelo teste: o store real assina eventos do Tauri.
const fx = vi.hoisted(() => ({ state: null as unknown as AppState }));

vi.mock("../store", () => ({
  useAppState: () => fx.state,
}));

vi.mock("../api", () => ({
  appIcon: vi.fn(async () => null),
  updateDeviceMeta: vi.fn(async () => undefined),
  setDefaultDevice: vi.fn(async () => undefined),
  onMicLevel: vi.fn(async () => () => undefined),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

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

function setDevices(devices: DeviceView[]) {
  fx.state = {
    devices,
    sessions: [],
    rules: [],
    profiles: [],
    settings: { shortcut: "Super+Alt+V", autostart: false },
    routing_available: true,
    last_profile: null,
    save_error: null,
  };
}

function renderDevices(tab: "render" | "capture" = "render") {
  render(<Devices tab={tab} onTabChange={vi.fn()} onBack={vi.fn()} />);
}

describe("Devices", () => {
  it("device_name_counter_and_limit", async () => {
    setDevices([
      device({ id: "c1", display_name: "Caixa", windows_name: "Alto-falantes", is_default: true }),
    ]);
    renderDevices();
    fireEvent.click(screen.getByRole("button", { name: "Editar Caixa" }));

    const input = screen.getByLabelText("Nome no painel") as HTMLInputElement;
    expect(input.value).toBe("");
    fireEvent.change(input, { target: { value: "a".repeat(21) } });
    expect(screen.getByText("21 / 20")).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("no máximo 20 caracteres");

    fireEvent.keyDown(input, { key: "Enter" });
    expect(api.updateDeviceMeta).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "a".repeat(20) } });
    expect(screen.getByText("20 / 20")).toBeTruthy();
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() =>
      expect(api.updateDeviceMeta).toHaveBeenCalledWith("c1", "a".repeat(20), null, false),
    );
  });

  it("restore_original_name_clears_custom_name", async () => {
    setDevices([
      device({
        id: "c1",
        name: "Caixa",
        display_name: "Caixa",
        windows_name: "Alto-falantes",
        icon: "headphones",
      }),
    ]);
    renderDevices();
    fireEvent.click(screen.getByRole("button", { name: "Editar Caixa" }));
    fireEvent.click(screen.getByRole("button", { name: "Restaurar nome original do Windows" }));

    // Nome vazio é o contrato de restaurar; ícone e oculto seguem os do cartão.
    await waitFor(() =>
      expect(api.updateDeviceMeta).toHaveBeenCalledWith("c1", "", "headphones", false),
    );
    expect(api.updateDeviceMeta).toHaveBeenCalledTimes(1);
    expect((screen.getByLabelText("Nome no painel") as HTMLInputElement).value).toBe("");
  });

  it("hide_eye_keeps_name_and_icon", async () => {
    setDevices([
      device({ id: "m", display_name: "Monitor", windows_name: "NVIDIA HDMI", icon: "monitor" }),
    ]);
    renderDevices();
    fireEvent.click(screen.getByRole("button", { name: "Ocultar Monitor no painel" }));
    await waitFor(() =>
      expect(api.updateDeviceMeta).toHaveBeenCalledWith("m", "", "monitor", true),
    );
  });

  it("hidden_device_not_listed_in_panel_chips", () => {
    const devices = [
      device({ id: "caixa", display_name: "Caixa" }),
      device({ id: "monitor", display_name: "Monitor", hidden: true }),
      device({ id: "buds", display_name: "Galaxy Buds", connected: false, has_rules: true }),
      device({ id: "mic", flow: "capture", display_name: "Mic" }),
    ];
    expect(visibleDevices(devices).map((d) => d.id)).toEqual(["caixa", "mic"]);
  });

  it("hidden_device_not_listed_in_output_chips", () => {
    setDevices([
      device({ id: "caixa", display_name: "Caixa", is_default: true }),
      device({ id: "monitor", display_name: "Monitor", hidden: true }),
    ]);
    render(<OutputTab state={fx.state} />);
    expect(screen.getByRole("radio", { name: "Caixa" })).toBeTruthy();
    expect(screen.queryByRole("radio", { name: "Monitor" })).toBeNull();
  });

  it("disconnected_device_shows_rules_kept_label", () => {
    setDevices([
      device({
        id: "buds",
        display_name: "Galaxy Buds",
        windows_name: "Fones Bluetooth",
        connected: false,
        has_rules: true,
      }),
      device({
        id: "cam",
        display_name: "Webcam",
        windows_name: "Logitech C920",
        connected: false,
        has_rules: false,
      }),
    ]);
    renderDevices();
    expect(screen.getByText("Desconectado · regras guardadas")).toBeTruthy();
    expect(screen.getByText("Desconectado")).toBeTruthy();
  });

  it("mic_meter_unlistens_on_unmount", async () => {
    const unlisten = vi.fn();
    vi.mocked(api.onMicLevel).mockResolvedValueOnce(unlisten);
    setDevices([
      device({
        id: "m",
        flow: "capture",
        display_name: "Mic do headset",
        windows_name: "Microfone",
        icon: "headphones",
        is_default: true,
      }),
    ]);
    const { unmount } = render(
      <Devices tab="capture" onTabChange={vi.fn()} onBack={vi.fn()} />,
    );
    await waitFor(() => expect(api.onMicLevel).toHaveBeenCalledTimes(1));
    await act(async () => undefined);
    expect(unlisten).not.toHaveBeenCalled();

    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
