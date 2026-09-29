export const TOKEN_DECIMALS = 8

function scale(decimals: number): bigint {
  if (!Number.isInteger(decimals) || decimals < 0 || decimals > 18)
    throw new Error("Token decimals must be between 0 and 18")
  return 10n ** BigInt(decimals)
}

export type AmountResult = { ok: true; value: bigint } | { ok: false; reason: string }

export function parseTokenAmount(input: string, decimals = TOKEN_DECIMALS): AmountResult {
  const normalized = input.trim()
  const pattern =
    decimals === 0 ? /^(?:0|[1-9]\d*)$/ : new RegExp(`^(?:0|[1-9]\\d*)(?:\\.\\d{1,${decimals}})?$`)
  if (!pattern.test(normalized)) {
    return {
      ok: false,
      reason: `Enter a positive token amount with no more than ${decimals} decimal places.`,
    }
  }
  const [whole = "0", fraction = ""] = normalized.split(".")
  const value = BigInt(whole) * scale(decimals) + BigInt(fraction.padEnd(decimals, "0") || "0")
  return value > 0n
    ? { ok: true, value }
    : { ok: false, reason: "Amount must be greater than zero." }
}

export function formatTokenAmount(value: bigint, decimals = TOKEN_DECIMALS): string {
  const tokenScale = scale(decimals)
  const whole = value / tokenScale
  const fraction = (value % tokenScale).toString().padStart(decimals, "0").replace(/0+$/, "")
  return fraction ? `${whole}.${fraction}` : whole.toString()
}

export function estimatedAmountOut(amount: bigint, serviceFee: bigint): bigint {
  return amount > serviceFee ? amount - serviceFee : 0n
}

export function requiredDepositBalance(
  amount: bigint,
  ledgerFee: bigint,
  allowance: bigint,
): bigint {
  const requiredAllowance = amount + ledgerFee
  const approvalFee = allowance < requiredAllowance ? ledgerFee : 0n
  return amount + ledgerFee + approvalFee
}

export function maximumDepositAmount(
  balance: bigint,
  ledgerFee: bigint,
  allowance: bigint,
): bigint {
  const amountWithTransferFee =
    balance > ledgerFee
      ? minBigInt(balance - ledgerFee, allowance > ledgerFee ? allowance - ledgerFee : 0n)
      : 0n
  const amountWithApprovalAndTransferFees = balance > ledgerFee * 2n ? balance - ledgerFee * 2n : 0n
  return amountWithTransferFee > amountWithApprovalAndTransferFees
    ? amountWithTransferFee
    : amountWithApprovalAndTransferFees
}

function minBigInt(left: bigint, right: bigint): bigint {
  return left < right ? left : right
}
