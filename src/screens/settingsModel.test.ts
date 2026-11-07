import { describe, expect, it } from "vitest";
import type { Binding, Profile } from "../types";
import { suggestProfileBindings } from "./settingsModel";

function profile(id: string, name?: string): Profile {
  return { id, icon: "layers", ...(name ? { name } : {}), items: [] };
}

describe("suggestProfileBindings", () => {
  it("propõe Ctrl+Alt+1, 2 e 3 para três perfis sem atalho", () => {
    const profiles = [profile("p1", "Jogos"), profile("p2", "Trabalho"), profile("p3", "Call")];

    expect(suggestProfileBindings(profiles, [])).toEqual([
      { accelerator: "Ctrl+Alt+1", action: { type: "activate_profile", profile_id: "p1" } },
      { accelerator: "Ctrl+Alt+2", action: { type: "activate_profile", profile_id: "p2" } },
      { accelerator: "Ctrl+Alt+3", action: { type: "activate_profile", profile_id: "p3" } },
    ]);
  });

  it("pula o acelerador que já está em uso por outro binding", () => {
    const profiles = [profile("p1"), profile("p2"), profile("p3")];
    const existing: Binding[] = [
      { accelerator: "Ctrl+Alt+1", action: { type: "toggle_output_mute" } },
    ];

    const suggestions = suggestProfileBindings(profiles, existing);

    expect(suggestions.map((b) => b.accelerator)).toEqual([
      "Ctrl+Alt+2",
      "Ctrl+Alt+3",
      "Ctrl+Alt+4",
    ]);
    expect(suggestions.map((b) => b.accelerator)).not.toContain("Ctrl+Alt+1");
  });

  it("não repete acelerador já usado, nem entre as próprias sugestões", () => {
    const profiles = [profile("p1"), profile("p2"), profile("p3")];
    const existing: Binding[] = [
      { accelerator: "ctrl+alt+1", action: { type: "toggle_output_mute" } },
      { accelerator: "Ctrl+Alt+2", action: { type: "toggle_output_mute" } },
    ];

    const suggestions = suggestProfileBindings(profiles, existing);

    const accelerators = suggestions.map((b) => b.accelerator.toLowerCase());
    expect(new Set(accelerators).size).toBe(accelerators.length);
    expect(accelerators).not.toContain("ctrl+alt+1");
    expect(accelerators).not.toContain("ctrl+alt+2");
  });

  it("não entra perfil que já tem binding de perfil", () => {
    const profiles = [profile("p1"), profile("p2")];
    const existing: Binding[] = [
      { accelerator: "Ctrl+Alt+9", action: { type: "activate_profile", profile_id: "p1" } },
    ];

    const suggestions = suggestProfileBindings(profiles, existing);

    expect(suggestions).toEqual([
      { accelerator: "Ctrl+Alt+2", action: { type: "activate_profile", profile_id: "p2" } },
    ]);
  });

  it("no máximo nove sugestões, mesmo com mais de nove perfis", () => {
    const profiles = Array.from({ length: 14 }, (_, i) => profile(`p${i + 1}`));

    const suggestions = suggestProfileBindings(profiles, []);

    expect(suggestions).toHaveLength(9);
    expect(suggestions[8].accelerator).toBe("Ctrl+Alt+9");
  });

  it("não sugere nada quando não há perfis", () => {
    expect(suggestProfileBindings([], [])).toEqual([]);
  });
});
