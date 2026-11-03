// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Chip } from "./Chip";
import { clampPercent } from "./Slider";

afterEach(() => cleanup());

describe("clampPercent", () => {
  it("slider_clamps_to_0_100", () => {
    expect(clampPercent(-5)).toBe(0);
    expect(clampPercent(150)).toBe(100);
    expect(clampPercent(42)).toBe(42);
    expect(clampPercent(0)).toBe(0);
    expect(clampPercent(100)).toBe(100);
    expect(clampPercent(Number.NaN)).toBe(0);
  });
});

describe("Chip", () => {
  it("chip_renders_label_and_tone_class", () => {
    const { container, unmount } = render(<Chip kind="device" tone="brass" label="Headset" />);
    const brass = container.querySelector(".chip") as HTMLElement;
    expect(brass.textContent).toBe("Headset");
    expect(brass.classList.contains("hs")).toBe(true);
    expect(brass.classList.contains("sp")).toBe(false);
    unmount();

    const { container: sage } = render(<Chip kind="mic" tone="sage" label="Microfone" />);
    const chip = sage.querySelector(".chip") as HTMLElement;
    expect(chip.textContent).toBe("Microfone");
    expect(chip.classList.contains("sp")).toBe(true);
    expect(chip.classList.contains("hs")).toBe(false);
  });

  it("chip_calls_onPress_on_click", () => {
    const onPress = vi.fn();
    render(<Chip kind="device" tone="brass" label="Caixa" onPress={onPress} />);
    const button = screen.getByRole("button", { name: "Caixa" });
    expect(button.getAttribute("aria-haspopup")).toBe("menu");
    fireEvent.click(button);
    expect(onPress).toHaveBeenCalledTimes(1);
  });
});
