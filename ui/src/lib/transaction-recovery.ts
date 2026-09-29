import { decodeEventLog, decodeFunctionData, hexToBytes, type Hex } from "viem"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { multiTokenBridgeAbi } from "@/generated/abi/multitokenbridge.generated"
import { basePublicClient } from "@/lib/evm/client"
import { readBaseReceipt, sharedBaseRead } from "@/lib/base-transaction-observation"
import { decodeWithdrawalDestination } from "@/lib/withdrawal-history"

export class TransactionEvidenceMismatch extends Error {}

export interface WithdrawalContractBinding {
  assetId?: Hex
  bridgeAddress?: Hex
  shared?: boolean
}

export async function withdrawalReceiptDetails(
  hash: Hex,
  expectedOwner?: string,
  binding: WithdrawalContractBinding = {},
) {
  const bridgeAddress = (binding.bridgeAddress ?? deploymentProfile.bridgeAddress) as Hex
  const abi = binding.shared ? multiTokenBridgeAbi : bridgeAbi
  const receipt = await readBaseReceipt(hash)
  if (receipt.status !== "success")
    throw new TransactionEvidenceMismatch("The Base transaction has not succeeded.")
  const events = receipt.logs.flatMap((log) => {
    if (log.address.toLowerCase() !== bridgeAddress.toLowerCase()) return []
    try {
      const event = decodeEventLog({
        abi,
        eventName: "WithdrawalCommitted",
        data: log.data,
        topics: log.topics,
        strict: true,
      })
      return event.eventName === "WithdrawalCommitted" ? [{ log, args: event.args }] : []
    } catch {
      return []
    }
  })
  if (events.length !== 1)
    throw new TransactionEvidenceMismatch(
      "The Base receipt does not contain one matching withdrawal.",
    )
  const { args, log } = events[0]!
  if (
    binding.shared &&
    (!("assetId" in args) || args.assetId.toLowerCase() !== binding.assetId?.toLowerCase())
  )
    throw new TransactionEvidenceMismatch("The withdrawal asset does not match this transfer.")
  const destinationAccount = decodeWithdrawalDestination(args.owner, args.subaccount)
  if (expectedOwner && destinationAccount.owner !== expectedOwner)
    throw new TransactionEvidenceMismatch(
      "The withdrawal destination does not match this transfer.",
    )
  if (
    args.amount !== args.amountOut + args.chargedServiceFee ||
    args.chargedServiceFee > args.maxServiceFee
  )
    throw new TransactionEvidenceMismatch("The withdrawal amounts do not match.")
  const transaction = await sharedBaseRead(`transaction:${hash}`, () =>
    basePublicClient.getTransaction({ hash }),
  )
  if (transaction.to?.toLowerCase() !== bridgeAddress.toLowerCase())
    throw new TransactionEvidenceMismatch("The transaction targets another contract.")
  const call = decodeFunctionData({ abi, data: transaction.input })
  if (call.functionName !== "createWithdrawal")
    throw new TransactionEvidenceMismatch("The transaction is not a withdrawal.")
  const callArgs = call.args as readonly (bigint | Hex)[]
  const suppliedAssetId = binding.shared ? (callArgs[0] as Hex) : undefined
  const offset = binding.shared ? 1 : 0
  const amount = callArgs[offset] as bigint
  const maximumFee = callArgs[offset + 1] as bigint
  const owner = callArgs[offset + 2] as Hex
  const subaccount = callArgs[offset + 3] as Hex
  if (
    (binding.shared && suppliedAssetId?.toLowerCase() !== binding.assetId?.toLowerCase()) ||
    amount !== args.amount ||
    maximumFee !== args.maxServiceFee ||
    owner.toLowerCase() !== args.owner.toLowerCase() ||
    subaccount.toLowerCase() !== args.subaccount.toLowerCase() ||
    transaction.from.toLowerCase() !== args.requester.toLowerCase()
  )
    throw new TransactionEvidenceMismatch(
      "The withdrawal receipt does not match the submitted transaction.",
    )
  return {
    id: args.withdrawalId,
    assetId: binding.shared && binding.assetId ? hexToBytes(binding.assetId) : undefined,
    amount: args.amount,
    amountOut: args.amountOut,
    hash,
    blockNumber: receipt.blockNumber,
    logIndex: log.logIndex,
    createdAtNs: 0n,
    destinationAccount,
    requester: args.requester,
  }
}
