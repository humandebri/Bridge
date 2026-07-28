import {
  isAlreadyKnown,
  parseOptions,
  redactedErrorMessage,
} from "../tools/governance-relayer/cli"

describe("governance relayer CLI", () => {
  it("parses explicit and inline options without accepting positional input", () => {
    expect(parseOptions(["--operation-id", "7", "--max-fee=100", "--help"])).toEqual({
      "operation-id": "7",
      "max-fee": "100",
      help: true,
    })
    expect(() => parseOptions(["unexpected"])).toThrow("Unexpected argument")
  })

  it("treats idempotent RPC submission responses as already relayed", () => {
    expect(isAlreadyKnown(new Error("already known"))).toBe(true)
    expect(isAlreadyKnown(new Error("nonce too low"))).toBe(false)
    expect(isAlreadyKnown(new Error("insufficient funds"))).toBe(false)
  })

  it("redacts identity paths and RPC credentials from errors", () => {
    const environment = {
      BASE_RPC_URL: "https://rpc.example/key-secret",
      IC_IDENTITY_PEM: "/secure/governance-secret.pem",
    }
    const message = redactedErrorMessage(
      new Error(`request to ${environment.BASE_RPC_URL} failed for ${environment.IC_IDENTITY_PEM}`),
      environment,
    )
    expect(message).toBe("request to [REDACTED] failed for [REDACTED]")
  })
})
