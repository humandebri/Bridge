import type { DepositView } from "@/generated/bridge.did"

export interface DepositHistoryData {
  items: DepositView[]
  nextCursor: bigint | null
  oldestAvailableCursor: bigint | null
  historyTruncated: boolean
}

interface DepositPage {
  nextCursor: bigint | null
  oldestAvailableCursor: bigint | null
  historyTruncated: boolean
}

export function mergeDepositHistoryPage(previous: DepositHistoryData | undefined, additions: DepositView[], page: DepositPage, mode: "refresh" | "older"): DepositHistoryData {
  // Map keeps the last value for a duplicate deposit ID. Newly fetched records
  // must follow the cache so state transitions replace stale entries.
  const records = [...(previous?.items ?? []), ...additions]
  const unique = new Map(records.map((record) => [depositKey(record), record]))
  const items = [...unique.values()].sort((left, right) => left.owner_sequence === right.owner_sequence ? 0 : left.owner_sequence > right.owner_sequence ? -1 : 1)
  return {
    items,
    nextCursor: mode === "refresh" && previous ? previous.nextCursor : page.nextCursor,
    oldestAvailableCursor: page.oldestAvailableCursor ?? previous?.oldestAvailableCursor ?? null,
    historyTruncated: page.historyTruncated || Boolean(previous?.historyTruncated),
  }
}

function depositKey(record: DepositView): string {
  return Array.from(record.deposit_id, (value) => Number(value).toString(16).padStart(2, "0")).join("")
}
