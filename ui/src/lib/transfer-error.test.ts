import { describe, expect, it } from "vitest"
import { RelyingPartyResponseError } from "@dfinity/oisy-wallet-signer"
import { transferAttentionTitle, transferErrorMessage } from "./transfer-error"

describe("transfer error presentation", () => {
  it("does_not_infer_a_cause_or_submission_outcome_from_the_deployed_generic_429", () => {
    const message = transferErrorMessage(
      new Error("Error: Consent message capacity is temporarily unavailable. (Code: 429)"),
    )
    expect(message).toContain("did not provide a specific reason")
    expect(message).not.toContain("not submitted")
    expect(message).not.toContain("cycles")
    expect(message).not.toContain("Too many")
    expect(transferAttentionTitle(message)).toBe(
      "The bridge could not prepare the approval message",
    )
  })
  it("preserves_explicit_consent_reasons_without_treating_unrelated_429s_as_consent_errors", () => {
    const message =
      "Bridge operating cycles are low. This is not a shortage in your wallet. (Code: 4601)"
    expect(transferErrorMessage(new Error(`Error: ${message}`))).toBe(message)
    expect(transferAttentionTitle(message)).toBe("Bridge operating cycles are low")
    expect(transferErrorMessage(new Error("RPC error 429"))).toBe("RPC error 429")
    expect(transferAttentionTitle("RPC error 429")).toBe("This transfer needs attention")
  })
  it("uses_structured_wallet_cancellation_and_keeps_unknown_outcomes_conservative", () => {
    const error = new RelyingPartyResponseError({ code: 3001, message: "canceled" })
    expect(transferErrorMessage(error)).toContain("Wallet approval was canceled")
    expect(transferErrorMessage(error)).toContain("Check the previous request")
    expect(transferErrorMessage({ code: 3001 })).not.toContain("canceled")
    expect(transferErrorMessage(undefined)).toContain("Check its status in History")
    const rpc = transferErrorMessage(
      new Error("HTTP 403 https://base-mainnet.g.alchemy.com/v2/test-private-key?token=secret"),
    )
    expect(rpc).toContain("HTTP 403")
    expect(rpc).not.toContain("test-private-key")
    expect(rpc).not.toContain("token=secret")
  })
})
