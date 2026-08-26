export const MINT_AUTHORIZATION_TTL_SECONDS = 900n
export const MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS = 300n

export interface MintAuthorizationWindow {
  deadline: bigint
  remainingSeconds: bigint
  hasMinimumRemainingTime: boolean
}

export function mintAuthorizationWindow(
  finalizedBlockTimestamp: bigint,
  nowSeconds: bigint,
): MintAuthorizationWindow {
  const deadline = finalizedBlockTimestamp + MINT_AUTHORIZATION_TTL_SECONDS
  const remainingSeconds = deadline - nowSeconds
  return {
    deadline,
    remainingSeconds,
    hasMinimumRemainingTime: remainingSeconds >= MINIMUM_MINT_AUTHORIZATION_REMAINING_SECONDS,
  }
}
