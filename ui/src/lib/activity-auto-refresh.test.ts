import { act, cleanup, renderHook } from "@testing-library/react"
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"
import { ACTIVITY_AUTO_REFRESH_INTERVAL_MS, useActivityAutoRefresh } from "./activity-auto-refresh"

describe("activity auto refresh", () => {
  beforeEach(() => vi.useFakeTimers())

  afterEach(() => {
    cleanup()
    vi.useRealTimers()
  })

  it("keeps the original interval while renders replace the refresh callback", async () => {
    const first = vi.fn()
    const latest = vi.fn()
    const view = renderHook(({ enabled, refresh }) => useActivityAutoRefresh(enabled, refresh), {
      initialProps: { enabled: true, refresh: first },
    })

    await act(() => vi.advanceTimersByTimeAsync(45_000))
    view.rerender({ enabled: true, refresh: latest })
    await act(() => vi.advanceTimersByTimeAsync(ACTIVITY_AUTO_REFRESH_INTERVAL_MS - 45_000))

    expect(first).not.toHaveBeenCalled()
    expect(latest).toHaveBeenCalledOnce()

    view.rerender({ enabled: false, refresh: latest })
    await act(() => vi.advanceTimersByTimeAsync(ACTIVITY_AUTO_REFRESH_INTERVAL_MS))
    expect(latest).toHaveBeenCalledOnce()
  })
})
