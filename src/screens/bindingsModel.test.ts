import { describe, expect, it } from "vitest";
import {
  ACTION_CHOICES,
  actionOfKind,
  familyKind,
  muteState,
  muteStateLabel,
  muteStateOptions,
  withMuteState,
} from "./bindingsModel";

describe("estado do mudo", () => {
  it("o seletor tem uma ação de mudo por alvo, e não duas", () => {
    const kinds = ACTION_CHOICES.map((c) => c.kind);
    expect(kinds).toContain("toggle_mic_mute");
    expect(kinds).toContain("toggle_output_mute");
    expect(kinds).not.toContain("set_mic_mute");
    expect(kinds).not.toContain("set_output_mute");
  });

  it("alternar e definir são a mesma família", () => {
    expect(familyKind("set_mic_mute")).toBe("toggle_mic_mute");
    expect(familyKind("set_output_mute")).toBe("toggle_output_mute");
    expect(familyKind("set_app_mute")).toBe("set_app_mute");
  });

  it("os três estados do microfone mapeiam para a ação certa", () => {
    const mic = actionOfKind("toggle_mic_mute");
    expect(muteState(mic)).toBe("toggle");
    expect(withMuteState(mic, "mute")).toEqual({ type: "set_mic_mute", muted: true });
    expect(withMuteState(mic, "unmute")).toEqual({ type: "set_mic_mute", muted: false });
    expect(withMuteState(withMuteState(mic, "mute"), "toggle")).toEqual({ type: "toggle_mic_mute" });
  });

  it("os três estados da saída mapeiam para a ação certa", () => {
    const out = actionOfKind("toggle_output_mute");
    expect(withMuteState(out, "mute")).toEqual({ type: "set_output_mute", muted: true });
    expect(withMuteState(out, "unmute")).toEqual({ type: "set_output_mute", muted: false });
  });

  it("o texto de ativar acompanha o alvo", () => {
    expect(muteStateLabel({ type: "set_mic_mute", muted: false })).toBe("Ativar o microfone");
    expect(muteStateLabel({ type: "set_output_mute", muted: false })).toBe("Ativar o som");
    expect(muteStateLabel({ type: "toggle_mic_mute" })).toBe("Alternar");
  });

  it("o app não tem alternar", () => {
    const app = { type: "set_app_mute", exe: "spotify.exe", muted: true } as const;
    expect(muteStateOptions(app).map((o) => o.state)).toEqual(["mute", "unmute"]);
    expect(withMuteState(app, "toggle")).toEqual({ ...app, muted: true });
  });
});
