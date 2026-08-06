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
const mintExpectation = {
  depositId: `0x${"11".repeat(32)}` as const,
  authorizationDigest: `0x${"33".repeat(32)}` as const,
  recipient: `0x${"44".repeat(20)}` as const,
  grossAmount: "500000000",
  chargedServiceFee: "50000000",
  mintedAmount: "450000000",
}

describe("pending finalized confirmations", () => {
  beforeEach(() => window.localStorage.clear())

  it("persists and removes a deployment-scoped pending mint transaction", async () => {
    const transactionHash = `0x${"22".repeat(32)}` as const
    const pending = { ...mintExpectation, transactionHash }

    await savePendingMint(pending)
    expect(readPendingMint(mintExpectation)).toEqual(pending)

    await removePendingMint(mintExpectation)
    expect(readPendingMint(mintExpectation)).toBeUndefined()
  })

  it("retains a session recovery hash when durable storage is unavailable", async () => {
    const transactionHash = `0x${"44".repeat(32)}` as const
    const pending = { ...mintExpectation, transactionHash }
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("storage unavailable")
    })

    await expect(savePendingMint(pending)).resolves.toBeUndefined()
    expect(readPendingMint(mintExpectation)).toEqual(pending)

    setItem.mockRestore()
    await removePendingMint(mintExpectation)
  })

  it("ignores obsolete pending mint v1 and mismatched v2 payloads", () => {
    const legacyKey = [
      "kinic.bridge.pending-mint.v1",
      deploymentProfile.chainId,
      deploymentProfile.bridgeAddress?.toLowerCase(),
      deploymentProfile.bridgeCanisterId,
      mintExpectation.depositId,
    ].join(":")
    window.localStorage.setItem(legacyKey, `0x${"22".repeat(32)}`)
    expect(readPendingMint(mintExpectation)).toBeUndefined()
  })

  it("does not associate a saved mint with another authorization digest", async () => {
    await savePendingMint({ ...mintExpectation, transactionHash: `0x${"22".repeat(32)}` })
    expect(readPendingMint({
      ...mintExpectation,
      authorizationDigest: `0x${"55".repeat(32)}`,
    })).toBeUndefined()
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
