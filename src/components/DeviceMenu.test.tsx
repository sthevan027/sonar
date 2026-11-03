// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DeviceView } from "../types";
import { DeviceMenu } from "./DeviceMenu";

afterEach(() => cleanup());

const devices: DeviceView[] = [
  {
    id: "c",
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
  {
    id: "h",
    flow: "render",
    windows_name: "Fone",
    name: null,
    display_name: "Headset",
    icon: "headphones",
    hidden: false,
    connected: true,
    is_default: false,
    has_rules: true,
    volume: 1,
    muted: false,
  },
];

function renderMenu(onSelect = vi.fn(), selectedId: string | null = "c") {
  render(
    <DeviceMenu
      kind="device"
      tone="sage"
      label="Caixa"
      menuLabel="Saída do Spotify"
      devices={devices}
      selectedId={selectedId}
      onSelect={onSelect}
    />,
  );
  return onSelect;
}

describe("DeviceMenu", () => {
  it("device_menu_opens_lists_devices_and_default_item", () => {
    renderMenu();
    expect(screen.queryByRole("menu")).toBeNull();

    const trigger = screen.getByRole("button", { name: "Caixa" });
    expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
    fireEvent.click(trigger);

    expect(trigger.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("menu", { name: "Saída do Spotify" })).toBeTruthy();
    const items = screen.getAllByRole("menuitemradio");
    expect(items.map((i) => i.textContent)).toEqual(["Padrão do Windows", "Caixa", "Headset"]);
    expect(screen.getByRole("menuitemradio", { name: "Caixa" }).getAttribute("aria-checked")).toBe("true");
    // Foco vai para a opção marcada ao abrir.
    expect(document.activeElement).toBe(screen.getByRole("menuitemradio", { name: "Caixa" }));
  });

  it("device_menu_escape_closes_and_returns_focus_to_chip", () => {
    renderMenu();
    const trigger = screen.getByRole("button", { name: "Caixa" });
    fireEvent.click(trigger);

    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("device_menu_arrow_keys_move_focus_between_items", () => {
    renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    const menu = screen.getByRole("menu");
    const [dflt, caixa, headset] = screen.getAllByRole("menuitemradio");

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(headset);
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(dflt);
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(document.activeElement).toBe(headset);
    fireEvent.keyDown(menu, { key: "Home" });
    expect(document.activeElement).toBe(dflt);
    expect(caixa).toBeTruthy();
  });

  it("device_menu_selecting_device_calls_onSelect_and_closes", () => {
    const onSelect = renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Headset" }));
    expect(onSelect).toHaveBeenCalledWith("h");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("device_menu_default_item_calls_onSelect_with_null", () => {
    const onSelect = renderMenu(vi.fn(), "h");
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Padrão do Windows" }));
    expect(onSelect).toHaveBeenCalledWith(null);
  });
});
