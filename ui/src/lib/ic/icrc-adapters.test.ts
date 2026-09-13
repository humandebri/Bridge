import { beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  createAgent: vi.fn(),
  ledgerCreate: vi.fn(),
  balance: vi.fn(),
  metadata: vi.fn(),
  allowance: vi.fn(),
  transactionFee: vi.fn(),
}))

vi.mock("@/lib/ic/agent", () => ({ createIcAgent: mocks.createAgent }))
vi.mock("@icp-sdk/canisters/ledger/icrc", () => ({
  IcrcLedgerCanister: { create: mocks.ledgerCreate },
}))

import { createLedgerActor, ledgerAccount } from "./ledger"

describe("official ICRC adapters", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.createAgent.mockResolvedValue({})
    mocks.ledgerCreate.mockReturnValue({
      balance: mocks.balance,
      metadata: mocks.metadata,
      allowance: mocks.allowance,
      transactionFee: mocks.transactionFee,
    })
    mocks.metadata.mockResolvedValue([
      ["icrc1:name", { Text: "KINIC" }],
      ["icrc1:symbol", { Text: "KINIC" }],
      ["icrc1:decimals", { Nat: 8n }],
    ])
    mocks.allowance.mockResolvedValue({ allowance: 7n, expires_at: [] })
  })

  it("extracts metadata once and adapts account arguments", async () => {
    const ledger = await createLedgerActor("http://127.0.0.1:4943", "aaaaa-aa")
    const subaccount = new Uint8Array(32).fill(7)
    const owner = ledgerAccount("aaaaa-aa", subaccount)
    const spender = ledgerAccount("2vxsx-fae")
    await ledger.icrc1_balance_of(owner)
    expect(mocks.balance).toHaveBeenCalledWith({ owner: owner.owner, subaccount, certified: false })
    await ledger.icrc1_balance_of(spender)
    expect(mocks.balance).toHaveBeenLastCalledWith({
      owner: spender.owner,
      subaccount: undefined,
      certified: false,
    })
    expect(await ledger.icrc1_name()).toBe("KINIC")
    expect(await ledger.icrc1_symbol()).toBe("KINIC")
    expect(await ledger.icrc1_decimals()).toBe(8)
    await ledger.icrc2_allowance({ account: owner, spender })
    expect(mocks.allowance).toHaveBeenCalledWith({
      account: { owner: owner.owner, subaccount: [subaccount] },
      spender: { owner: spender.owner, subaccount: [] },
      certified: false,
    })
    expect(mocks.metadata).toHaveBeenCalledOnce()
  })
})
