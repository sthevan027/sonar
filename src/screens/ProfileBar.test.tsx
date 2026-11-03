// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { Profile } from "../types";
import { LONG_PRESS_MS, ProfileBar } from "./ProfileBar";

vi.mock("../api", () => ({
  activateProfile: vi.fn(async () => undefined),
}));

const jogo: Profile = { id: "jogo", icon: "gamepad-2", name: "Jogo", items: [] };

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
});

function renderBar(onEdit = vi.fn()) {
  render(
    <ProfileBar
      profiles={[jogo]}
      lastProfile={null}
      onAdd={vi.fn()}
      onEdit={onEdit}
    />,
  );
  return { onEdit, button: screen.getByRole("button", { name: "Jogo" }) };
}

describe("ProfileBar", () => {
  it("long_press_opens_editor_and_does_not_activate_the_profile", () => {
    const { onEdit, button } = renderBar();

    fireEvent.pointerDown(button);
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onEdit).toHaveBeenCalledWith("jogo");

    // O clique que vem depois do toque longo não ativa o perfil.
    fireEvent.pointerUp(button);
    fireEvent.click(button);
    expect(api.activateProfile).not.toHaveBeenCalled();
  });

  it("short_press_activates_the_profile_and_does_not_open_editor", () => {
    const { onEdit, button } = renderBar();

    fireEvent.pointerDown(button);
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS - 100);
    });
    fireEvent.pointerUp(button);
    fireEvent.click(button);

    expect(api.activateProfile).toHaveBeenCalledTimes(1);
    expect(api.activateProfile).toHaveBeenCalledWith("jogo");
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onEdit).not.toHaveBeenCalled();
  });

  it("keyboard_focus_shows_pencil_for_inactive_profile_and_edits_without_activating", () => {
    const onEdit = vi.fn();
    const beta: Profile = { id: "beta", icon: "music", name: "Beta", items: [] };
    render(
      <ProfileBar
        profiles={[jogo, beta]}
        lastProfile="jogo"
        onAdd={vi.fn()}
        onEdit={onEdit}
      />,
    );

    // Sem foco, o lápis é do perfil ativo.
    expect(screen.getByRole("button", { name: "Editar Jogo" })).toBeTruthy();

    act(() => screen.getByRole("button", { name: "Beta" }).focus());
    const pencil = screen.getByRole("button", { name: "Editar Beta" });
    // O lápis é alcançável por teclado: o foco entrar nele não some com ele.
    act(() => pencil.focus());
    expect(screen.getByRole("button", { name: "Editar Beta" })).toBeTruthy();

    fireEvent.click(pencil);
    expect(onEdit).toHaveBeenCalledWith("beta");
    expect(api.activateProfile).not.toHaveBeenCalled();
  });

  it("marks_the_active_profile_with_aria_pressed", () => {
    const beta: Profile = { id: "beta", icon: "music", name: "Beta", items: [] };
    render(
      <ProfileBar profiles={[jogo, beta]} lastProfile="beta" onAdd={vi.fn()} onEdit={vi.fn()} />,
    );
    expect(screen.getByRole("button", { name: "Beta" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "Jogo" }).getAttribute("aria-pressed")).toBe("false");
  });
});
