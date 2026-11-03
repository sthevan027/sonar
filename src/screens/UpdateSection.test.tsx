// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { UpdateInfo } from "../types";
import { UpdateSection } from "./UpdateSection";

vi.mock("../api", () => ({
  pendingUpdate: vi.fn(async () => null),
  onUpdateAvailable: vi.fn(async () => () => {}),
  onUpdateProgress: vi.fn(async () => () => {}),
  checkForUpdate: vi.fn(async () => null),
  installUpdate: vi.fn(async () => undefined),
}));

const found: UpdateInfo = {
  version: "0.2.0",
  notes: "Corrige o medidor.\nMelhora o painel.",
  date: Date.UTC(2026, 10, 3, 12) / 1000,
};

function checkButton() {
  return screen.getByRole("button", { name: /Verificar atualizações/ });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("UpdateSection", () => {
  it("check_shows_up_to_date_when_there_is_no_newer_version", async () => {
    render(<UpdateSection />);

    fireEvent.click(checkButton());

    expect(await screen.findByText("Você está na versão mais recente.")).toBeTruthy();
    expect(api.checkForUpdate).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: "Atualizar e reiniciar" })).toBeNull();
  });

  it("check_shows_available_version_with_notes_and_install_button", async () => {
    vi.mocked(api.checkForUpdate).mockResolvedValueOnce(found);
    render(<UpdateSection />);

    fireEvent.click(checkButton());

    expect(await screen.findByText("Versão 0.2.0 disponível")).toBeTruthy();
    expect(screen.getByText(/Corrige o medidor/)).toBeTruthy();
    expect(screen.getByText(/Publicada em 3 de novembro de 2026/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Atualizar e reiniciar" })).toBeTruthy();
  });

  it("check_error_shows_the_rust_message_inline_and_releases_the_button", async () => {
    vi.mocked(api.checkForUpdate).mockRejectedValueOnce(
      "Sem conexão com a internet. Verifique a rede e tente de novo.",
    );
    render(<UpdateSection />);

    fireEvent.click(checkButton());

    expect((await screen.findByRole("alert")).textContent).toBe(
      "Sem conexão com a internet. Verifique a rede e tente de novo.",
    );
    expect(checkButton().hasAttribute("disabled")).toBe(false);
  });

  it("install_calls_the_api_once_even_after_repeated_clicks", async () => {
    vi.mocked(api.checkForUpdate).mockResolvedValueOnce(found);
    // A instalação não volta (no Windows o app fecha): a tela segue em "instalando".
    vi.mocked(api.installUpdate).mockImplementation(() => new Promise<void>(() => {}));
    render(<UpdateSection />);
    fireEvent.click(checkButton());
    const install = await screen.findByRole("button", { name: "Atualizar e reiniciar" });

    fireEvent.click(install);
    fireEvent.click(install);

    expect(api.installUpdate).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("progressbar")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Atualizar e reiniciar" })).toBeNull();
  });

  it("install_error_returns_to_available_with_the_message", async () => {
    vi.mocked(api.checkForUpdate).mockResolvedValueOnce(found);
    vi.mocked(api.installUpdate).mockRejectedValueOnce(
      "A assinatura da atualização não confere. Nada foi instalado.",
    );
    render(<UpdateSection />);
    fireEvent.click(checkButton());

    fireEvent.click(await screen.findByRole("button", { name: "Atualizar e reiniciar" }));

    expect((await screen.findByRole("alert")).textContent).toContain("não confere");
    expect(screen.getByRole("button", { name: "Atualizar e reiniciar" })).toBeTruthy();
    expect(api.installUpdate).toHaveBeenCalledTimes(1);
  });
});
