import { describe, expect, it } from "vitest"
import { kinicTransactionExplorerUrl } from "./transaction-explorer"

describe("KINIC transaction explorer URL", () => {
  it("builds the canonical SNS transaction URL", () => {
    expect(kinicTransactionExplorerUrl("7jkta-eyaaa-aaaaq-aaarq-cai", 97_754n)).toBe(
      "https://dashboard.internetcomputer.org/sns/7jkta-eyaaa-aaaaq-aaarq-cai/transaction/97754",
    )
  })

  it("does not link test profiles, invalid principals, or negative indexes", () => {
    expect(kinicTransactionExplorerUrl(null, 1n)).toBeUndefined()
    expect(kinicTransactionExplorerUrl("not-a-principal", 1n)).toBeUndefined()
    expect(kinicTransactionExplorerUrl("7jkta-eyaaa-aaaaq-aaarq-cai", -1n)).toBeUndefined()
  })
})
