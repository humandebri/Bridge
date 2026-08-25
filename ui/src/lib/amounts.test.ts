import { describe, expect, it } from "vitest"
import { estimatedAmountOut, formatTokenAmount, maximumDepositAmount, parseTokenAmount, requiredDepositBalance } from "./amounts"

describe("eight-decimal token amount handling", () => {
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

  it("reserves one ledger fee when allowance covers the full balance", () => {
    expect(maximumDepositAmount(1_000n, 10n, 1_000n)).toBe(990n)
  })

  it("reserves approval and transfer fees when allowance is insufficient", () => {
    expect(maximumDepositAmount(1_000n, 10n, 0n)).toBe(980n)
  })

  it("uses a partial allowance when it permits a larger deposit without approval", () => {
    expect(maximumDepositAmount(1_000n, 10n, 995n)).toBe(985n)
  })

  it("returns zero when the balance cannot cover the required ledger fees", () => {
    expect(maximumDepositAmount(20n, 10n, 0n)).toBe(0n)
    expect(maximumDepositAmount(0n, 10n, 1_000n)).toBe(0n)
  })
})
