import { describe, expect, it } from "vitest";
import {
  formatUpdateDate,
  progressPercent,
  progressText,
  UPDATE_FALLBACK_ERROR,
  updateErrorText,
} from "./updateModel";

const MB = 1024 * 1024;

describe("updateErrorText", () => {
  it("usa a frase do Rust quando veio texto", () => {
    expect(updateErrorText("Sem conexão com a internet.")).toBe("Sem conexão com a internet.");
  });

  it("cai na frase genérica para vazio ou erro sem texto", () => {
    expect(updateErrorText("")).toBe(UPDATE_FALLBACK_ERROR);
    expect(updateErrorText(new Error("x"))).toBe(UPDATE_FALLBACK_ERROR);
    expect(updateErrorText(undefined)).toBe(UPDATE_FALLBACK_ERROR);
  });
});

describe("formatUpdateDate", () => {
  it("formata em português e aceita ausência", () => {
    expect(formatUpdateDate(Date.UTC(2026, 10, 3, 12) / 1000)).toBe("3 de novembro de 2026");
    expect(formatUpdateDate(null)).toBeNull();
  });
});

describe("progressPercent", () => {
  it("é null quando o tamanho total não é conhecido", () => {
    expect(progressPercent({ downloaded: 10, total: null })).toBeNull();
  });

  it("arredonda e não passa de 100", () => {
    expect(progressPercent({ downloaded: 1, total: 4 })).toBe(25);
    expect(progressPercent({ downloaded: 2, total: 3 })).toBe(67);
    expect(progressPercent({ downloaded: 9, total: 4 })).toBe(100);
  });
});

describe("progressText", () => {
  it("mostra preparando antes do primeiro pedaço", () => {
    expect(progressText(null)).toBe("Preparando o download…");
  });

  it("mostra só o baixado quando o total é desconhecido", () => {
    expect(progressText({ downloaded: 1.5 * MB, total: null })).toBe("Baixando… 1,5 MB");
  });

  it("mostra baixado de total e passa para instalando ao chegar ao fim", () => {
    expect(progressText({ downloaded: 1.5 * MB, total: 4 * MB })).toBe(
      "Baixando… 1,5 MB de 4 MB",
    );
    expect(progressText({ downloaded: 4 * MB, total: 4 * MB })).toBe(
      "Instalando… o Sonar vai reiniciar.",
    );
  });
});
