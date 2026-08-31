import { Link } from "@tanstack/react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ArrowDownUp, ArrowRight, LoaderCircle, LockKeyhole, RefreshCcw, TriangleAlert } from "lucide-react"
import { Principal } from "@icp-sdk/core/principal"
import { useEffect, useMemo, useRef, useState } from "react"
import { toast } from "sonner"
import { hexToBytes } from "viem"
import { useAccount, useChainId, useConnectorClient, useWriteContract } from "wagmi"
import baseLogo from "@/assets/base-square.svg"
import icpLogo from "@/assets/icp-logo-mark.svg"
import { Button } from "@/components/ui/button"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { deploymentProfile } from "@/config/profile"
import { useCurrentBaseQuote, useRuntimeHeartbeat, useRuntimeValidation } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { useWalletDialog } from "@/features/wallet/wallet-controls"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import type { DepositView } from "@/generated/bridge.did"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { estimatedAmountOut, formatTokenAmount, maximumDepositAmount, parseTokenAmount, requiredDepositBalance } from "@/lib/amounts"
import { shortenWalletAddress } from "@/lib/wallet-address"
import { classifyDepositRecoverySequence } from "@/lib/deposit-recovery"
import { createLedgerActor, ledgerAccount } from "@/lib/ic/ledger"
import { createBridgeActor } from "@/lib/ic/bridge"
import type { DepositCall, IcAccount } from "@/lib/ic/wallet"
import { basePublicClient } from "@/lib/evm/client"
import { refetchRuntimeAttestedWriteReady, runtimeWriteBlocker, RUNTIME_VALIDATION_TTL_MS, type FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import { currentInjectedWallet, requireWalletSnapshot, sameIcAccount } from "@/lib/wallet-snapshot"
import { createWithdrawalAfterRevalidation } from "@/lib/withdrawal-submit"
import { savePendingConfirmation } from "@/lib/pending-confirmations"
import { readDepositIntent, removeDepositIntent, saveDepositIntent } from "@/lib/deposit-intents"
import { withBrowserLock } from "@/lib/browser-lock"
import { depositContinuation, isDepositTerminal } from "@/lib/settlement-phase"
import type { BridgeProgressPhase } from "@/lib/bridge-progress"

export type BridgeDirection = "deposit" | "withdraw"
type BridgeNetwork = "ic" | "base"
const automaticQueryOptions = {
  refetchOnWindowFocus: true,
  refetchOnReconnect: true,
  staleTime: RUNTIME_VALIDATION_TTL_MS,
} as const

const NETWORKS: Record<BridgeNetwork, { label: string; logo: string }> = {
  ic: { label: "Internet Computer", logo: icpLogo },
  base: { label: "Base", logo: baseLogo },
}

interface UnresolvedDepositAttempt {
  call: DepositCall
  account: IcAccount
  recipient: `0x${string}`
}

type DepositProgress = "idle" | "checking" | "oisy-action" | "authorization"
interface DepositWriteGate {
  base: NonNullable<FinalizedRuntimeObservation["snapshot"]>
  ledger: { balance: bigint; fee: bigint; allowance: bigint }
  sequence: bigint
  observation: FinalizedRuntimeObservation
}

export function validatedDepositWriteGate(input: {
  amount: bigint
  expectedSequence: bigint
  observation: FinalizedRuntimeObservation
  ledger: DepositWriteGate["ledger"]
  sequence: bigint
}): DepositWriteGate {
  const { amount, expectedSequence, observation, ledger, sequence } = input
  const quote = observation.snapshot
  if (!quote) throw new Error("Finalized Base snapshot is unavailable")
  if (quote.depositsPaused) throw new Error("Deposits are paused on Base")
  if (amount > quote.perDepositLimit) throw new Error("Amount exceeds the current per-deposit limit")
  if (amount <= quote.serviceFee) throw new Error("Amount must exceed the current service fee")
  const windowEndsAt = quote.startedAt + quote.duration
  if (quote.blockTimestamp === windowEndsAt) throw new Error("The finalized mint window snapshot is at its rollover boundary; refresh and review again")
  if (quote.blockTimestamp < windowEndsAt && quote.minted + amount - quote.serviceFee > quote.limit) throw new Error("Amount exceeds the remaining mint window limit")
  if (sequence !== expectedSequence) throw new Error("Another deposit used this owner sequence; refresh and review again")
  if (ledger.balance < requiredDepositBalance(amount, ledger.fee, ledger.allowance)) throw new Error(`${deploymentProfile.icToken.symbol} balance does not cover the deposit and required ledger fees`)
  return { base: quote, ledger, sequence, observation }
}
interface ReviewedDeposit {
  amount: bigint
  account: IcAccount
  recipient: `0x${string}`
  gate: DepositWriteGate
}
interface DepositMutationInput {
  attempt: UnresolvedDepositAttempt
  closeWalletSession: () => Promise<void>
  progressId: string
}

type PreflightCheckId = "wallets" | "runtime" | "financials" | "availability"
type PreflightCheckStatus = "waiting" | "checking" | "passed" | "failed"
type PreflightPhase = "checking" | "ready" | "failed"
interface PreflightCheck {
  id: PreflightCheckId
  label: string
  status: PreflightCheckStatus
  error?: string
}
interface PreflightState {
  runId: number
  direction: BridgeDirection
  phase: PreflightPhase
  checks: PreflightCheck[]
}

const PREFLIGHT_CHECKS: ReadonlyArray<Pick<PreflightCheck, "id" | "label">> = [
  { id: "wallets", label: "Wallets connected" },
  { id: "runtime", label: "Bridge configuration verified" },
  { id: "financials", label: "Balance and fees checked" },
  { id: "availability", label: "Transfer availability checked" },
]
class StalePreflightError extends Error {}

function initialPreflight(runId: number, direction: BridgeDirection): PreflightState {
  return {
    runId,
    direction,
    phase: "checking",
    checks: PREFLIGHT_CHECKS.map((check) => ({ ...check, status: "waiting" })),
  }
}

export function BridgePage({ direction, onDirectionChange }: { direction: BridgeDirection; onDirectionChange: (direction: BridgeDirection) => void }) {
  const [depositAmount, setDepositAmount] = useState("")
  const [withdrawAmount, setWithdrawAmount] = useState("")
  const [confirming, setConfirming] = useState(false)
  const [depositProgress, setDepositProgress] = useState<DepositProgress>("idle")
  const [reviewedDeposit, setReviewedDeposit] = useState<ReviewedDeposit>()
  const [reviewedWithdrawalAccount, setReviewedWithdrawalAccount] = useState<IcAccount>()
  const [reviewedObservation, setReviewedObservation] = useState<FinalizedRuntimeObservation>()
  const [unresolvedDeposit, setUnresolvedDeposit] = useState<UnresolvedDepositAttempt>()
  const [resolvedIntentOwner, setResolvedIntentOwner] = useState<string>()
  const [checkingDeposit, setCheckingDeposit] = useState(false)
  const [activeDeposit, setActiveDeposit] = useState<{ owner: string; sequence: bigint }>()
  const [submittingWithdrawal, setSubmittingWithdrawal] = useState(false)
  const [reviewedApprovalNeeded, setReviewedApprovalNeeded] = useState<boolean>()
  const [preflight, setPreflight] = useState<PreflightState>()
  const preflightRunId = useRef(0)
  const activeDepositProgressSeen = useRef(false)
  const queryClient = useQueryClient()
  const bridgeProgress = useBridgeProgress()
  const { address, isConnected } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const wallets = useWalletDialog()
  const write = useWriteContract()
  const connectorClient = useConnectorClient()
  const currentBaseWallet = () => currentInjectedWallet(connectorClient.data?.transport)
  const runtime = useRuntimeValidation(chainId, {
    enabled: false,
    gcTime: Infinity,
    staleTime: RUNTIME_VALIDATION_TTL_MS,
  })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, {
    enabled: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
  const baseQuote = useCurrentBaseQuote({ enabled: true, staleTime: 15_000 })
  const sendToken = direction === "deposit" ? deploymentProfile.icToken : deploymentProfile.baseToken
  const receiveToken = direction === "deposit" ? deploymentProfile.baseToken : deploymentProfile.icToken
  const baseData = baseQuote.data
  const depositParsed = useMemo(() => parseTokenAmount(depositAmount), [depositAmount])
  const withdrawParsed = useMemo(() => parseTokenAmount(withdrawAmount), [withdrawAmount])

  const ownerSequenceKey = ["deposit-owner-sequence", ic.account?.owner] as const
  const ownerSequence = useQuery({
    queryKey: ownerSequenceKey,
    enabled: direction === "deposit"
      && Boolean(ic.account)
      && resolvedIntentOwner === ic.account?.owner
      && !unresolvedDeposit,
    ...automaticQueryOptions,
    queryFn: async () => {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      return actor.get_next_deposit_sequence(Principal.fromText(ic.account!.owner))
    },
  })
  const activeDepositRecord = useQuery({
    queryKey: ["active-deposit", activeDeposit?.owner, activeDeposit?.sequence.toString()],
    enabled: direction === "deposit" && Boolean(activeDeposit),
    refetchInterval: 5_000,
    refetchIntervalInBackground: false,
    queryFn: async () => {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      const result = await actor.get_deposit_by_owner_sequence(
        Principal.fromText(activeDeposit!.owner),
        activeDeposit!.sequence,
      )
      if (!result[0]) throw new Error("Canonical deposit is not available yet")
      return result[0]
    },
  })
  const effectiveDepositProgress: DepositProgress = depositProgress === "authorization"
    && activeDepositRecord.data
    && !isDepositAuthorizationPending(activeDepositRecord.data.state)
    ? "idle"
    : depositProgress
  const activeDepositTerminal = Boolean(
    activeDepositRecord.data && isDepositTerminal(activeDepositRecord.data.state),
  )
  useEffect(() => {
    const progress = bridgeProgress.progress
    const record = activeDepositRecord.data
    if (!progress || progress.direction !== "deposit" || !record || !activeDeposit) return
    if (progress.deposit
      && (progress.deposit.owner !== activeDeposit.owner || progress.deposit.ownerSequence !== activeDeposit.sequence.toString())) return
    const continuation = depositContinuation(record)
    if ("Minted" in record.state) {
      bridgeProgress.update(progress.id, { phase: "complete", completionMessage: `${progress.receiveAmount} ${progress.receiveSymbol} was minted on Base.` })
    } else if ("AuthorizationAvailable" in record.state && !progress.transactionHash) {
      bridgeProgress.update(progress.id, { phase: "awaiting-base-mint" })
    } else if (continuation.mode === "stopped") {
      bridgeProgress.update(progress.id, {
        phase: "attention",
        attentionPhase: "authorization-generating",
        attentionMessage: continuation.message ?? "This deposit stopped and needs attention.",
      })
    } else if (continuation.mode === "automatic" && continuation.reason) {
      bridgeProgress.update(progress.id, {
        phase: "attention",
        attentionPhase: "authorization-generating",
        attentionMessage: continuation.message ?? "The previous attempt stopped temporarily. The Bridge will retry automatically.",
      })
    } else if (isDepositAuthorizationPending(record.state)) {
      bridgeProgress.update(progress.id, { phase: "authorization-generating" })
    } else if ("RefundAvailable" in record.state || "RefundProcessing" in record.state || "Refunded" in record.state || "FundingReconciliationHold" in record.state || "Cancelled" in record.state) {
      bridgeProgress.update(progress.id, { phase: "attention", attentionMessage: "This deposit is not minting on Base. Open History to review the available refund path." })
    }
  }, [activeDeposit, activeDepositRecord.data, bridgeProgress])
  useEffect(() => {
    if (!activeDepositTerminal) return
    const reset = window.setTimeout(() => {
      setDepositAmount("")
      setReviewedDeposit(undefined)
      setActiveDeposit(undefined)
      setDepositProgress("idle")
    }, 0)
    return () => window.clearTimeout(reset)
  }, [activeDepositTerminal])
  useEffect(() => {
    if (!activeDeposit) {
      activeDepositProgressSeen.current = false
      return
    }
    const progress = bridgeProgress.progress
    if (progress?.direction === "deposit"
      && progress.deposit?.owner === activeDeposit.owner
      && progress.deposit.ownerSequence === activeDeposit.sequence.toString()) {
      activeDepositProgressSeen.current = true
      return
    }
    if (progress || !activeDepositProgressSeen.current) return
    activeDepositProgressSeen.current = false
    setDepositAmount("")
    setReviewedDeposit(undefined)
    setActiveDeposit(undefined)
    setDepositProgress("idle")
  }, [activeDeposit, bridgeProgress.progress])
  const ledger = useQuery({
    queryKey: ["deposit-ledger", ic.account?.owner, bytesHex(ic.account?.subaccount ?? new Uint8Array())],
    enabled: direction === "deposit" && Boolean(ic.account),
    ...automaticQueryOptions,
    queryFn: async () => {
      const ledgerActor = await createLedgerActor(deploymentProfile.icHost, deploymentProfile.ledgerCanisterId as string)
      const account = ledgerAccount(ic.account!.owner, ic.account!.subaccount)
      const spender = ledgerAccount(deploymentProfile.bridgeCanisterId as string)
      const [balance, allowance, fee] = await Promise.all([
        ledgerActor.icrc1_balance_of(account),
        ledgerActor.icrc2_allowance({ account, spender }),
        ledgerActor.icrc1_fee(),
      ])
      return {
        balance,
        fee,
        allowance: allowance.allowance,
      }
    },
  })
  const bsnsBalance = useQuery({
    queryKey: ["bsns-balance", address],
    enabled: direction === "withdraw" && Boolean(address),
    ...automaticQueryOptions,
    queryFn: () => basePublicClient.readContract({ address: deploymentProfile.bsnsAddress as `0x${string}`, abi: bsnsAbi, functionName: "balanceOf", args: [address!] }),
  })
  const ledgerData = !ledger.isError && !ledger.isStale ? ledger.data : undefined
  const bsnsBalanceData = !bsnsBalance.isError && !bsnsBalance.isStale ? bsnsBalance.data : undefined
  const refreshing = baseQuote.isFetching || ledger.isFetching || bsnsBalance.isFetching || (!unresolvedDeposit && ownerSequence.isFetching)
  const refreshBridgeData = () => {
    const calls: Promise<unknown>[] = [baseQuote.refetch()]
    if (direction === "deposit" && ic.account) {
      calls.push(ledger.refetch())
      if (!unresolvedDeposit) calls.push(ownerSequence.refetch())
    }
    if (direction === "withdraw" && address) calls.push(bsnsBalance.refetch())
    void Promise.all(calls)
  }
  useEffect(() => {
    const account = ic.account
    let active = true
    queueMicrotask(() => {
      if (active) {
        setUnresolvedDeposit(account ? readDepositIntent(account) : undefined)
        setResolvedIntentOwner(account?.owner)
      }
    })
    return () => { active = false }
  }, [ic.account])

  const deposit = useMutation({
    mutationFn: async ({ attempt, closeWalletSession }: DepositMutationInput) => {
      if (!address || !isConnected || !ic.account || !ic.adapter) throw new Error("Reconnect the wallets used for this deposit")
      const activeEvm = await currentBaseWallet()
      const activeIc = await ic.adapter.getAccount()
      requireWalletSnapshot(
        { address: attempt.recipient, chainId: deploymentProfile.chainId, icAccount: attempt.account },
        { ...activeEvm, icAccount: activeIc },
        "before submitting this deposit",
      )
      await saveDepositIntent({ ...attempt, state: "submitted" })
      setUnresolvedDeposit(attempt)
      let receipt
      try {
        receipt = await withBrowserLock(`kinic-wallet-prompt:ic:${attempt.account.owner}`, () => ic.adapter!.requestDeposit(attempt.call))
      } finally {
        await closeWalletSession().catch(() => undefined)
      }
      const [postEvm, postIc] = await Promise.all([currentBaseWallet(), ic.adapter.getAccount()])
      requireWalletSnapshot(
        { address: attempt.recipient, chainId: deploymentProfile.chainId, icAccount: attempt.account },
        { ...postEvm, icAccount: postIc },
        "during the wallet prompt",
      )
      return receipt
    },
    onSuccess: async (receipt, { attempt, progressId }) => {
      queryClient.setQueryData(["deposit-owner-sequence", attempt.account.owner], receipt.owner_sequence + 1n)
      setActiveDeposit({ owner: attempt.account.owner, sequence: receipt.owner_sequence })
      setDepositProgress("authorization")
      bridgeProgress.update(progressId, {
        phase: "ic-deposit-accepted",
        deposit: {
          owner: attempt.account.owner,
          ownerSequence: receipt.owner_sequence.toString(),
          depositId: bytesHex(receipt.deposit_id),
        },
      })
      try { await removeDepositIntent(attempt.account) } catch { /* The canonical receipt is the recovery source. */ }
      setUnresolvedDeposit(undefined)
      void Promise.allSettled([
        queryClient.invalidateQueries({ queryKey: ["deposit-ledger"] }),
        queryClient.invalidateQueries({ queryKey: ["base-quote"] }),
        queryClient.invalidateQueries({ queryKey: ["runtime-validation"] }),
        queryClient.invalidateQueries({ queryKey: ["deposit-history"] }),
      ])
      toast.success(`Deposit ${bytesHex(receipt.deposit_id).slice(0, 14)}… accepted. Mint Authorization is being generated.`)
    },
    onError: (error, { progressId }) => {
      setDepositProgress("idle")
      bridgeProgress.update(progressId, {
        phase: "attention",
        attentionMessage: error instanceof Error
          ? `${error.message}. Check History before starting another deposit.`
          : "The deposit response is unresolved. Check History before starting another deposit.",
      })
      toast.error(error instanceof Error ? `${error.message}. Retry the same deposit or check whether it was accepted.` : "Deposit response is unresolved")
    },
  })

  const submitDeposit = async (progressId: string) => {
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
      if (!unresolvedDeposit && !reviewedDeposit) throw new Error("Check the deposit again before opening OISY")
      setDepositProgress("oisy-action")
      const walletSession = ic.adapter.prepare()
      if (unresolvedDeposit) {
        closeWalletSession = onceAsync(await walletSession)
        await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
        bridgeProgress.update(progressId, { phase: "awaiting-ic-deposit" })
        await withBrowserLock(`kinic-deposit-owner:${unresolvedDeposit.account.owner}`, () => deposit.mutateAsync({ attempt: unresolvedDeposit, closeWalletSession: closeWalletSession!, progressId }))
        return
      }
      const reviewed = reviewedDeposit!
      closeWalletSession = onceAsync(await walletSession)
      const confirmedAccount = reviewed.account
      const confirmedRecipient = reviewed.recipient
      const activeEvm = await currentBaseWallet()
      const activeIc = await ic.adapter.getAccount()
      const expectedWallets = { address: confirmedRecipient, chainId: deploymentProfile.chainId, icAccount: confirmedAccount }
      requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc })
      await withBrowserLock(`kinic-deposit-owner:${confirmedAccount.owner}`, async () => {
        const beforeApproval = await refetchDepositWriteGate(
          reviewed.amount,
          reviewed.gate.sequence,
        )
        const requiredAllowance = reviewed.amount + beforeApproval.ledger.fee
        if (beforeApproval.ledger.allowance < requiredAllowance) {
          bridgeProgress.update(progressId, { phase: "awaiting-ic-allowance" })
          await withBrowserLock(`kinic-wallet-prompt:ic:${confirmedAccount.owner}`, () => ic.adapter!.approve({ amount: requiredAllowance, currentAllowance: beforeApproval.ledger.allowance, ledgerFee: beforeApproval.ledger.fee }))
        }
        bridgeProgress.update(progressId, { phase: "awaiting-ic-deposit" })
        const [finalEvm, finalIc] = await Promise.all([currentBaseWallet(), ic.adapter!.getAccount()])
        requireWalletSnapshot(expectedWallets, { ...finalEvm, icAccount: finalIc }, "during approval")
        const final = await refetchDepositWriteGate(
          reviewed.amount,
          beforeApproval.sequence,
          undefined,
        )
        const attempt: UnresolvedDepositAttempt = {
          call: { ownerSequence: final.sequence, baseRecipient: hexToBytes(confirmedRecipient), grossAmount: reviewed.amount, maxServiceFee: final.base.serviceFee },
          account: { owner: confirmedAccount.owner, subaccount: confirmedAccount.subaccount?.slice() },
          recipient: confirmedRecipient,
        }
        await saveDepositIntent({ ...attempt, state: "prepared" })
        setUnresolvedDeposit(attempt)
        await deposit.mutateAsync({ attempt, closeWalletSession: closeWalletSession!, progressId })
      })
    } catch (error) {
      setDepositProgress("idle")
      bridgeProgress.update(progressId, {
        phase: "attention",
        attentionMessage: error instanceof Error ? error.message : "The deposit could not continue.",
      })
      toast.error(error instanceof Error ? error.message : "Deposit failed")
    } finally {
      await closeWalletSession?.().catch(() => undefined)
      setReviewedDeposit(undefined)
    }
  }

  const refetchDepositWriteGate = async (
    amount: bigint,
    expectedSequence: bigint,
    reusableObservation?: FinalizedRuntimeObservation,
  ): Promise<DepositWriteGate> => {
    const observationPromise = reusableObservation && runtimeWriteBlocker(reusableObservation) === undefined
      ? Promise.resolve(reusableObservation)
      : refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
    const [observation, ledgerResult, sequenceResult] = await Promise.all([observationPromise, ledger.refetch(), ownerSequence.refetch()])
    if (ledgerResult.isError || ledgerResult.isStale || !ledgerResult.data || sequenceResult.isError || sequenceResult.isStale || sequenceResult.data === undefined) {
      throw new Error("Deposit limits, balance, fee, allowance, or sequence could not be verified")
    }
    return validatedDepositWriteGate({
      amount,
      expectedSequence,
      observation,
      ledger: ledgerResult.data,
      sequence: sequenceResult.data,
    })
  }

  const assertActivePreflight = (runId: number) => {
    if (preflightRunId.current !== runId) throw new StalePreflightError()
  }
  const updatePreflightCheck = (runId: number, id: PreflightCheckId, status: PreflightCheckStatus, error?: string) => {
    setPreflight((current) => current?.runId === runId
      ? {
          ...current,
          phase: status === "failed" ? "failed" : current.phase,
          checks: current.checks.map((check) => check.id === id ? { ...check, status, error } : check),
        }
      : current)
  }
  const runPreflightCheck = async <T,>(runId: number, id: PreflightCheckId, action: () => Promise<T> | T): Promise<T> => {
    assertActivePreflight(runId)
    updatePreflightCheck(runId, id, "checking")
    try {
      const result = await action()
      assertActivePreflight(runId)
      updatePreflightCheck(runId, id, "passed")
      return result
    } catch (error) {
      if (error instanceof StalePreflightError) throw error
      const message = error instanceof Error ? error.message : "This check could not be completed"
      updatePreflightCheck(runId, id, "failed", message)
      throw error
    }
  }
  const completePreflight = (runId: number) => {
    assertActivePreflight(runId)
    setPreflight((current) => current?.runId === runId ? { ...current, phase: "ready" } : current)
  }

  const runDepositPreflight = async (runId: number) => {
    setDepositProgress("checking")
    setReviewedDeposit(undefined)
    setReviewedObservation(undefined)
    try {
      const walletSnapshot = await runPreflightCheck(runId, "wallets", async () => {
        if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
        if (!address || !isConnected) throw new Error("Connect the Base recipient wallet")
        const account = unresolvedDeposit?.account ?? { owner: ic.account.owner, subaccount: ic.account.subaccount }
        const recipient = unresolvedDeposit?.recipient ?? address
        const expectedWallets = { address: recipient, chainId: deploymentProfile.chainId, icAccount: account }
        const [activeEvm, activeIc] = await Promise.all([currentBaseWallet(), ic.adapter.getAccount()])
        requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc }, "before opening the wallet prompt")
        return { account, recipient }
      })
      const observation = await runPreflightCheck(runId, "runtime", () => refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch))
      const financials = await runPreflightCheck(runId, "financials", async () => {
        if (unresolvedDeposit) return undefined
        if (!depositParsed.ok) throw new Error(depositParsed.reason)
        const [ledgerResult, sequenceResult] = await Promise.all([ledger.refetch(), ownerSequence.refetch()])
        if (ledgerResult.isError || ledgerResult.isStale || !ledgerResult.data || sequenceResult.isError || sequenceResult.isStale || sequenceResult.data === undefined) {
          throw new Error("Balance, allowance, or deposit sequence could not be verified")
        }
        return { ledger: ledgerResult.data, sequence: sequenceResult.data }
      })
      const gate = await runPreflightCheck(runId, "availability", () => {
        if (unresolvedDeposit) return undefined
        if (!depositParsed.ok || !financials) throw new Error("Deposit amount or financial information is unavailable")
        return validatedDepositWriteGate({
          amount: depositParsed.value,
          expectedSequence: financials.sequence,
          observation,
          ledger: financials.ledger,
          sequence: financials.sequence,
        })
      })
      assertActivePreflight(runId)
      if (!unresolvedDeposit && depositParsed.ok && gate) {
        setReviewedDeposit({
          amount: depositParsed.value,
          account: { owner: walletSnapshot.account.owner, subaccount: walletSnapshot.account.subaccount?.slice() },
          recipient: walletSnapshot.recipient,
          gate,
        })
        setReviewedApprovalNeeded(gate.ledger.allowance < depositParsed.value + gate.ledger.fee)
      } else if (unresolvedDeposit) {
        setReviewedApprovalNeeded(false)
      }
      setReviewedObservation(observation)
      completePreflight(runId)
    } catch {
      // The failed step already owns the user-visible error.
    } finally {
      if (preflightRunId.current === runId) setDepositProgress("idle")
    }
  }

  const runWithdrawalPreflight = async (runId: number) => {
    setReviewedObservation(undefined)
    setReviewedWithdrawalAccount(undefined)
    try {
      const reviewedAccount = await runPreflightCheck(runId, "wallets", async () => {
        if (!address || !isConnected) throw new Error("Connect the EVM wallet that owns bSNS")
        if (!ic.account || !ic.adapter) throw new Error("Connect the destination IC wallet")
        let closeWalletSession: (() => Promise<void>) | undefined
        const expectedWallets = {
          address,
          chainId: deploymentProfile.chainId,
          icAccount: { owner: ic.account.owner, subaccount: ic.account.subaccount },
        }
        try {
          closeWalletSession = await ic.adapter.prepare()
          const [activeEvm, activeIc] = await Promise.all([currentBaseWallet(), ic.adapter.getAccount()])
          requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc }, "during destination verification")
          return { owner: activeIc.owner, subaccount: activeIc.subaccount?.slice() }
        } finally {
          await closeWalletSession?.()
        }
      })
      const observation = await runPreflightCheck(runId, "runtime", () => refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch))
      const balance = await runPreflightCheck(runId, "financials", async () => {
        if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
        const quote = observation.snapshot
        const balanceResult = await bsnsBalance.refetch()
        if (!quote || balanceResult.isError || balanceResult.isStale || balanceResult.data === undefined) {
          throw new Error("Withdrawal fee or balance could not be verified")
        }
        if (withdrawParsed.value <= quote.serviceFee) throw new Error("Amount must be greater than the current service fee")
        if (balanceResult.data < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
        return balanceResult.data
      })
      const allowance = await runPreflightCheck(runId, "availability", async () => {
        if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
        const quote = observation.snapshot
        if (!quote) throw new Error("Withdrawal availability could not be verified")
        if (quote.withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
        if (withdrawParsed.value <= quote.serviceFee) throw new Error("Amount must be greater than the current service fee")
        if (balance < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
        return basePublicClient.readContract({
          address: deploymentProfile.bsnsAddress as `0x${string}`,
          abi: bsnsAbi,
          functionName: "allowance",
          args: [address!, deploymentProfile.bridgeAddress as `0x${string}`],
        })
      })
      if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
      setReviewedWithdrawalAccount(reviewedAccount)
      setReviewedApprovalNeeded(allowance < withdrawParsed.value)
      setReviewedObservation(observation)
      completePreflight(runId)
    } catch {
      // The failed step already owns the user-visible error.
    }
  }

  const beginBridgeReview = () => {
    if (bridgeProgress.progress || direction === "deposit" && effectiveDepositProgress !== "idle") return
    const runId = preflightRunId.current + 1
    preflightRunId.current = runId
    setPreflight(initialPreflight(runId, direction))
    setReviewedApprovalNeeded(undefined)
    setConfirming(true)
    if (direction === "deposit") void runDepositPreflight(runId)
    else void runWithdrawalPreflight(runId)
  }

  const checkUnresolvedDeposit = async () => {
    if (!unresolvedDeposit) return
    setCheckingDeposit(true)
    try {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      const nextSequence = await actor.get_next_deposit_sequence(Principal.fromText(unresolvedDeposit.account.owner))
      const status = classifyDepositRecoverySequence(unresolvedDeposit.call.ownerSequence, nextSequence)
      if (status === "not-accepted") {
        queryClient.setQueryData(["deposit-owner-sequence", unresolvedDeposit.account.owner], nextSequence)
        await removeDepositIntent(unresolvedDeposit.account)
        setUnresolvedDeposit(undefined)
        toast.info("The deposit was not accepted. You can edit the form or submit a new request.")
      } else if (status === "accepted-or-conflicted") {
        const record = await actor.get_deposit_by_owner_sequence(
          Principal.fromText(unresolvedDeposit.account.owner),
          unresolvedDeposit.call.ownerSequence,
        )
        if (!record[0]
          || record[0].gross_amount !== unresolvedDeposit.call.grossAmount
          || record[0].max_service_fee !== unresolvedDeposit.call.maxServiceFee
          || bytesHex(record[0].base_recipient).toLowerCase() !== bytesHex(unresolvedDeposit.call.baseRecipient).toLowerCase()
          || bytesHex(record[0].from_subaccount[0] ?? new Uint8Array(32)) !== bytesHex(unresolvedDeposit.account.subaccount ?? new Uint8Array(32))) {
          throw new Error("Canonical deposit does not match the saved intent")
        }
        const canonical = record[0]
        const existingProgress = bridgeProgress.progress
        const matchesExistingProgress = existingProgress?.direction === "deposit"
          && existingProgress.deposit?.owner === unresolvedDeposit.account.owner
          && existingProgress.deposit.ownerSequence === unresolvedDeposit.call.ownerSequence.toString()
        if (existingProgress && !matchesExistingProgress) {
          throw new Error("Another transfer is active. Close it before recovering this deposit from History.")
        }
        const progressState = recoveredDepositProgressState(canonical)
        const depositIdentity = {
          owner: unresolvedDeposit.account.owner,
          ownerSequence: unresolvedDeposit.call.ownerSequence.toString(),
          depositId: bytesHex(canonical.deposit_id),
        }
        if (existingProgress) {
          bridgeProgress.update(existingProgress.id, { ...progressState, deposit: depositIdentity })
        } else {
          const quotedNetAmount = canonical.quote[0]?.net_amount
            ?? (canonical.gross_amount > canonical.max_service_fee ? canonical.gross_amount - canonical.max_service_fee : 0n)
          bridgeProgress.start({
            direction: "deposit",
            ...progressState,
            tokenApproval: "required",
            source: unresolvedDeposit.account.owner,
            destination: unresolvedDeposit.recipient,
            sendAmount: formatTokenAmount(canonical.gross_amount),
            receiveAmount: formatTokenAmount(quotedNetAmount),
            sendSymbol: deploymentProfile.icToken.symbol,
            receiveSymbol: deploymentProfile.baseToken.symbol,
            deposit: depositIdentity,
          })
        }
        setActiveDeposit({ owner: unresolvedDeposit.account.owner, sequence: unresolvedDeposit.call.ownerSequence })
        queryClient.setQueryData(["deposit-owner-sequence", unresolvedDeposit.account.owner], nextSequence)
        await removeDepositIntent(unresolvedDeposit.account)
        setUnresolvedDeposit(undefined)
        toast.success("The accepted deposit was recovered from canonical history.")
      } else {
        toast.error("This deposit needs attention. Check History before continuing.")
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "The deposit could not be checked. Try again from History.")
    } finally {
      setCheckingDeposit(false)
    }
  }

  const submitWithdrawal = async (progressId: string) => {
    try {
      setSubmittingWithdrawal(true)
      if (!address) throw new Error("Connect the EVM wallet that owns bSNS")
      if (!reviewedWithdrawalAccount) throw new Error("Verify the destination IC wallet again")
      if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
      if (baseData === undefined || bsnsBalanceData === undefined) throw new Error("Fee or balance data is unavailable or stale")
      if (withdrawParsed.value <= baseData.serviceFee) throw new Error("Amount must be greater than the current service fee")
      if (bsnsBalanceData < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
      const confirmedIcAccount = {
        owner: reviewedWithdrawalAccount.owner,
        subaccount: reviewedWithdrawalAccount.subaccount?.slice(),
      }
      const snapshotAddress = address
      const activeEvm = await currentBaseWallet()
      const expectedWallets = { address: snapshotAddress, chainId: deploymentProfile.chainId, icAccount: confirmedIcAccount }
      requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: confirmedIcAccount })
      const owner = Principal.fromText(confirmedIcAccount.owner).toUint8Array()
      const subaccount = confirmedIcAccount.subaccount ?? new Uint8Array(32)
      const [approvalObservation, approvalBalance] = await Promise.all([
        refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch),
        bsnsBalance.refetch(),
      ])
      const approvalQuote = approvalObservation.snapshot
      if (!approvalQuote) throw new Error("Finalized Base snapshot is unavailable")
      if (approvalBalance.isError || approvalBalance.isStale || approvalBalance.data === undefined) throw new Error("Withdrawal limits, fee, or balance could not be verified")
      if (approvalQuote.withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
      if (withdrawParsed.value <= approvalQuote.serviceFee || approvalBalance.data < withdrawParsed.value) throw new Error("Withdrawal fee or balance changed; review again")
      const client = basePublicClient
      const allowance = await client.readContract({
        address: deploymentProfile.bsnsAddress as `0x${string}`,
        abi: bsnsAbi,
        functionName: "allowance",
        args: [snapshotAddress, deploymentProfile.bridgeAddress as `0x${string}`],
      })
      if (allowance < withdrawParsed.value) {
        bridgeProgress.update(progressId, { phase: "awaiting-base-allowance", tokenApproval: "required" })
        const approvalHash = await withBrowserLock(`kinic-wallet-prompt:base:${snapshotAddress.toLowerCase()}`, () => write.writeContractAsync({
          account: snapshotAddress,
          address: deploymentProfile.bsnsAddress as `0x${string}`,
          abi: bsnsAbi,
          functionName: "approve",
          args: [deploymentProfile.bridgeAddress as `0x${string}`, withdrawParsed.value],
        }))
        const approvalReceipt = await client.waitForTransactionReceipt({ hash: approvalHash })
        if (approvalReceipt.status !== "success") throw new Error("Token approval failed")
        bridgeProgress.update(progressId, { phase: "awaiting-base-withdrawal" })
      } else {
        bridgeProgress.update(progressId, { phase: "awaiting-base-withdrawal", tokenApproval: "not-required" })
      }
      const broadcast = await createWithdrawalAfterRevalidation({
        expectedWallets,
        refetchRuntime: async () => ({ data: await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch) }),
        currentEvmWallet: currentBaseWallet,
        currentIcAccount: () => Promise.resolve({
          owner: confirmedIcAccount.owner,
          subaccount: confirmedIcAccount.subaccount?.slice(),
        }),
        refetchFinancials: async (observation) => {
          const quote = observation.snapshot
          if (!quote) throw new Error("Finalized Base snapshot is unavailable")
          const balanceResult = await bsnsBalance.refetch()
          if (balanceResult.isError || balanceResult.isStale || balanceResult.data === undefined) throw new Error("Fee or balance data changed and could not be verified")
          return { serviceFee: quote.serviceFee, balance: balanceResult.data, withdrawalsPaused: quote.withdrawalsPaused }
        },
        validateFinancials: ({ serviceFee, balance: finalBalance, withdrawalsPaused }) => {
          if (withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
          if (withdrawParsed.value <= serviceFee) throw new Error("Amount must be greater than the current service fee")
          if (finalBalance < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
        },
        createWithdrawal: ({ serviceFee }) => withBrowserLock(`kinic-wallet-prompt:base:${snapshotAddress.toLowerCase()}`, () => write.writeContractAsync({ account: snapshotAddress, address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, functionName: "createWithdrawal", args: [withdrawParsed.value, serviceFee, bytesToHex(owner), bytesToHex(subaccount)] })),
        onBroadcast: async (transactionHash) => {
          bridgeProgress.update(progressId, { phase: "base-withdrawal-submitted", transactionHash })
          return savePendingConfirmation({
            kind: "withdrawal",
            transactionHash,
            owner: confirmedIcAccount.owner,
          })
        },
      })
      setWithdrawAmount("")
      if (broadcast.pendingSaved) {
        toast.success(`Withdrawal submitted: ${broadcast.transactionHash.slice(0, 12)}…. Confirmation is pending. Check History after finalization if it has not completed.`)
      } else {
        toast.warning(`Withdrawal ${broadcast.transactionHash} was submitted, but this browser could not save it. Copy the transaction hash; after it succeeds, recover it from History.`)
      }
    } catch (error) {
      bridgeProgress.update(progressId, { phase: "attention", attentionMessage: error instanceof Error ? error.message : "The withdrawal could not continue." })
      toast.error(error instanceof Error ? error.message : "Withdrawal failed")
    }
    finally { setSubmittingWithdrawal(false) }
  }

  const retryAccountMatches = unresolvedDeposit && ic.account ? sameIcAccount(ic.account, unresolvedDeposit.account) : false
  const retryRecipientMatches = unresolvedDeposit && address ? address.toLowerCase() === unresolvedDeposit.recipient.toLowerCase() : false
  const reviewedQuote = reviewedDeposit?.gate.base ?? reviewedObservation?.snapshot
  const quoteForDisplay = reviewedQuote ?? baseData
  const depositsConfirmedPaused = baseData?.depositsPaused === true
  const withdrawalsConfirmedPaused = baseData?.withdrawalsPaused === true
  const activeTransferReason = bridgeProgress.progress
    ? "Complete or close the current transfer before starting another one"
    : undefined
  const depositBlockers = unresolvedDeposit
    ? [activeTransferReason, depositsConfirmedPaused && "Deposits are paused on Base", !ic.account && "Reconnect the original IC wallet", !address && "Reconnect the original EVM wallet", ic.account && !retryAccountMatches && "Reconnect the original IC wallet", address && !retryRecipientMatches && "Reconnect the original EVM wallet"].filter(Boolean) as string[]
    : [activeTransferReason, !address && "Connect both wallets", !ic.account && "Connect both wallets", depositsConfirmedPaused && "Deposits are paused on Base", !depositParsed.ok && (depositParsed.reason ?? "Enter an amount")].filter(Boolean) as string[]
  const withdrawalBlockers = [activeTransferReason, !address && "Connect both wallets", !ic.account && "Connect both wallets", withdrawalsConfirmedPaused && "Withdrawals are paused on Base", !withdrawParsed.ok && (withdrawParsed.reason ?? "Enter an amount")].filter(Boolean) as string[]
  const blockers = direction === "deposit" ? depositBlockers : withdrawalBlockers
  const awaitingDepositAuthorization = direction === "deposit"
    && Boolean(activeDeposit)
    && (!activeDepositRecord.data || isDepositAuthorizationPending(activeDepositRecord.data.state))
  const depositActionPending = direction === "deposit" && (effectiveDepositProgress !== "idle" || deposit.isPending || awaitingDepositAuthorization)
  const amountError = !unresolvedDeposit && (direction === "deposit" ? (!depositParsed.ok ? depositParsed.reason : undefined) : (!withdrawParsed.ok ? withdrawParsed.reason : undefined))
  const amount = direction === "deposit" ? (unresolvedDeposit ? formatTokenAmount(unresolvedDeposit.call.grossAmount) : depositAmount) : withdrawAmount
  const balance = direction === "deposit" ? ledgerData?.balance : bsnsBalanceData
  const fee = unresolvedDeposit?.call.maxServiceFee ?? quoteForDisplay?.serviceFee
  const feeLabel = reviewedQuote || baseData ? "Current bridge fee" : "Bridge fee"
  const receive = direction === "deposit" ? (unresolvedDeposit ? (unresolvedDeposit.call.grossAmount > unresolvedDeposit.call.maxServiceFee ? unresolvedDeposit.call.grossAmount - unresolvedDeposit.call.maxServiceFee : 0n) : depositParsed.ok && fee !== undefined ? (depositParsed.value > fee ? depositParsed.value - fee : 0n) : undefined) : withdrawParsed.ok && fee !== undefined && withdrawParsed.value > fee ? estimatedAmountOut(withdrawParsed.value, fee) : undefined
  const source = direction === "deposit" ? { network: "ic" as const, wallet: unresolvedDeposit?.account.owner ?? ic.account?.owner ?? "Connect IC wallet" } : { network: "base" as const, wallet: address ?? "Connect EVM wallet" }
  const destination = direction === "deposit" ? { network: "base" as const, wallet: unresolvedDeposit?.recipient ?? address ?? "Connect EVM wallet" } : { network: "ic" as const, wallet: reviewedWithdrawalAccount?.owner ?? ic.account?.owner ?? "Connect IC wallet" }
  const depositFlowActive = direction === "deposit" && Boolean(activeDeposit)
  const depositControlsLocked = direction === "deposit"
    && (Boolean(unresolvedDeposit) || effectiveDepositProgress !== "idle" || depositFlowActive)
  const maximumAmount = direction === "deposit"
    ? ledgerData !== undefined
      ? maximumDepositAmount(ledgerData.balance, ledgerData.fee, ledgerData.allowance)
      : undefined
    : bsnsBalanceData
  const maximumAmountDisabled = depositControlsLocked || maximumAmount === undefined || maximumAmount === 0n
  const useMaximumAmount = () => {
    if (maximumAmountDisabled || maximumAmount === undefined) return
    const formatted = formatTokenAmount(maximumAmount)
    if (direction === "deposit") setDepositAmount(formatted)
    else setWithdrawAmount(formatted)
  }

  const changeDirection = () => { if (depositControlsLocked) return; setConfirming(false); onDirectionChange(direction === "deposit" ? "withdraw" : "deposit") }
  const setBridgeReviewOpen = (open: boolean) => {
    if (open) {
      setConfirming(true)
      return
    }
    preflightRunId.current += 1
    setConfirming(false)
    setPreflight(undefined)
    setReviewedDeposit(undefined)
    setReviewedWithdrawalAccount(undefined)
    setReviewedObservation(undefined)
    setDepositProgress((current) => current === "checking" ? "idle" : current)
  }
  const confirmBridgeReview = () => {
    preflightRunId.current += 1
    setConfirming(false)
    setPreflight(undefined)
    let progress
    try {
      if (direction === "withdraw" && !reviewedWithdrawalAccount) {
        throw new Error("Verify the destination IC wallet again")
      }
      progress = bridgeProgress.start({
        direction,
        phase: direction === "deposit"
          ? reviewedApprovalNeeded === false ? "awaiting-ic-deposit" : "awaiting-ic-allowance"
          : "verifying-ic-destination",
        tokenApproval: reviewedApprovalNeeded === false ? "not-required" : "required",
        source: source.wallet,
        destination: destination.wallet,
        sendAmount: amount || "—",
        receiveAmount: receive !== undefined ? formatTokenAmount(receive) : "—",
        sendSymbol: sendToken.symbol,
        receiveSymbol: receiveToken.symbol,
        deposit: direction === "deposit"
          ? unresolvedDeposit
            ? { owner: unresolvedDeposit.account.owner, ownerSequence: unresolvedDeposit.call.ownerSequence.toString() }
            : reviewedDeposit
              ? { owner: reviewedDeposit.account.owner, ownerSequence: reviewedDeposit.gate.sequence.toString() }
              : undefined
          : undefined,
        withdrawal: direction === "withdraw" && reviewedWithdrawalAccount
          ? { owner: reviewedWithdrawalAccount.owner }
          : undefined,
      })
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Another transfer is already active")
      return
    }
    if (direction === "deposit") void submitDeposit(progress.id)
    else void submitWithdrawal(progress.id)
  }
  const depositActionLabel = effectiveDepositProgress === "checking"
    ? "Checking deposit…"
    : effectiveDepositProgress === "oisy-action" || deposit.isPending
      ? "Confirming deposit…"
      : effectiveDepositProgress === "authorization" || awaitingDepositAuthorization
        ? "Generating authorization…"
        : unresolvedDeposit
          ? "Retry same deposit"
          : "Bridge to Base"
  return <div className="route-enter mx-auto w-full max-w-[620px] pb-6 pt-4 lg:pb-10 lg:pt-10">
    <section className="overflow-hidden rounded-[24px] border border-[var(--line)] bg-[var(--panel)] p-4 shadow-[0_24px_70px_rgba(20,34,53,.09)] sm:p-5" aria-label="KINIC bridge" data-testid="bridge-panel">
      <div className="mb-5 flex items-center justify-between gap-4">
        <div className={`kinic-rail ${direction === "withdraw" ? "is-withdraw" : ""}`} aria-hidden="true"><i /><i /><i /><i /></div>
        <Button size="sm" variant="ghost" disabled={refreshing} onClick={refreshBridgeData}><RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />{refreshing ? "Refreshing…" : "Refresh"}</Button>
      </div>
      <div className="relative grid gap-2 sm:grid-cols-2">
        <EndpointCard label="From" network={source.network} wallet={source.wallet} disabled={depositControlsLocked} onClick={() => wallets.openFor(direction === "deposit" ? "ic" : "base")} />
        <EndpointCard label="To" network={destination.network} wallet={destination.wallet} disabled={depositControlsLocked} onClick={() => wallets.openFor(direction === "deposit" ? "base" : "ic")} />
        <button type="button" disabled={depositControlsLocked} onClick={changeDirection} className="absolute left-1/2 top-1/2 z-10 grid size-8 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border-2 border-[var(--panel)] bg-black text-white transition duration-300 hover:rotate-180 hover:bg-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:bg-[var(--muted)] disabled:hover:rotate-0" aria-label="Reverse bridge direction"><ArrowDownUp className="size-3.5 sm:rotate-90" /></button>
      </div>
      <div className="mt-3 rounded-2xl bg-white p-4">
        <div className="flex items-center justify-between gap-4"><Label htmlFor="bridge-amount">You send</Label><span className="text-sm text-[var(--muted)]">Balance {balance !== undefined ? formatTokenAmount(balance) : "—"} {sendToken.symbol}</span></div>
        <div className="mt-1 flex items-center gap-2 sm:gap-3"><Input id="bridge-amount" disabled={depositControlsLocked} aria-invalid={Boolean(amountError)} aria-describedby="bridge-amount-feedback" className="font-numeric h-14 min-w-0 border-0 px-0 text-3xl font-semibold focus:ring-0" inputMode="decimal" placeholder="0.00000000" value={amount} onChange={(event) => { if (direction === "deposit") setDepositAmount(event.target.value); else setWithdrawAmount(event.target.value) }} /><Button type="button" size="sm" variant="ghost" className="h-9 shrink-0 rounded-xl px-3" disabled={maximumAmountDisabled} onClick={useMaximumAmount}>MAX</Button><span className="shrink-0 rounded-xl bg-[var(--panel)] px-3 py-2 text-sm font-bold">{sendToken.symbol}</span></div>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3 rounded-2xl bg-white p-4 text-sm"><Quote label={feeLabel} value={fee !== undefined ? `${formatTokenAmount(fee)} ${sendToken.symbol}` : "—"} /><Quote label="Estimated receive" value={receive !== undefined ? `${formatTokenAmount(receive)} ${receiveToken.symbol}` : "—"} /></div>
      {direction === "deposit" && (effectiveDepositProgress === "oisy-action" || deposit.isPending) && (
        <DepositProgressCard title="Confirming deposit…" detail="Confirm the action in Oisy. After confirmation, its window stays open while the bridge verifies Deposit acceptance." />
      )}
      {direction === "deposit" && activeDepositRecord.data && (
        ("AuthorizationAvailable" in activeDepositRecord.data.state)
          ? <DepositProgressCard title="Mint Authorization ready" detail="Continue from the transfer progress window to confirm the Base mint transaction." />
          : <div className="mt-4 rounded-2xl border border-[var(--line)] bg-white p-4 text-sm">
              <p className="font-bold text-black">Generating authorization…</p>
              <p className="mt-1 text-[var(--muted)]">{depositPhaseLabel(activeDepositRecord.data)}</p>
              <p className="mt-1 text-[var(--muted)]">Wait here for the authorization, or recover it later from History.</p>
            </div>
      )}
      {direction === "deposit" && effectiveDepositProgress === "authorization" && !activeDepositRecord.data && (
        <DepositProgressCard title="Generating authorization…" detail="The Deposit was accepted. Waiting for the Mint Authorization to become available." />
      )}
      {unresolvedDeposit && !deposit.isPending && <div className="mt-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] p-4 text-sm text-[#8a4b08]"><p className="font-bold text-black">Deposit status unavailable</p><p className="mt-1 leading-5">Check whether the deposit was accepted before starting another one.</p><div className="mt-3 flex flex-wrap gap-2"><Button size="sm" variant="ghost" disabled={checkingDeposit} onClick={() => void checkUnresolvedDeposit()}>{checkingDeposit ? "Checking…" : "Check status"}</Button><Link to="/history" className="inline-flex h-9 items-center rounded-xl px-3 text-sm font-bold underline underline-offset-4">Open History</Link></div></div>}
      {!depositFlowActive && <Button className="mt-3 h-14 w-full" size="lg" aria-busy={depositActionPending} disabled={blockers.length > 0 || depositActionPending || write.isPending || submittingWithdrawal} onClick={beginBridgeReview}>
          {direction === "deposit" ? depositActionLabel : "Bridge to IC"}
          {depositActionPending
            ? <LoaderCircle className="size-4 animate-spin" aria-hidden="true" />
            : <ArrowRight className="size-4" aria-hidden="true" />}
        </Button>}
      <p id="bridge-amount-feedback" className="mt-3 min-h-4 text-center text-xs text-[var(--muted)]" aria-live="polite">
        {depositFlowActive
          ? <>Complete the current deposit above or continue from <Link to="/history" className="font-bold underline underline-offset-4">History</Link>.</>
          : blockers.length > 0 ? `Next: ${blockers[0]}` : null}
      </p>
    </section>
    <BridgeConfirmationDialog direction={direction} open={confirming} setOpen={setBridgeReviewOpen} preflight={preflight} source={source.wallet} destination={destination.wallet} amount={amount} receive={receive} fee={fee} sendSymbol={sendToken.symbol} receiveSymbol={receiveToken.symbol} pending={deposit.isPending || write.isPending || submittingWithdrawal} onRetry={beginBridgeReview} onConfirm={confirmBridgeReview} />
  </div>
}

function recoveredDepositProgressState(record: DepositView): {
  phase: BridgeProgressPhase
  attentionMessage?: string
  completionMessage?: string
} {
  if ("Minted" in record.state) {
    return { phase: "complete", completionMessage: "This deposit was already minted on Base." }
  }
  if ("AuthorizationAvailable" in record.state) return { phase: "awaiting-base-mint" }
  if ("EscrowedUnquoted" in record.state || "AuthorizationPending" in record.state) {
    return { phase: "authorization-generating" }
  }
  return {
    phase: "attention",
    attentionMessage: "This deposit cannot continue to Base minting. Open History to review its refund or reconciliation state.",
  }
}

function EndpointCard({ label, network, wallet, disabled, onClick }: { label: string; network: BridgeNetwork; wallet: string; disabled?: boolean; onClick: () => void }) {
  const details = NETWORKS[network]
  const displayWallet = shortenWalletAddress(wallet)
  return <button type="button" disabled={disabled} onClick={() => onClick()} className="min-w-0 rounded-2xl border border-[var(--line)] bg-white p-3.5 text-left transition duration-300 hover:-translate-y-[2px] hover:border-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:hover:translate-y-0 disabled:hover:border-[var(--line)]"><span className="text-xs font-medium text-[var(--muted)]">{label}</span><span className="mt-0.5 flex items-center justify-between gap-3"><span className="flex min-w-0 items-center gap-2"><img src={details.logo} alt="" aria-hidden="true" data-network-logo={network} className="h-[22px] w-auto shrink-0" /><strong className="truncate text-base text-black">{details.label}</strong></span><LockKeyhole className="size-4 shrink-0 text-[var(--pink)]" /></span><span className="mt-1 block truncate text-xs text-[var(--muted)]" title={displayWallet === wallet ? undefined : wallet}>{displayWallet}</span></button>
}
function Quote({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="font-numeric mt-1 font-bold text-black">{value}</p></div> }
function DepositProgressCard({ title, detail }: { title: string; detail: string }) { return <div className="mt-4 rounded-2xl border border-[var(--line)] bg-white p-4 text-sm" role="status"><p className="font-bold text-black">{title}</p><p className="mt-1 leading-5 text-[var(--muted)]">{detail}</p></div> }
function ConfirmRow({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-1 break-all text-sm font-bold text-black">{value}</p></div> }
function onceAsync(action: () => Promise<void>): () => Promise<void> {
  let called = false
  return async () => {
    if (called) return
    called = true
    await action()
  }
}

function preflightAnnouncement(preflight?: PreflightState): string {
  if (!preflight) return ""
  if (preflight.phase === "ready") return "Transfer review ready."
  const failed = preflight.checks.find((check) => check.status === "failed")
  if (failed) return "Transfer check failed. Review the displayed error."
  const checking = preflight.checks.find((check) => check.status === "checking")
  return checking ? `Checking ${checking.label}.` : "Preparing preflight checks."
}

export function BridgeConfirmationDialog({ direction, open, setOpen, preflight, source, destination, amount, receive, fee, sendSymbol, receiveSymbol, pending, onRetry, onConfirm }: {
  direction: BridgeDirection
  open: boolean
  setOpen: (open: boolean) => void
  preflight?: PreflightState
  source: string
  destination: string
  amount: string
  receive?: bigint
  fee?: bigint
  sendSymbol: string
  receiveSymbol: string
  pending: boolean
  onRetry: () => void
  onConfirm: () => void
}) {
  const close = (value: boolean) => {
    setOpen(value)
  }
  const ready = preflight?.phase === "ready"
  const failed = preflight?.phase === "failed"
  const failedCheck = preflight?.checks.find((check) => check.status === "failed")
  const description = ready
    ? "Review the transfer details before continuing."
    : failed ? "No transaction was sent." : "Checking current bridge conditions. No transaction has been sent."
  return <Dialog open={open} onOpenChange={close}>
    <DialogContent className="max-h-[min(760px,calc(100vh-2rem))] max-w-[560px] overflow-y-auto">
      <DialogHeader>
        <DialogTitle>{direction === "deposit" ? "Review bridge to Base" : "Review bridge to IC"}</DialogTitle>
        <DialogDescription>{description}</DialogDescription>
      </DialogHeader>
      <p className="sr-only" aria-live="polite">{preflightAnnouncement(preflight)}</p>
      {!ready && !failed && <div className="mt-5 flex items-center gap-3 rounded-2xl border border-[#bfd7ff] bg-[#eef5ff] p-4" role="status"><LoaderCircle className="size-5 shrink-0 animate-spin text-[var(--pink)]" /><p className="text-sm font-bold text-black">Checking your wallets, balance, fees, and bridge availability…</p></div>}
      {failed && failedCheck && <div className="mt-5 rounded-2xl border border-[#ffbdad] bg-[#fff0ec] p-4" role="alert"><div className="flex items-center gap-2 font-bold text-[#b42318]"><TriangleAlert className="size-4" />{failedCheck.label}</div><p className="mt-2 text-sm leading-6 text-[#7a271a]">{failedCheck.error ?? "This check could not be completed."}</p></div>}
      {ready && <><div className="mt-5 grid gap-4 rounded-2xl bg-[var(--panel)] p-4 sm:grid-cols-2">
        <ConfirmRow label="You send" value={`${amount || "—"} ${sendSymbol}`} />
        <ConfirmRow label="You receive" value={`${receive !== undefined ? formatTokenAmount(receive) : "—"} ${receiveSymbol}`} />
        <ConfirmRow label="Bridge fee" value={`${fee !== undefined ? formatTokenAmount(fee) : "—"} ${sendSymbol}`} />
        <ConfirmRow label="From" value={source} />
        <div className="sm:col-span-2"><ConfirmRow label="Recipient" value={destination} /></div>
      </div></>}
      <DialogFooter>
        <DialogClose asChild><Button variant="ghost">{failed ? "Close" : "Cancel"}</Button></DialogClose>
        {failed && <Button onClick={onRetry}>Try again</Button>}
        {ready && <Button disabled={pending} onClick={onConfirm}>{direction === "deposit" ? "Continue to IC wallet" : "Continue to Base wallet"}</Button>}
      </DialogFooter>
    </DialogContent>
  </Dialog>
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` { return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}` }
function bytesToHex(bytes: Uint8Array): `0x${string}` { return `0x${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}` }
function depositPhaseLabel(record: { state: Record<string, unknown> }): string {
  if ("AuthorizationPending" in record.state) return "Signing Mint Authorization…"
  if ("RefundAvailable" in record.state) return "Refund available from History"
  if ("Minted" in record.state) return "Base mint finalized"
  if ("RefundProcessing" in record.state) return "Processing IC refund…"
  if ("Refunded" in record.state) return "Refunded to IC"
  return "Processing Ledger escrow…"
}

export function isDepositAuthorizationPending(state: Record<string, unknown>): boolean {
  return "EscrowedUnquoted" in state || "AuthorizationPending" in state
}
