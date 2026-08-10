import type { DepositView, WithdrawalView } from "@/generated/bridge.did"
import { depositKinicTransactions, withdrawalKinicTransactions } from "@/routes/history"
import { describe, expect, it } from "vitest"

describe("History KINIC transactions", () => {
  it("shows both the funding and completed refund blocks in order", () => {
    const record = {
      funding_ledger_block_index: [41n],
      refund: [{ refund_ledger_block_index: [43n] }],
    } as unknown as DepositView

    expect(depositKinicTransactions(record)).toEqual([
      { kind: "deposit", blockIndex: 41n },
      { kind: "refund", blockIndex: 43n },
    ])
  })

  it("does not present an unconfirmed funding or refund transfer", () => {
    const record = {
      funding_ledger_block_index: [],
      refund: [{ refund_ledger_block_index: [] }],
    } as unknown as DepositView

    expect(depositKinicTransactions(record)).toEqual([])
  })

  it("shows a payout only after the withdrawal release is confirmed", () => {
    expect(withdrawalKinicTransactions(undefined)).toEqual([])
    expect(withdrawalKinicTransactions({ release_ledger_block_index: [] } as unknown as WithdrawalView)).toEqual([])
    expect(withdrawalKinicTransactions({ release_ledger_block_index: [99n] } as unknown as WithdrawalView)).toEqual([
      { kind: "payout", blockIndex: 99n },
    ])
  })
})
