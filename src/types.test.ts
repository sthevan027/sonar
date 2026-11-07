import { describe, expect, it } from "vitest";
import type { Action, ActionResult, BindingStatusEntry } from "./types";
import { toTarget } from "./types";

describe("toTarget", () => {
  it("mapeia 'keep' para undefined, que some do JSON e mantém a rota atual", () => {
    expect(toTarget("keep")).toBeUndefined();
    const item = { exe: "game.exe", output: toTarget("keep") };
    expect(JSON.stringify(item)).toBe('{"exe":"game.exe"}');
  });

  it("mantém 'default' como o padrão do Windows", () => {
    expect(toTarget("default")).toBe("default");
  });

  it("devolve o id do dispositivo sem alterar", () => {
    expect(toTarget("{0.0.0.00000000}.{2329d0e9-306a-408e-9c4b-4061e553cdb4}")).toBe(
      "{0.0.0.00000000}.{2329d0e9-306a-408e-9c4b-4061e553cdb4}",
    );
  });
});

describe("espelho das ações globais", () => {
  it("o estado de um binding é o enum com tag, em snake_case", () => {
    const active: BindingStatusEntry = { accelerator: "Ctrl+Alt+M", status: { type: "active" } };
    const unavailable: BindingStatusEntry = {
      accelerator: "Ctrl+Alt+X",
      status: { type: "unavailable", message: "Tecla já em uso por outro aplicativo." },
    };
    expect(JSON.stringify(active)).toBe(
      '{"accelerator":"Ctrl+Alt+M","status":{"type":"active"}}',
    );
    expect(JSON.stringify(unavailable)).toBe(
      '{"accelerator":"Ctrl+Alt+X","status":{"type":"unavailable","message":"Tecla já em uso por outro aplicativo."}}',
    );
  });

  it("a ação viaja com a tag type em snake_case", () => {
    const actions: Action[] = [
      { type: "toggle_panel" },
      { type: "activate_profile", profile_id: "jogos" },
      { type: "cycle_profile", direction: "next" },
      { type: "set_mic_mute", muted: true },
      { type: "toggle_mic_mute" },
      { type: "set_output_mute", muted: true },
      { type: "toggle_output_mute" },
      { type: "step_output_volume", delta: 0.05 },
      { type: "set_output_volume", value: 0.5 },
      { type: "set_app_mute", exe: "spotify.exe", muted: false },
      { type: "move_app", exe: "chrome.exe", device: null },
      { type: "cycle_default_output", direction: "prev" },
    ];
    expect(actions.map((action) => JSON.stringify(action))).toEqual([
      '{"type":"toggle_panel"}',
      '{"type":"activate_profile","profile_id":"jogos"}',
      '{"type":"cycle_profile","direction":"next"}',
      '{"type":"set_mic_mute","muted":true}',
      '{"type":"toggle_mic_mute"}',
      '{"type":"set_output_mute","muted":true}',
      '{"type":"toggle_output_mute"}',
      '{"type":"step_output_volume","delta":0.05}',
      '{"type":"set_output_volume","value":0.5}',
      '{"type":"set_app_mute","exe":"spotify.exe","muted":false}',
      '{"type":"move_app","exe":"chrome.exe","device":null}',
      '{"type":"cycle_default_output","direction":"prev"}',
    ]);
  });

  it("o resultado da ação tem as mesmas cinco chaves, mesmo na falha parcial", () => {
    const partial: ActionResult = {
      ok: false,
      changed: true,
      resolved: "p1",
      error_code: "unsupported",
      message:
        "O perfil foi ativado, mas este Windows não permite trocar o dispositivo dos aplicativos.",
    };
    expect(JSON.stringify(partial)).toBe(
      '{"ok":false,"changed":true,"resolved":"p1","error_code":"unsupported","message":"O perfil foi ativado, mas este Windows não permite trocar o dispositivo dos aplicativos."}',
    );
  });
});
