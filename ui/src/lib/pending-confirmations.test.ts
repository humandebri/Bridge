import { beforeEach, describe, expect, it } from "vitest"
import { deploymentProfile } from "@/config/profile"
import {
  readPendingConfirmations,
  removePendingConfirmation,
  restorePendingConfirmation,
  savePendingConfirmation,
  setPendingConfirmationBlocked,
} from "./pending-confirmations"

const entry = {
  kind: "deposit" as const,
  settlementId: `0x${"11".repeat(32)}` as const,
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

  it("persists, updates, blocks, and removes a settlement", () => {
    savePendingConfirmation(entry)
    expect(readPendingConfirmations()).toEqual([{ ...entry, ...scope, blocked: false }])

    setPendingConfirmationBlocked(entry, true)
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)

    savePendingConfirmation({ ...entry, transactionHash: `0x${"33".repeat(32)}`, blocked: false })
    expect(readPendingConfirmations()).toHaveLength(1)
    expect(readPendingConfirmations()[0]).toMatchObject({ transactionHash: `0x${"33".repeat(32)}`, blocked: false })

    removePendingConfirmation(entry)
    expect(readPendingConfirmations()).toEqual([])
  })

  it("does not unblock a failed confirmation during History restoration", () => {
    savePendingConfirmation({ ...entry, blocked: true })
    restorePendingConfirmation(entry)
    expect(readPendingConfirmations()[0]?.blocked).toBe(true)
  })

  it("fails closed for malformed storage", () => {
    window.localStorage.setItem("kinic.bridge.pending-confirmations.v2", JSON.stringify([{ ...entry, ...scope, blocked: false, transactionHash: "0x12" }]))
    expect(readPendingConfirmations()).toEqual([])
  })

  it("persists withdrawals and ignores entries from another deployment", () => {
    const withdrawal = { kind: "withdrawal" as const, transactionHash: entry.transactionHash, owner: entry.owner }
    savePendingConfirmation(withdrawal)
    expect(readPendingConfirmations()).toEqual([{ ...withdrawal, ...scope, blocked: false }])

    window.localStorage.setItem("kinic.bridge.pending-confirmations.v2", JSON.stringify([{
      ...withdrawal,
      ...scope,
      chainId: scope.chainId + 1,
      blocked: false,
    }]))
    expect(readPendingConfirmations()).toEqual([])
  })

  it("does not migrate the obsolete v1 queue", () => {
    window.localStorage.setItem("kinic.bridge.pending-confirmations.v1", JSON.stringify([{ ...entry, blocked: false }]))
    expect(readPendingConfirmations()).toEqual([])
  })
})
