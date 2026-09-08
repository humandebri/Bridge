import { beforeEach, afterEach, describe, expect, it, vi } from "vitest"
import { clearBaseObservationCache, sharedBaseRead } from "./base-transaction-observation"

vi.mock("@/lib/evm/client", () => ({ basePublicClient: {} }))

beforeEach(() => {
  vi.useFakeTimers()
  clearBaseObservationCache()
})
afterEach(() => vi.useRealTimers())

describe("shared Base observations", () => {
  it("deduplicates_concurrent_reads_and_refreshes_after_ten_seconds", async () => {
    const read = vi.fn().mockResolvedValue(42)
    expect(
      await Promise.all([sharedBaseRead("receipt:a", read), sharedBaseRead("receipt:a", read)]),
    ).toEqual([42, 42])
    expect(read).toHaveBeenCalledOnce()
    await vi.advanceTimersByTimeAsync(10_000)
    await sharedBaseRead("receipt:a", read)
    expect(read).toHaveBeenCalledTimes(2)
  })
  it("backs_off_restricted_and_transient_rpc_failures", async () => {
    const restricted = vi.fn().mockRejectedValue({ status: 403 })
    await expect(sharedBaseRead("restricted", restricted)).rejects.toEqual({ status: 403 })
    await vi.advanceTimersByTimeAsync(10_000)
    await expect(sharedBaseRead("restricted", restricted)).rejects.toEqual({ status: 403 })
    expect(restricted).toHaveBeenCalledOnce()
    const temporary = vi.fn().mockRejectedValue({ status: 429 })
    await expect(sharedBaseRead("temporary", temporary)).rejects.toEqual({ status: 429 })
    await vi.advanceTimersByTimeAsync(10_000)
    await expect(sharedBaseRead("temporary", temporary)).rejects.toEqual({ status: 429 })
    await vi.advanceTimersByTimeAsync(10_000)
    await expect(sharedBaseRead("temporary", temporary)).rejects.toEqual({ status: 429 })
    expect(temporary).toHaveBeenCalledTimes(2)
  })
})
