// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createThrottled, useThrottledCallback } from "./useThrottledCallback";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("createThrottled", () => {
  it("throttle_sends_first_call_now_and_last_queued_at_window_end", () => {
    const fn = vi.fn();
    const throttled = createThrottled(fn, 60);
    throttled(1);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenLastCalledWith(1);

    throttled(2);
    throttled(3);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(throttled.pending()).toBe(true);

    vi.advanceTimersByTime(59);
    expect(fn).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(2);
    expect(fn).toHaveBeenLastCalledWith(3);
    expect(throttled.pending()).toBe(false);

    // Sem nada novo, a janela fecha e a próxima chamada sai na hora.
    vi.advanceTimersByTime(200);
    expect(fn).toHaveBeenCalledTimes(2);
    throttled(4);
    expect(fn).toHaveBeenCalledTimes(3);
  });

  it("throttle_drag_sends_intermediate_values_at_most_every_window_and_final_value", () => {
    const sent: { at: number; value: number }[] = [];
    const throttled = createThrottled((value: number) => sent.push({ at: Date.now(), value }), 60);
    const start = Date.now();
    for (let step = 0; step <= 30; step += 1) {
      throttled(step);
      vi.advanceTimersByTime(10);
    }
    vi.advanceTimersByTime(200);

    expect(sent.length).toBeGreaterThan(2);
    for (let i = 1; i < sent.length; i += 1) {
      expect(sent[i].at - sent[i - 1].at).toBeGreaterThanOrEqual(60);
    }
    expect(sent[0]).toEqual({ at: start, value: 0 });
    expect(sent[sent.length - 1].value).toBe(30);
  });

  it("throttle_flush_sends_queued_value_now_and_cancel_drops_it", () => {
    const fn = vi.fn();
    const throttled = createThrottled(fn, 60);
    throttled(5);
    throttled(6);
    throttled.flush();
    expect(fn).toHaveBeenLastCalledWith(6);
    expect(throttled.pending()).toBe(false);

    // Depois do flush a janela fechou: 7 sai na hora; 8 fica guardado e o cancel o descarta.
    throttled(7);
    throttled(8);
    throttled.cancel();
    vi.advanceTimersByTime(200);
    expect(fn).toHaveBeenCalledTimes(3);
    expect(fn).toHaveBeenLastCalledWith(7);
  });
});

describe("useThrottledCallback", () => {
  it("throttle_hook_flushes_queued_value_on_unmount", () => {
    const fn = vi.fn();
    const { result, unmount } = renderHook(() => useThrottledCallback(fn, 60));
    result.current(1);
    result.current(2);
    expect(fn).toHaveBeenCalledTimes(1);
    unmount();
    expect(fn).toHaveBeenCalledTimes(2);
    expect(fn).toHaveBeenLastCalledWith(2);
  });
});
