import { renderToString } from "react-dom/server";
import { describe, expect, it } from "vitest";
import App from "./App";

describe("App", () => {
  it("renderiza o nome Sonar", () => {
    expect(renderToString(<App />)).toContain("Sonar");
  });
});
