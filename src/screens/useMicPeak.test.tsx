// @vitest-environment jsdom
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { useMicPeak } from "./useMicPeak";

vi.mock("../api", () => ({
  onMicLevel: vi.fn(),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function Probe() {
  const peak = useMicPeak("m");
  return <span data-testid="peak">{peak}</span>;
}

describe("useMicPeak", () => {
  it("strict_mode_double_mount_cancels_the_first_subscription", async () => {
    const firstStop = vi.fn();
    const secondStop = vi.fn();
    // A primeira assinatura só resolve depois da desmontagem fingida do StrictMode.
    let finishFirst!: (stop: () => void) => void;
    vi.mocked(api.onMicLevel)
      .mockImplementationOnce(() => new Promise((resolve) => (finishFirst = resolve)))
      .mockImplementationOnce(async () => secondStop);

    const { unmount } = render(
      <StrictMode>
        <Probe />
      </StrictMode>,
    );
    await waitFor(() => expect(api.onMicLevel).toHaveBeenCalledTimes(2));

    await act(async () => finishFirst(firstStop));
    // A assinatura cancelada é encerrada na hora; a válida segue viva.
    expect(firstStop).toHaveBeenCalledTimes(1);
    expect(secondStop).not.toHaveBeenCalled();

    unmount();
    expect(secondStop).toHaveBeenCalledTimes(1);
    expect(firstStop).toHaveBeenCalledTimes(1);
  });
});
