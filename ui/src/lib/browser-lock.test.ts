import { describe, expect, it, vi } from "vitest"
import { withBrowserLock } from "./browser-lock"

describe("browser lock fallback", () => {
  it("holds a localStorage lease while serializing work without Web Locks", async () => {
    let releaseFirst!: () => void
    let firstEntered!: () => void
    const entered = new Promise<void>((resolve) => { firstEntered = resolve })
    const blocked = new Promise<void>((resolve) => { releaseFirst = resolve })
    const order: string[] = []

    const first = withBrowserLock("fallback-test", async () => {
      order.push("first")
      firstEntered()
      await blocked
    })
    await entered
    expect(Object.keys(window.localStorage).some((key) => key.includes("browser-lock.v1:fallback-test"))).toBe(true)

    const second = withBrowserLock("fallback-test", () => { order.push("second") })
    await new Promise((resolve) => window.setTimeout(resolve, 50))
    expect(order).toEqual(["first"])

    releaseFirst()
    await Promise.all([first, second])
    expect(order).toEqual(["first", "second"])
  })

  it("fails closed for wallet work whenever Web Locks are unavailable", async () => {
    const action = vi.fn()

    await expect(withBrowserLock("kinic-wallet-prompt:ic:owner", action)).rejects.toThrow("Web Locks are required")
    expect(action).not.toHaveBeenCalled()
  })

  it("allows session-only bookkeeping to degrade to a local queue", async () => {
    const getItem = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("denied", "SecurityError")
    })
    const action = vi.fn(() => "saved in session")

    await expect(withBrowserLock("kinic-storage:test", action)).resolves.toBe("saved in session")
    expect(action).toHaveBeenCalledOnce()
    getItem.mockRestore()
  })
})
