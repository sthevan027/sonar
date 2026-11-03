import { describe, expect, it } from "vitest";
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
