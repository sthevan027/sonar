// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Tabs } from "./Tabs";

afterEach(() => cleanup());

const items = [
  { id: "out", label: "Saída", icon: <span>o</span> },
  { id: "mic", label: "Microfone", icon: <span>m</span> },
  { id: "app", label: "Apps", icon: <span>a</span> },
];

describe("Tabs", () => {
  it("tabs_roving_focus_and_arrow_keys", () => {
    const onChange = vi.fn();
    render(<Tabs items={items} active="out" onChange={onChange} />);
    const tabs = screen.getAllByRole("tab");

    // Só a aba ativa entra na ordem de Tab.
    expect(tabs.map((t) => t.tabIndex)).toEqual([0, -1, -1]);
    expect(tabs[0].getAttribute("aria-selected")).toBe("true");
    expect(tabs[1].getAttribute("aria-selected")).toBe("false");

    tabs[0].focus();
    fireEvent.keyDown(tabs[0], { key: "ArrowRight" });
    expect(onChange).toHaveBeenLastCalledWith("mic");
    expect(document.activeElement).toBe(tabs[1]);

    fireEvent.keyDown(tabs[0], { key: "ArrowLeft" });
    expect(onChange).toHaveBeenLastCalledWith("app");
    expect(document.activeElement).toBe(tabs[2]);

    fireEvent.keyDown(tabs[0], { key: "End" });
    expect(onChange).toHaveBeenLastCalledWith("app");

    fireEvent.keyDown(tabs[0], { key: "Home" });
    expect(onChange).toHaveBeenLastCalledWith("out");
    expect(document.activeElement).toBe(tabs[0]);
  });

  it("tabs_click_selects_tab", () => {
    const onChange = vi.fn();
    render(<Tabs items={items} active="out" onChange={onChange} />);
    fireEvent.click(screen.getByRole("tab", { name: /Microfone/ }));
    expect(onChange).toHaveBeenCalledWith("mic");
  });
});
