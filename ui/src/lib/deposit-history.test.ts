import { describe, expect, it } from "vitest"
import type { DepositView } from "@/generated/bridge.did"
import { depositIdsForRefresh, mergeDepositHistoryPage } from "./deposit-history"

describe("deposit history pagination", () => {
  it("preserves the deepest cursor on refresh and appends older pages without duplicates", () => {
    const first = mergeDepositHistoryPage(undefined, [deposit(5), deposit(4)], { nextCursor: 3n, oldestAvailableCursor: 1n, historyTruncated: false }, "refresh")
    const older = mergeDepositHistoryPage(first, [deposit(3), deposit(4)], { nextCursor: 2n, oldestAvailableCursor: 1n, historyTruncated: true }, "older")
    const refreshed = mergeDepositHistoryPage(older, [deposit(6), deposit(5)], { nextCursor: 4n, oldestAvailableCursor: 1n, historyTruncated: false }, "refresh")

    expect(refreshed.items.map((item) => item.owner_sequence)).toEqual([6n, 5n, 4n, 3n])
    expect(refreshed.nextCursor).toBe(2n)
    expect(refreshed.historyTruncated).toBe(true)
  })

  it("replaces a cached deposit with its refreshed state", () => {
    const pending = deposit(5)
    pending.state = { AuthorizationPending: null }
    const cached = mergeDepositHistoryPage(undefined, [pending], { nextCursor: null, oldestAvailableCursor: 1n, historyTruncated: false }, "refresh")
    const minted = deposit(5)

    const refreshed = mergeDepositHistoryPage(cached, [minted], { nextCursor: null, oldestAvailableCursor: 1n, historyTruncated: false }, "refresh")

    expect(refreshed.items).toHaveLength(1)
    expect(refreshed.items[0]?.state).toEqual({ Minted: null })
  })

  it("refreshes only the newest page and explicitly selected nonterminal cached deposits", () => {
    const cached = mergeDepositHistoryPage(undefined, Array.from({ length: 21 }, (_, index) => deposit(21 - index)), { nextCursor: null, oldestAvailableCursor: 1n, historyTruncated: false }, "refresh")
    const latestIds = Array.from({ length: 20 }, (_, index) => new Uint8Array(32).fill(22 - index))

    const ids = depositIdsForRefresh(cached, latestIds, (record) => record.owner_sequence === 1n)

    expect(ids).toHaveLength(21)
    expect(ids.some((id) => id.every((byte) => byte === 1))).toBe(true)
  })
})

function deposit(sequence: number): DepositView {
  return {
    deposit_id: new Uint8Array(32).fill(sequence),
    owner_sequence: BigInt(sequence),
    created_at_ns: BigInt(sequence),
    gross_amount: 100n,
    quote: [{ net_amount: 90n, service_fee: 10n }],
    refund: [],
    available_refund_amount: [],
    max_service_fee: 10n,
    from_subaccount: [],
    base_recipient: new Uint8Array(20),
    state: { Minted: null },
    last_settlement_stop_reason: [],
    mint_authorization: [],
    automatic_progress: [],
  }
}
