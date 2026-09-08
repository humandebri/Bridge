import { decodeEventLog, decodeFunctionData, hexToBytes, type Hex } from "viem"
import { deploymentProfile } from "@/config/profile"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { basePublicClient } from "@/lib/evm/client"
import { readBaseReceipt, sharedBaseRead } from "@/lib/base-transaction-observation"
import { createBridgeActor } from "@/lib/ic/bridge"
import { receiptContainsExactDepositMint } from "@/lib/deposit-mint-finalization"
import { decodeWithdrawalDestination } from "@/lib/withdrawal-history"
import { ensurePendingWithdrawalConfirmation, savePendingMint } from "@/lib/pending-confirmations"

export class TransactionEvidenceMismatch extends Error {}

export async function withdrawalReceiptDetails(hash: Hex, expectedOwner?: string) {
  const receipt = await readBaseReceipt(hash)
  if (receipt.status !== "success")
    throw new TransactionEvidenceMismatch("The Base transaction has not succeeded.")
  const events = receipt.logs.flatMap((log) => {
    if (log.address.toLowerCase() !== deploymentProfile.bridgeAddress?.toLowerCase()) return []
    try {
      const event = decodeEventLog({
        abi: bridgeAbi,
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
  if (transaction.to?.toLowerCase() !== deploymentProfile.bridgeAddress?.toLowerCase())
    throw new TransactionEvidenceMismatch("The transaction targets another contract.")
  const call = decodeFunctionData({ abi: bridgeAbi, data: transaction.input })
  if (call.functionName !== "createWithdrawal")
    throw new TransactionEvidenceMismatch("The transaction is not a withdrawal.")
  const [amount, maximumFee, owner, subaccount] = call.args
  if (
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

export async function recoverTransaction(hash: string, account: { evm?: string; ic?: string }) {
  if (!/^0x[0-9a-fA-F]{64}$/.test(hash)) throw new Error("Enter a valid Base transaction hash.")
  const txHash = hash as Hex
  const receipt = await readBaseReceipt(txHash)
  if (receipt.status !== "success") throw new Error("The Base transaction has not succeeded.")
  const mintEvents = receipt.logs.flatMap((log) => {
    if (log.address.toLowerCase() !== deploymentProfile.bridgeAddress?.toLowerCase()) return []
    try {
      const event = decodeEventLog({
        abi: bridgeAbi,
        eventName: "DepositMinted",
        data: log.data,
        topics: log.topics,
        strict: true,
      })
      return event.eventName === "DepositMinted" ? [event.args] : []
    } catch {
      return []
    }
  })
  if (mintEvents.length === 1) {
    const event = mintEvents[0]!
    if (account.evm?.toLowerCase() !== event.recipient.toLowerCase())
      throw new Error("Connect the Base recipient wallet to restore this mint.")
    const actor = await createBridgeActor(
      deploymentProfile.icHost,
      deploymentProfile.bridgeCanisterId as string,
    )
    const record = (await actor.get_deposit(hexToBytes(event.depositId)))[0]
    const authorization = record?.mint_authorization[0]
    if (!record || !authorization) throw new Error("The mint authorization could not be found.")
    const bytesHex = (bytes: Uint8Array | number[]): Hex =>
      `0x${Array.from(bytes, (n) => n.toString(16).padStart(2, "0")).join("")}`
    const expected = {
      depositId: bytesHex(record.deposit_id),
      recipient: bytesHex(authorization.recipient),
      authorizationDigest: bytesHex(authorization.digest),
      grossAmount: authorization.gross_amount,
      serviceFee: authorization.charged_service_fee,
      mintedAmount: authorization.gross_amount - authorization.charged_service_fee,
    }
    if (
      !receiptContainsExactDepositMint(
        expected,
        receipt.logs,
        deploymentProfile.bridgeAddress as Hex,
      )
    )
      throw new Error("The receipt does not match this mint authorization.")
    await savePendingMint({
      depositId: expected.depositId,
      recipient: expected.recipient,
      authorizationDigest: expected.authorizationDigest,
      grossAmount: expected.grossAmount.toString(),
      chargedServiceFee: expected.serviceFee.toString(),
      mintedAmount: expected.mintedAmount.toString(),
      transactionHash: txHash,
    })
    return "Mint restored. Its success will be recorded on the IC after confirmation."
  }
  const withdrawal = await withdrawalReceiptDetails(txHash)
  if (
    account.evm?.toLowerCase() !== withdrawal.requester.toLowerCase() &&
    account.ic !== withdrawal.destinationAccount.owner
  )
    throw new Error("Connect the sender or recipient wallet to restore this withdrawal.")
  await ensurePendingWithdrawalConfirmation({
    kind: "withdrawal",
    transactionHash: txHash,
    owner: withdrawal.destinationAccount.owner,
  })
  return "Withdrawal restored. IC settlement will resume after confirmation."
}
