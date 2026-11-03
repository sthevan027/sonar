import { describe, expect, it } from "vitest";
import { appFromPath } from "./editorModel";

describe("appFromPath", () => {
  it("aceita .exe em qualquer caixa e devolve exe minúsculo com nome sem extensão", () => {
    expect(appFromPath("C:\\Jogos\\Game.EXE")).toEqual({ exe: "game.exe", name: "Game" });
    expect(appFromPath("  c:/apps/Spotify.exe ")).toEqual({ exe: "spotify.exe", name: "Spotify" });
  });

  it("rejeita o que não termina em .exe", () => {
    expect(appFromPath("C:\\x\\doc.txt")).toBeNull();
    expect(appFromPath("C:\\x\\app.exe.txt")).toBeNull();
    expect(appFromPath("C:\\x\\app.dll")).toBeNull();
    expect(appFromPath("C:\\x\\")).toBeNull();
    expect(appFromPath("")).toBeNull();
  });

  it("rejeita .exe sem nome antes", () => {
    expect(appFromPath("C:\\x\\.exe")).toBeNull();
  });
});
