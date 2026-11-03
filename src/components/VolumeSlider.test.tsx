// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { VOLUME_SETTLE_MS, VolumeSlider } from "./VolumeSlider";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function slider() {
  return screen.getByRole("slider", { name: "Volume do Spotify" }) as HTMLInputElement;
}

describe("VolumeSlider", () => {
  it("volume_draft_survives_stale_echo_then_settles", () => {
    const onCommit = vi.fn(async () => undefined);
    const { rerender } = render(
      <VolumeSlider label="Volume do Spotify" value={0.3} onCommit={onCommit} />,
    );

    fireEvent.change(slider(), { target: { value: "50" } });
    expect(onCommit).toHaveBeenCalledWith(0.5);
    expect(slider().value).toBe("50");

    // Eco do estado com o volume antigo: o rascunho segura a barra.
    rerender(<VolumeSlider label="Volume do Spotify" value={0.2} onCommit={onCommit} />);
    expect(slider().value).toBe("50");

    // Sem novos movimentos, depois do prazo o estado manda de novo.
    act(() => {
      vi.advanceTimersByTime(VOLUME_SETTLE_MS);
    });
    expect(slider().value).toBe("20");
  });

  it("volume_draft_clears_when_incoming_value_matches", () => {
    const onCommit = vi.fn(async () => undefined);
    const { rerender } = render(
      <VolumeSlider label="Volume do Spotify" value={0.3} onCommit={onCommit} />,
    );
    fireEvent.change(slider(), { target: { value: "50" } });

    rerender(<VolumeSlider label="Volume do Spotify" value={0.5} onCommit={onCommit} />);
    // Rascunho cumpriu o papel: um valor novo do estado passa a valer de imediato.
    rerender(<VolumeSlider label="Volume do Spotify" value={0.6} onCommit={onCommit} />);
    expect(slider().value).toBe("60");
  });
});
