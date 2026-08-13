export type TransferAvailability = "Available" | "Paused" | "Unavailable" | "Unknown"

export interface BridgeAvailability {
  status: TransferAvailability
  available: boolean
  toBase: TransferAvailability
  toIc: TransferAvailability
}

export function bridgeAvailability(input: {
  observationsAccepted: boolean
  baseStatus?: { depositsPaused: boolean; withdrawalsPaused: boolean }
  icDepositsPaused?: boolean
  cyclesSufficient?: boolean
}): BridgeAvailability {
  const { baseStatus, icDepositsPaused, cyclesSufficient } = input
  if (!input.observationsAccepted || baseStatus === undefined || icDepositsPaused === undefined || cyclesSufficient === undefined) {
    return { status: "Unknown", available: false, toBase: "Unknown", toIc: "Unknown" }
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
  const available = toBase === "Available" || toIc === "Available"
  const status = available
    ? "Available"
    : toBase === "Paused" && toIc === "Paused"
      ? "Paused"
      : "Unavailable"
  return { status, available, toBase, toIc }
}

export const STATUS_FRESHNESS_MS = 60_000

export function statusDataIsFresh(input: {
  baseUpdatedAt?: number
  canisterUpdatedAt?: number
  now?: number
}): boolean {
  const now = input.now ?? Date.now()
  return [input.baseUpdatedAt, input.canisterUpdatedAt].every(
    (timestamp) => timestamp !== undefined && timestamp > 0 && timestamp <= now && now - timestamp <= STATUS_FRESHNESS_MS,
  )
}

export function displayCyclesSufficient(input: {
  cyclesBalance: bigint
  requiredCycles: bigint
}): boolean {
  return input.cyclesBalance >= input.requiredCycles
}
