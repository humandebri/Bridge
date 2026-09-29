import { Link } from "@tanstack/react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import {
  ArrowDownUp,
  ArrowRight,
  LoaderCircle,
  LockKeyhole,
  RefreshCcw,
  TriangleAlert,
} from "lucide-react"
import { Principal } from "@icp-sdk/core/principal"
import { useEffect, useMemo, useReducer, useRef, useState } from "react"
import { toast } from "sonner"
import { hexToBytes } from "viem"
import { useAccount, useChainId, useConnectorClient, useWriteContract } from "wagmi"
import baseLogo from "@/assets/base-square.svg"
import icpLogo from "@/assets/icp-logo-mark.svg"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { deploymentProfile } from "@/config/profile"
import {
  useCurrentBaseQuote,
  useRuntimeHeartbeat,
  useRuntimeValidation,
} from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { useWalletDialog } from "@/features/wallet/wallet-controls"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import type { DepositView } from "@/generated/bridge.did"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { multiTokenBridgeAbi } from "@/generated/abi/multitokenbridge.generated"
import {
  estimatedAmountOut,
  formatTokenAmount,
  maximumDepositAmount,
  parseTokenAmount,
  requiredDepositBalance,
} from "@/lib/amounts"
import { shortenWalletAddress } from "@/lib/wallet-address"
import { classifyDepositRecoverySequence } from "@/lib/deposit-recovery"
import { transferErrorMessage } from "@/lib/transfer-error"
import { createLedgerActor, ledgerAccount } from "@/lib/ic/ledger"
import { createBridgeActor } from "@/lib/ic/bridge"
import { basePublicClient } from "@/lib/evm/client"
import {
  refetchRuntimeAttestedWriteReady,
  runtimeWriteBlocker,
  RUNTIME_VALIDATION_TTL_MS,
  type FinalizedRuntimeObservation,
} from "@/lib/runtime-validation"
import { currentInjectedWallet, requireWalletSnapshot, sameIcAccount } from "@/lib/wallet-snapshot"
import {
  createWithdrawalAfterRevalidation,
  withdrawalAbi,
  type ApprovalReceipt,
} from "@/lib/withdrawal-submit"
import { savePendingConfirmation } from "@/lib/pending-confirmations"
import { readDepositIntent, removeDepositIntent, saveDepositIntent } from "@/lib/deposit-intents"
import { withBrowserLock } from "@/lib/browser-lock"
import { isDepositTerminal } from "@/lib/settlement-phase"
import type { BridgeProgressPhase } from "@/lib/bridge-progress"
import {
  bridgePageReducer,
  initialBridgePageState,
  type BridgeDirection,
  type DepositProgress,
  type DepositWriteGate,
  type PreflightCheck,
  type PreflightCheckId,
  type PreflightCheckStatus,
  type PreflightState,
  type ReviewedDeposit,
  type UnresolvedDepositAttempt,
} from "./bridge-page-state"

export type { BridgeDirection } from "./bridge-page-state"
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

export function validatedDepositWriteGate(input: {
  bridgeAddress?: string
  tokenAddress?: string
  tokenSymbol?: string
  recipient: string
  amount: bigint
  expectedSequence: bigint
  observation: FinalizedRuntimeObservation
  ledger: DepositWriteGate["ledger"]
  sequence: bigint
}): DepositWriteGate {
  const { amount, expectedSequence, observation, ledger, sequence, recipient } = input
  if (
    !/^0x[0-9a-fA-F]{40}$/.test(recipient) ||
    [
      "0x0000000000000000000000000000000000000000",
      input.bridgeAddress ?? deploymentProfile.bridgeAddress,
      input.tokenAddress ?? deploymentProfile.bsnsAddress,
    ].some((address) => address?.toLowerCase() === recipient.toLowerCase())
  )
    throw new Error("Recipient cannot be zero, the Bridge contract, or the token contract")
  const quote = observation.snapshot
  if (!quote) throw new Error("Finalized Base snapshot is unavailable")
  if (quote.depositsPaused) throw new Error("Deposits are paused on Base")
  if (amount <= quote.serviceFee) throw new Error("Amount must exceed the current service fee")
  if (amount - quote.serviceFee > quote.perDepositLimit)
    throw new Error("Amount exceeds the current per-deposit limit")
  if (amount - quote.serviceFee <= ledger.fee)
    throw new Error("Amount must exceed the service fee plus the refund ledger fee")
  const windowEndsAt = quote.startedAt + quote.duration
  if (quote.blockTimestamp === windowEndsAt)
    throw new Error(
      "The finalized mint window snapshot is at its rollover boundary; refresh and review again",
    )
  if (quote.blockTimestamp < windowEndsAt && quote.minted + amount - quote.serviceFee > quote.limit)
    throw new Error("Amount exceeds the remaining mint window limit")
  if (sequence !== expectedSequence)
    throw new Error("Another deposit used this owner sequence; refresh and review again")
  if (ledger.balance < requiredDepositBalance(amount, ledger.fee, ledger.allowance))
    throw new Error(
      `${input.tokenSymbol ?? deploymentProfile.icToken.symbol} balance does not cover the deposit and required ledger fees`,
    )
  return { base: quote, ledger, sequence, observation }
}
interface DepositMutationInput {
  attempt: UnresolvedDepositAttempt
  closeWalletSession: () => Promise<void>
  progressId: string
}

const PREFLIGHT_CHECKS: ReadonlyArray<Pick<PreflightCheck, "id" | "label">> = [
  { id: "wallets", label: "Wallets connected" },
  { id: "runtime", label: "Bridge configuration check" },
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

function runtimeObservationCheckedAt(): number {
  return Date.now()
}

export function BridgePage({
  direction,
  onDirectionChange,
}: {
  direction: BridgeDirection
  onDirectionChange: (direction: BridgeDirection) => void
}) {
  const [pageState, dispatch] = useReducer(bridgePageReducer, initialBridgePageState)
  const { depositAmount, withdrawAmount } = pageState
  const confirming = pageState.review.status !== "closed"
  const preflight = pageState.review.status === "closed" ? undefined : pageState.review.preflight
  const reviewedDeposit =
    pageState.review.status === "ready" &&
    pageState.review.direction === "deposit" &&
    pageState.review.mode === "new"
      ? pageState.review.deposit
      : undefined
  const reviewedWithdrawalAccount =
    pageState.review.status === "ready" && pageState.review.direction === "withdraw"
      ? pageState.review.account
      : undefined
  const reviewedObservation =
    pageState.review.status === "ready" ? pageState.review.observation : undefined
  const reviewedApprovalNeeded =
    pageState.review.status === "ready" ? pageState.review.approvalNeeded : undefined
  const unresolvedDeposit =
    pageState.depositRecovery.status === "unresolved"
      ? pageState.depositRecovery.attempt
      : undefined
  const resolvedIntentOwner =
    pageState.depositRecovery.status === "resolving" ? undefined : pageState.depositRecovery.owner
  const checkingDeposit =
    pageState.depositRecovery.status === "unresolved" && pageState.depositRecovery.checking
  const depositProgress: DepositProgress = pageState.depositExecution.status
  const activeDeposit =
    pageState.depositExecution.status === "authorization"
      ? pageState.depositExecution.active
      : undefined
  const submittingWithdrawal = pageState.withdrawal.status === "submitting"
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
  const assetsQuery = useQuery({
    queryKey: ["bridge-assets", deploymentProfile.bridgeCanisterId],
    enabled: Boolean(deploymentProfile.bridgeCanisterId),
    staleTime: 60_000,
    queryFn: async () => {
      const actor = await createBridgeActor(
        deploymentProfile.icHost,
        deploymentProfile.bridgeCanisterId as string,
      )
      const result = await actor.list_assets()
      if ("Err" in result) throw new Error("Registered bridge assets are unavailable")
      return result.Ok
    },
  })
  const [selectedAssetKey, setSelectedAssetKey] = useState<string>()
  const availableAssets = useMemo(() => assetsQuery.data ?? [], [assetsQuery.data])
  const preferredAsset =
    availableAssets.find((asset) => "LegacySingleToken" in asset.bridge_kind) ?? availableAssets[0]
  const effectiveAssetKey = unresolvedDeposit?.call.assetId
    ? bytesHex(unresolvedDeposit.call.assetId)
    : (selectedAssetKey ?? (preferredAsset ? bytesHex(preferredAsset.asset_id) : undefined))
  const selectedAsset = availableAssets.find(
    (asset) => bytesHex(asset.asset_id) === effectiveAssetKey,
  )
  const selectedAssetId = selectedAsset
    ? (bytesHex(selectedAsset.asset_id) as `0x${string}`)
    : undefined
  const selectedBridgeAddress = selectedAsset
    ? (bytesHex(selectedAsset.bridge_contract) as `0x${string}`)
    : undefined
  const selectedTokenAddress = selectedAsset
    ? (bytesHex(selectedAsset.token_contract) as `0x${string}`)
    : undefined
  const selectedLedgerCanisterId = selectedAsset?.ledger_canister_id.toText()
  const selectedShared = Boolean(selectedAsset && "SharedMultiToken" in selectedAsset.bridge_kind)
  const quoteAsset =
    selectedAsset && selectedAssetId && selectedBridgeAddress && selectedTokenAddress
      ? {
          assetId: selectedAssetId,
          bridgeAddress: selectedBridgeAddress,
          tokenAddress: selectedTokenAddress,
          expectedBridgeRuntimeSha256: bytesHex(selectedAsset.expected_bridge_runtime_sha256),
          expectedTokenRuntimeSha256: bytesHex(selectedAsset.expected_token_runtime_sha256),
          shared: selectedShared,
        }
      : undefined
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
  const baseQuote = useCurrentBaseQuote(
    { enabled: Boolean(selectedAsset), staleTime: 15_000 },
    quoteAsset,
  )
  const sharedRuntimeObservation = (
    quote: NonNullable<typeof baseQuote.data>,
    checkedAt: number,
  ): FinalizedRuntimeObservation => {
    if (!selectedAsset || !selectedShared)
      throw new Error("The selected shared asset is unavailable")
    const expectedSigner = bytesHex(selectedAsset.expected_bridge_signer)
    if (quote.bridgeSigner.toLowerCase() !== expectedSigner.toLowerCase())
      throw new Error("Shared Bridge signer does not match the registered asset")
    if (!("Enabled" in selectedAsset.lifecycle))
      throw new Error("The selected asset is not enabled for deposits")
    return {
      ready: true,
      blockers: [],
      checkedAt,
      chainId: Number(selectedAsset.base_chain_id),
      snapshot: quote,
    }
  }
  const refetchSelectedRuntime = async (): Promise<FinalizedRuntimeObservation> => {
    if (!selectedShared)
      return refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
    const result = await baseQuote.refetch()
    if (result.isError || !result.data) throw new Error("Shared Bridge runtime is unavailable")
    return sharedRuntimeObservation(result.data, runtimeObservationCheckedAt())
  }
  const selectedToken = selectedAsset
    ? { name: selectedAsset.name, symbol: selectedAsset.symbol, decimals: selectedAsset.decimals }
    : deploymentProfile.icToken
  const sendToken = selectedToken
  const receiveToken = selectedToken
  const selectedDecimals = selectedToken.decimals
  const baseData = baseQuote.data
  const depositParsed = parseTokenAmount(depositAmount, selectedDecimals)
  const withdrawParsed = parseTokenAmount(withdrawAmount, selectedDecimals)
  const formatSelectedAmount = (value: bigint) => formatTokenAmount(value, selectedToken.decimals)

  const ownerSequenceKey = ["deposit-owner-sequence", ic.account?.owner] as const
  const ownerSequence = useQuery({
    queryKey: ownerSequenceKey,
    enabled:
      direction === "deposit" &&
      Boolean(ic.account) &&
      resolvedIntentOwner === ic.account?.owner &&
      !unresolvedDeposit,
    ...automaticQueryOptions,
    queryFn: async () => {
      const actor = await createBridgeActor(
        deploymentProfile.icHost,
        deploymentProfile.bridgeCanisterId as string,
      )
      return actor.get_next_deposit_sequence(Principal.fromText(ic.account!.owner))
    },
  })
  const activeDepositRecord = useQuery({
    queryKey: ["active-deposit", activeDeposit?.owner, activeDeposit?.sequence.toString()],
    enabled: direction === "deposit" && Boolean(activeDeposit),
    refetchInterval: 5_000,
    refetchIntervalInBackground: true,
    queryFn: async () => {
      const actor = await createBridgeActor(
        deploymentProfile.icHost,
        deploymentProfile.bridgeCanisterId as string,
      )
      const result = await actor.get_deposit_by_owner_sequence(
        Principal.fromText(activeDeposit!.owner),
        activeDeposit!.sequence,
      )
      if (!result[0]) throw new Error("Canonical deposit is not available yet")
      return result[0]
    },
  })
  const effectiveDepositProgress: DepositProgress =
    depositProgress === "authorization" &&
    activeDepositRecord.data &&
    !isDepositAuthorizationPending(activeDepositRecord.data.state)
      ? "idle"
      : depositProgress
  const activeDepositTerminal = Boolean(
    activeDepositRecord.data && isDepositTerminal(activeDepositRecord.data.state),
  )
  useEffect(() => {
    if (!activeDepositTerminal || !activeDeposit) return
    const terminalDeposit = activeDeposit
    const reset = window.setTimeout(() => {
      dispatch({
        type: "deposit-terminal-reset",
        owner: terminalDeposit.owner,
        sequence: terminalDeposit.sequence,
      })
    }, 0)
    return () => window.clearTimeout(reset)
  }, [activeDeposit, activeDepositTerminal])
  useEffect(() => {
    if (!activeDeposit) {
      activeDepositProgressSeen.current = false
      return
    }
    const progress = bridgeProgress.progress
    if (
      progress?.direction === "deposit" &&
      progress.deposit?.owner === activeDeposit.owner &&
      progress.deposit.ownerSequence === activeDeposit.sequence.toString()
    ) {
      activeDepositProgressSeen.current = true
      return
    }
    if (progress || !activeDepositProgressSeen.current) return
    activeDepositProgressSeen.current = false
    dispatch({
      type: "deposit-terminal-reset",
      owner: activeDeposit.owner,
      sequence: activeDeposit.sequence,
    })
  }, [activeDeposit, bridgeProgress.progress])
  const ledger = useQuery({
    queryKey: [
      "deposit-ledger",
      effectiveAssetKey,
      ic.account?.owner,
      bytesHex(ic.account?.subaccount ?? new Uint8Array()),
    ],
    enabled: direction === "deposit" && Boolean(ic.account && selectedLedgerCanisterId),
    ...automaticQueryOptions,
    queryFn: async () => {
      const ledgerActor = await createLedgerActor(
        deploymentProfile.icHost,
        selectedLedgerCanisterId as string,
      )
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
    queryKey: ["base-token-balance", effectiveAssetKey, address],
    enabled: direction === "withdraw" && Boolean(address && selectedTokenAddress),
    ...automaticQueryOptions,
    queryFn: () =>
      basePublicClient.readContract({
        address: selectedTokenAddress as `0x${string}`,
        abi: bsnsAbi,
        functionName: "balanceOf",
        args: [address!],
      }),
  })
  const ledgerData = !ledger.isError && !ledger.isStale ? ledger.data : undefined
  const bsnsBalanceData =
    !bsnsBalance.isError && !bsnsBalance.isStale ? bsnsBalance.data : undefined
  const refreshing =
    baseQuote.isFetching ||
    ledger.isFetching ||
    bsnsBalance.isFetching ||
    (!unresolvedDeposit && ownerSequence.isFetching)
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
        dispatch({
          type: "deposit-intent-resolved",
          owner: account?.owner,
          attempt: account ? readDepositIntent(account) : undefined,
        })
      }
    })
    return () => {
      active = false
    }
  }, [ic.account])
  const deposit = useMutation({
    mutationFn: async ({ attempt, closeWalletSession }: DepositMutationInput) => {
      if (!address || !isConnected || !ic.account || !ic.adapter)
        throw new Error("Reconnect the wallets used for this deposit")
      const activeEvm = await currentBaseWallet()
      const activeIc = await ic.adapter.getAccount()
      requireWalletSnapshot(
        {
          address: attempt.recipient,
          chainId: deploymentProfile.chainId,
          icAccount: attempt.account,
        },
        { ...activeEvm, icAccount: activeIc },
        "before submitting this deposit",
      )
      await saveDepositIntent({ ...attempt, state: "submitted" })
      dispatch({ type: "deposit-intent-saved", attempt })
      let receipt
      try {
        receipt = await withBrowserLock(`kinic-wallet-prompt:ic:${attempt.account.owner}`, () =>
          ic.adapter!.requestDeposit(attempt.call),
        )
      } finally {
        await closeWalletSession().catch(() => undefined)
      }
      const [postEvm, postIc] = await Promise.all([currentBaseWallet(), ic.adapter.getAccount()])
      requireWalletSnapshot(
        {
          address: attempt.recipient,
          chainId: deploymentProfile.chainId,
          icAccount: attempt.account,
        },
        { ...postEvm, icAccount: postIc },
        "during the wallet prompt",
      )
      return receipt
    },
    onSuccess: async (receipt, { attempt, progressId }) => {
      queryClient.setQueryData(
        ["deposit-owner-sequence", attempt.account.owner],
        receipt.owner_sequence + 1n,
      )
      dispatch({
        type: "deposit-accepted",
        owner: attempt.account.owner,
        sequence: receipt.owner_sequence,
      })
      bridgeProgress.update(progressId, {
        phase: "ic-deposit-accepted",
        deposit: {
          owner: attempt.account.owner,
          ownerSequence: receipt.owner_sequence.toString(),
          depositId: bytesHex(receipt.deposit_id),
        },
      })
      try {
        await removeDepositIntent(attempt.account)
      } catch {
        /* The canonical receipt is the recovery source. */
      }
      void Promise.allSettled([
        queryClient.invalidateQueries({ queryKey: ["deposit-ledger"] }),
        queryClient.invalidateQueries({ queryKey: ["base-quote"] }),
        queryClient.invalidateQueries({ queryKey: ["runtime-validation"] }),
        queryClient.invalidateQueries({ queryKey: ["deposit-history"] }),
      ])
      toast.success(
        `Deposit ${bytesHex(receipt.deposit_id).slice(0, 14)}… accepted. Mint Authorization is being generated.`,
      )
    },
    onError: (error, { progressId }) => {
      dispatch({ type: "deposit-progress-changed", progress: "idle" })
      bridgeProgress.update(progressId, {
        phase: "attention",
        attentionMessage: `${transferErrorMessage(error)} Check the previous deposit before starting another one.`,
      })
      toast.error(`${transferErrorMessage(error)} Check the previous deposit before trying again.`)
    },
  })

  const submitDeposit = async (progressId: string) => {
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
      if (!unresolvedDeposit && !reviewedDeposit)
        throw new Error("Check the deposit again before opening OISY")
      dispatch({ type: "deposit-progress-changed", progress: "oisy-action" })
      const walletSession = ic.adapter.prepare(
        unresolvedDeposit?.call.ledgerCanisterId ?? selectedLedgerCanisterId,
      )
      if (unresolvedDeposit) {
        closeWalletSession = onceAsync(await walletSession)
        await refetchSelectedRuntime()
        bridgeProgress.update(progressId, { phase: "awaiting-ic-deposit" })
        await withBrowserLock(`kinic-deposit-owner:${unresolvedDeposit.account.owner}`, () =>
          deposit.mutateAsync({
            attempt: unresolvedDeposit,
            closeWalletSession: closeWalletSession!,
            progressId,
          }),
        )
        return
      }
      const reviewed = reviewedDeposit!
      closeWalletSession = onceAsync(await walletSession)
      const confirmedAccount = reviewed.account
      const confirmedRecipient = reviewed.recipient
      const activeEvm = await currentBaseWallet()
      const activeIc = await ic.adapter.getAccount()
      const expectedWallets = {
        address: confirmedRecipient,
        chainId: deploymentProfile.chainId,
        icAccount: confirmedAccount,
      }
      requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc })
      await withBrowserLock(`kinic-deposit-owner:${confirmedAccount.owner}`, async () => {
        const beforeApproval = await refetchDepositWriteGate(
          reviewed.amount,
          reviewed.gate.sequence,
          confirmedRecipient,
        )
        const requiredAllowance = reviewed.amount + beforeApproval.ledger.fee
        if (beforeApproval.ledger.allowance < requiredAllowance) {
          bridgeProgress.update(progressId, { phase: "awaiting-ic-allowance" })
          await withBrowserLock(`kinic-wallet-prompt:ic:${confirmedAccount.owner}`, () =>
            ic.adapter!.approve({
              ledgerCanisterId: selectedLedgerCanisterId,
              amount: requiredAllowance,
              currentAllowance: beforeApproval.ledger.allowance,
              ledgerFee: beforeApproval.ledger.fee,
            }),
          )
        }
        bridgeProgress.update(progressId, { phase: "awaiting-ic-deposit" })
        const [finalEvm, finalIc] = await Promise.all([
          currentBaseWallet(),
          ic.adapter!.getAccount(),
        ])
        requireWalletSnapshot(
          expectedWallets,
          { ...finalEvm, icAccount: finalIc },
          "during approval",
        )
        const final = await refetchDepositWriteGate(
          reviewed.amount,
          beforeApproval.sequence,
          confirmedRecipient,
          undefined,
        )
        const attempt: UnresolvedDepositAttempt = {
          call: {
            assetId:
              selectedShared && selectedAsset ? Uint8Array.from(selectedAsset.asset_id) : undefined,
            ledgerCanisterId: selectedLedgerCanisterId,
            ownerSequence: final.sequence,
            baseRecipient: hexToBytes(confirmedRecipient),
            grossAmount: reviewed.amount,
            maxServiceFee: final.base.serviceFee,
          },
          account: {
            owner: confirmedAccount.owner,
            subaccount: confirmedAccount.subaccount?.slice(),
          },
          recipient: confirmedRecipient,
        }
        await saveDepositIntent({ ...attempt, state: "prepared" })
        dispatch({ type: "deposit-intent-saved", attempt })
        await deposit.mutateAsync({ attempt, closeWalletSession: closeWalletSession!, progressId })
      })
    } catch (error) {
      dispatch({ type: "deposit-progress-changed", progress: "idle" })
      bridgeProgress.update(progressId, {
        phase: "attention",
        attentionMessage: transferErrorMessage(error),
      })
      toast.error(transferErrorMessage(error))
    } finally {
      await closeWalletSession?.().catch(() => undefined)
    }
  }

  const refetchDepositWriteGate = async (
    amount: bigint,
    expectedSequence: bigint,
    recipient: string,
    reusableObservation?: FinalizedRuntimeObservation,
  ): Promise<DepositWriteGate> => {
    const observationPromise =
      reusableObservation && runtimeWriteBlocker(reusableObservation) === undefined
        ? Promise.resolve(reusableObservation)
        : refetchSelectedRuntime()
    const [observation, ledgerResult, sequenceResult] = await Promise.all([
      observationPromise,
      ledger.refetch(),
      ownerSequence.refetch(),
    ])
    if (
      ledgerResult.isError ||
      ledgerResult.isStale ||
      !ledgerResult.data ||
      sequenceResult.isError ||
      sequenceResult.isStale ||
      sequenceResult.data === undefined
    ) {
      throw new Error("Deposit limits, balance, fee, allowance, or sequence could not be verified")
    }
    return validatedDepositWriteGate({
      bridgeAddress: selectedBridgeAddress,
      tokenAddress: selectedTokenAddress,
      tokenSymbol: selectedToken.symbol,
      amount,
      expectedSequence,
      recipient,
      observation,
      ledger: ledgerResult.data,
      sequence: sequenceResult.data,
    })
  }

  const assertActivePreflight = (runId: number) => {
    if (preflightRunId.current !== runId) throw new StalePreflightError()
  }
  const updatePreflightCheck = (
    runId: number,
    id: PreflightCheckId,
    status: PreflightCheckStatus,
    error?: string,
  ) => {
    dispatch({ type: "preflight-check-updated", runId, id, status, error })
  }
  const runPreflightCheck = async <T,>(
    runId: number,
    id: PreflightCheckId,
    action: () => Promise<T> | T,
  ): Promise<T> => {
    assertActivePreflight(runId)
    updatePreflightCheck(runId, id, "checking")
    try {
      const result = await action()
      assertActivePreflight(runId)
      updatePreflightCheck(runId, id, "passed")
      return result
    } catch (error) {
      if (error instanceof StalePreflightError) throw error
      const message =
        id === "runtime"
          ? "Bridge availability could not be confirmed. Please try again shortly."
          : error instanceof Error
            ? error.message
            : "This check could not be completed"
      updatePreflightCheck(runId, id, "failed", message)
      throw error
    }
  }
  const runDepositPreflight = async (runId: number) => {
    dispatch({ type: "deposit-progress-changed", progress: "checking" })
    try {
      const walletSnapshot = await runPreflightCheck(runId, "wallets", async () => {
        if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
        if (!address || !isConnected) throw new Error("Connect the Base recipient wallet")
        const account = unresolvedDeposit?.account ?? {
          owner: ic.account.owner,
          subaccount: ic.account.subaccount,
        }
        const recipient = unresolvedDeposit?.recipient ?? address
        const expectedWallets = {
          address: recipient,
          chainId: deploymentProfile.chainId,
          icAccount: account,
        }
        const [activeEvm, activeIc] = await Promise.all([
          currentBaseWallet(),
          ic.adapter.getAccount(),
        ])
        requireWalletSnapshot(
          expectedWallets,
          { ...activeEvm, icAccount: activeIc },
          "before opening the wallet prompt",
        )
        return { account, recipient }
      })
      const observation = await runPreflightCheck(runId, "runtime", () => refetchSelectedRuntime())
      const financials = await runPreflightCheck(runId, "financials", async () => {
        if (unresolvedDeposit) return undefined
        if (!depositParsed.ok) throw new Error(depositParsed.reason)
        const [ledgerResult, sequenceResult] = await Promise.all([
          ledger.refetch(),
          ownerSequence.refetch(),
        ])
        if (
          ledgerResult.isError ||
          ledgerResult.isStale ||
          !ledgerResult.data ||
          sequenceResult.isError ||
          sequenceResult.isStale ||
          sequenceResult.data === undefined
        ) {
          throw new Error("Balance, allowance, or deposit sequence could not be verified")
        }
        return { ledger: ledgerResult.data, sequence: sequenceResult.data }
      })
      const gate = await runPreflightCheck(runId, "availability", () => {
        if (unresolvedDeposit) return undefined
        if (!depositParsed.ok || !financials)
          throw new Error("Deposit amount or financial information is unavailable")
        return validatedDepositWriteGate({
          bridgeAddress: selectedBridgeAddress,
          tokenAddress: selectedTokenAddress,
          tokenSymbol: selectedToken.symbol,
          amount: depositParsed.value,
          recipient: walletSnapshot.recipient,
          expectedSequence: financials.sequence,
          observation,
          ledger: financials.ledger,
          sequence: financials.sequence,
        })
      })
      assertActivePreflight(runId)
      if (!unresolvedDeposit && depositParsed.ok && gate) {
        const reviewed: ReviewedDeposit = {
          amount: depositParsed.value,
          account: {
            owner: walletSnapshot.account.owner,
            subaccount: walletSnapshot.account.subaccount?.slice(),
          },
          recipient: walletSnapshot.recipient,
          gate,
        }
        dispatch({
          type: "deposit-review-ready",
          runId,
          deposit: reviewed,
          observation,
          approvalNeeded: gate.ledger.allowance < depositParsed.value + gate.ledger.fee,
        })
      } else if (unresolvedDeposit) {
        dispatch({
          type: "deposit-review-ready",
          runId,
          observation,
          approvalNeeded: false,
        })
      }
    } catch {
      // The failed step already owns the user-visible error.
    } finally {
      if (preflightRunId.current === runId)
        dispatch({ type: "deposit-progress-changed", progress: "idle" })
    }
  }

  const runWithdrawalPreflight = async (runId: number) => {
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
          closeWalletSession = await ic.adapter.prepare(selectedLedgerCanisterId)
          const [activeEvm, activeIc] = await Promise.all([
            currentBaseWallet(),
            ic.adapter.getAccount(),
          ])
          requireWalletSnapshot(
            expectedWallets,
            { ...activeEvm, icAccount: activeIc },
            "during destination verification",
          )
          return { owner: activeIc.owner, subaccount: activeIc.subaccount?.slice() }
        } finally {
          await closeWalletSession?.()
        }
      })
      const observation = await runPreflightCheck(runId, "runtime", () => refetchSelectedRuntime())
      const balance = await runPreflightCheck(runId, "financials", async () => {
        if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
        const quote = observation.snapshot
        const balanceResult = await bsnsBalance.refetch()
        if (
          !quote ||
          balanceResult.isError ||
          balanceResult.isStale ||
          balanceResult.data === undefined
        ) {
          throw new Error("Withdrawal fee or balance could not be verified")
        }
        if (withdrawParsed.value <= quote.serviceFee)
          throw new Error("Amount must be greater than the current service fee")
        if (balanceResult.data < withdrawParsed.value)
          throw new Error("bSNS balance is insufficient")
        return balanceResult.data
      })
      const allowance = await runPreflightCheck(runId, "availability", async () => {
        if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
        const quote = observation.snapshot
        if (!quote) throw new Error("Withdrawal availability could not be verified")
        if (quote.withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
        if (withdrawParsed.value <= quote.serviceFee)
          throw new Error("Amount must be greater than the current service fee")
        if (balance < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
        return basePublicClient.readContract({
          address: selectedTokenAddress as `0x${string}`,
          abi: bsnsAbi,
          functionName: "allowance",
          args: [address!, selectedBridgeAddress as `0x${string}`],
        })
      })
      if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
      dispatch({
        type: "withdraw-review-ready",
        runId,
        account: reviewedAccount,
        observation,
        approvalNeeded: allowance < withdrawParsed.value,
      })
    } catch {
      // The failed step already owns the user-visible error.
    }
  }

  const beginBridgeReview = () => {
    if (bridgeProgress.progress || (direction === "deposit" && effectiveDepositProgress !== "idle"))
      return
    const runId = preflightRunId.current + 1
    preflightRunId.current = runId
    dispatch({ type: "review-started", preflight: initialPreflight(runId, direction) })
    if (direction === "deposit") void runDepositPreflight(runId)
    else void runWithdrawalPreflight(runId)
  }

  const checkUnresolvedDeposit = async () => {
    if (!unresolvedDeposit) return
    const intentOwner = unresolvedDeposit.account.owner
    dispatch({ type: "deposit-intent-check-started", owner: intentOwner })
    try {
      const actor = await createBridgeActor(
        deploymentProfile.icHost,
        deploymentProfile.bridgeCanisterId as string,
      )
      const nextSequence = await actor.get_next_deposit_sequence(
        Principal.fromText(unresolvedDeposit.account.owner),
      )
      const status = classifyDepositRecoverySequence(
        unresolvedDeposit.call.ownerSequence,
        nextSequence,
      )
      if (status === "not-accepted") {
        queryClient.setQueryData(
          ["deposit-owner-sequence", unresolvedDeposit.account.owner],
          nextSequence,
        )
        await removeDepositIntent(unresolvedDeposit.account)
        dispatch({ type: "deposit-intent-cleared", owner: unresolvedDeposit.account.owner })
        toast.info(
          "The previous deposit was not accepted. You can now edit the form or start a new deposit.",
        )
      } else if (status === "accepted-or-conflicted") {
        const record = await actor.get_deposit_by_owner_sequence(
          Principal.fromText(unresolvedDeposit.account.owner),
          unresolvedDeposit.call.ownerSequence,
        )
        if (
          !record[0] ||
          record[0].gross_amount !== unresolvedDeposit.call.grossAmount ||
          record[0].max_service_fee !== unresolvedDeposit.call.maxServiceFee ||
          bytesHex(record[0].base_recipient).toLowerCase() !==
            bytesHex(unresolvedDeposit.call.baseRecipient).toLowerCase() ||
          bytesHex(record[0].from_subaccount[0] ?? new Uint8Array(32)) !==
            bytesHex(unresolvedDeposit.account.subaccount ?? new Uint8Array(32))
        ) {
          throw new Error(
            "The recorded deposit does not match the saved request. Do not start another deposit. Review its details in History.",
          )
        }
        const canonical = record[0]
        const existingProgress = bridgeProgress.progress
        const matchesExistingProgress =
          existingProgress?.direction === "deposit" &&
          existingProgress.deposit?.owner === unresolvedDeposit.account.owner &&
          existingProgress.deposit.ownerSequence === unresolvedDeposit.call.ownerSequence.toString()
        if (existingProgress && !matchesExistingProgress) {
          throw new Error(
            "Another transfer is active. Close it before recovering this deposit from History.",
          )
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
          const quotedNetAmount =
            canonical.quote[0]?.net_amount ??
            (canonical.gross_amount > canonical.max_service_fee
              ? canonical.gross_amount - canonical.max_service_fee
              : 0n)
          bridgeProgress.start({
            direction: "deposit",
            ...progressState,
            tokenApproval: "required",
            source: unresolvedDeposit.account.owner,
            destination: unresolvedDeposit.recipient,
            sendAmount: formatSelectedAmount(canonical.gross_amount),
            receiveAmount: formatSelectedAmount(quotedNetAmount),
            sendSymbol: selectedToken.symbol,
            receiveSymbol: selectedToken.symbol,
            deposit: depositIdentity,
          })
        }
        dispatch({
          type: "deposit-accepted",
          owner: unresolvedDeposit.account.owner,
          sequence: unresolvedDeposit.call.ownerSequence,
        })
        queryClient.setQueryData(
          ["deposit-owner-sequence", unresolvedDeposit.account.owner],
          nextSequence,
        )
        await removeDepositIntent(unresolvedDeposit.account)
        toast.success(
          "The previous deposit was accepted. Continue this deposit instead of starting another one.",
        )
      } else {
        toast.error(
          "We still could not confirm the previous deposit. New deposits remain blocked. Review its details in History.",
        )
      }
    } catch (error) {
      toast.error(
        `We still could not confirm the previous deposit. New deposits remain blocked. ${transferErrorMessage(error)}`,
      )
    } finally {
      dispatch({ type: "deposit-intent-check-finished", owner: intentOwner })
    }
  }

  const submitWithdrawal = async (progressId: string) => {
    let walletDispatched = false
    let broadcastSucceeded = false
    try {
      dispatch({ type: "withdrawal-submission-started" })
      if (!address) throw new Error("Connect the EVM wallet that owns bSNS")
      if (!reviewedWithdrawalAccount) throw new Error("Verify the destination IC wallet again")
      if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
      if (baseData === undefined || bsnsBalanceData === undefined)
        throw new Error("Fee or balance data is unavailable or stale")
      if (withdrawParsed.value <= baseData.serviceFee)
        throw new Error("Amount must be greater than the current service fee")
      if (bsnsBalanceData < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
      const confirmedIcAccount = {
        owner: reviewedWithdrawalAccount.owner,
        subaccount: reviewedWithdrawalAccount.subaccount?.slice(),
      }
      const snapshotAddress = address
      const activeEvm = await currentBaseWallet()
      const expectedWallets = {
        address: snapshotAddress,
        chainId: deploymentProfile.chainId,
        icAccount: confirmedIcAccount,
      }
      requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: confirmedIcAccount })
      const owner = Principal.fromText(confirmedIcAccount.owner).toUint8Array()
      const subaccount = confirmedIcAccount.subaccount ?? new Uint8Array(32)
      const [approvalObservation, approvalBalance] = await Promise.all([
        refetchSelectedRuntime(),
        bsnsBalance.refetch(),
      ])
      const approvalQuote = approvalObservation.snapshot
      if (!approvalQuote) throw new Error("Finalized Base snapshot is unavailable")
      if (approvalBalance.isError || approvalBalance.isStale || approvalBalance.data === undefined)
        throw new Error("Withdrawal limits, fee, or balance could not be verified")
      if (approvalQuote.withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
      if (
        withdrawParsed.value <= approvalQuote.serviceFee ||
        approvalBalance.data < withdrawParsed.value
      )
        throw new Error("Withdrawal fee or balance changed; review again")
      const client = basePublicClient
      const allowance = await client.readContract({
        address: selectedTokenAddress as `0x${string}`,
        abi: bsnsAbi,
        functionName: "allowance",
        args: [snapshotAddress, selectedBridgeAddress as `0x${string}`],
      })
      let approvalReceipt: ApprovalReceipt | undefined
      if (allowance < withdrawParsed.value) {
        bridgeProgress.update(progressId, {
          phase: "awaiting-base-allowance",
          tokenApproval: "required",
        })
        const approvalHash = await withBrowserLock(
          `kinic-wallet-prompt:base:${snapshotAddress.toLowerCase()}`,
          () =>
            write.writeContractAsync({
              account: snapshotAddress,
              address: selectedTokenAddress as `0x${string}`,
              abi: bsnsAbi,
              functionName: "approve",
              args: [selectedBridgeAddress as `0x${string}`, withdrawParsed.value],
            }),
        )
        approvalReceipt = await client.waitForTransactionReceipt({ hash: approvalHash })
        if (approvalReceipt.status !== "success") throw new Error("Token approval failed")
        bridgeProgress.update(progressId, { phase: "awaiting-base-withdrawal" })
      } else {
        bridgeProgress.update(progressId, {
          phase: "awaiting-base-withdrawal",
          tokenApproval: "not-required",
        })
      }
      bridgeProgress.update(progressId, { phase: "awaiting-base-approval-reflection" })
      const simulateWithdrawal = (serviceFee: bigint, blockNumber: bigint) =>
        selectedShared
          ? client.simulateContract({
              account: snapshotAddress,
              address: selectedBridgeAddress as `0x${string}`,
              abi: multiTokenBridgeAbi,
              functionName: "createWithdrawal",
              args: [
                selectedAssetId as `0x${string}`,
                withdrawParsed.value,
                serviceFee,
                bytesToHex(owner),
                bytesToHex(subaccount),
              ],
              blockNumber,
            })
          : client.simulateContract({
              account: snapshotAddress,
              address: selectedBridgeAddress as `0x${string}`,
              abi: withdrawalAbi,
              functionName: "createWithdrawal",
              args: [withdrawParsed.value, serviceFee, bytesToHex(owner), bytesToHex(subaccount)],
              blockNumber,
            })
      const sendWithdrawal = (serviceFee: bigint) =>
        selectedShared
          ? write.writeContractAsync({
              account: snapshotAddress,
              address: selectedBridgeAddress as `0x${string}`,
              abi: multiTokenBridgeAbi,
              functionName: "createWithdrawal",
              args: [
                selectedAssetId as `0x${string}`,
                withdrawParsed.value,
                serviceFee,
                bytesToHex(owner),
                bytesToHex(subaccount),
              ],
            })
          : write.writeContractAsync({
              account: snapshotAddress,
              address: selectedBridgeAddress as `0x${string}`,
              abi: withdrawalAbi,
              functionName: "createWithdrawal",
              args: [withdrawParsed.value, serviceFee, bytesToHex(owner), bytesToHex(subaccount)],
            })
      const broadcast = await withBrowserLock(
        `kinic-wallet-prompt:base:${snapshotAddress.toLowerCase()}`,
        () =>
          createWithdrawalAfterRevalidation({
            approval: {
              receipt: approvalReceipt,
              amount: withdrawParsed.value,
              getBlock: (blockNumber) =>
                client.getBlock(
                  blockNumber === undefined ? { blockTag: "latest" } : { blockNumber },
                ),
              readAllowance: (blockNumber) =>
                client.readContract({
                  address: selectedTokenAddress as `0x${string}`,
                  abi: bsnsAbi,
                  functionName: "allowance",
                  args: [snapshotAddress, selectedBridgeAddress as `0x${string}`],
                  blockNumber,
                }),
            },
            expectedWallets,
            refetchRuntime: async () => ({
              data: await refetchSelectedRuntime(),
            }),
            currentEvmWallet: currentBaseWallet,
            currentIcAccount: () =>
              Promise.resolve({
                owner: confirmedIcAccount.owner,
                subaccount: confirmedIcAccount.subaccount?.slice(),
              }),
            refetchFinancials: async (observation) => {
              const quote = observation.snapshot
              if (!quote) throw new Error("Finalized Base snapshot is unavailable")
              const balanceResult = await bsnsBalance.refetch()
              if (
                balanceResult.isError ||
                balanceResult.isStale ||
                balanceResult.data === undefined
              )
                throw new Error("Fee or balance data changed and could not be verified")
              return {
                serviceFee: quote.serviceFee,
                balance: balanceResult.data,
                withdrawalsPaused: quote.withdrawalsPaused,
              }
            },
            validateFinancials: ({ serviceFee, balance: finalBalance, withdrawalsPaused }) => {
              if (withdrawalsPaused) throw new Error("Withdrawals are paused on Base")
              if (withdrawParsed.value <= serviceFee)
                throw new Error("Amount must be greater than the current service fee")
              if (finalBalance < withdrawParsed.value)
                throw new Error("bSNS balance is insufficient")
            },
            simulateWithdrawal: ({ serviceFee }, blockNumber) =>
              simulateWithdrawal(serviceFee, blockNumber),
            createWithdrawal: ({ serviceFee }) => {
              walletDispatched = true
              bridgeProgress.update(progressId, { phase: "awaiting-base-withdrawal" })
              return sendWithdrawal(serviceFee)
            },
            onBroadcast: async (transactionHash) => {
              bridgeProgress.update(progressId, {
                phase: "base-withdrawal-submitted",
                transactionHash,
              })
              return savePendingConfirmation({
                kind: "withdrawal",
                transactionHash,
                owner: confirmedIcAccount.owner,
                assetId: selectedAssetId,
                contractAddress: selectedBridgeAddress,
                shared: selectedShared,
              })
            },
          }),
      )
      broadcastSucceeded = true
      if (broadcast.pendingSaved) {
        toast.success(
          `Withdrawal submitted: ${broadcast.transactionHash.slice(0, 12)}…. Confirmation is pending. Check History after finalization if it has not completed.`,
        )
      } else {
        bridgeProgress.update(progressId, {
          storageWarning: "Transaction sent, but storage failed. Keep the transaction hash.",
        })
        toast.warning(
          `Withdrawal ${broadcast.transactionHash} was submitted, but this browser could not save it. Keep the transaction hash and check its status in your wallet.`,
        )
      }
    } catch (error) {
      bridgeProgress.update(progressId, {
        phase: "attention",
        issue: walletDispatched ? "unknown" : "stopped",
        attentionMessage: walletDispatched
          ? "Check your wallet. No automatic retry."
          : transferErrorMessage(error),
      })
      toast.error(transferErrorMessage(error))
    } finally {
      dispatch({
        type: "withdrawal-submission-finished",
        clearAmount: broadcastSucceeded,
      })
    }
  }

  const retryAccountMatches =
    unresolvedDeposit && ic.account ? sameIcAccount(ic.account, unresolvedDeposit.account) : false
  const retryRecipientMatches =
    unresolvedDeposit && address
      ? address.toLowerCase() === unresolvedDeposit.recipient.toLowerCase()
      : false
  const reviewedQuote = reviewedDeposit?.gate.base ?? reviewedObservation?.snapshot
  const quoteForDisplay = reviewedQuote ?? baseData
  const depositsConfirmedPaused = baseData?.depositsPaused === true
  const withdrawalsConfirmedPaused = baseData?.withdrawalsPaused === true
  const assetDepositEnabled = Boolean(selectedAsset && "Enabled" in selectedAsset.lifecycle)
  const assetWithdrawalEnabled = Boolean(selectedAsset && !("Prepared" in selectedAsset.lifecycle))
  const activeTransferReason = bridgeProgress.progress
    ? "Complete or close the current transfer before starting another one"
    : undefined
  const depositBlockers = unresolvedDeposit
    ? ([
        activeTransferReason,
        !selectedAsset && "Select an available asset",
        selectedAsset && !assetDepositEnabled && "This asset is not enabled for deposits",
        depositsConfirmedPaused && "Deposits are paused on Base",
        !ic.account && "Reconnect the original IC wallet",
        !address && "Reconnect the original EVM wallet",
        ic.account && !retryAccountMatches && "Reconnect the original IC wallet",
        address && !retryRecipientMatches && "Reconnect the original EVM wallet",
      ].filter(Boolean) as string[])
    : ([
        activeTransferReason,
        !selectedAsset && "Select an available asset",
        selectedAsset && !assetDepositEnabled && "This asset is not enabled for deposits",
        !address && "Connect both wallets",
        !ic.account && "Connect both wallets",
        depositsConfirmedPaused && "Deposits are paused on Base",
        !depositParsed.ok && (depositParsed.reason ?? "Enter an amount"),
      ].filter(Boolean) as string[])
  const withdrawalBlockers = [
    activeTransferReason,
    !selectedAsset && "Select an available asset",
    selectedAsset && !assetWithdrawalEnabled && "This asset is not enabled for withdrawals",
    !address && "Connect both wallets",
    !ic.account && "Connect both wallets",
    withdrawalsConfirmedPaused && "Withdrawals are paused on Base",
    !withdrawParsed.ok && (withdrawParsed.reason ?? "Enter an amount"),
  ].filter(Boolean) as string[]
  const blockers = direction === "deposit" ? depositBlockers : withdrawalBlockers
  const awaitingDepositAuthorization =
    direction === "deposit" &&
    Boolean(activeDeposit) &&
    (!activeDepositRecord.data || isDepositAuthorizationPending(activeDepositRecord.data.state))
  const depositActionPending =
    direction === "deposit" &&
    (effectiveDepositProgress !== "idle" || deposit.isPending || awaitingDepositAuthorization)
  const amountError =
    !unresolvedDeposit &&
    (direction === "deposit"
      ? !depositParsed.ok
        ? depositParsed.reason
        : undefined
      : !withdrawParsed.ok
        ? withdrawParsed.reason
        : undefined)
  const amount =
    direction === "deposit"
      ? unresolvedDeposit
        ? formatSelectedAmount(unresolvedDeposit.call.grossAmount)
        : depositAmount
      : withdrawAmount
  const balance = direction === "deposit" ? ledgerData?.balance : bsnsBalanceData
  const fee = unresolvedDeposit?.call.maxServiceFee ?? quoteForDisplay?.serviceFee
  const feeLabel = reviewedQuote || baseData ? "Current bridge fee" : "Bridge fee"
  const receive =
    direction === "deposit"
      ? unresolvedDeposit
        ? unresolvedDeposit.call.grossAmount > unresolvedDeposit.call.maxServiceFee
          ? unresolvedDeposit.call.grossAmount - unresolvedDeposit.call.maxServiceFee
          : 0n
        : depositParsed.ok && fee !== undefined
          ? depositParsed.value > fee
            ? depositParsed.value - fee
            : 0n
          : undefined
      : withdrawParsed.ok && fee !== undefined && withdrawParsed.value > fee
        ? estimatedAmountOut(withdrawParsed.value, fee)
        : undefined
  const source =
    direction === "deposit"
      ? {
          network: "ic" as const,
          wallet: unresolvedDeposit?.account.owner ?? ic.account?.owner ?? "Connect IC wallet",
        }
      : { network: "base" as const, wallet: address ?? "Connect EVM wallet" }
  const destination =
    direction === "deposit"
      ? {
          network: "base" as const,
          wallet: unresolvedDeposit?.recipient ?? address ?? "Connect EVM wallet",
        }
      : {
          network: "ic" as const,
          wallet: reviewedWithdrawalAccount?.owner ?? ic.account?.owner ?? "Connect IC wallet",
        }
  const depositFlowActive = direction === "deposit" && Boolean(activeDeposit)
  const depositControlsLocked =
    direction === "deposit" &&
    (Boolean(unresolvedDeposit) || effectiveDepositProgress !== "idle" || depositFlowActive)
  const assetControlsLocked =
    depositControlsLocked || submittingWithdrawal || confirming || Boolean(bridgeProgress.progress)
  const maximumAmount =
    direction === "deposit"
      ? ledgerData !== undefined
        ? maximumDepositAmount(ledgerData.balance, ledgerData.fee, ledgerData.allowance)
        : undefined
      : bsnsBalanceData
  const maximumAmountDisabled =
    depositControlsLocked || maximumAmount === undefined || maximumAmount === 0n
  const useMaximumAmount = () => {
    if (maximumAmountDisabled || maximumAmount === undefined) return
    const formatted = formatSelectedAmount(maximumAmount)
    dispatch({ type: "amount-changed", direction, value: formatted })
  }

  const changeDirection = () => {
    if (depositControlsLocked) return
    preflightRunId.current += 1
    dispatch({ type: "review-closed" })
    onDirectionChange(direction === "deposit" ? "withdraw" : "deposit")
  }
  const setBridgeReviewOpen = (open: boolean) => {
    if (open) return
    preflightRunId.current += 1
    dispatch({ type: "review-closed" })
    if (depositProgress === "checking")
      dispatch({ type: "deposit-progress-changed", progress: "idle" })
  }
  const confirmBridgeReview = () => {
    preflightRunId.current += 1
    dispatch({ type: "review-closed" })
    let progress
    try {
      if (direction === "withdraw" && !reviewedWithdrawalAccount) {
        throw new Error("Verify the destination IC wallet again")
      }
      progress = bridgeProgress.start({
        direction,
        phase:
          direction === "deposit"
            ? reviewedApprovalNeeded === false
              ? "awaiting-ic-deposit"
              : "awaiting-ic-allowance"
            : "verifying-ic-destination",
        tokenApproval: reviewedApprovalNeeded === false ? "not-required" : "required",
        source: source.wallet,
        destination: destination.wallet,
        sendAmount: amount || "—",
        receiveAmount: receive !== undefined ? formatSelectedAmount(receive) : "—",
        sendSymbol: sendToken.symbol,
        receiveSymbol: receiveToken.symbol,
        assetId: selectedAssetId,
        contractAddress: selectedBridgeAddress,
        shared: selectedShared,
        deposit:
          direction === "deposit"
            ? unresolvedDeposit
              ? {
                  owner: unresolvedDeposit.account.owner,
                  ownerSequence: unresolvedDeposit.call.ownerSequence.toString(),
                }
              : reviewedDeposit
                ? {
                    owner: reviewedDeposit.account.owner,
                    ownerSequence: reviewedDeposit.gate.sequence.toString(),
                  }
                : undefined
            : undefined,
        withdrawal:
          direction === "withdraw" && reviewedWithdrawalAccount
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
  const depositActionLabel =
    effectiveDepositProgress === "checking"
      ? "Checking deposit…"
      : effectiveDepositProgress === "oisy-action" || deposit.isPending
        ? "Confirming deposit…"
        : effectiveDepositProgress === "authorization" || awaitingDepositAuthorization
          ? "Generating authorization…"
          : unresolvedDeposit
            ? "Retry the same deposit"
            : "Bridge to Base"
  return (
    <div className="route-enter mx-auto w-full max-w-[620px] pb-6 pt-4 lg:pb-10 lg:pt-10">
      <section
        className="overflow-hidden rounded-[24px] border border-[var(--line)] bg-[var(--panel)] p-4 shadow-[0_24px_70px_rgba(20,34,53,.09)] sm:p-5"
        aria-label={`${selectedToken.symbol} bridge`}
        data-testid="bridge-panel"
      >
        <div className="mb-5 flex items-center justify-between gap-4">
          <div
            className={`kinic-rail ${direction === "withdraw" ? "is-withdraw" : ""}`}
            aria-hidden="true"
          >
            <i />
            <i />
            <i />
            <i />
          </div>
          <Button size="sm" variant="ghost" disabled={refreshing} onClick={refreshBridgeData}>
            <RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />
            {refreshing ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
        <div className="mb-3 rounded-2xl bg-white p-4">
          <Label htmlFor="bridge-asset">Asset</Label>
          <select
            id="bridge-asset"
            value={effectiveAssetKey ?? ""}
            disabled={assetControlsLocked || assetsQuery.isLoading}
            onChange={(event) => {
              preflightRunId.current += 1
              dispatch({ type: "review-closed" })
              setSelectedAssetKey(event.target.value)
            }}
            className="mt-2 h-11 w-full rounded-xl border border-[var(--line)] bg-white px-3 text-sm font-bold text-black focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:text-[var(--muted)]"
          >
            {availableAssets.map((asset) => {
              const key = bytesHex(asset.asset_id)
              return (
                <option key={key} value={key}>
                  {asset.name} ({asset.symbol})
                </option>
              )
            })}
          </select>
        </div>
        <div className="relative grid gap-2 sm:grid-cols-2">
          <EndpointCard
            label="From"
            network={source.network}
            wallet={source.wallet}
            disabled={depositControlsLocked}
            onClick={() => wallets.openFor(direction === "deposit" ? "ic" : "base")}
          />
          <EndpointCard
            label="To"
            network={destination.network}
            wallet={destination.wallet}
            disabled={depositControlsLocked}
            onClick={() => wallets.openFor(direction === "deposit" ? "base" : "ic")}
          />
          <button
            type="button"
            disabled={depositControlsLocked}
            onClick={changeDirection}
            className="absolute left-1/2 top-1/2 z-10 grid size-8 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border-2 border-[var(--panel)] bg-black text-white transition duration-300 hover:rotate-180 hover:bg-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:bg-[var(--muted)] disabled:hover:rotate-0"
            aria-label="Reverse bridge direction"
          >
            <ArrowDownUp className="size-3.5 sm:rotate-90" />
          </button>
        </div>
        <div className="mt-3 rounded-2xl bg-white p-4">
          <div className="flex items-center justify-between gap-4">
            <Label htmlFor="bridge-amount">You send</Label>
            <span className="text-sm text-[var(--muted)]">
              Balance {balance !== undefined ? formatSelectedAmount(balance) : "—"}{" "}
              {sendToken.symbol}
            </span>
          </div>
          <div className="mt-1 flex items-center gap-2 sm:gap-3">
            <Input
              id="bridge-amount"
              disabled={depositControlsLocked}
              aria-invalid={Boolean(amountError)}
              aria-describedby="bridge-amount-feedback"
              className="font-numeric h-14 min-w-0 border-0 px-0 text-3xl font-semibold focus:ring-0"
              inputMode="decimal"
              placeholder="0.00000000"
              value={amount}
              onChange={(event) => {
                dispatch({ type: "amount-changed", direction, value: event.target.value })
              }}
            />
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="h-9 shrink-0 rounded-xl px-3"
              disabled={maximumAmountDisabled}
              onClick={useMaximumAmount}
            >
              MAX
            </Button>
            <span className="shrink-0 rounded-xl bg-[var(--panel)] px-3 py-2 text-sm font-bold">
              {sendToken.symbol}
            </span>
          </div>
        </div>
        <div className="mt-3 grid grid-cols-2 gap-3 rounded-2xl bg-white p-4 text-sm">
          <Quote
            label={feeLabel}
            value={fee !== undefined ? `${formatSelectedAmount(fee)} ${sendToken.symbol}` : "—"}
          />
          <Quote
            label="Estimated receive"
            value={
              receive !== undefined
                ? `${formatSelectedAmount(receive)} ${receiveToken.symbol}`
                : "—"
            }
          />
        </div>
        {direction === "deposit" &&
          (effectiveDepositProgress === "oisy-action" || deposit.isPending) && (
            <DepositProgressCard
              title="Confirming deposit…"
              detail="Confirm the action in Oisy. After confirmation, its window stays open while the bridge verifies Deposit acceptance."
            />
          )}
        {direction === "deposit" &&
          activeDepositRecord.data &&
          ("AuthorizationAvailable" in activeDepositRecord.data.state ? (
            <DepositProgressCard
              title="Mint Authorization ready"
              detail="Continue from the transfer progress window to confirm the Base mint transaction."
            />
          ) : (
            <div className="mt-4 rounded-2xl border border-[var(--line)] bg-white p-4 text-sm">
              <p className="font-bold text-black">Generating authorization…</p>
              <p className="mt-1 text-[var(--muted)]">
                {depositPhaseLabel(activeDepositRecord.data)}
              </p>
              <p className="mt-1 text-[var(--muted)]">
                Wait here for the authorization, or recover it later from History.
              </p>
            </div>
          ))}
        {direction === "deposit" &&
          effectiveDepositProgress === "authorization" &&
          !activeDepositRecord.data && (
            <DepositProgressCard
              title="Generating authorization…"
              detail="The Deposit was accepted. Waiting for the Mint Authorization to become available."
            />
          )}
        {unresolvedDeposit && !deposit.isPending && (
          <div
            role="alert"
            className="mt-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] p-4 text-sm text-[#8a4b08]"
          >
            <p className="font-bold text-black">Previous deposit outcome is unconfirmed</p>
            <p className="mt-1 leading-5">
              We could not confirm whether your previous deposit request was accepted. To prevent
              duplicate deposits, starting a new deposit is blocked until this check is resolved.
            </p>
            <p className="mt-1 leading-5">
              Retrying the same deposit uses the saved request without changing its details. A
              previous token approval may still be active.
            </p>
            <div className="mt-3 flex flex-wrap gap-2">
              <Button
                size="sm"
                variant="ghost"
                disabled={checkingDeposit}
                onClick={() => void checkUnresolvedDeposit()}
              >
                {checkingDeposit ? "Checking previous deposit…" : "Check previous deposit"}
              </Button>
              <Link
                to="/history"
                className="inline-flex h-9 items-center rounded-xl px-3 text-sm font-bold underline underline-offset-4"
              >
                View deposit history
              </Link>
            </div>
          </div>
        )}
        {!depositFlowActive && (
          <Button
            className="mt-3 h-14 w-full"
            size="lg"
            aria-busy={depositActionPending}
            disabled={
              blockers.length > 0 || depositActionPending || write.isPending || submittingWithdrawal
            }
            onClick={beginBridgeReview}
          >
            {direction === "deposit" ? depositActionLabel : "Bridge to IC"}
            {depositActionPending ? (
              <LoaderCircle className="size-4 animate-spin" aria-hidden="true" />
            ) : (
              <ArrowRight className="size-4" aria-hidden="true" />
            )}
          </Button>
        )}
        <p
          id="bridge-amount-feedback"
          className="mt-3 min-h-4 text-center text-xs text-[var(--muted)]"
          aria-live="polite"
        >
          {depositFlowActive ? (
            <>
              Complete the current deposit above or continue from{" "}
              <Link to="/history" className="font-bold underline underline-offset-4">
                History
              </Link>
              .
            </>
          ) : blockers.length > 0 ? (
            `Next: ${blockers[0]}`
          ) : null}
        </p>
      </section>
      <BridgeConfirmationDialog
        direction={direction}
        open={confirming}
        setOpen={setBridgeReviewOpen}
        preflight={preflight}
        source={source.wallet}
        destination={destination.wallet}
        amount={amount}
        receive={receive}
        fee={fee}
        sendSymbol={sendToken.symbol}
        receiveSymbol={receiveToken.symbol}
        decimals={selectedToken.decimals}
        pending={deposit.isPending || write.isPending || submittingWithdrawal}
        onRetry={beginBridgeReview}
        onConfirm={confirmBridgeReview}
      />
    </div>
  )
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
    attentionMessage:
      "This deposit cannot continue to Base minting. Open History to review its refund or reconciliation state.",
  }
}

function EndpointCard({
  label,
  network,
  wallet,
  disabled,
  onClick,
}: {
  label: string
  network: BridgeNetwork
  wallet: string
  disabled?: boolean
  onClick: () => void
}) {
  const details = NETWORKS[network]
  const displayWallet = shortenWalletAddress(wallet)
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={() => onClick()}
      className="min-w-0 rounded-2xl border border-[var(--line)] bg-white p-3.5 text-left transition duration-300 hover:-translate-y-[2px] hover:border-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:hover:translate-y-0 disabled:hover:border-[var(--line)]"
    >
      <span className="text-xs font-medium text-[var(--muted)]">{label}</span>
      <span className="mt-0.5 flex items-center justify-between gap-3">
        <span className="flex min-w-0 items-center gap-2">
          <img
            src={details.logo}
            alt=""
            aria-hidden="true"
            data-network-logo={network}
            className="h-[22px] w-auto shrink-0"
          />
          <strong className="truncate text-base text-black">{details.label}</strong>
        </span>
        <LockKeyhole className="size-4 shrink-0 text-[var(--pink)]" />
      </span>
      <span
        className="mt-1 block truncate text-xs text-[var(--muted)]"
        title={displayWallet === wallet ? undefined : wallet}
      >
        {displayWallet}
      </span>
    </button>
  )
}
function Quote({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-xs text-[var(--muted)]">{label}</p>
      <p className="font-numeric mt-1 font-bold text-black">{value}</p>
    </div>
  )
}
function DepositProgressCard({ title, detail }: { title: string; detail: string }) {
  return (
    <div
      className="mt-4 rounded-2xl border border-[var(--line)] bg-white p-4 text-sm"
      role="status"
    >
      <p className="font-bold text-black">{title}</p>
      <p className="mt-1 leading-5 text-[var(--muted)]">{detail}</p>
    </div>
  )
}
function ConfirmRow({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-xs text-[var(--muted)]">{label}</p>
      <p className="mt-1 break-all text-sm font-bold text-black">{value}</p>
    </div>
  )
}
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

export function BridgeConfirmationDialog({
  direction,
  open,
  setOpen,
  preflight,
  source,
  destination,
  amount,
  receive,
  fee,
  sendSymbol,
  receiveSymbol,
  decimals = 8,
  pending,
  onRetry,
  onConfirm,
}: {
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
  decimals?: number
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
    : failed
      ? "No transaction was sent."
      : "Checking current bridge conditions. No transaction has been sent."
  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="max-h-[min(760px,calc(100vh-2rem))] max-w-[560px] overflow-y-auto">
        <DialogHeader>
          <DialogTitle>
            {direction === "deposit" ? "Review bridge to Base" : "Review bridge to IC"}
          </DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <p className="sr-only" aria-live="polite">
          {preflightAnnouncement(preflight)}
        </p>
        {!ready && !failed && (
          <div
            className="mt-5 flex items-center gap-3 rounded-2xl border border-[#bfd7ff] bg-[#eef5ff] p-4"
            role="status"
          >
            <LoaderCircle className="size-5 shrink-0 animate-spin text-[var(--pink)]" />
            <p className="text-sm font-bold text-black">
              Checking your wallets, balance, fees, and bridge availability…
            </p>
          </div>
        )}
        {failed && failedCheck && (
          <div className="mt-5 rounded-2xl border border-[#ffbdad] bg-[#fff0ec] p-4" role="alert">
            <div className="flex items-center gap-2 font-bold text-[#b42318]">
              <TriangleAlert className="size-4" />
              {failedCheck.label}
            </div>
            <p className="mt-2 text-sm leading-6 text-[#7a271a]">
              {failedCheck.error ?? "This check could not be completed."}
            </p>
          </div>
        )}
        {ready && (
          <>
            <div className="mt-5 grid gap-4 rounded-2xl bg-[var(--panel)] p-4 sm:grid-cols-2">
              <ConfirmRow label="You send" value={`${amount || "—"} ${sendSymbol}`} />
              <ConfirmRow
                label="You receive"
                value={`${receive !== undefined ? formatTokenAmount(receive, decimals) : "—"} ${receiveSymbol}`}
              />
              <ConfirmRow
                label="Bridge fee"
                value={`${fee !== undefined ? formatTokenAmount(fee, decimals) : "—"} ${sendSymbol}`}
              />
              <ConfirmRow label="From" value={source} />
              <div className="sm:col-span-2">
                <ConfirmRow label="Recipient" value={destination} />
              </div>
            </div>
          </>
        )}
        <DialogFooter>
          <DialogClose asChild>
            <Button variant="ghost">{failed ? "Close" : "Cancel"}</Button>
          </DialogClose>
          {failed && <Button onClick={onRetry}>Try again</Button>}
          {ready && (
            <Button disabled={pending} onClick={onConfirm}>
              {direction === "deposit" ? "Continue to IC wallet" : "Continue to Base wallet"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}
function bytesToHex(bytes: Uint8Array): `0x${string}` {
  return `0x${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}`
}
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
