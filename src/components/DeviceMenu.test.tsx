// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DeviceView } from "../types";
import { chooseSide, DeviceMenu } from "./DeviceMenu";

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

function renderMenu(onSelect = vi.fn(), selectedId: string | null = "c", list = devices) {
  render(
    <DeviceMenu
      kind="device"
      tone="sage"
      label="Caixa"
      menuLabel="Saída do Spotify"
      devices={list}
      selectedId={selectedId}
      onSelect={onSelect}
    />,
  );
  return onSelect;
}

describe("chooseSide", () => {
  it("device_menu_opens_down_when_it_fits", () => {
    expect(
      chooseSide({ triggerTop: 300, triggerBottom: 330, menuHeight: 200, boundsTop: 0, boundsBottom: 668 }),
    ).toBe("down");
  });

  it("device_menu_flips_up_near_bottom_edge", () => {
    expect(
      chooseSide({ triggerTop: 600, triggerBottom: 630, menuHeight: 200, boundsTop: 0, boundsBottom: 668 }),
    ).toBe("up");
  });

  it("device_menu_stays_down_when_neither_side_is_better", () => {
    expect(
      chooseSide({ triggerTop: 20, triggerBottom: 50, menuHeight: 200, boundsTop: 0, boundsBottom: 100 }),
    ).toBe("down");
  });
});

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

  it("device_menu_escape_closes_from_chip_too", () => {
    renderMenu();
    const trigger = screen.getByRole("button", { name: "Caixa" });
    fireEvent.click(trigger);
    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("device_menu_arrow_down_on_chip_opens_and_focuses_checked_item", () => {
    renderMenu();
    const trigger = screen.getByRole("button", { name: "Caixa" });
    trigger.focus();
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    expect(screen.getByRole("menu")).toBeTruthy();
    expect(document.activeElement).toBe(screen.getByRole("menuitemradio", { name: "Caixa" }));
  });

  it("device_menu_arrow_keys_move_focus_and_roving_tabindex", () => {
    renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    const menu = screen.getByRole("menu");
    const [dflt, caixa, headset] = screen.getAllByRole("menuitemradio");

    // Só o item ativo entra na ordem de Tab.
    expect([dflt, caixa, headset].map((i) => i.tabIndex)).toEqual([-1, 0, -1]);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(headset);
    expect(headset.tabIndex).toBe(0);
    expect(caixa.tabIndex).toBe(-1);

    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toBe(dflt);
    fireEvent.keyDown(menu, { key: "ArrowUp" });
    expect(document.activeElement).toBe(headset);
    fireEvent.keyDown(menu, { key: "Home" });
    expect(document.activeElement).toBe(dflt);
    fireEvent.keyDown(menu, { key: "End" });
    expect(document.activeElement).toBe(headset);
  });

  it("device_menu_tab_closes_the_menu", () => {
    renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Tab" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("device_menu_blur_outside_closes_but_pointer_inside_does_not", () => {
    renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    const menu = screen.getByRole("menu");
    const item = screen.getByRole("menuitemradio", { name: "Caixa" });

    // Clique no respiro do menu: o foco sai do item, mas o menu continua aberto.
    fireEvent.pointerDown(menu);
    fireEvent.focusOut(item, { relatedTarget: null });
    expect(screen.queryByRole("menu")).not.toBeNull();

    // Depois de soltar o botão, sair do menu fecha de novo.
    fireEvent.pointerUp(document);
    fireEvent.focusOut(item, { relatedTarget: null });
    expect(screen.queryByRole("menu")).toBeNull();
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

  it("device_menu_marks_hidden_device_in_label", () => {
    const hidden: DeviceView = { ...devices[1], id: "hh", display_name: "Fone antigo", hidden: true };
    renderMenu(vi.fn(), "hh", [hidden]);
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    expect(screen.getByRole("menuitemradio", { name: "Fone antigo (oculto)" }).getAttribute("aria-checked")).toBe(
      "true",
    );
  });
});

describe("DeviceMenu guard and clamp", () => {
  it("device_menu_pointercancel_releases_pointer_guard", () => {
    renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    const menu = screen.getByRole("menu");
    const item = screen.getByRole("menuitemradio", { name: "Caixa" });

    fireEvent.pointerDown(menu);
    document.dispatchEvent(new Event("pointercancel"));
    fireEvent.focusOut(item, { relatedTarget: null });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("device_menu_close_resets_pointer_guard", () => {
    renderMenu();
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    // Botão preso dentro do menu, depois o menu fecha pelo Esc sem pointerup.
    fireEvent.pointerDown(screen.getByRole("menu"));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    const item = screen.getByRole("menuitemradio", { name: "Caixa" });
    fireEvent.focusOut(item, { relatedTarget: null });
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("device_menu_clamps_active_item_when_devices_shrink", () => {
    const { rerender } = render(
      <DeviceMenu kind="device" tone="sage" label="Caixa" menuLabel="Saída" devices={devices} selectedId="c" onSelect={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "End" });
    expect(screen.getByRole("menuitemradio", { name: "Headset" }).tabIndex).toBe(0);

    // A lista encolhe com o menu aberto: o item ativo vai para o último que existe.
    rerender(
      <DeviceMenu kind="device" tone="sage" label="Caixa" menuLabel="Saída" devices={[devices[0]]} selectedId="c" onSelect={vi.fn()} />,
    );
    const items = screen.getAllByRole("menuitemradio");
    expect(items).toHaveLength(2);
    expect(items.map((i) => i.tabIndex)).toEqual([-1, 0]);
  });
});
