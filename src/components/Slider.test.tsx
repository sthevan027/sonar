import { describe, expect, it } from "vitest";
import { clampPercent } from "./Slider";

describe("clampPercent", () => {
  it("slider_clamps_to_0_100", () => {
    expect(clampPercent(-5)).toBe(0);
    expect(clampPercent(150)).toBe(100);
    expect(clampPercent(42)).toBe(42);
    expect(clampPercent(0)).toBe(0);
    expect(clampPercent(100)).toBe(100);
    expect(clampPercent(Number.NaN)).toBe(0);
  });
});
