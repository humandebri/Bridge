export const TOKEN_DECIMALS = 8
const SCALE = 100_000_000n

export type AmountResult = { ok: true; value: bigint } | { ok: false; reason: string }

export function parseTokenAmount(input: string): AmountResult {
  const normalized = input.trim()
  if (!/^(?:0|[1-9]\d*)(?:\.\d{1,8})?$/.test(normalized)) {
    return { ok: false, reason: "Enter a positive token amount with no more than 8 decimal places." }
  }
  const [whole = "0", fraction = ""] = normalized.split(".")
  const value = BigInt(whole) * SCALE + BigInt(fraction.padEnd(TOKEN_DECIMALS, "0"))
  return value > 0n ? { ok: true, value } : { ok: false, reason: "Amount must be greater than zero." }
}

export function formatTokenAmount(value: bigint): string {
  const whole = value / SCALE
  const fraction = (value % SCALE).toString().padStart(TOKEN_DECIMALS, "0").replace(/0+$/, "")
  return fraction ? `${whole}.${fraction}` : whole.toString()
}

export function estimatedAmountOut(amount: bigint, serviceFee: bigint, ledgerFee: bigint): bigint {
  const fees = serviceFee + ledgerFee
  return amount > fees ? amount - fees : 0n
}

export function requiredDepositBalance(amount: bigint, ledgerFee: bigint, allowance: bigint): bigint {
  const requiredAllowance = amount + ledgerFee
  const approvalFee = allowance < requiredAllowance ? ledgerFee : 0n
  return amount + ledgerFee + approvalFee
}
