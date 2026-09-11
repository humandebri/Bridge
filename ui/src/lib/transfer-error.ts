import { BaseError, ContractFunctionRevertedError } from "viem"
import { RelyingPartyResponseError } from "@dfinity/oisy-wallet-signer"

// Presentation only: never use these messages to infer submission or clear a saved intent.
export function transferErrorMessage(error: unknown): string {
  const revert =
    error instanceof BaseError
      ? error.walk((cause) => cause instanceof ContractFunctionRevertedError)
      : undefined
  if (
    revert instanceof ContractFunctionRevertedError &&
    revert.data?.errorName === "ERC20InsufficientAllowance"
  ) {
    const args = revert.data.args
    if (args && typeof args[1] === "bigint" && typeof args[2] === "bigint")
      return `Token allowance is insufficient. Allowed: ${args[1]}; required: ${args[2]} (token base units). Check the previous request before trying again.`
  }
  const raw = error instanceof Error ? redactRpcUrls(error.message) : ""
  const message = raw.replace(/^Error:\s*/, "")
  if (/^Consent message capacity is temporarily unavailable\.(?: \(Code: 429\))?$/.test(message))
    return "The bridge could not prepare the approval message. The bridge did not provide a specific reason. Error code: 429."
  if (transferAttentionTitle(message) !== "This transfer needs attention") return message
  if (error instanceof RelyingPartyResponseError && error.code === 3001)
    return "Wallet approval was canceled. Check the previous request before starting another transfer."
  if (error instanceof RelyingPartyResponseError && error.code === 4000)
    return message
      ? `The wallet could not complete the request. ${message} Check the previous request before trying again.`
      : "The wallet could not complete the request. Check the previous request before trying again."
  return (
    message || "The transfer could not continue. Check its status in History before trying again."
  )
}

export function transferAttentionTitle(message: string | undefined): string {
  for (const title of [
    "Bridge operating cycles are low",
    "Too many consent requests",
    "Bridge requests are currently disabled",
    "The bridge could not prepare the approval message",
    "Wallet approval was canceled",
  ]) {
    if (message?.startsWith(`${title}.`)) return title
  }
  return "This transfer needs attention"
}

export function redactRpcUrls(message: string): string {
  return message.replace(/https?:\/\/[^\s)"'<>]+/g, (value) => {
    try {
      return `${new URL(value).origin}/[redacted]`
    } catch {
      return "[redacted RPC URL]"
    }
  })
}
