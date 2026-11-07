// @vitest-environment jsdom
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import type { ActionResult } from "../types";
import { ActionResultToasts } from "./ActionResultToasts";
import { ToastProvider } from "./Toast";

vi.mock("../api", () => ({
  onActionResult: vi.fn(async () => () => {}),
}));

function result(overrides: Partial<ActionResult>): ActionResult {
  return { ok: false, changed: false, resolved: null, error_code: null, message: null, ...overrides };
}

/** Monta o componente e devolve o callback que o app registrou em `action-result`. */
async function mount() {
  let emit: (r: ActionResult) => void = () => undefined;
  vi.mocked(api.onActionResult).mockImplementation(async (cb) => {
    emit = cb;
    return () => {};
  });
  render(
    <ToastProvider>
      <ActionResultToasts />
    </ToastProvider>,
  );
  await act(async () => {});
  return (r: ActionResult) => act(() => emit(r));
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("ActionResultToasts", () => {
  it("mostra a mensagem de uma ação que falhou, em qualquer tela", async () => {
    const emit = await mount();
    emit(result({ error_code: "device_gone", message: "O dispositivo foi removido." }));
    expect(await screen.findByText("O dispositivo foi removido.")).toBeTruthy();
  });

  it("usa a frase genérica quando a falha vem sem mensagem", async () => {
    const emit = await mount();
    emit(result({ message: null }));
    expect(await screen.findByText("Não foi possível executar a ação.")).toBeTruthy();
  });

  it("não avisa quando a ação deu certo", async () => {
    const emit = await mount();
    emit(result({ ok: true, message: "feito" }));
    expect(screen.queryByText("feito")).toBeNull();
  });

  it("cancela a assinatura ao desmontar", async () => {
    const unlisten = vi.fn();
    vi.mocked(api.onActionResult).mockResolvedValue(unlisten);
    const view = render(
      <ToastProvider>
        <ActionResultToasts />
      </ToastProvider>,
    );
    await act(async () => {});
    view.unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("cancela a assinatura mesmo quando ela chega depois de desmontar", async () => {
    const unlisten = vi.fn();
    let finish: (stop: () => void) => void = () => undefined;
    vi.mocked(api.onActionResult).mockReturnValue(new Promise((resolve) => (finish = resolve)));
    const view = render(
      <ToastProvider>
        <ActionResultToasts />
      </ToastProvider>,
    );
    view.unmount();
    await act(async () => finish(unlisten));
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
