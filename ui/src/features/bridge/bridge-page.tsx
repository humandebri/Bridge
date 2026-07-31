import { Link } from "@tanstack/react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ArrowDownUp, ArrowRight, Check, Circle, LoaderCircle, LockKeyhole, RefreshCcw, TriangleAlert } from "lucide-react"
import { Principal } from "@dfinity/principal"
import { useEffect, useMemo, useRef, useState } from "react"
import { toast } from "sonner"
import { hexToBytes } from "viem"
import { useAccount, useChainId, useConnectorClient, useWriteContract } from "wagmi"
import baseLogo from "@/assets/base-square.svg"
import icpLogo from "@/assets/icp-logo-mark.svg"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { deploymentProfile } from "@/config/profile"
import { finalizedObservationQuote, useRuntimeHeartbeat, useRuntimeValidation, useRuntimeWriteReadiness } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { useWalletDialog } from "@/features/wallet/wallet-controls"
import { MintAuthorizationAction, type MintConfirmation } from "@/features/bridge/mint-authorization-action"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { estimatedAmountOut, formatTokenAmount, parseTokenAmount, requiredDepositBalance } from "@/lib/amounts"
import { shortenWalletAddress } from "@/lib/wallet-address"
import { classifyDepositRecoverySequence } from "@/lib/deposit-recovery"
import { createLedgerActor, ledgerAccount } from "@/lib/ic/ledger"
import { createBridgeActor } from "@/lib/ic/bridge"
import type { DepositCall, IcAccount } from "@/lib/ic/wallet"
import { basePublicClient } from "@/lib/evm/client"
import { refetchRuntimeWriteReady, runtimeWriteBlocker, RUNTIME_VALIDATION_TTL_MS, type FinalizedRuntimeObservation } from "@/lib/runtime-validation"
import { currentInjectedWallet, requireWalletSnapshot, sameIcAccount } from "@/lib/wallet-snapshot"
import { createWithdrawalAfterRevalidation } from "@/lib/withdrawal-submit"
import { savePendingConfirmation } from "@/lib/pending-confirmations"
import { readDepositIntent, removeDepositIntent, saveDepositIntent } from "@/lib/deposit-intents"
import { withBrowserLock } from "@/lib/browser-lock"
import { isDepositTerminal } from "@/lib/settlement-phase"

export type BridgeDirection = "deposit" | "withdraw"
type BridgeNetwork = "ic" | "base"
const AUTO_REFRESH_INTERVAL_MS = 45_000
const automaticQueryOptions = {
  refetchInterval: AUTO_REFRESH_INTERVAL_MS,
  refetchIntervalInBackground: false,
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
  ledger: { balance: bigint; fee: bigint; allowance: bigint; mintAuthorizationTtlSeconds?: bigint }
  sequence: bigint
  observation: FinalizedRuntimeObservation
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
  const [unresolvedDeposit, setUnresolvedDeposit] = useState<UnresolvedDepositAttempt>()
  const [resolvedIntentOwner, setResolvedIntentOwner] = useState<string>()
  const [checkingDeposit, setCheckingDeposit] = useState(false)
  const [activeDeposit, setActiveDeposit] = useState<{ owner: string; sequence: bigint }>()
  const [mintCompletion, setMintCompletion] = useState<MintConfirmation>()
  const [submittingWithdrawal, setSubmittingWithdrawal] = useState(false)
  const [preflight, setPreflight] = useState<PreflightState>()
  const preflightRunId = useRef(0)
  const queryClient = useQueryClient()
  const { address, isConnected } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const wallets = useWalletDialog()
  const write = useWriteContract()
  const connectorClient = useConnectorClient()
  const currentBaseWallet = () => currentInjectedWallet(connectorClient.data?.transport)
  const runtime = useRuntimeValidation(chainId, {
    enabled: true,
    gcTime: Infinity,
    retryNotReadyAfterMs: 1_000,
    staleTime: Infinity,
  })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, {
    enabled: runtime.data?.ready === true,
    refetchInterval: AUTO_REFRESH_INTERVAL_MS,
  })
  const activeRuntimeValidation = runtime.data?.ready === true ? heartbeat.data : runtime.data
  const runtimeReadiness = useRuntimeWriteReadiness(activeRuntimeValidation)
  const sendToken = direction === "deposit" ? deploymentProfile.icToken : deploymentProfile.baseToken
  const receiveToken = direction === "deposit" ? deploymentProfile.baseToken : deploymentProfile.icToken
  const baseData = runtimeReadiness.ready ? finalizedObservationQuote(heartbeat.data) : undefined
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
  useEffect(() => {
    if (!activeDepositRecord.data || !isDepositTerminal(activeDepositRecord.data.state)) return
    setDepositAmount("")
    setReviewedDeposit(undefined)
    setActiveDeposit(undefined)
    setDepositProgress("idle")
  }, [activeDepositRecord.data])
  const ledger = useQuery({
    queryKey: ["deposit-ledger", ic.account?.owner, bytesHex(ic.account?.subaccount ?? new Uint8Array())],
    enabled: direction === "deposit" && Boolean(ic.account),
    ...automaticQueryOptions,
    queryFn: async () => {
      const ledgerActor = await createLedgerActor(deploymentProfile.icHost, deploymentProfile.ledgerCanisterId as string)
      const bridgeActor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      const account = ledgerAccount(ic.account!.owner, ic.account!.subaccount)
      const spender = ledgerAccount(deploymentProfile.bridgeCanisterId as string)
      const [balance, allowance, publicConfig] = await Promise.all([
        ledgerActor.icrc1_balance_of(account),
        ledgerActor.icrc2_allowance({ account, spender }),
        bridgeActor.get_public_config(),
      ])
      return {
        balance,
        fee: publicConfig.ledger_fee,
        allowance: allowance.allowance,
        mintAuthorizationTtlSeconds:
          typeof publicConfig.mint_authorization_ttl_seconds === "bigint"
            ? publicConfig.mint_authorization_ttl_seconds
            : undefined,
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
  const estimate = withdrawParsed.ok && baseData ? estimatedAmountOut(withdrawParsed.value, baseData.serviceFee) : 0n
  const ownerSequenceData = !ownerSequence.isError && !ownerSequence.isStale ? ownerSequence.data : undefined
  const refreshing = runtime.isFetching || runtime.isAutoRetryPending || heartbeat.isFetching || ledger.isFetching || bsnsBalance.isFetching || (!unresolvedDeposit && ownerSequence.isFetching)
  const refreshBridgeData = () => {
    const calls: Promise<unknown>[] = [runtime.refetch(), heartbeat.refetch()]
    if (direction === "deposit" && ic.account) {
      calls.push(ledger.refetch())
      if (!unresolvedDeposit) calls.push(ownerSequence.refetch())
    }
    if (direction === "withdraw" && address) calls.push(bsnsBalance.refetch())
    void Promise.all(calls)
  }
  const refetchBaseSnapshot = async () => {
    const observation = await refetchRuntimeWriteReady(() => heartbeat.refetch())
    if (!observation.snapshot) throw new Error("Finalized Base snapshot is unavailable")
    return observation.snapshot
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
    onSuccess: async (receipt, { attempt }) => {
      queryClient.setQueryData(["deposit-owner-sequence", attempt.account.owner], receipt.owner_sequence + 1n)
      setActiveDeposit({ owner: attempt.account.owner, sequence: receipt.owner_sequence })
      setDepositProgress("authorization")
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
    onError: (error) => {
      setDepositProgress("idle")
      toast.error(error instanceof Error ? `${error.message}. Retry the same deposit or check whether it was accepted.` : "Deposit response is unresolved")
    },
  })

  const submitDeposit = async () => {
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
      if (!unresolvedDeposit && !reviewedDeposit) throw new Error("Check the deposit again before opening OISY")
      setDepositProgress("oisy-action")
      const walletSession = ic.adapter.prepare()
      if (unresolvedDeposit) {
        closeWalletSession = onceAsync(await walletSession)
        await withBrowserLock(`kinic-deposit-owner:${unresolvedDeposit.account.owner}`, () => deposit.mutateAsync({ attempt: unresolvedDeposit, closeWalletSession: closeWalletSession! }))
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
        const beforeApproval = reviewed.gate
        const requiredAllowance = reviewed.amount + beforeApproval.ledger.fee
        if (beforeApproval.ledger.allowance < requiredAllowance) {
          await withBrowserLock(`kinic-wallet-prompt:ic:${confirmedAccount.owner}`, () => ic.adapter!.approve({ amount: requiredAllowance, currentAllowance: beforeApproval.ledger.allowance, ledgerFee: beforeApproval.ledger.fee }))
        }
        const [finalEvm, finalIc] = await Promise.all([currentBaseWallet(), ic.adapter!.getAccount()])
        requireWalletSnapshot(expectedWallets, { ...finalEvm, icAccount: finalIc }, "during approval")
        const final = await refetchDepositWriteGate(reviewed.amount, beforeApproval.sequence, beforeApproval.observation)
        const attempt: UnresolvedDepositAttempt = {
          call: { ownerSequence: final.sequence, baseRecipient: hexToBytes(confirmedRecipient), grossAmount: reviewed.amount, maxServiceFee: final.base.serviceFee },
          account: { owner: confirmedAccount.owner, subaccount: confirmedAccount.subaccount?.slice() },
          recipient: confirmedRecipient,
        }
        await saveDepositIntent({ ...attempt, state: "prepared" })
        setUnresolvedDeposit(attempt)
        await deposit.mutateAsync({ attempt, closeWalletSession: closeWalletSession! })
      })
    } catch (error) {
      setDepositProgress("idle")
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
      : refetchRuntimeWriteReady(() => heartbeat.refetch())
    const [observation, ledgerResult, sequenceResult] = await Promise.all([observationPromise, ledger.refetch(), ownerSequence.refetch()])
    const quote = observation.snapshot
    if (!quote || ledgerResult.isError || ledgerResult.isStale || !ledgerResult.data || sequenceResult.isError || sequenceResult.isStale || sequenceResult.data === undefined) {
      throw new Error("Deposit limits, balance, fee, allowance, or sequence could not be verified")
    }
    if (quote.depositsPaused) throw new Error("Deposits are paused on Base")
    if (amount > quote.perDepositLimit) throw new Error("Amount exceeds the current per-deposit limit")
    if (amount <= quote.serviceFee) throw new Error("Amount must exceed the current service fee")
    const now = BigInt(Math.floor(Date.now() / 1000))
    if (now < quote.startedAt + quote.duration && quote.minted + amount - quote.serviceFee > quote.limit) throw new Error("Amount exceeds the remaining mint window limit")
    if (sequenceResult.data !== expectedSequence) throw new Error("Another deposit used this owner sequence; refresh and review again")
    if (ledgerResult.data.balance < requiredDepositBalance(amount, ledgerResult.data.fee, ledgerResult.data.allowance)) throw new Error(`${deploymentProfile.icToken.symbol} balance does not cover the deposit and required ledger fees`)
    return { base: quote, ledger: ledgerResult.data, sequence: sequenceResult.data, observation }
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
      const observation = await runPreflightCheck(runId, "runtime", () => refetchRuntimeWriteReady(() => heartbeat.refetch()))
      await runPreflightCheck(runId, "financials", () => {
        if (unresolvedDeposit) return
        if (!depositParsed.ok) throw new Error(depositParsed.reason)
        if (!ledgerData || !baseData || ownerSequenceData === undefined) throw new Error("Balance or fee information is unavailable. Choose Refresh.")
        if (ledgerData.balance < requiredDepositBalance(depositParsed.value, ledgerData.fee, ledgerData.allowance)) {
          throw new Error(`${deploymentProfile.icToken.symbol} balance does not cover the deposit and required ledger fees`)
        }
      })
      const gate = await runPreflightCheck(runId, "availability", async () => {
        if (unresolvedDeposit) return undefined
        if (!depositParsed.ok || ownerSequenceData === undefined) throw new Error("Deposit amount or sequence is unavailable")
        return refetchDepositWriteGate(depositParsed.value, ownerSequenceData, observation)
      })
      assertActivePreflight(runId)
      if (!unresolvedDeposit && depositParsed.ok && gate) {
        setReviewedDeposit({
          amount: depositParsed.value,
          account: { owner: walletSnapshot.account.owner, subaccount: walletSnapshot.account.subaccount?.slice() },
          recipient: walletSnapshot.recipient,
          gate,
        })
      }
      completePreflight(runId)
    } catch {
      // The failed step already owns the user-visible error.
    } finally {
      if (preflightRunId.current === runId) setDepositProgress("idle")
    }
  }

  const runWithdrawalPreflight = async (runId: number) => {
    try {
      await runPreflightCheck(runId, "wallets", async () => {
        if (!address || !isConnected) throw new Error("Connect the EVM wallet that owns bSNS")
        if (!ic.account || !ic.adapter) throw new Error("Connect the destination IC wallet")
        const expectedWallets = {
          address,
          chainId: deploymentProfile.chainId,
          icAccount: { owner: ic.account.owner, subaccount: ic.account.subaccount },
        }
        const [activeEvm, activeIc] = await Promise.all([currentBaseWallet(), ic.adapter.getAccount()])
        requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc }, "before opening the wallet prompt")
      })
      const observation = await runPreflightCheck(runId, "runtime", () => refetchRuntimeWriteReady(() => heartbeat.refetch()))
      await runPreflightCheck(runId, "financials", () => {
        if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
        if (baseData === undefined || bsnsBalanceData === undefined) throw new Error("Fee or balance data is unavailable or stale")
        if (withdrawParsed.value <= baseData.serviceFee) throw new Error("Amount must be greater than the current service fee")
        if (bsnsBalanceData < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
      })
      await runPreflightCheck(runId, "availability", async () => {
        if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
        const quote = observation.snapshot
        const balanceResult = await bsnsBalance.refetch()
        if (!quote || balanceResult.isError || balanceResult.isStale || balanceResult.data === undefined) {
          throw new Error("Withdrawal limits, fee, or balance could not be verified")
        }
        if (quote.withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
        if (withdrawParsed.value <= quote.serviceFee) throw new Error("Amount must be greater than the current service fee")
        if (balanceResult.data < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
      })
      completePreflight(runId)
    } catch {
      // The failed step already owns the user-visible error.
    }
  }

  const beginBridgeReview = () => {
    if (direction === "deposit" && effectiveDepositProgress !== "idle") return
    const runId = preflightRunId.current + 1
    preflightRunId.current = runId
    setPreflight(initialPreflight(runId, direction))
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

  const submitWithdrawal = async () => {
    try {
      setSubmittingWithdrawal(true)
      if (!address) throw new Error("Connect the EVM wallet that owns bSNS")
      if (!ic.account || !ic.adapter) throw new Error("Connect the destination IC wallet")
      if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
      if (baseData === undefined || bsnsBalanceData === undefined) throw new Error("Fee or balance data is unavailable or stale")
      if (withdrawParsed.value <= baseData.serviceFee) throw new Error("Amount must be greater than the current service fee")
      if (bsnsBalanceData < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
      const confirmedIcAccount = { owner: ic.account.owner, subaccount: ic.account.subaccount }
      const snapshotAddress = address
      const activeEvm = await currentBaseWallet()
      const activeIc = await ic.adapter.getAccount()
      const expectedWallets = { address: snapshotAddress, chainId: deploymentProfile.chainId, icAccount: confirmedIcAccount }
      requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc })
      const owner = Principal.fromText(confirmedIcAccount.owner).toUint8Array()
      const subaccount = confirmedIcAccount.subaccount ?? new Uint8Array(32)
      const [approvalQuote, approvalBalance] = await Promise.all([refetchBaseSnapshot(), bsnsBalance.refetch()])
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
        const approvalHash = await withBrowserLock(`kinic-wallet-prompt:base:${snapshotAddress.toLowerCase()}`, () => write.writeContractAsync({
          account: snapshotAddress,
          address: deploymentProfile.bsnsAddress as `0x${string}`,
          abi: bsnsAbi,
          functionName: "approve",
          args: [deploymentProfile.bridgeAddress as `0x${string}`, withdrawParsed.value],
        }))
        const approvalReceipt = await client.waitForTransactionReceipt({ hash: approvalHash })
        if (approvalReceipt.status !== "success") throw new Error("Token approval failed")
      }
      const broadcast = await createWithdrawalAfterRevalidation({
        expectedWallets,
        refetchRuntime: () => heartbeat.refetch(),
        currentEvmWallet: currentBaseWallet,
        currentIcAccount: () => ic.adapter!.getAccount(),
        refetchFinancials: async () => {
          const [quote, balanceResult] = await Promise.all([refetchBaseSnapshot(), bsnsBalance.refetch()])
          if (balanceResult.isError || balanceResult.isStale || balanceResult.data === undefined) throw new Error("Fee or balance data changed and could not be verified")
          return { serviceFee: quote.serviceFee, balance: balanceResult.data, withdrawalsPaused: quote.withdrawalsPaused }
        },
        validateFinancials: ({ serviceFee, balance: finalBalance, withdrawalsPaused }) => {
          if (withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
          if (withdrawParsed.value <= serviceFee) throw new Error("Amount must be greater than the current service fee")
          if (finalBalance < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
        },
        createWithdrawal: ({ serviceFee }) => withBrowserLock(`kinic-wallet-prompt:base:${snapshotAddress.toLowerCase()}`, () => write.writeContractAsync({ account: snapshotAddress, address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, functionName: "createWithdrawal", args: [withdrawParsed.value, serviceFee, bytesToHex(owner), bytesToHex(subaccount)] })),
        onBroadcast: (transactionHash) => savePendingConfirmation({
          kind: "withdrawal",
          transactionHash,
          owner: confirmedIcAccount.owner,
        }),
      })
      setWithdrawAmount("")
      if (broadcast.pendingSaved) {
        toast.success(`Withdrawal submitted: ${broadcast.transactionHash.slice(0, 12)}…. Confirmation is pending. Check History after finalization if it has not completed.`)
      } else {
        toast.warning(`Withdrawal ${broadcast.transactionHash} was submitted, but this browser could not save it. Copy the transaction hash; after it succeeds, recover it from History.`)
      }
    } catch (error) { toast.error(error instanceof Error ? error.message : "Withdrawal failed") }
    finally { setSubmittingWithdrawal(false) }
  }

  const retryAccountMatches = unresolvedDeposit && ic.account ? sameIcAccount(ic.account, unresolvedDeposit.account) : false
  const retryRecipientMatches = unresolvedDeposit && address ? address.toLowerCase() === unresolvedDeposit.recipient.toLowerCase() : false
  const runtimeReason = runtimeReadiness.ready
    ? undefined
    : runtime.isFetching || runtime.isAutoRetryPending || heartbeat.isFetching
      ? "Checking availability…"
      : activeRuntimeValidation
        ? "Bridge is temporarily unavailable. Try Refresh."
        : "Refresh before continuing."
  const depositBlockers = unresolvedDeposit
    ? [runtimeReason, !ic.account && "Reconnect the original IC wallet", !address && "Reconnect the original EVM wallet", ic.account && !retryAccountMatches && "Reconnect the original IC wallet", address && !retryRecipientMatches && "Reconnect the original EVM wallet"].filter(Boolean) as string[]
    : [!address && "Connect both wallets", !ic.account && "Connect both wallets", runtimeReason, (!baseData || !ledgerData || ownerSequenceData === undefined) && "Balance or fee information is unavailable", !depositParsed.ok && (depositParsed.reason ?? "Enter an amount")].filter(Boolean) as string[]
  const withdrawalBlockers = [!address && "Connect both wallets", !ic.account && "Connect both wallets", runtimeReason, (!baseData || bsnsBalanceData === undefined) && "Fee and balance data is unavailable", !withdrawParsed.ok && (withdrawParsed.reason ?? "Enter an amount"), withdrawParsed.ok && baseData && withdrawParsed.value <= baseData.serviceFee && "Amount must exceed the service fee"].filter(Boolean) as string[]
  const blockers = direction === "deposit" ? depositBlockers : withdrawalBlockers
  const awaitingDepositAuthorization = direction === "deposit"
    && Boolean(activeDeposit)
    && (!activeDepositRecord.data || isDepositAuthorizationPending(activeDepositRecord.data.state))
  const depositActionPending = direction === "deposit" && (effectiveDepositProgress !== "idle" || deposit.isPending || awaitingDepositAuthorization)
  const amountError = !unresolvedDeposit && (direction === "deposit" ? (!depositParsed.ok ? depositParsed.reason : undefined) : (!withdrawParsed.ok ? withdrawParsed.reason : undefined))
  const amount = direction === "deposit" ? (unresolvedDeposit ? formatTokenAmount(unresolvedDeposit.call.grossAmount) : depositAmount) : withdrawAmount
  const balance = direction === "deposit" ? ledgerData?.balance : bsnsBalanceData
  const fee = unresolvedDeposit?.call.maxServiceFee ?? baseData?.serviceFee
  const receive = direction === "deposit" ? (unresolvedDeposit ? (unresolvedDeposit.call.grossAmount > unresolvedDeposit.call.maxServiceFee ? unresolvedDeposit.call.grossAmount - unresolvedDeposit.call.maxServiceFee : 0n) : depositParsed.ok && fee !== undefined ? (depositParsed.value > fee ? depositParsed.value - fee : 0n) : undefined) : (estimate > 0n ? estimate : undefined)
  const source = direction === "deposit" ? { network: "ic" as const, wallet: unresolvedDeposit?.account.owner ?? ic.account?.owner ?? "Connect IC wallet" } : { network: "base" as const, wallet: address ?? "Connect EVM wallet" }
  const destination = direction === "deposit" ? { network: "base" as const, wallet: unresolvedDeposit?.recipient ?? address ?? "Connect EVM wallet" } : { network: "ic" as const, wallet: ic.account?.owner ?? "Connect IC wallet" }
  const depositFlowActive = direction === "deposit" && Boolean(activeDeposit)
  const depositControlsLocked = direction === "deposit"
    && (Boolean(unresolvedDeposit) || effectiveDepositProgress !== "idle" || depositFlowActive)

  const changeDirection = () => { if (depositControlsLocked) return; setConfirming(false); onDirectionChange(direction === "deposit" ? "withdraw" : "deposit") }
  const completeMint = (confirmation: MintConfirmation) => {
    setDepositAmount("")
    setReviewedDeposit(undefined)
    setActiveDeposit(undefined)
    setDepositProgress("idle")
    setMintCompletion(confirmation)
  }
  const setBridgeReviewOpen = (open: boolean) => {
    if (open) {
      setConfirming(true)
      return
    }
    preflightRunId.current += 1
    setConfirming(false)
    setPreflight(undefined)
    setReviewedDeposit(undefined)
    setDepositProgress((current) => current === "checking" ? "idle" : current)
  }
  const confirmBridgeReview = () => {
    preflightRunId.current += 1
    setConfirming(false)
    setPreflight(undefined)
    if (direction === "deposit") void submitDeposit()
    else void submitWithdrawal()
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
  return <div className="route-enter grid items-start gap-8 pb-6 pt-4 lg:grid-cols-[minmax(0,1fr)_minmax(560px,620px)] lg:gap-16 lg:pb-7 lg:pt-14 xl:gap-20">
    <div className="lg:sticky lg:top-28 lg:pt-12" data-testid="bridge-intro">
      <h1 className="font-display max-w-[460px] text-[42px] leading-[1.02] text-black sm:text-[52px] lg:text-[58px]">Bridge KINIC</h1>
      <p className="mt-5 max-w-[460px] text-[16px] leading-7 text-[var(--muted)] sm:text-[17px]">Move tokens between IC and Base.</p>
      <div className="mt-8 hidden items-center gap-3 text-xs font-bold uppercase tracking-[.12em] text-[var(--support)] lg:flex"><span className="h-px w-12 bg-[var(--pink)]" />1:1 across both networks</div>
    </div>
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
        <div className="mt-1 flex items-center gap-3"><Input id="bridge-amount" disabled={depositControlsLocked} aria-invalid={Boolean(amountError)} aria-describedby="bridge-amount-feedback" className="font-numeric h-14 border-0 px-0 text-3xl font-semibold focus:ring-0" inputMode="decimal" placeholder="0.00000000" value={amount} onChange={(event) => { if (direction === "deposit") setDepositAmount(event.target.value); else setWithdrawAmount(event.target.value) }} /><span className="rounded-xl bg-[var(--panel)] px-3 py-2 text-sm font-bold">{sendToken.symbol}</span></div>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3 rounded-2xl bg-white p-4 text-sm"><Quote label="Current bridge fee" value={fee !== undefined ? `${formatTokenAmount(fee)} ${sendToken.symbol}` : "—"} /><Quote label="Estimated receive" value={receive !== undefined ? `${formatTokenAmount(receive)} ${receiveToken.symbol}` : "—"} /></div>
      {direction === "deposit" && (effectiveDepositProgress === "oisy-action" || deposit.isPending) && (
        <DepositProgressCard title="Confirming deposit…" detail="Confirm the action in Oisy. After confirmation, its window stays open while the bridge verifies Deposit acceptance." />
      )}
      {direction === "deposit" && activeDepositRecord.data && (
        ("AuthorizationAvailable" in activeDepositRecord.data.state)
          ? <MintAuthorizationAction record={activeDepositRecord.data} autoPromptOwner={activeDeposit?.owner} onMintConfirmed={completeMint} />
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
      {runtimeReason && <div className="mt-4 flex items-center justify-between gap-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] px-4 py-3 text-sm text-[#d5691b]"><span>{runtimeReason}</span><Link to="/status" className="font-bold underline underline-offset-4">View status</Link></div>}
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
    <BridgeConfirmationDialog direction={direction} open={confirming} setOpen={setBridgeReviewOpen} preflight={preflight} source={source.wallet} destination={destination.wallet} amount={amount} receive={receive} sendSymbol={sendToken.symbol} receiveSymbol={receiveToken.symbol} pending={deposit.isPending || write.isPending || submittingWithdrawal} onRetry={beginBridgeReview} onConfirm={confirmBridgeReview} />
    <MintCompletionDialog confirmation={mintCompletion} onClose={() => setMintCompletion(undefined)} />
  </div>
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

function MintCompletionDialog({ confirmation, onClose }: { confirmation?: MintConfirmation; onClose: () => void }) {
  return <Dialog open={Boolean(confirmation)} onOpenChange={(open) => { if (!open) onClose() }}>
    <DialogContent>
      <DialogHeader>
        <DialogTitle>Bridge complete</DialogTitle>
        <DialogDescription>Your tokens were minted on Base. The form is ready for a new bridge.</DialogDescription>
      </DialogHeader>
      {confirmation && <div className="mt-5 space-y-4 rounded-2xl bg-[var(--panel)] p-4">
        <ConfirmRow label="Minted" value={`${formatTokenAmount(confirmation.mintedAmount)} ${deploymentProfile.baseToken.symbol}`} />
        <ConfirmRow label="Recipient" value={confirmation.recipient} />
        <ConfirmRow label="Base transaction" value={confirmation.transactionHash} />
      </div>}
      <DialogFooter>
        <Button asChild variant="ghost"><Link to="/history">View History</Link></Button>
        <DialogClose asChild><Button>Close</Button></DialogClose>
      </DialogFooter>
    </DialogContent>
  </Dialog>
}

function PreflightStepper({ checks }: { checks: PreflightCheck[] }) {
  return <ol className="mt-5 rounded-2xl border border-[var(--line)] bg-[var(--panel)] px-4 py-4" aria-label="Preflight checks">
    {checks.map((check, index) => {
      const icon = check.status === "checking"
        ? <LoaderCircle className="size-4 animate-spin" />
        : check.status === "passed"
          ? <Check className="size-4" />
          : check.status === "failed"
            ? <TriangleAlert className="size-4" />
            : <Circle className="size-3" />
      const iconClass = check.status === "checking"
        ? "border-[var(--pink)] bg-[var(--pink-soft)] text-[var(--pink)]"
        : check.status === "passed"
          ? "border-[#9ed8b3] bg-[#eaf8ef] text-[#157347]"
          : check.status === "failed"
            ? "border-[#ffbdad] bg-[#fff0ec] text-[#b42318]"
            : "border-[var(--line)] bg-white text-[var(--muted)]"
      return <li key={check.id} data-status={check.status} className="relative flex min-h-12 gap-3 pb-4 last:min-h-0 last:pb-0">
        {index < checks.length - 1 && <span aria-hidden="true" className="absolute left-[15px] top-8 h-[calc(100%-1rem)] w-px bg-[var(--line)]" />}
        <span aria-hidden="true" className={`relative z-10 grid size-8 shrink-0 place-items-center rounded-full border ${iconClass}`}>{icon}</span>
        <div className="min-w-0 pt-1">
          <p className={`text-sm font-bold ${check.status === "waiting" ? "text-[var(--muted)]" : "text-black"}`}>
            {check.label}
            <span className="sr-only">: {check.status}</span>
          </p>
          {check.error && <p className="mt-1 break-words text-sm leading-5 text-[#b42318]">{check.error}</p>}
        </div>
      </li>
    })}
  </ol>
}

function preflightAnnouncement(preflight?: PreflightState): string {
  if (!preflight) return ""
  if (preflight.phase === "ready") return "All preflight checks passed."
  const failed = preflight.checks.find((check) => check.status === "failed")
  if (failed) return `${failed.label} failed. ${failed.error ?? ""}`.trim()
  const checking = preflight.checks.find((check) => check.status === "checking")
  return checking ? `Checking ${checking.label}.` : "Preparing preflight checks."
}

export function BridgeConfirmationDialog({ direction, open, setOpen, preflight, source, destination, amount, receive, sendSymbol, receiveSymbol, pending, onRetry, onConfirm }: {
  direction: BridgeDirection
  open: boolean
  setOpen: (open: boolean) => void
  preflight?: PreflightState
  source: string
  destination: string
  amount: string
  receive?: bigint
  sendSymbol: string
  receiveSymbol: string
  pending: boolean
  onRetry: () => void
  onConfirm: () => void
}) {
  const [burnAcknowledged, setBurnAcknowledged] = useState(false)
  const close = (value: boolean) => {
    if (!value) setBurnAcknowledged(false)
    setOpen(value)
  }
  const ready = preflight?.phase === "ready"
  const failed = preflight?.phase === "failed"
  const description = ready
    ? "All checks passed. Review the transfer before opening your wallet."
    : failed
      ? "One check needs attention. No transaction has been sent."
      : "Checking current bridge conditions. No transaction has been sent."
  return <Dialog open={open} onOpenChange={close}>
    <DialogContent className="max-h-[min(760px,calc(100vh-2rem))] max-w-[560px] overflow-y-auto">
      <DialogHeader>
        <DialogTitle>{direction === "deposit" ? "Review bridge to Base" : "Review bridge to IC"}</DialogTitle>
        <DialogDescription>{description}</DialogDescription>
      </DialogHeader>
      <p className="sr-only" aria-live="polite">{preflightAnnouncement(preflight)}</p>
      {preflight && <PreflightStepper checks={preflight.checks} />}
      {ready && <div className="mt-5 space-y-4 rounded-2xl bg-[var(--panel)] p-4">
        <ConfirmRow label="Source" value={source} />
        <ConfirmRow label="Destination" value={destination} />
        <ConfirmRow label="Send / receive" value={`${amount || "—"} ${sendSymbol} / ${receive !== undefined ? formatTokenAmount(receive) : "—"} ${receiveSymbol}`} />
      </div>}
      {ready && direction === "withdraw" && <label className="mt-4 flex items-start gap-3 text-sm leading-5">
        <Checkbox aria-label="Acknowledge irreversible burn" checked={burnAcknowledged} onCheckedChange={(checked) => setBurnAcknowledged(checked === true)} />
        <span>I understand that confirming burns the Base tokens and no Base refund is available.</span>
      </label>}
      <DialogFooter>
        <DialogClose asChild><Button variant="ghost">{failed ? "Close" : "Cancel"}</Button></DialogClose>
        {failed && <Button onClick={() => { setBurnAcknowledged(false); onRetry() }}>Try again</Button>}
        {ready && <Button disabled={pending || (direction === "withdraw" && !burnAcknowledged)} onClick={() => { setBurnAcknowledged(false); onConfirm() }}>Confirm and open wallet</Button>}
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
