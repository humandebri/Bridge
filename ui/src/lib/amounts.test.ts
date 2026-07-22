import { describe, expect, it } from "vitest"
import { estimatedAmountOut, formatTokenAmount, KINIC_LEDGER_FEE, parseTokenAmount, requiredDepositBalance } from "./amounts"

describe("eight-decimal token amount handling", () => {
  it("uses the immutable KINIC ledger fee", () => {
    expect(KINIC_LEDGER_FEE).toBe(10_000n)
  })

  it("parses and formats at exactly eight decimal places without numbers", () => {
    expect(parseTokenAmount("1.00000001")).toEqual({ ok: true, value: 100_000_001n })
    expect(formatTokenAmount(100_000_001n)).toBe("1.00000001")
  })

  it("rejects rounding, exponents, signs, and zero", () => {
    for (const input of ["0", "1.000000001", "1e8", "+1", "-1", ".5"]) {
      expect(parseTokenAmount(input).ok, input).toBe(false)
    }
  })

  it("floors an insolvent quote at zero", () => {
    expect(estimatedAmountOut(100n, 40n)).toBe(60n)
    expect(estimatedAmountOut(10n, 10n)).toBe(0n)
  })

  it("charges an approval fee only when the allowance is insufficient", () => {
    expect(requiredDepositBalance(100n, 10n, 110n)).toBe(110n)
    expect(requiredDepositBalance(100n, 10n, 111n)).toBe(110n)
    expect(requiredDepositBalance(100n, 10n, 109n)).toBe(120n)
  })
})
