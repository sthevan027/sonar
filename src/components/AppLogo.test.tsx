// @vitest-environment jsdom
import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppLogo, initialOf } from "./AppLogo";
import * as api from "../api";

vi.mock("../api", () => ({ appIcon: vi.fn() }));

afterEach(() => cleanup());

describe("AppLogo", () => {
  it("applogo_falls_back_to_neutral_initial", async () => {
    vi.mocked(api.appIcon).mockResolvedValue(null);
    const { container } = render(<AppLogo exe="spotify.exe" />);
    await waitFor(() => expect(api.appIcon).toHaveBeenCalledWith("spotify.exe"));
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toBe("S");
  });

  it("applogo_shows_icon_returned_by_api", async () => {
    vi.mocked(api.appIcon).mockResolvedValue("data:image/png;base64,AAAA");
    const { container } = render(<AppLogo exe="discord.exe" />);
    await waitFor(() => expect(container.querySelector("img")).not.toBeNull());
    expect(container.querySelector("img")?.getAttribute("src")).toBe("data:image/png;base64,AAAA");
  });

  it("initialOf_counts_code_points", () => {
    expect(initialOf("chrome.exe")).toBe("C");
    expect(initialOf("Érudito.exe")).toBe("É");
    expect(initialOf("")).toBe("?");
  });
});
