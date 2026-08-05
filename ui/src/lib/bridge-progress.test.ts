import { beforeEach, describe, expect, it } from "vitest"
import {
  bridgeProgressSteps,
  createBridgeProgress,
  readLatestBridgeProgress,
  saveLatestBridgeProgress,
} from "./bridge-progress"

beforeEach(() => window.localStorage.clear())

describe("latest bridge progress persistence", () => {
  it("persists_immutable_transfer_identity_and_a_broadcast_hash_without_persisting_observed_presentation_state", () => {
    const record = createBridgeProgress({
      direction: "deposit",
      phase: "base-mint-finalizing",
      source: "aaaaa-aa",
      destination: "0x0000000000000000000000000000000000000002",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "TICRC1",
      receiveSymbol: "KINIC",
      transactionHash: `0x${"22".repeat(32)}`,
      receiptBlockNumber: "123",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3", depositId: `0x${"07".repeat(32)}` },
      attentionMessage: "stale presentation",
    })

    saveLatestBridgeProgress(record)

    const stored = Array.from({ length: window.localStorage.length }, (_, index) => window.localStorage.getItem(window.localStorage.key(index)!))[0]!
    expect(stored).not.toContain("base-mint-finalizing")
    expect(stored).not.toContain("receiptBlockNumber")
    expect(stored).not.toContain("stale presentation")
    expect(readLatestBridgeProgress()).toMatchObject({
      id: record.id,
      phase: "base-mint-submitted",
      transactionHash: record.transactionHash,
      deposit: record.deposit,
    })
  })

  it("removes a completed latest transfer instead of restoring a stale completion", () => {
    const record = createBridgeProgress({
      direction: "withdraw",
      phase: "complete",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
    })

    saveLatestBridgeProgress(record)
    expect(readLatestBridgeProgress()).toBeUndefined()
  })

  it("does not restore a planned deposit as canonically accepted", () => {
    const record = createBridgeProgress({
      direction: "deposit",
      phase: "awaiting-ic-deposit",
      source: "aaaaa-aa",
      destination: "0x0000000000000000000000000000000000000002",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "TICRC1",
      receiveSymbol: "KINIC",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3" },
    })

    saveLatestBridgeProgress(record)

    expect(readLatestBridgeProgress()).toBeUndefined()
  })

  it("restores accepted deposits and terminal attention without inferring from optional fields", () => {
    const accepted = createBridgeProgress({
      direction: "deposit",
      phase: "ic-deposit-accepted",
      source: "aaaaa-aa",
      destination: "0x0000000000000000000000000000000000000002",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "TICRC1",
      receiveSymbol: "KINIC",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3", depositId: `0x${"07".repeat(32)}` },
    })
    saveLatestBridgeProgress(accepted)
    expect(readLatestBridgeProgress()).toMatchObject({ phase: "authorization-generating", deposit: accepted.deposit })

    const attention = createBridgeProgress({
      direction: "withdraw",
      phase: "attention",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      transactionHash: `0x${"33".repeat(32)}`,
      attentionMessage: "The Base transaction reverted.",
    })
    saveLatestBridgeProgress(attention)
    expect(readLatestBridgeProgress()).toMatchObject({ phase: "attention", attentionMessage: attention.attentionMessage })
  })

  it("rejects malformed nested identities from browser storage", () => {
    const record = createBridgeProgress({
      direction: "deposit",
      phase: "authorization-generating",
      source: "aaaaa-aa",
      destination: "0x0000000000000000000000000000000000000002",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "TICRC1",
      receiveSymbol: "KINIC",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3", depositId: `0x${"07".repeat(32)}` },
    })
    saveLatestBridgeProgress(record)
    const key = window.localStorage.key(0)!
    const stored = JSON.parse(window.localStorage.getItem(key)!) as { deposit: { ownerSequence: string } }
    stored.deposit.ownerSequence = "not-a-number"
    window.localStorage.setItem(key, JSON.stringify(stored))

    expect(readLatestBridgeProgress()).toBeUndefined()
  })

  it("places accepted Deposit attention after the canonical IC deposit", () => {
    const record = createBridgeProgress({
      direction: "deposit",
      phase: "attention",
      source: "aaaaa-aa",
      destination: "0x0000000000000000000000000000000000000002",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "TICRC1",
      receiveSymbol: "KINIC",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3", depositId: `0x${"07".repeat(32)}` },
      attentionMessage: "Review the canonical refund state.",
    })

    expect(bridgeProgressSteps(record)).toEqual([
      { label: "IC wallet", status: "complete" },
      { label: "IC deposit", status: "complete" },
      { label: "Bridge authorization", status: "current" },
      { label: "Base transaction", status: "waiting" },
      { label: "Base finality", status: "waiting" },
      { label: "Complete", status: "waiting" },
    ])
  })
})
