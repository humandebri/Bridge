import { beforeEach, describe, expect, it, vi } from "vitest"
import { deploymentProfile } from "@/config/profile"
import {
  readPendingMint,
  readPendingConfirmations,
  removePendingMint,
  removePendingConfirmation,
  restorePendingConfirmation,
  savePendingMint,
  savePendingConfirmation,
  setPendingConfirmationBlocked,
  upsertPendingConfirmation,
} from "./pending-confirmations"

const entry = {
  kind: "withdrawal" as const,
  transactionHash: `0x${"22".repeat(32)}` as const,
  owner: "aaaaa-aa",
}
const scope = {
  bridgeCanisterId: deploymentProfile.bridgeCanisterId ?? "",
  chainId: deploymentProfile.chainId,
  bridgeAddress: deploymentProfile.bridgeAddress?.toLowerCase() ?? "",
}

describe("pending finalized confirmations", () => {
  beforeEach(() => window.localStorage.clear())

  it("persists and removes a deployment-scoped pending mint transaction", async () => {
    const depositId = `0x${"11".repeat(32)}` as const
    const transactionHash = `0x${"22".repeat(32)}` as const

    await savePendingMint(depositId, transactionHash)
    expect(readPendingMint(depositId)).toBe(transactionHash)

    await removePendingMint(depositId)
    expect(readPendingMint(depositId)).toBeUndefined()
  })

  it("retains a session recovery hash when durable storage is unavailable", async () => {
    const depositId = `0x${"33".repeat(32)}` as const
    const transactionHash = `0x${"44".repeat(32)}` as const
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage unavailable")
    })

    await expect(savePendingMint(depositId, transactionHash)).resolves.toBeUndefined()
    expect(readPendingMint(depositId)).toBe(transactionHash)

    setItem.mockRestore()
    await removePendingMint(depositId)
  })

  it("persists, updates, blocks, and removes a settlement", async () => {
    await savePendingConfirmation(entry)
    expect(readPendingConfirmations()).toEqual([{ ...entry, ...scope, blocked: false }])

    await setPendingConfirmationBlocked(entry, true)
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)

    await savePendingConfirmation({ ...entry, owner: "updated-owner", blocked: false })
    expect(readPendingConfirmations()).toHaveLength(1)
    expect(readPendingConfirmations()[0]).toMatchObject({ owner: "updated-owner", blocked: false })

    await removePendingConfirmation(entry)
    expect(readPendingConfirmations()).toEqual([])
  })

  it("does not unblock a failed confirmation during History restoration", async () => {
    await savePendingConfirmation({ ...entry, blocked: true })
    await restorePendingConfirmation(entry)
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)
  })

  it("fails closed for malformed storage", () => {
    const key = `kinic.bridge.pending-confirmations.v5:${scope.chainId}:${scope.bridgeAddress}:${scope.bridgeCanisterId}`
    window.localStorage.setItem(key, JSON.stringify([{ ...entry, ...scope, blocked: false, transactionHash: "0x12" }]))
    expect(readPendingConfirmations()).toEqual([])
  })

  it("ignores withdrawals from another deployment", async () => {
    await savePendingConfirmation(entry)
    expect(readPendingConfirmations()).toEqual([{ ...entry, ...scope, blocked: false }])

    const key = `kinic.bridge.pending-confirmations.v5:${scope.chainId}:${scope.bridgeAddress}:${scope.bridgeCanisterId}`
    window.localStorage.setItem(key, JSON.stringify({ version: 5, entries: [{
      ...entry,
      ...scope,
      chainId: scope.chainId + 1,
      blocked: false,
    }] }))
    expect(readPendingConfirmations()).toEqual([])
    await savePendingConfirmation(entry)
    const stored = JSON.parse(window.localStorage.getItem(key) ?? "null") as { entries: unknown[] }
    expect(stored.entries).toHaveLength(1)
  })

  it("does not migrate obsolete queue versions", () => {
    window.localStorage.setItem("kinic.bridge.pending-confirmations.v1", JSON.stringify([{ ...entry, blocked: false }]))
    window.localStorage.setItem(
      `kinic.bridge.pending-confirmations.v4:${scope.chainId}:${scope.bridgeAddress}:${scope.bridgeCanisterId}`,
      JSON.stringify({ version: 4, entries: [{ ...entry, ...scope, blocked: false }] }),
    )
    expect(readPendingConfirmations()).toEqual([])
  })

  it("repairs canonical owner metadata without clearing a blocked state", async () => {
    await savePendingConfirmation({ ...entry, owner: "wrong-owner", blocked: true })
    await restorePendingConfirmation(entry)
    expect(readPendingConfirmations()[0]).toMatchObject({ owner: entry.owner, blocked: true })
  })

  it("pure serialized upserts preserve a different settlement and a blocked retry", () => {
    const first = { ...entry, ...scope, blocked: true }
    const second = {
      ...entry,
      ...scope,
      transactionHash: `0x${"55".repeat(32)}` as const,
      blocked: false,
    }
    const saved = upsertPendingConfirmation([first], second, false)
    expect(saved).toEqual([first, second])

    const restored = upsertPendingConfirmation(saved, { ...first, owner: "canonical-owner", blocked: false }, true)
    expect(restored).toEqual([{ ...first, owner: "canonical-owner", blocked: true }, second])
  })

  it("retains the session queue when durable storage is unavailable", async () => {
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage unavailable")
    })
    await expect(savePendingConfirmation(entry)).rejects.toThrow("storage unavailable")
    expect(readPendingConfirmations()).toEqual([{ ...entry, ...scope, blocked: false }])
    setItem.mockRestore()
    await removePendingConfirmation(entry)
  })

  it("keeps a failed durable deletion authoritative in the session", async () => {
    await savePendingConfirmation(entry)
    const originalSetItem = window.localStorage.setItem.bind(window.localStorage)
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation((key, value) => {
      if (key.startsWith("kinic.bridge.pending-confirmations")) throw new Error("storage unavailable")
      return originalSetItem(key, value)
    })

    await expect(removePendingConfirmation(entry)).rejects.toThrow("storage unavailable")
    expect(readPendingConfirmations()).toEqual([])

    setItem.mockRestore()
    await removePendingConfirmation(entry)
  })
})
