import { decodeEventLog, type Hex } from "viem"
import { bridgeAbi } from "@/generated/abi/bridge.generated"

export interface ExpectedDepositMint {
  depositId: `0x${string}`
  recipient: `0x${string}`
  authorizationDigest: `0x${string}`
  grossAmount: bigint
  serviceFee: bigint
  mintedAmount: bigint
}

export interface ObservedDepositMint {
  depositId: `0x${string}`
  recipient: `0x${string}`
  authorizationDigest: `0x${string}`
  grossAmount: bigint
  serviceFee: bigint
  mintedAmount: bigint
}
export type DepositMintFinalizationStatus = "checking" | "minted" | "absent" | "unavailable"
export type MintReceiptFinalization = "pending" | "finalized" | "conflict" | "reverted"

export function depositMintEventMatches(
  expected: ExpectedDepositMint,
  observed: ObservedDepositMint,
): boolean {
  return (
    observed.depositId.toLowerCase() === expected.depositId.toLowerCase() &&
    observed.recipient.toLowerCase() === expected.recipient.toLowerCase() &&
    observed.authorizationDigest.toLowerCase() === expected.authorizationDigest.toLowerCase() &&
    observed.grossAmount === expected.grossAmount &&
    observed.serviceFee === expected.serviceFee &&
    observed.mintedAmount === expected.mintedAmount
  )
}

export function receiptContainsExactDepositMint(
  expected: ExpectedDepositMint,
  logs: readonly { address: Hex; data: Hex; topics: readonly Hex[] }[],
  expectedBridgeAddress: Hex,
): boolean {
  const candidates = logs.flatMap((log) => {
    if (log.address.toLowerCase() !== expectedBridgeAddress.toLowerCase()) return []
    try {
      const decoded = decodeEventLog({
        abi: bridgeAbi,
        eventName: "DepositMinted",
        data: log.data,
        topics: log.topics as [Hex, ...Hex[]],
        strict: true,
      })
      if (
        decoded.eventName !== "DepositMinted" ||
        decoded.args.depositId.toLowerCase() !== expected.depositId.toLowerCase()
      )
        return []
      return [decoded.args]
    } catch {
      return []
    }
  })
  return candidates.length === 1 && depositMintEventMatches(expected, candidates[0]!)
}

export function exactMintReceiptFinalization({
  expected,
  expectedBridgeAddress,
  receipt,
  finalizedBlockNumber,
  canonicalReceiptBlockHash,
}: {
  expected: ExpectedDepositMint
  expectedBridgeAddress: Hex
  receipt: {
    status: "success" | "reverted"
    blockNumber: bigint | null
    blockHash: Hex | null
    logs: readonly { address: Hex; data: Hex; topics: readonly Hex[] }[]
  }
  finalizedBlockNumber: bigint
  canonicalReceiptBlockHash?: Hex | null
}): MintReceiptFinalization {
  if (
    receipt.blockNumber === null ||
    receipt.blockHash === null ||
    receipt.blockNumber > finalizedBlockNumber ||
    !canonicalReceiptBlockHash ||
    canonicalReceiptBlockHash.toLowerCase() !== receipt.blockHash.toLowerCase()
  ) {
    return "pending"
  }
  if (receipt.status === "reverted") return "reverted"
  return receiptContainsExactDepositMint(expected, receipt.logs, expectedBridgeAddress)
    ? "finalized"
    : "conflict"
}
