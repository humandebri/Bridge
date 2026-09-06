import { beforeEach, describe, expect, it } from "vitest"
import { browserLocalStorage } from "./browser-lock"
import {
  bridgeProgressLabel,
  bridgeProgressSteps,
  createBridgeProgress,
  readLatestBridgeProgress,
  saveLatestBridgeProgress,
  withdrawalFinalityProgress,
} from "./bridge-progress"

beforeEach(() => browserLocalStorage().clear())

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
      baseTransactionOutcome: "success",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3", depositId: `0x${"07".repeat(32)}` },
      attentionMessage: "stale presentation",
    })

    saveLatestBridgeProgress(record)

    const storage = browserLocalStorage()
    const stored = Array.from({ length: storage.length }, (_, index) =>
      storage.getItem(storage.key(index)!),
    )[0]!
    expect(stored).toContain('"version":3')
    expect(stored).toContain('"tokenApproval":"not-required"')
    expect(stored).not.toContain("base-mint-finalizing")
    expect(stored).not.toContain("receiptBlockNumber")
    expect(stored).not.toContain("baseTransactionOutcome")
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
    expect(readLatestBridgeProgress()).toMatchObject({
      phase: "authorization-generating",
      deposit: accepted.deposit,
    })

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
    expect(readLatestBridgeProgress()).toMatchObject({
      phase: "attention",
      attentionMessage: attention.attentionMessage,
    })
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
    const storage = browserLocalStorage()
    const key = storage.key(0)!
    const stored = JSON.parse(storage.getItem(key)!) as { deposit: { ownerSequence: string } }
    stored.deposit.ownerSequence = "not-a-number"
    storage.setItem(key, JSON.stringify(stored))

    expect(readLatestBridgeProgress()).toBeUndefined()
  })

  it("does not restore the obsolete v2 progress format", () => {
    const current = createBridgeProgress({
      direction: "withdraw",
      phase: "base-withdrawal-submitted",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      transactionHash: `0x${"33".repeat(32)}`,
      withdrawal: { owner: "aaaaa-aa" },
    })
    saveLatestBridgeProgress(current)
    const storage = browserLocalStorage()
    const key = storage.key(0)!
    storage.setItem(
      key,
      JSON.stringify({
        version: 2,
        id: current.id,
        direction: "withdraw",
        phase: "base-withdrawal-submitted",
        source: "0x0000000000000000000000000000000000000002",
        destination: "aaaaa-aa",
        sendAmount: "2",
        receiveAmount: "1.5",
        sendSymbol: "KINIC",
        receiveSymbol: "TICRC1",
        createdAt: 1,
        transactionHash: `0x${"33".repeat(32)}`,
        withdrawal: { owner: "aaaaa-aa" },
      }),
    )

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
      attentionPhase: "authorization-generating",
    })

    expect(bridgeProgressLabel(record)).toBe("Bridge processing paused")
    expect(bridgeProgressSteps(record)).toEqual([
      { label: "IC token approval", status: "complete", note: "Not required" },
      { label: "IC deposit transaction", status: "complete" },
      { label: "Bridge authorization", status: "attention" },
      { label: "Base mint transaction", status: "waiting" },
    ])
  })

  it("ends Deposit presentation at a successful Base transaction without changing Withdrawal steps", () => {
    const deposit = createBridgeProgress({
      direction: "deposit",
      phase: "base-mint-included",
      source: "aaaaa-aa",
      destination: "0x0000000000000000000000000000000000000002",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "TICRC1",
      receiveSymbol: "KINIC",
      transactionHash: `0x${"22".repeat(32)}`,
      receiptBlockNumber: "123",
      baseTransactionOutcome: "success",
      deposit: { owner: "aaaaa-aa", ownerSequence: "3", depositId: `0x${"07".repeat(32)}` },
    })
    expect(bridgeProgressSteps(deposit)).toEqual([
      { label: "IC token approval", status: "complete", note: "Not required" },
      { label: "IC deposit transaction", status: "complete" },
      { label: "Bridge authorization", status: "complete" },
      { label: "Base mint transaction", status: "complete" },
    ])

    const withdrawal = createBridgeProgress({
      direction: "withdraw",
      phase: "base-withdrawal-finalizing",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      transactionHash: `0x${"33".repeat(32)}`,
      receiptBlockNumber: "123",
      withdrawal: { owner: "aaaaa-aa" },
    })
    expect(bridgeProgressSteps(withdrawal).map(({ label }) => label)).toEqual([
      "IC destination verification",
      "Base token approval",
      "Base withdrawal transaction",
      "Base finality",
      "IC notification",
      "Ledger payout",
      "Complete",
    ])
  })

  it("separates wallet approval, transaction, notification, and payout steps", () => {
    const withdrawal = createBridgeProgress({
      direction: "withdraw",
      phase: "verifying-ic-destination",
      tokenApproval: "required",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      withdrawal: { owner: "aaaaa-aa" },
    })

    expect(bridgeProgressSteps(withdrawal)).toEqual([
      { label: "IC destination verification", status: "current" },
      { label: "Base token approval", status: "waiting" },
      { label: "Base withdrawal transaction", status: "waiting" },
      { label: "Base finality", status: "waiting" },
      { label: "IC notification", status: "waiting" },
      { label: "Ledger payout", status: "waiting" },
      { label: "Complete", status: "waiting" },
    ])
    expect(bridgeProgressSteps({ ...withdrawal, phase: "awaiting-base-allowance" })[1]).toEqual({
      label: "Base token approval",
      status: "current",
    })
    expect(bridgeProgressSteps({ ...withdrawal, phase: "awaiting-base-withdrawal" })[2]).toEqual({
      label: "Base withdrawal transaction",
      status: "current",
    })
    expect(bridgeProgressSteps({ ...withdrawal, phase: "awaiting-ic-notification" })[4]).toEqual({
      label: "IC notification",
      status: "current",
    })
    expect(bridgeProgressSteps({ ...withdrawal, phase: "ledger-payout" })[5]).toEqual({
      label: "Ledger payout",
      status: "current",
    })
  })

  it("marks every Withdrawal step complete after the payout reaches its terminal state", () => {
    const withdrawal = createBridgeProgress({
      direction: "withdraw",
      phase: "complete",
      tokenApproval: "not-required",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      withdrawal: { owner: "aaaaa-aa", withdrawalId: `0x${"07".repeat(32)}` },
    })

    expect(bridgeProgressSteps(withdrawal)).toEqual([
      { label: "IC destination verification", status: "complete" },
      { label: "Base token approval", status: "complete", note: "Not required" },
      { label: "Base withdrawal transaction", status: "complete" },
      { label: "Base finality", status: "complete" },
      { label: "IC notification", status: "complete" },
      { label: "Ledger payout", status: "complete" },
      { label: "Complete", status: "complete" },
    ])
  })

  it("shows an unnecessary approval as complete and preserves the attention source step", () => {
    const withdrawal = createBridgeProgress({
      direction: "withdraw",
      phase: "attention",
      tokenApproval: "not-required",
      attentionPhase: "awaiting-base-withdrawal",
      attentionMessage: "Withdrawal failed.",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      withdrawal: { owner: "aaaaa-aa" },
    })

    expect(bridgeProgressSteps(withdrawal)[1]).toEqual({
      label: "Base token approval",
      status: "complete",
      note: "Not required",
    })
    expect(bridgeProgressSteps(withdrawal)[2]).toEqual({
      label: "Base withdrawal transaction",
      status: "attention",
    })
    expect(
      bridgeProgressSteps({ ...withdrawal, attentionPhase: "verifying-ic-destination" })[0],
    ).toEqual({ label: "IC destination verification", status: "attention" })
  })

  it("reports exact Withdrawal finality block progress without changing Deposit presentation", () => {
    const withdrawal = createBridgeProgress({
      direction: "withdraw",
      phase: "base-withdrawal-finalizing",
      source: "0x0000000000000000000000000000000000000002",
      destination: "aaaaa-aa",
      sendAmount: "2",
      receiveAmount: "1.5",
      sendSymbol: "KINIC",
      receiveSymbol: "TICRC1",
      receiptBlockNumber: "45115968",
      finalizedBlockNumber: "45115603",
      withdrawal: { owner: "aaaaa-aa" },
    })

    expect(withdrawalFinalityProgress(withdrawal)).toEqual({
      finalizedBlockNumber: "45,115,603",
      targetBlockNumber: "45,115,968",
      remainingBlocks: "365",
    })
    expect(withdrawalFinalityProgress({ ...withdrawal, direction: "deposit" })).toBeUndefined()
  })
})
