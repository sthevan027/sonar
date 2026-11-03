// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Chip } from "./Chip";

afterEach(() => cleanup());

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
    fireEvent.click(screen.getByRole("button", { name: "Caixa" }));
    expect(onPress).toHaveBeenCalledTimes(1);
  });
});
