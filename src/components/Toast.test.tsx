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
    // Mensagens diferentes, mesma chave: só uma por intervalo.
    render(
      <ToastProvider>
        <Many messages={["Volume 1", "Volume 2"]} keyed />
      </ToastProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Volume 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Volume 2" }));
    expect(screen.getByRole("status").querySelectorAll(".toast")).toHaveLength(1);

    act(() => {
      vi.advanceTimersByTime(2000);
    });
    fireEvent.click(screen.getByRole("button", { name: "Volume 2" }));
    expect(screen.getByRole("status").querySelectorAll(".toast")).toHaveLength(2);
  });

  it("toast_without_provider_is_silent", () => {
    render(<Trigger message="ninguém ouve" />);
    expect(() => fireEvent.click(screen.getByRole("button", { name: "avisar" }))).not.toThrow();
  });
});

function Many({ messages, keyed = false }: { messages: string[]; keyed?: boolean }) {
  const notify = useToast();
  return (
    <>
      {messages.map((m) => (
        <button key={m} onClick={() => notify(m, keyed ? { key: "volume", minIntervalMs: 2000 } : undefined)}>
          {m}
        </button>
      ))}
    </>
  );
}

describe("Toast cap and dedupe", () => {
  it("toast_dedupes_identical_message_within_window", () => {
    render(
      <ToastProvider>
        <Trigger message="Mesma coisa." />
      </ToastProvider>,
    );
    const button = screen.getByRole("button", { name: "avisar" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(screen.getByRole("status").querySelectorAll(".toast")).toHaveLength(1);

    // Passada a janela, a mesma mensagem volta a aparecer.
    act(() => {
      vi.advanceTimersByTime(TOAST_MS);
    });
    fireEvent.click(button);
    expect(screen.getByRole("status").querySelectorAll(".toast")).toHaveLength(1);
    expect(screen.getByRole("status").textContent).toContain("Mesma coisa.");
  });

  it("toast_caps_visible_messages_dropping_oldest", () => {
    render(
      <ToastProvider>
        <Many messages={["um", "dois", "três", "quatro"]} />
      </ToastProvider>,
    );
    for (const name of ["um", "dois", "três", "quatro"]) {
      fireEvent.click(screen.getByRole("button", { name }));
    }
    const shown = Array.from(screen.getByRole("status").querySelectorAll(".toast")).map((t) => t.textContent);
    expect(shown).toEqual(["dois", "três", "quatro"]);
  });
});
