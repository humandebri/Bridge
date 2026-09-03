import { cleanup, render, screen } from "@testing-library/react"
import { createElement } from "react"
import { deploymentProfile } from "@/config/profile"
import type { DepositView, WithdrawalView } from "@/generated/bridge.did"
import {
  depositKinicTransactions,
  KinicTransactionLink,
  withdrawalKinicTransactions,
} from "@/routes/history"
import { afterEach, describe, expect, it } from "vitest"

const originalSnsRootCanisterId = deploymentProfile.snsRootCanisterId

afterEach(() => {
  cleanup()
  deploymentProfile.snsRootCanisterId = originalSnsRootCanisterId
})

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
    expect(
      withdrawalKinicTransactions({ release_ledger_block_index: [] } as unknown as WithdrawalView),
    ).toEqual([])
    expect(
      withdrawalKinicTransactions({
        release_ledger_block_index: [99n],
      } as unknown as WithdrawalView),
    ).toEqual([{ kind: "payout", blockIndex: 99n }])
  })

  it("renders a block number without a link when the deployment has no SNS Root", () => {
    deploymentProfile.snsRootCanisterId = null
    render(createElement(KinicTransactionLink, { kind: "deposit", blockIndex: 41n }))

    expect(screen.getByText("Deposit #41")).toBeInTheDocument()
    expect(screen.queryByRole("link")).not.toBeInTheDocument()
  })

  it("links through the deployment-specific SNS Root when configured", () => {
    deploymentProfile.snsRootCanisterId = "7jkta-eyaaa-aaaaq-aaarq-cai"
    render(createElement(KinicTransactionLink, { kind: "payout", blockIndex: 97_754n }))

    expect(
      screen.getByRole("link", { name: "Open KINIC payout transaction 97754 in explorer" }),
    ).toHaveAttribute(
      "href",
      "https://dashboard.internetcomputer.org/sns/7jkta-eyaaa-aaaaq-aaarq-cai/transaction/97754",
    )
  })
})
