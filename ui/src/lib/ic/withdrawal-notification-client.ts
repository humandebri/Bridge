import { Ed25519KeyIdentity } from "@dfinity/identity"
import type { NotifyWithdrawalError, NotifyWithdrawalReceipt, SettlementActionError, SettlementActionResult } from "@/generated/bridge.did"
import { deploymentProfile, type DeploymentProfile } from "@/config/profile"
import { browserLocalStorage, withBrowserLock } from "@/lib/browser-lock"
import { createBridgeActor } from "@/lib/ic/bridge"
import { isSettlementActionResult } from "@/lib/settlement-phase"

const IDENTITY_STORAGE_PREFIX = "kinic.bridge.withdrawal-notification-identity.v1"
const sessionIdentities = new Map<string, Ed25519KeyIdentity>()

type NotificationDeployment = Pick<DeploymentProfile, "chainId" | "bridgeCanisterId" | "deploymentInstanceId" | "icHost">

export type NotifyWithdrawalErrorCode = NotifyWithdrawalError extends infer Variant
  ? Variant extends Record<string, unknown> ? keyof Variant : never
  : never
export type ContinueWithdrawalErrorCode = SettlementActionError extends infer Variant
  ? Variant extends Record<string, unknown> ? keyof Variant : never
  : never

export class NotifyWithdrawalCallError extends Error {
  constructor(
    readonly code: NotifyWithdrawalErrorCode,
    message: string,
  ) {
    super(message)
    this.name = "NotifyWithdrawalCallError"
  }
}

export class ContinueWithdrawalCallError extends Error {
  constructor(readonly code: ContinueWithdrawalErrorCode, message: string) {
    super(message)
    this.name = "ContinueWithdrawalCallError"
  }
}

export async function getWithdrawalNotificationIdentity(
  profile: NotificationDeployment = deploymentProfile,
): Promise<Ed25519KeyIdentity> {
  const storageKey = withdrawalNotificationIdentityStorageKey(profile)
  return withBrowserLock(`kinic-notification-identity:${storageKey}`, () => {
    const sessionIdentity = sessionIdentities.get(storageKey)
    if (sessionIdentity) return sessionIdentity

    try {
      const stored = browserLocalStorage().getItem(storageKey)
      if (stored !== null) {
        try {
          const identity = Ed25519KeyIdentity.fromJSON(stored)
          sessionIdentities.set(storageKey, identity)
          return identity
        } catch {
          // A malformed or obsolete notification key has no authority and can be replaced safely.
        }
      }
      const identity = Ed25519KeyIdentity.generate()
      browserLocalStorage().setItem(storageKey, JSON.stringify(identity.toJSON()))
      sessionIdentities.set(storageKey, identity)
      return identity
    } catch {
      const identity = Ed25519KeyIdentity.generate()
      sessionIdentities.set(storageKey, identity)
      return identity
    }
  })
}

export async function notifyWithdrawalWithBrowserIdentity(
  transactionHash: Uint8Array,
  profile: NotificationDeployment = deploymentProfile,
): Promise<NotifyWithdrawalReceipt> {
  if (!profile.bridgeCanisterId) throw new Error("Bridge canister ID is unavailable")
  const identity = await getWithdrawalNotificationIdentity(profile)
  const transactionKey = Array.from(transactionHash, (value) => value.toString(16).padStart(2, "0")).join("")
  return withBrowserLock(
    `kinic-withdrawal-notification:${profile.bridgeCanisterId}:${transactionKey}`,
    async () => {
      const actor = await createBridgeActor(profile.icHost, profile.bridgeCanisterId as string, identity)
      return unwrapNotifyWithdrawalResult(await actor.notify_withdrawal({ transaction_hash: transactionHash }))
    },
  )
}

export async function continueWithdrawalWithBrowserIdentity(
  withdrawalId: Uint8Array,
  profile: NotificationDeployment = deploymentProfile,
): Promise<SettlementActionResult> {
  if (!profile.bridgeCanisterId) throw new Error("Bridge canister ID is unavailable")
  const identity = await getWithdrawalNotificationIdentity(profile)
  const withdrawalKey = Array.from(withdrawalId, (value) => value.toString(16).padStart(2, "0")).join("")
  return withBrowserLock(
    `kinic-withdrawal-continuation:${profile.bridgeCanisterId}:${withdrawalKey}`,
    async () => {
      const actor = await createBridgeActor(profile.icHost, profile.bridgeCanisterId as string, identity)
      return unwrapContinueWithdrawalResult(await actor.continue_withdrawal(withdrawalId))
    },
  )
}

export function withdrawalNotificationIdentityStorageKey(profile: NotificationDeployment): string {
  return [
    IDENTITY_STORAGE_PREFIX,
    profile.chainId,
    profile.bridgeCanisterId ?? "",
    profile.deploymentInstanceId?.toLowerCase() ?? "",
  ].join(":")
}

export function unwrapNotifyWithdrawalResult(result: unknown): NotifyWithdrawalReceipt {
  if (!isObject(result)) throw new Error("Bridge returned an invalid notification result")
  const decoded = result as Record<string, unknown>
  if ("Err" in decoded) {
    const error = decoded.Err
    const code = notifyWithdrawalErrorCode(error)
    if (!code) throw new Error("Bridge returned an invalid withdrawal notification error")
    throw new NotifyWithdrawalCallError(code, notifyWithdrawalErrorMessage(error as NotifyWithdrawalError))
  }
  const receipt: unknown = decoded.Ok
  if (!isObject(receipt)) throw new Error("Bridge returned an invalid notification receipt")
  const receiptKeys = Object.keys(receipt)
  const receiptKey = receiptKeys[0]
  if (receiptKeys.length !== 1 || (receiptKey !== "Ingested" && receiptKey !== "Duplicate")) {
    throw new Error("Bridge returned an invalid notification receipt")
  }
  const payload: unknown = (receipt as Record<string, unknown>)[receiptKey]
  if (!isObject(payload)) throw new Error("Bridge returned an invalid notification receipt")
  const payloadRecord = payload as Record<string, unknown>
  const withdrawalId: unknown = payloadRecord.withdrawal_id
  if (!(withdrawalId instanceof Uint8Array) && !Array.isArray(withdrawalId)) {
    throw new Error("Bridge returned an invalid withdrawal ID")
  }
  if (receiptKey === "Ingested" && typeof payloadRecord.finalized_checkpoint_block_number !== "bigint") {
    throw new Error("Bridge returned an invalid finalized block")
  }
  return receipt as NotifyWithdrawalReceipt
}

export function unwrapContinueWithdrawalResult(result: unknown): SettlementActionResult {
  if (!isObject(result)) throw new Error("Bridge returned an invalid withdrawal continuation result")
  const decoded = result as Record<string, unknown>
  if (decoded.Err !== undefined) {
    const error = decoded.Err
    if (!isObject(error)) throw new Error("Bridge returned an invalid withdrawal continuation error")
    const key = Object.keys(error)[0]
    const code = key && [
      "AnonymousCaller",
      "InvalidId",
      "NotFound",
      "Unauthorized",
      "Busy",
      "StorageFailure",
      "WrongState",
      "AutomaticProgressPending",
      "RateLimited",
      "InsufficientCycles",
    ].includes(key) ? key as ContinueWithdrawalErrorCode : undefined
    if (!code) throw new Error("Bridge returned an invalid withdrawal continuation error")
    throw new ContinueWithdrawalCallError(code, continueWithdrawalErrorMessage(error))
  }
  if (decoded.Ok === undefined || !isSettlementActionResult(decoded.Ok)) {
    throw new Error("Bridge returned an invalid withdrawal continuation result")
  }
  return decoded.Ok
}

function continueWithdrawalErrorMessage(error: object): string {
  if ("InsufficientCycles" in error) return "The Bridge Canister does not have enough cycles to continue this payout."
  if ("RateLimited" in error) return "This payout was continued too recently. Try again later."
  if ("Busy" in error) return "This payout is already being continued."
  if ("AnonymousCaller" in error) return "The browser continuation identity was not accepted."
  return `Bridge rejected withdrawal continuation: ${stringify(error)}`
}

export function notifyWithdrawalErrorMessage(error: NotifyWithdrawalError): string {
  const key = Object.keys(error)[0]
  const messages: Record<string, string> = {
    LedgerFeeExceedsServiceFee: "Withdrawals were stopped because the ledger fee exceeded the charged service fee. Contact the bridge operator before resuming from History.",
    RpcUnavailable: "Base RPC is unavailable. Retry the IC notification when the service recovers.",
    RpcInconsistent: "Base RPC providers disagreed. Automatic notification retries have stopped.",
    InvalidBaseResponse: "Base returned an invalid withdrawal response.",
    TransactionNotFound: "The Base withdrawal transaction was not found.",
    TransactionNotConfirmed: "The Base withdrawal transaction has not reached the finalized head yet.",
    TransactionReverted: "The Base withdrawal transaction reverted.",
    BaseStateMismatch: "The finalized Bridge withdrawal state does not match its creation event.",
    BridgeSignerMismatch: "The finalized Bridge signer does not match the configured canister signer.",
    WithdrawalConflict: "A different withdrawal payload already uses this withdrawal ID.",
    InvalidTransactionHash: "The withdrawal transaction hash is invalid.",
    StorageFailure: "The Bridge could not save the withdrawal.",
    AnonymousCaller: "The browser notification identity was not accepted.",
    Busy: "This withdrawal notification or withdrawal record is already being processed.",
    RateLimited: "Withdrawal notifications are temporarily rate limited. Retry the IC notification later.",
    InsufficientCycles: "The Bridge Canister does not have enough cycles to process this notification.",
  }
  const message = key === undefined ? undefined : messages[key]
  return message ?? `Bridge rejected withdrawal notification: ${stringify(error)}`
}

function notifyWithdrawalErrorCode(error: unknown): NotifyWithdrawalErrorCode | undefined {
  if (!isObject(error)) return undefined
  const code = Object.keys(error)[0]
  if (code && [
    "LedgerFeeExceedsServiceFee", "Busy", "RpcUnavailable", "TransactionNotConfirmed",
    "WithdrawalConflict", "RpcInconsistent", "InvalidTransactionHash",
    "TransactionReverted", "StorageFailure", "BaseStateMismatch",
    "TransactionNotFound", "BridgeSignerMismatch", "AnonymousCaller", "InvalidBaseResponse",
    "RateLimited", "InsufficientCycles",
  ].includes(code)) return code as NotifyWithdrawalErrorCode
  return undefined
}

function isObject(value: unknown): value is object {
  return typeof value === "object" && value !== null
}

function stringify(value: unknown): string {
  return JSON.stringify(value, (_key, item: unknown) => typeof item === "bigint" ? item.toString() : item)
}
