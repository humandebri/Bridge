export type TransferAvailability = "Available" | "Paused" | "Unavailable"

export interface BridgeAvailability {
  available: boolean
  toBase: TransferAvailability
  toIc: TransferAvailability
}

export function bridgeAvailability(input: {
  runtimeReady: boolean
  baseStatus?: { depositsPaused: boolean; withdrawalsPaused: boolean }
  icDepositsPaused?: boolean
  cyclesSufficient?: boolean
}): BridgeAvailability {
  const { baseStatus, icDepositsPaused, cyclesSufficient } = input
  if (!input.runtimeReady || baseStatus === undefined || icDepositsPaused === undefined || cyclesSufficient === undefined) {
    return { available: false, toBase: "Unavailable", toIc: "Unavailable" }
  }

  const toBase = baseStatus.depositsPaused || icDepositsPaused
    ? "Paused"
    : cyclesSufficient
      ? "Available"
      : "Unavailable"
  const toIc = baseStatus.withdrawalsPaused
    ? "Paused"
    : cyclesSufficient
      ? "Available"
      : "Unavailable"
  return { available: toBase === "Available" || toIc === "Available", toBase, toIc }
}

export const STATUS_FRESHNESS_MS = 60_000

export function statusDataIsFresh(input: {
  runtimeCheckedAt?: number
  baseUpdatedAt?: number
  canisterUpdatedAt?: number
  now?: number
}): boolean {
  const now = input.now ?? Date.now()
  return [input.runtimeCheckedAt, input.baseUpdatedAt, input.canisterUpdatedAt].every(
    (timestamp) => timestamp !== undefined && timestamp > 0 && timestamp <= now && now - timestamp <= STATUS_FRESHNESS_MS,
  )
}

export function displayCyclesSufficient(input: {
  cyclesBalance: bigint
  requiredCycles: bigint
}): boolean {
  return input.cyclesBalance >= input.requiredCycles
}
