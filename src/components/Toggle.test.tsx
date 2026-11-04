// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Toggle } from "./Toggle";

afterEach(() => cleanup());

describe("Toggle", () => {
  it("toggle_click_flips_value_and_aria_checked", () => {
    const onChange = vi.fn();
    const { rerender } = render(
      <Toggle on={false} onChange={onChange} label="Iniciar com o Windows" />,
    );
    const sw = screen.getByRole("switch", { name: "Iniciar com o Windows" });
    expect(sw.getAttribute("aria-checked")).toBe("false");
    expect(sw.classList.contains("off")).toBe(true);

    fireEvent.click(sw);
    expect(onChange).toHaveBeenLastCalledWith(true);

    rerender(<Toggle on onChange={onChange} label="Iniciar com o Windows" />);
    expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("true");
    fireEvent.click(screen.getByRole("switch"));
    expect(onChange).toHaveBeenLastCalledWith(false);
  });
});
