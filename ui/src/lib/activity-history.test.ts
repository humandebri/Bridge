import { describe, expect, it } from "vitest"
import type { DepositView } from "@/generated/bridge.did"
import {
  activityAutoRefreshEnabled,
  mergeActivityItems,
  olderActivitySources,
  visibleActivityItems,
  type ActivityBoundaries,
  type WithdrawalHistoryItem,
} from "./activity-history"

describe("activity history", () => {
  it("keeps polling while a connected activity source is visible", () => {
    expect(activityAutoRefreshEnabled(true, true, false)).toBe(true)
    expect(activityAutoRefreshEnabled(true, false, true)).toBe(true)
    expect(activityAutoRefreshEnabled(true, false, false)).toBe(false)
    expect(activityAutoRefreshEnabled(false, true, true)).toBe(false)
  })

  it("merges both directions newest-first with a stable tie break", () => {
    const items = mergeActivityItems(
      [deposit(1, 20n), deposit(2, 10n)],
      [withdrawal("0xbb", 0, 30n), withdrawal("0xaa", 1, 20n)],
    )

    expect(items.map((item) => item.key)).toEqual([
      "withdrawal:0xbb:0",
      `deposit:${"01".repeat(32)}`,
      "withdrawal:0xaa:1",
      `deposit:${"02".repeat(32)}`,
    ])
  })

  it("replaces duplicate records instead of rendering duplicate rows", () => {
    const first = withdrawal("0xaa", 1, 20n)
    const refreshed = { ...first, amountOut: 99n }

    const items = mergeActivityItems([], [first, refreshed])

    expect(items).toHaveLength(1)
    expect(items[0]?.direction === "to-ic" && items[0].withdrawal.amountOut).toBe(99n)
  })

  it("holds rows older than the newest unseen boundary in the combined feed", () => {
    const items = mergeActivityItems(
      [deposit(1, 50n), deposit(2, 20n)],
      [withdrawal("0xaa", 0, 40n), withdrawal("0xbb", 0, 30n)],
    )
    const boundaries = bothBoundaries(20n, 30n)

    expect(visibleActivityItems(items, "all", boundaries).map((item) => item.createdAtNs)).toEqual([50n, 40n])
    expect(visibleActivityItems(items, "to-base", boundaries).map((item) => item.createdAtNs)).toEqual([50n, 20n])
  })

  it("loads the source that currently limits the safe combined boundary", () => {
    expect(olderActivitySources("all", bothBoundaries(20n, 30n))).toEqual(["to-ic"])
    expect(olderActivitySources("to-base", bothBoundaries(20n, 30n))).toEqual(["to-base"])
  })

  it("shows a connected source directly when the other source is unavailable", () => {
    const items = mergeActivityItems([deposit(1, 20n)], [])
    const boundaries = bothBoundaries(20n, undefined)
    boundaries.withdrawal.enabled = false

    expect(visibleActivityItems(items, "all", boundaries)).toEqual(items)
  })
})

function bothBoundaries(depositNs?: bigint, withdrawalNs?: bigint): ActivityBoundaries {
  return {
    deposit: { enabled: true, hasMore: depositNs !== undefined, unseenBeforeNs: depositNs },
    withdrawal: { enabled: true, hasMore: withdrawalNs !== undefined, unseenBeforeNs: withdrawalNs },
  }
}

function deposit(sequence: number, createdAtNs: bigint): DepositView {
  return {
    deposit_id: new Uint8Array(32).fill(sequence),
    owner_sequence: BigInt(sequence),
    created_at_ns: createdAtNs,
    gross_amount: 100n,
    quote: [{ net_amount: 90n, service_fee: 10n }],
    refund: [],
    max_service_fee: 10n,
    from_subaccount: [],
    base_recipient: new Uint8Array(20),
    state: { Minted: null },
    last_settlement_stop_reason: [],
    mint_authorization: [],
    automatic_progress: [],
  }
}

function withdrawal(hash: `0x${string}`, logIndex: number, createdAtNs: bigint): WithdrawalHistoryItem {
  return {
    id: BigInt(logIndex),
    amount: 100n,
    amountOut: 90n,
    hash,
    blockNumber: createdAtNs,
    logIndex,
    createdAtNs,
  }
}
