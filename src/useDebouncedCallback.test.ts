// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createDebounced, useDebouncedCallback } from "./useDebouncedCallback";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("createDebounced", () => {
  it("debounce_coalesces_calls_and_uses_last_args", () => {
    const fn = vi.fn();
    const debounced = createDebounced(fn, 60);
    debounced(10);
    vi.advanceTimersByTime(30);
    debounced(20);
    vi.advanceTimersByTime(30);
    debounced(30);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(59);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith(30);
  });

  it("debounce_flush_runs_pending_call_now_and_cancel_drops_it", () => {
    const fn = vi.fn();
    const debounced = createDebounced(fn, 60);
    debounced(1);
    expect(debounced.pending()).toBe(true);
    debounced.flush();
    expect(fn).toHaveBeenCalledWith(1);
    expect(debounced.pending()).toBe(false);
    vi.advanceTimersByTime(100);
    expect(fn).toHaveBeenCalledTimes(1);

    debounced(2);
    debounced.cancel();
    vi.advanceTimersByTime(100);
    expect(fn).toHaveBeenCalledTimes(1);
  });
});

describe("useDebouncedCallback", () => {
  it("debounce_hook_flushes_pending_value_on_unmount", () => {
    const fn = vi.fn();
    const { result, unmount } = renderHook(() => useDebouncedCallback(fn, 60));
    result.current(7);
    expect(fn).not.toHaveBeenCalled();
    unmount();
    expect(fn).toHaveBeenCalledTimes(1);
    expect(fn).toHaveBeenCalledWith(7);
  });
});
