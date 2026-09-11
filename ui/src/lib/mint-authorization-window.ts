export const MINT_AUTHORIZATION_TTL_SECONDS = 900n
const MAX_NAT64 = (1n << 64n) - 1n

export interface MintAuthorizationWindow {
  deadline: bigint
  remainingSeconds: bigint
  isUnexpired: boolean
}

export function hasCanonicalMintAuthorizationDeadline(
  issuedAtTimestamp: bigint,
  deadline: bigint,
): boolean {
  return (
    issuedAtTimestamp <= MAX_NAT64 - MINT_AUTHORIZATION_TTL_SECONDS &&
    deadline === issuedAtTimestamp + MINT_AUTHORIZATION_TTL_SECONDS
  )
}

export function mintAuthorizationWindow(
  deadline: bigint,
  currentBaseTimestamp: bigint,
): MintAuthorizationWindow {
  const remainingSeconds = deadline - currentBaseTimestamp
  return {
    deadline,
    remainingSeconds,
    isUnexpired: remainingSeconds >= 0n,
  }
}
