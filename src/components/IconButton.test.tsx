// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { IconButton } from "./IconButton";

afterEach(() => cleanup());

describe("IconButton", () => {
  it("expõe aria-pressed quando recebe selected", () => {
    render(
      <>
        <IconButton label="Jogos" icon={<span />} size={38} selected onPress={vi.fn()} />
        <IconButton label="Música" icon={<span />} size={38} selected={false} onPress={vi.fn()} />
      </>,
    );
    expect(screen.getByRole("button", { name: "Jogos" }).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByRole("button", { name: "Música" }).getAttribute("aria-pressed")).toBe("false");
  });

  it("não põe aria-pressed quando selected não é informado", () => {
    render(<IconButton label="Voltar" icon={<span />} onPress={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Voltar" }).hasAttribute("aria-pressed")).toBe(false);
  });

  it("chama onPress ao clicar", () => {
    const onPress = vi.fn();
    render(<IconButton label="Voltar" icon={<span />} onPress={onPress} />);
    fireEvent.click(screen.getByRole("button", { name: "Voltar" }));
    expect(onPress).toHaveBeenCalledTimes(1);
  });
});
