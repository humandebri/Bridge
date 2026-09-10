import { beforeEach, afterEach, describe, expect, it, vi } from "vitest"
import type { MintExecutionRequest } from "./mint-execution"
const hash = `0x${"12".repeat(32)}` as const
function deferred<T>() {
  let resolve!: (v: T) => void
  const promise = new Promise<T>((r) => {
    resolve = r
  })
  return { promise, resolve }
}
function request(): MintExecutionRequest<number> {
  return {
    key: "deployment:deposit:digest",
    wallet: "0xabc",
    source: "manual",
    connected: () => true,
    readPending: () => undefined,
    prepare: vi.fn().mockResolvedValue(1),
    beforeWallet: vi.fn(),
    send: vi.fn().mockResolvedValue(hash),
    save: vi.fn().mockResolvedValue(undefined),
  }
}
beforeEach(() => {
  vi.resetModules()
  localStorage.clear()
  Object.defineProperty(navigator, "locks", {
    configurable: true,
    value: { request: vi.fn(async (_n, _o, run) => run({ name: "mint" })) },
  })
})
afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})
it("rejects_late_completion_even_when_the_browser_timer_is_throttled", async () => {
  const m = await import("./mint-execution")
  let clock = 0
  vi.spyOn(performance, "now").mockImplementation(() => clock)
  const r = request()
  r.prepare = async () => {
    clock = 30_001
    return 1
  }
  await m.startMintExecution(r)
  expect(r.send).not.toHaveBeenCalled()
  expect(m.mintExecutionSnapshot(r.key).phase).toBe("failed")
})

it("rejects_a_disconnected_wallet_before_starting_preflight", async () => {
  const m = await import("./mint-execution")
  const r = request()
  r.connected = () => false
  await m.startMintExecution(r)
  expect(r.prepare).not.toHaveBeenCalled()
  expect(r.send).not.toHaveBeenCalled()
  expect(m.mintExecutionSnapshot(r.key).phase).toBe("failed")
})

describe("mint execution", () => {
  it("coalesces_automatic_and_history_requests_into_one_wallet_prompt", async () => {
    const m = await import("./mint-execution")
    const p = deferred<number>()
    const r = request()
    r.prepare = () => p.promise
    const first = m.startMintExecution({ ...r, source: "automatic" })
    expect(m.startMintExecution(r)).toBe(first)
    p.resolve(1)
    await first
    expect(r.send).toHaveBeenCalledOnce()
    expect(m.mintExecutionSnapshot(r.key)).toMatchObject({
      phase: "submitted",
      transactionHash: hash,
    })
  })
  it("invalidates_late_responses_after_30_seconds_and_permits_only_manual_retry", async () => {
    vi.useFakeTimers()
    const m = await import("./mint-execution")
    const p = deferred<number>()
    const r = request()
    r.prepare = vi.fn(() => p.promise)
    const first = m.startMintExecution({ ...r, source: "automatic" })
    await vi.advanceTimersByTimeAsync(30000)
    await first
    expect(m.mintExecutionSnapshot(r.key).phase).toBe("failed")
    p.resolve(1)
    await Promise.resolve()
    expect(r.send).not.toHaveBeenCalled()
    await m.startMintExecution({ ...r, source: "automatic" })
    expect(r.prepare).toHaveBeenCalledOnce()
    await m.startMintExecution(request())
    expect(m.mintExecutionSnapshot(r.key).phase).toBe("submitted")
  })
  it("cancels_preflight_without_opening_a_late_wallet_prompt", async () => {
    const m = await import("./mint-execution")
    const p = deferred<number>()
    const r = request()
    r.prepare = () => p.promise
    const run = m.startMintExecution(r)
    await Promise.resolve()
    await Promise.resolve()
    m.cancelMintPreflight(r.key)
    await run
    p.resolve(1)
    await Promise.resolve()
    expect(r.send).not.toHaveBeenCalled()
  })
  it("does_not_queue_behind_another_wallet_operation", async () => {
    const m = await import("./mint-execution")
    const r = request()
    vi.mocked(navigator.locks.request).mockImplementation((async (
      _n: string,
      _o: unknown,
      run: (lock: null) => Promise<void>,
    ) => run(null)) as typeof navigator.locks.request)
    await m.startMintExecution(r)
    expect(r.prepare).not.toHaveBeenCalled()
    expect(m.mintExecutionSnapshot(r.key).message).toContain("Another wallet operation")
  })
  it("rechecks_connection_and_deadline_before_wallet_dispatch", async () => {
    const m = await import("./mint-execution")
    const r = request()
    r.prepare = async () => {
      r.connected = () => false
      return 1
    }
    await m.startMintExecution(r)
    expect(r.send).not.toHaveBeenCalled()
    const next = request()
    next.beforeWallet = () => {
      throw new Error("Expired")
    }
    await m.startMintExecution(next)
    expect(next.send).not.toHaveBeenCalled()
  })
  it("does_not_timeout_or_duplicate_a_wallet_request", async () => {
    vi.useFakeTimers()
    const m = await import("./mint-execution")
    const p = deferred<`0x${string}`>()
    const r = request()
    r.send = vi.fn(() => p.promise)
    const run = m.startMintExecution(r)
    await vi.advanceTimersByTimeAsync(120000)
    expect(m.mintExecutionSnapshot(r.key).phase).toBe("wallet")
    expect(m.startMintExecution(request())).toBe(run)
    p.resolve(hash)
    await run
    expect(r.send).toHaveBeenCalledOnce()
  })
  it("blocks_unresolved_requests_after_reload", async () => {
    localStorage.setItem(
      "kinic.bridge.mint-attempt.v1:deployment:deposit:digest",
      JSON.stringify({ phase: "wallet" }),
    )
    const m = await import("./mint-execution")
    const r = request()
    await m.startMintExecution(r)
    expect(m.mintExecutionSnapshot(r.key).phase).toBe("unknown")
    expect(r.send).not.toHaveBeenCalled()
  })
  it("allows_retry_only_for_explicit_rejection", async () => {
    const m = await import("./mint-execution")
    const r = request()
    r.send = vi.fn().mockRejectedValue({ code: 4001 })
    await m.startMintExecution(r)
    expect(m.mintExecutionSnapshot(r.key).phase).toBe("rejected")
    const next = request()
    next.send = vi.fn().mockRejectedValue(new Error("transport"))
    await m.startMintExecution(next)
    expect(next.send).toHaveBeenCalledOnce()
    expect(m.mintExecutionSnapshot(r.key).phase).toBe("unknown")
    const again = request()
    await m.startMintExecution(again)
    expect(again.send).not.toHaveBeenCalled()
  })
  it("stops_before_wallet_if_intent_cannot_be_persisted", async () => {
    const m = await import("./mint-execution")
    const r = request()
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota")
    })
    await m.startMintExecution(r)
    expect(r.send).not.toHaveBeenCalled()
  })
  it("retains_hash_when_persistence_fails_after_send", async () => {
    const m = await import("./mint-execution")
    const r = request()
    r.send = vi.fn(async () => {
      vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
        throw new Error("quota")
      })
      return hash
    })
    r.save = vi.fn().mockRejectedValue(new Error("quota"))
    await m.startMintExecution(r)
    expect(m.mintExecutionSnapshot(r.key)).toMatchObject({
      phase: "submitted",
      transactionHash: hash,
    })
  })
  it("checks_pending_transaction_under_the_lock", async () => {
    const m = await import("./mint-execution")
    const r = request()
    r.readPending = () => hash
    await m.startMintExecution(r)
    expect(r.send).not.toHaveBeenCalled()
    expect(m.mintExecutionSnapshot(r.key).transactionHash).toBe(hash)
  })
  it("bounds_diagnostics_without_exposing_request_data_or_raw_errors", async () => {
    const m = await import("./mint-execution")
    for (let i = 0; i < 25; i++) {
      const r = request()
      r.key += i
      r.prepare = async () => {
        throw new Error("secret rpc credential")
      }
      await m.startMintExecution(r)
    }
    const text = m.mintExecutionDiagnostics()
    expect(JSON.parse(text).execution).toHaveLength(100)
    expect(text).not.toContain("secret")
    expect(text).not.toContain("0xabc")
  })
})
