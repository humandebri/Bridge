import type { DepositView, WithdrawalView } from "@/generated/bridge.did"

export type ActivityFilter = "all" | "to-base" | "to-ic"
export type ActivityDirection = Exclude<ActivityFilter, "all">

export interface WithdrawalHistoryItem {
  id?: bigint
  amount?: bigint
  amountOut?: bigint
  hash: `0x${string}`
  blockNumber: bigint
  logIndex: number
  createdAtNs: bigint
  canister?: WithdrawalView
}

export type ActivityItem =
  | {
      key: string
      direction: "to-base"
      createdAtNs: bigint
      deposit: DepositView
    }
  | {
      key: string
      direction: "to-ic"
      createdAtNs: bigint
      withdrawal: WithdrawalHistoryItem
    }

export interface ActivityBoundary {
  enabled: boolean
  hasMore: boolean
  unseenBeforeNs?: bigint
}

export interface ActivityBoundaries {
  deposit: ActivityBoundary
  withdrawal: ActivityBoundary
}

export function activityAutoRefreshEnabled(pageVisible: boolean, icConnected: boolean, evmConnected: boolean): boolean {
  return pageVisible && (icConnected || evmConnected)
}

export function mergeActivityItems(deposits: DepositView[], withdrawals: WithdrawalHistoryItem[]): ActivityItem[] {
  const unique = new Map<string, ActivityItem>()
  for (const deposit of deposits) {
    const key = `deposit:${bytesKey(deposit.deposit_id)}`
    unique.set(key, { key, direction: "to-base", createdAtNs: deposit.created_at_ns, deposit })
  }
  for (const withdrawal of withdrawals) {
    const key = `withdrawal:${withdrawal.hash.toLowerCase()}:${withdrawal.logIndex}`
    unique.set(key, { key, direction: "to-ic", createdAtNs: withdrawal.createdAtNs, withdrawal })
  }
  return [...unique.values()].sort((left, right) => {
    if (left.createdAtNs !== right.createdAtNs) return left.createdAtNs > right.createdAtNs ? -1 : 1
    return left.key.localeCompare(right.key)
  })
}

export function visibleActivityItems(items: ActivityItem[], filter: ActivityFilter, boundaries: ActivityBoundaries): ActivityItem[] {
  const filtered = filter === "all" ? items : items.filter((item) => item.direction === filter)
  if (filter !== "all") return filtered
  const enabled = enabledBoundaries(boundaries)
  if (enabled.length < 2) return filtered
  const unfinished = enabled.filter((entry) => entry.boundary.hasMore)
  if (!unfinished.length) return filtered
  if (unfinished.some((entry) => entry.boundary.unseenBeforeNs === undefined)) return []
  const cutoff = unfinished.reduce((latest, entry) => {
    const value = entry.boundary.unseenBeforeNs as bigint
    return latest === undefined || value > latest ? value : latest
  }, undefined as bigint | undefined)
  return cutoff === undefined ? filtered : filtered.filter((item) => item.createdAtNs > cutoff)
}

export function olderActivitySources(filter: ActivityFilter, boundaries: ActivityBoundaries): ActivityDirection[] {
  if (filter === "to-base") return boundaries.deposit.enabled && boundaries.deposit.hasMore ? ["to-base"] : []
  if (filter === "to-ic") return boundaries.withdrawal.enabled && boundaries.withdrawal.hasMore ? ["to-ic"] : []
  const unfinished = enabledBoundaries(boundaries).filter((entry) => entry.boundary.hasMore)
  if (unfinished.length <= 1) return unfinished.map((entry) => entry.direction)
  const unknown = unfinished.filter((entry) => entry.boundary.unseenBeforeNs === undefined)
  if (unknown.length) return unknown.map((entry) => entry.direction)
  const latest = unfinished.reduce((value, entry) => {
    const boundary = entry.boundary.unseenBeforeNs as bigint
    return value === undefined || boundary > value ? boundary : value
  }, undefined as bigint | undefined)
  return unfinished.filter((entry) => entry.boundary.unseenBeforeNs === latest).map((entry) => entry.direction)
}

function enabledBoundaries(boundaries: ActivityBoundaries): Array<{ direction: ActivityDirection; boundary: ActivityBoundary }> {
  return [
    { direction: "to-base", boundary: boundaries.deposit },
    { direction: "to-ic", boundary: boundaries.withdrawal },
  ].filter((entry) => entry.boundary.enabled) as Array<{ direction: ActivityDirection; boundary: ActivityBoundary }>
}

function bytesKey(value: Uint8Array | number[]): string {
  return Array.from(value, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")
}
