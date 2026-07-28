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
  reserveSufficient?: boolean
}): BridgeAvailability {
  const { baseStatus, icDepositsPaused, reserveSufficient } = input
  if (!input.runtimeReady || baseStatus === undefined || icDepositsPaused === undefined || reserveSufficient === undefined) {
    return { available: false, toBase: "Unavailable", toIc: "Unavailable" }
  }

  const toBase = baseStatus.depositsPaused || icDepositsPaused
    ? "Paused"
    : reserveSufficient
      ? "Available"
      : "Unavailable"
  const toIc = baseStatus.withdrawalsPaused ? "Paused" : "Available"
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

export function displayReserveSufficient(input: {
  finalizedSignerBalance: bigint
  safeSignerBalance: bigint
  requiredEthWei: bigint
  cyclesBalance: bigint
  requiredCycles: bigint
}): boolean {
  const confirmedEthBalance = input.finalizedSignerBalance < input.safeSignerBalance
    ? input.finalizedSignerBalance
    : input.safeSignerBalance
  return confirmedEthBalance >= input.requiredEthWei && input.cyclesBalance >= input.requiredCycles
}
