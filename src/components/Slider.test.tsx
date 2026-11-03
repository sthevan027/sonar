// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Slider } from "./Slider";

afterEach(() => cleanup());

describe("Slider", () => {
  it("slider_keyboard_focus_and_change", () => {
    const onChange = vi.fn();
    render(<Slider value={40.4} onChange={onChange} label="Volume do Spotify" />);
    const input = screen.getByRole("slider", { name: "Volume do Spotify" }) as HTMLInputElement;

    // Valor exibido arredondado depois do clamp.
    expect(input.value).toBe("40");
    expect(screen.getByText("40")).toBeTruthy();

    // Setas nativas do range não são simuladas pelo jsdom; aqui checamos foco e a mudança.
    input.focus();
    expect(document.activeElement).toBe(input);
    fireEvent.change(input, { target: { value: "41" } });
    expect(onChange).toHaveBeenLastCalledWith(41);
  });
});
