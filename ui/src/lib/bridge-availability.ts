export type TransferAvailability = "Available" | "Paused" | "Unavailable"

export interface BridgeAvailability {
  available: boolean
  toBase: TransferAvailability
  toIc: TransferAvailability
}

export function bridgeAvailability(input: {
  runtimeReady: boolean
  baseStatus?: { depositsPaused: boolean; withdrawalsPaused: boolean }
  reserveSufficient?: boolean
}): BridgeAvailability {
  const { baseStatus, reserveSufficient } = input
  if (!input.runtimeReady || baseStatus === undefined || reserveSufficient === undefined) {
    return { available: false, toBase: "Unavailable", toIc: "Unavailable" }
  }

  const toBase = baseStatus.depositsPaused
    ? "Paused"
    : reserveSufficient
      ? "Available"
      : "Unavailable"
  const toIc = baseStatus.withdrawalsPaused ? "Paused" : "Available"
  return { available: toBase === "Available" || toIc === "Available", toBase, toIc }
}
