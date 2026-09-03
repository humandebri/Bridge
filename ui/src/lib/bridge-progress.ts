import { deploymentProfile } from "@/config/profile"
import { browserLocalStorage } from "@/lib/browser-lock"

export type BridgeProgressDirection = "deposit" | "withdraw"

export type BridgeProgressPhase =
  | "verifying-ic-destination"
  | "awaiting-ic-allowance"
  | "awaiting-ic-deposit"
  | "ic-deposit-accepted"
  | "authorization-generating"
  | "awaiting-base-mint"
  | "base-mint-submitted"
  | "base-mint-included"
  | "base-mint-finalizing"
  | "awaiting-base-allowance"
  | "awaiting-base-withdrawal"
  | "base-withdrawal-submitted"
  | "base-withdrawal-included"
  | "base-withdrawal-finalizing"
  | "awaiting-ic-notification"
  | "ic-notification-recorded"
  | "ledger-payout"
  | "complete"
  | "attention"

export type BridgeTokenApproval = "required" | "not-required"
type ActiveBridgeProgressPhase = Exclude<BridgeProgressPhase, "complete" | "attention">

export interface BridgeProgressDepositIdentity {
  owner: string
  ownerSequence: string
  depositId?: `0x${string}`
}

export interface BridgeProgressWithdrawalIdentity {
  owner: string
  withdrawalId?: `0x${string}`
}

export interface BridgeProgressRecord {
  version: 3
  id: string
  direction: BridgeProgressDirection
  phase: BridgeProgressPhase
  source: string
  destination: string
  sendAmount: string
  receiveAmount: string
  sendSymbol: string
  receiveSymbol: string
  tokenApproval: BridgeTokenApproval
  createdAt: number
  updatedAt: number
  transactionHash?: `0x${string}`
  receiptBlockNumber?: string
  finalizedBlockNumber?: string
  baseTransactionOutcome?: "success" | "reverted"
  deposit?: BridgeProgressDepositIdentity
  withdrawal?: BridgeProgressWithdrawalIdentity
  attentionMessage?: string
  attentionPhase?: ActiveBridgeProgressPhase
  completionMessage?: string
}

const STORAGE_VERSION = 3

type RestorableBridgeProgressPhase =
  | "authorization-generating"
  | "base-mint-submitted"
  | "base-withdrawal-submitted"
  | "ledger-payout"
  | "attention"

interface StoredBridgeProgress {
  version: 3
  id: string
  direction: BridgeProgressDirection
  phase: RestorableBridgeProgressPhase
  source: string
  destination: string
  sendAmount: string
  receiveAmount: string
  sendSymbol: string
  receiveSymbol: string
  tokenApproval: BridgeTokenApproval
  createdAt: number
  transactionHash?: `0x${string}`
  deposit?: BridgeProgressDepositIdentity
  withdrawal?: BridgeProgressWithdrawalIdentity
  attentionMessage?: string
  attentionPhase?: ActiveBridgeProgressPhase
}

function storageKey(): string {
  return [
    "kinic.bridge.latest-progress.v3",
    deploymentProfile.chainId,
    String(deploymentProfile.bridgeAddress).toLowerCase(),
    deploymentProfile.bridgeCanisterId ?? "",
    deploymentProfile.deploymentInstanceId?.toLowerCase() ?? "",
  ].join(":")
}

export function createBridgeProgress(
  input: Omit<
    BridgeProgressRecord,
    "version" | "id" | "createdAt" | "updatedAt" | "tokenApproval"
  > & { tokenApproval?: BridgeTokenApproval },
): BridgeProgressRecord {
  const now = Date.now()
  return {
    ...input,
    tokenApproval:
      input.tokenApproval ??
      (input.phase === "awaiting-ic-allowance" || input.phase === "awaiting-base-allowance"
        ? "required"
        : "not-required"),
    version: STORAGE_VERSION,
    id: `${input.direction}:${now}:${Math.random().toString(16).slice(2)}`,
    createdAt: now,
    updatedAt: now,
  }
}

export function saveLatestBridgeProgress(record: BridgeProgressRecord): void {
  if (typeof window === "undefined") return
  try {
    if (record.phase === "complete") {
      browserLocalStorage().removeItem(storageKey())
      return
    }
    const phase = restorablePhase(record)
    if (!phase) {
      browserLocalStorage().removeItem(storageKey())
      return
    }
    const stored: StoredBridgeProgress = {
      version: record.version,
      id: record.id,
      direction: record.direction,
      phase,
      source: record.source,
      destination: record.destination,
      sendAmount: record.sendAmount,
      receiveAmount: record.receiveAmount,
      sendSymbol: record.sendSymbol,
      receiveSymbol: record.receiveSymbol,
      tokenApproval: record.tokenApproval,
      createdAt: record.createdAt,
      transactionHash: record.transactionHash,
      deposit: record.deposit,
      withdrawal: record.withdrawal,
      attentionMessage: phase === "attention" ? record.attentionMessage : undefined,
      attentionPhase: phase === "attention" ? record.attentionPhase : undefined,
    }
    browserLocalStorage().setItem(storageKey(), JSON.stringify(stored))
  } catch {
    // The in-memory provider still owns the live transfer for this session.
  }
}

export function readLatestBridgeProgress(): BridgeProgressRecord | undefined {
  if (typeof window === "undefined") return undefined
  try {
    const value: unknown = JSON.parse(browserLocalStorage().getItem(storageKey()) ?? "null")
    if (!isStoredBridgeProgress(value)) return undefined
    return { ...value, updatedAt: Date.now() }
  } catch {
    return undefined
  }
}

export function removeLatestBridgeProgress(id?: string): void {
  if (typeof window === "undefined") return
  try {
    const current = readLatestBridgeProgress()
    if (!id || current?.id === id) browserLocalStorage().removeItem(storageKey())
  } catch {
    // A failed cleanup must not make a completed transfer look incomplete in memory.
  }
}

export function bridgeProgressLabel(record: BridgeProgressRecord): string {
  if (
    record.direction === "deposit" &&
    record.phase === "base-mint-included" &&
    record.baseTransactionOutcome === "reverted"
  ) {
    return "Base transaction reverted"
  }
  if (
    record.direction === "deposit" &&
    record.phase === "attention" &&
    record.attentionPhase === "authorization-generating"
  ) {
    return "Bridge processing paused"
  }
  const labels: Record<BridgeProgressPhase, string> = {
    "verifying-ic-destination": "Verifying the destination IC account",
    "awaiting-ic-allowance": "Confirm token access in your IC wallet",
    "awaiting-ic-deposit": "Confirm the deposit in your IC wallet",
    "ic-deposit-accepted": "Deposit accepted on the Internet Computer",
    "authorization-generating": "Bridge is preparing the Base mint",
    "awaiting-base-mint": "Confirm the mint in your Base wallet",
    "base-mint-submitted": "Waiting for the Base transaction",
    "base-mint-included": "Base transaction included",
    "base-mint-finalizing": "Waiting for Base finality",
    "awaiting-base-allowance": "Confirm token access in your Base wallet",
    "awaiting-base-withdrawal": "Confirm the withdrawal in your Base wallet",
    "base-withdrawal-submitted": "Waiting for the Base transaction",
    "base-withdrawal-included": "Base transaction included",
    "base-withdrawal-finalizing": "Waiting for Base finality",
    "awaiting-ic-notification": "Recording the finalized withdrawal on the Internet Computer",
    "ic-notification-recorded": "Withdrawal recorded on the Internet Computer",
    "ledger-payout": "Sending tokens to your IC wallet",
    complete: "Bridge complete",
    attention: "This transfer needs attention",
  }
  return labels[record.phase]
}

export function bridgeProgressDetail(record: BridgeProgressRecord): string {
  if (record.phase === "attention")
    return record.attentionMessage ?? "Review the transfer in History before trying again."
  if (record.phase === "complete")
    return record.completionMessage ?? "The transfer reached its destination."
  if (record.phase === "verifying-ic-destination")
    return "The bridge is checking that the connected IC account matches the reviewed destination."
  if (record.phase === "awaiting-ic-allowance")
    return `Allow the bridge to use the ${record.sendSymbol} required for this transfer.`
  if (record.phase === "awaiting-ic-deposit")
    return `${record.sendAmount} ${record.sendSymbol} will be deposited for ${shortDestination(record.destination)}.`
  if (record.phase === "authorization-generating")
    return "No wallet action is needed. The Bridge is signing the fixed mint recipient and amount."
  if (record.phase === "awaiting-base-mint")
    return `${record.receiveAmount} ${record.receiveSymbol} will be minted to ${shortDestination(record.destination)}. Your connected Base wallet pays gas.`
  if (record.phase === "awaiting-base-allowance")
    return `Allow the bridge to use the ${record.sendSymbol} required for this withdrawal.`
  if (record.phase === "awaiting-base-withdrawal")
    return `${record.sendAmount} ${record.sendSymbol} will be burned and sent to ${shortDestination(record.destination)}.`
  if (record.phase === "awaiting-ic-notification")
    return "No wallet action is needed. The browser is submitting the finalized Base withdrawal proof."
  if (record.phase === "ledger-payout")
    return "The withdrawal is recorded. Open History to continue the payout if it has not completed."
  if (
    record.direction === "deposit" &&
    record.phase === "base-mint-included" &&
    record.baseTransactionOutcome === "reverted"
  ) {
    return "The transaction reverted. Waiting for Base finality before enabling a safe retry."
  }
  if (
    record.receiptBlockNumber &&
    [
      "base-mint-included",
      "base-mint-finalizing",
      "base-withdrawal-included",
      "base-withdrawal-finalizing",
    ].includes(record.phase)
  ) {
    return `Included in Base block ${record.receiptBlockNumber}. Waiting for the finalized chain to confirm it.`
  }
  if (
    record.transactionHash &&
    ["base-mint-submitted", "base-withdrawal-submitted"].includes(record.phase)
  ) {
    return `Submitted ${record.transactionHash.slice(0, 12)}…. Waiting for inclusion in a Base block.`
  }
  return "The bridge is checking the next confirmed state."
}

export interface BridgeProgressStep {
  label: string
  status: "attention" | "complete" | "current" | "waiting"
  note?: string
}

export function bridgeProgressSteps(record: BridgeProgressRecord): BridgeProgressStep[] {
  const deposit = [
    ["IC token approval", ["awaiting-ic-allowance"]],
    ["IC deposit transaction", ["awaiting-ic-deposit", "ic-deposit-accepted"]],
    ["Bridge authorization", ["authorization-generating"]],
    [
      "Base mint transaction",
      ["awaiting-base-mint", "base-mint-submitted", "base-mint-included", "base-mint-finalizing"],
    ],
  ] as const
  const withdrawal = [
    ["IC destination verification", ["verifying-ic-destination"]],
    ["Base token approval", ["awaiting-base-allowance"]],
    [
      "Base withdrawal transaction",
      ["awaiting-base-withdrawal", "base-withdrawal-submitted", "base-withdrawal-included"],
    ],
    ["Base finality", ["base-withdrawal-finalizing"]],
    ["IC notification", ["awaiting-ic-notification"]],
    ["Ledger payout", ["ic-notification-recorded", "ledger-payout"]],
    ["Complete", ["complete"]],
  ] as const
  const steps = record.direction === "deposit" ? deposit : withdrawal
  const depositTransactionComplete = isDepositTransactionComplete(record)
  const currentIndex =
    record.phase === "attention"
      ? Math.max(
          0,
          steps.findIndex(([, phases]) =>
            (phases as readonly string[]).includes(record.attentionPhase ?? previousPhase(record)),
          ),
        )
      : Math.max(
          0,
          steps.findIndex(([, phases]) => (phases as readonly string[]).includes(record.phase)),
        )
  const approvalLabel = record.direction === "deposit" ? "IC token approval" : "Base token approval"
  return steps.map(([label], index) => {
    const approvalNotRequired = label === approvalLabel && record.tokenApproval === "not-required"
    const step: BridgeProgressStep = {
      label,
      status:
        record.phase === "attention" && index === currentIndex
          ? "attention"
          : record.phase === "complete" || depositTransactionComplete || approvalNotRequired
            ? "complete"
            : index < currentIndex
              ? "complete"
              : index === currentIndex
                ? "current"
                : "waiting",
    }
    if (approvalNotRequired) step.note = "Not required"
    return step
  })
}

export function withdrawalFinalityProgress(record: BridgeProgressRecord):
  | {
      finalizedBlockNumber: string
      targetBlockNumber: string
      remainingBlocks: string
    }
  | undefined {
  if (
    record.direction !== "withdraw" ||
    record.phase !== "base-withdrawal-finalizing" ||
    !record.finalizedBlockNumber ||
    !record.receiptBlockNumber
  )
    return undefined
  const finalized = BigInt(record.finalizedBlockNumber)
  const target = BigInt(record.receiptBlockNumber)
  return {
    finalizedBlockNumber: finalized.toLocaleString("en-US"),
    targetBlockNumber: target.toLocaleString("en-US"),
    remainingBlocks: (target > finalized ? target - finalized : 0n).toLocaleString("en-US"),
  }
}

/** True after Base has returned a successful receipt, before canonical finality is known. */
export function isDepositTransactionComplete(record: BridgeProgressRecord): boolean {
  return (
    record.direction === "deposit" &&
    record.baseTransactionOutcome === "success" &&
    (record.phase === "base-mint-included" || record.phase === "base-mint-finalizing")
  )
}

function previousPhase(record: BridgeProgressRecord): string {
  if (record.withdrawal?.withdrawalId) return "ledger-payout"
  if (record.receiptBlockNumber)
    return record.direction === "deposit" ? "base-mint-finalizing" : "base-withdrawal-finalizing"
  if (record.transactionHash)
    return record.direction === "deposit" ? "base-mint-submitted" : "base-withdrawal-submitted"
  if (record.direction === "deposit" && record.deposit?.depositId) return "authorization-generating"
  return record.direction === "deposit" ? "awaiting-ic-deposit" : "verifying-ic-destination"
}

function shortDestination(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}

function isStoredBridgeProgress(value: unknown): value is StoredBridgeProgress {
  if (!value || typeof value !== "object") return false
  const item = value as Partial<StoredBridgeProgress>
  if (
    !(
      item.version === STORAGE_VERSION &&
      typeof item.id === "string" &&
      item.id.length > 0 &&
      (item.direction === "deposit" || item.direction === "withdraw") &&
      typeof item.source === "string" &&
      typeof item.destination === "string" &&
      typeof item.sendAmount === "string" &&
      typeof item.receiveAmount === "string" &&
      typeof item.sendSymbol === "string" &&
      typeof item.receiveSymbol === "string" &&
      (item.tokenApproval === "required" || item.tokenApproval === "not-required") &&
      typeof item.createdAt === "number" &&
      Number.isFinite(item.createdAt) &&
      item.createdAt >= 0 &&
      validOptionalTransactionHash(item.transactionHash) &&
      validOptionalDepositIdentity(item.deposit) &&
      validOptionalWithdrawalIdentity(item.withdrawal)
    )
  )
    return false

  if (item.phase === "attention")
    return (
      typeof item.attentionMessage === "string" &&
      item.attentionMessage.length > 0 &&
      validOptionalActivePhase(item.attentionPhase)
    )
  if (item.direction === "deposit") {
    if (!item.deposit?.depositId) return false
    return (
      item.phase === "authorization-generating" ||
      (item.phase === "base-mint-submitted" && item.transactionHash !== undefined)
    )
  }
  if (!item.withdrawal) return false
  return (
    (item.phase === "base-withdrawal-submitted" && item.transactionHash !== undefined) ||
    (item.phase === "ledger-payout" && item.withdrawal.withdrawalId !== undefined)
  )
}

function restorablePhase(record: BridgeProgressRecord): RestorableBridgeProgressPhase | undefined {
  if (record.phase === "attention" && record.attentionMessage) return "attention"
  if (record.direction === "deposit" && record.deposit?.depositId) {
    return record.transactionHash ? "base-mint-submitted" : "authorization-generating"
  }
  if (record.direction === "withdraw" && record.withdrawal) {
    if (record.withdrawal.withdrawalId) return "ledger-payout"
    if (record.transactionHash) return "base-withdrawal-submitted"
  }
  return undefined
}

function validOptionalTransactionHash(value: unknown): value is `0x${string}` | undefined {
  return value === undefined || (typeof value === "string" && /^0x[0-9a-fA-F]{64}$/.test(value))
}

function validOptionalDepositIdentity(
  value: unknown,
): value is BridgeProgressDepositIdentity | undefined {
  if (value === undefined) return true
  if (!value || typeof value !== "object") return false
  const item = value as Partial<BridgeProgressDepositIdentity>
  return (
    typeof item.owner === "string" &&
    item.owner.length > 0 &&
    typeof item.ownerSequence === "string" &&
    /^(0|[1-9][0-9]*)$/.test(item.ownerSequence) &&
    (item.depositId === undefined || /^0x[0-9a-fA-F]{64}$/.test(item.depositId))
  )
}

function validOptionalWithdrawalIdentity(
  value: unknown,
): value is BridgeProgressWithdrawalIdentity | undefined {
  if (value === undefined) return true
  if (!value || typeof value !== "object") return false
  const item = value as Partial<BridgeProgressWithdrawalIdentity>
  return (
    typeof item.owner === "string" &&
    item.owner.length > 0 &&
    (item.withdrawalId === undefined || /^0x[0-9a-fA-F]{64}$/.test(item.withdrawalId))
  )
}

function validOptionalActivePhase(value: unknown): value is ActiveBridgeProgressPhase | undefined {
  return (
    value === undefined ||
    (typeof value === "string" &&
      [
        "verifying-ic-destination",
        "awaiting-ic-allowance",
        "awaiting-ic-deposit",
        "ic-deposit-accepted",
        "authorization-generating",
        "awaiting-base-mint",
        "base-mint-submitted",
        "base-mint-included",
        "base-mint-finalizing",
        "awaiting-base-allowance",
        "awaiting-base-withdrawal",
        "base-withdrawal-submitted",
        "base-withdrawal-included",
        "base-withdrawal-finalizing",
        "awaiting-ic-notification",
        "ic-notification-recorded",
        "ledger-payout",
      ].includes(value))
  )
}
