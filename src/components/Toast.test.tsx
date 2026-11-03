// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TOAST_MS, ToastProvider, useToast } from "./Toast";

function Trigger({ message, options }: { message: string; options?: { key?: string; minIntervalMs?: number } }) {
  const notify = useToast();
  return <button onClick={() => notify(message, options)}>avisar</button>;
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("Toast", () => {
  it("toast_shows_message_in_status_region_and_dismisses_it", () => {
    render(
      <ToastProvider>
        <Trigger message="Não foi possível trocar a saída." />
      </ToastProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "avisar" }));
    expect(screen.getByRole("status").textContent).toContain("Não foi possível trocar a saída.");

    act(() => {
      vi.advanceTimersByTime(TOAST_MS);
    });
    expect(screen.getByRole("status").textContent).toBe("");
  });

  it("toast_rate_limits_same_key", () => {
    render(
      <ToastProvider>
        <Trigger message="Volume falhou." options={{ key: "volume", minIntervalMs: 2000 }} />
      </ToastProvider>,
    );
    const button = screen.getByRole("button", { name: "avisar" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(screen.getByRole("status").querySelectorAll(".toast")).toHaveLength(1);

    act(() => {
      vi.advanceTimersByTime(2000);
    });
    fireEvent.click(button);
    expect(screen.getByRole("status").querySelectorAll(".toast")).toHaveLength(2);
  });

  it("toast_without_provider_is_silent", () => {
    render(<Trigger message="ninguém ouve" />);
    expect(() => fireEvent.click(screen.getByRole("button", { name: "avisar" }))).not.toThrow();
  });
});
