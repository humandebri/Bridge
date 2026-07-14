import { describe, expect, it } from "vitest"
import { estimatedAmountOut, formatTokenAmount, parseTokenAmount } from "./amounts"

describe("KINIC amount handling", () => {
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
    expect(estimatedAmountOut(100n, 40n, 10n)).toBe(50n)
    expect(estimatedAmountOut(10n, 10n, 1n)).toBe(0n)
  })
})
