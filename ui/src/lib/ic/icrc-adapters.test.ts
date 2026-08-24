import { beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  createAgent: vi.fn(),
  ledgerCreate: vi.fn(),
  indexCreate: vi.fn(),
  balance: vi.fn(),
  metadata: vi.fn(),
  allowance: vi.fn(),
  transactionFee: vi.fn(),
  ledgerId: vi.fn(),
  status: vi.fn(),
}))

vi.mock("@/lib/ic/agent", () => ({ createIcAgent: mocks.createAgent }))
vi.mock("@icp-sdk/canisters/ledger/icrc", () => ({
  IcrcLedgerCanister: { create: mocks.ledgerCreate },
  IcrcIndexCanister: { create: mocks.indexCreate },
}))

import { createIndexActor } from "./index"
import { createLedgerActor, ledgerAccount } from "./ledger"

describe("official ICRC adapters", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.createAgent.mockResolvedValue({})
    mocks.ledgerCreate.mockReturnValue({ balance: mocks.balance, metadata: mocks.metadata, allowance: mocks.allowance, transactionFee: mocks.transactionFee })
    mocks.indexCreate.mockReturnValue({ ledgerId: mocks.ledgerId, status: mocks.status })
    mocks.metadata.mockResolvedValue([["icrc1:name", { Text: "KINIC" }], ["icrc1:symbol", { Text: "KINIC" }], ["icrc1:decimals", { Nat: 8n }]])
    mocks.balance.mockResolvedValue(12n)
    mocks.allowance.mockResolvedValue({ allowance: 7n, expires_at: [] })
    mocks.transactionFee.mockResolvedValue(10n)
  })

  it("adapts ledger amounts, metadata, and allowance", async () => {
    const ledger = await createLedgerActor("http://127.0.0.1:4943", "aaaaa-aa")
    const owner = ledgerAccount("aaaaa-aa")
    expect(await ledger.icrc1_balance_of(owner)).toBe(12n)
    expect(await ledger.icrc1_fee()).toBe(10n)
    expect(await ledger.icrc1_name()).toBe("KINIC")
    expect(await ledger.icrc1_symbol()).toBe("KINIC")
    expect(await ledger.icrc1_decimals()).toBe(8)
    expect(await ledger.icrc2_allowance({ account: owner, spender: owner })).toEqual({ allowance: 7n, expires_at: [] })
    expect(mocks.metadata).toHaveBeenCalledOnce()
  })

  it("adapts index binding and status queries", async () => {
    mocks.ledgerId.mockResolvedValue({ toText: () => "aaaaa-aa" })
    mocks.status.mockResolvedValue({ num_blocks_synced: 3n })
    const index = await createIndexActor("https://icp-api.io", "aaaaa-aa")
    expect((await index.ledger_id()).toText()).toBe("aaaaa-aa")
    expect(await index.status()).toEqual({ num_blocks_synced: 3n })
  })
})
