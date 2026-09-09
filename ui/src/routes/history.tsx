import { redactRpcUrls } from "@/lib/transfer-error"
import {
  recoverTransaction,
  TransactionEvidenceMismatch,
  withdrawalReceiptDetails,
} from "@/lib/transaction-recovery"
import { observeDeposit, type MintObservation } from "@/lib/mint-observation"
import { readBaseBlock, readBaseReceipt } from "@/lib/base-transaction-observation"
import { Principal } from "@icp-sdk/core/principal"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import { createFileRoute } from "@tanstack/react-router"
import { Clock3, RefreshCcw } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import { toast } from "sonner"
import { hexToBytes } from "viem"
import { useAccount, useChainId } from "wagmi"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { deploymentProfile } from "@/config/profile"
import { MintAuthorizationAction } from "@/features/bridge/mint-authorization-action"
import { useBridgeProgress } from "@/features/bridge/bridge-progress-provider"
import { useRuntimeHeartbeat, useRuntimeValidation } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import type {
  AutomaticProgressView,
  DepositView,
  NonterminalDepositRef,
  NotifyWithdrawalReceipt,
  SettlementActionResult,
  WithdrawalView,
} from "@/generated/bridge.did"
import {
  activityAutoRefreshEnabled,
  mergeActivityItems,
  olderActivitySources,
  visibleActivityItems,
  type ActivityBoundaries,
  type ActivityItem,
  type WithdrawalHistoryItem,
} from "@/lib/activity-history"
import { useActivityAutoRefresh } from "@/lib/activity-auto-refresh"
import { formatTokenAmount } from "@/lib/amounts"
import { withBrowserLock } from "@/lib/browser-lock"
import {
  depositIdsForRefresh,
  mergeDepositHistoryPage,
  type DepositHistoryData,
} from "@/lib/deposit-history"
import {
  type DepositMintFinalizationStatus,
  type ExpectedDepositMint,
} from "@/lib/deposit-mint-finalization"
import { baseTransactionExplorerUrl } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import { kinicTransactionExplorerUrl } from "@/lib/ic/transaction-explorer"
import { continueWithdrawalWithBrowserIdentity } from "@/lib/ic/withdrawal-notification-client"
import {
  readPendingConfirmations,
  readPendingMint,
  removePendingConfirmation,
} from "@/lib/pending-confirmations"
import { refetchRuntimeAttestedWriteReady } from "@/lib/runtime-validation"
import {
  authorizationDeadlineRefundStatus,
  depositContinuation,
  depositPhaseName,
  depositPhaseTone,
  depositReconciliationMessage,
  depositUsesPendingMintStatus,
  isDepositTerminal,
  isWithdrawalTerminal,
  settlementStateName,
  withdrawalPhaseName,
  withdrawalPhaseTone,
} from "@/lib/settlement-phase"
import { fetchInBatches, notifyHistoryWithdrawal } from "@/lib/withdrawal-history"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"

export const Route = createFileRoute("/history")({ component: HistoryPage })

export interface WithdrawalHistoryData {
  items: WithdrawalHistoryItem[]
  nextCursor: Uint8Array | number[] | null
  olderBoundaryNs: bigint | null
}

export function mergeWithdrawalHistoryData(
  current: WithdrawalHistoryData | undefined,
  result: WithdrawalHistoryData,
): WithdrawalHistoryData {
  const items = new Map(
    (current?.items ?? []).map((item) => [item.id?.toString() ?? item.hash, item]),
  )
  for (const item of result.items) items.set(item.id?.toString() ?? item.hash, item)
  return {
    ...result,
    items: [...items.values()].sort((a, b) =>
      a.createdAtNs === b.createdAtNs ? 0 : a.createdAtNs > b.createdAtNs ? -1 : 1,
    ),
  }
}

type HistorySourceState = "disconnected" | "loading" | "ready" | "unavailable"

function HistoryPage() {
  const [recoveryHash, setRecoveryHash] = useState("")
  const [recovering, setRecovering] = useState(false)
  const { address } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const historyAccount = ic.account ?? ic.historyAccount
  const runtime = useRuntimeValidation(chainId, { enabled: false })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, { enabled: false })
  const queryClient = useQueryClient()
  const bridgeProgress = useBridgeProgress()
  const completeWithdrawalProgress = bridgeProgress.completeWithdrawal
  const [retryingHash, setRetryingHash] = useState<string>()
  const [actioningId, setActioningId] = useState<string>()
  const [loadingOlderWithdrawals, setLoadingOlderWithdrawals] = useState(false)
  const [loadingOlderDeposits, setLoadingOlderDeposits] = useState(false)
  const [pageVisible, setPageVisible] = useState(() => document.visibilityState === "visible")

  const depositQueryKey = ["deposit-history", historyAccount?.owner] as const
  const readDepositHistory = async (
    mode: "refresh" | "older",
    previous?: DepositHistoryData,
  ): Promise<DepositHistoryData> => {
    const actor = await createBridgeActor(
      deploymentProfile.icHost,
      deploymentProfile.bridgeCanisterId as string,
    )
    const beforeCursor = mode === "older" ? previous?.nextCursor : undefined
    let result = await actor.list_deposit_ids({
      owner: Principal.fromText(historyAccount!.owner),
      before_cursor: beforeCursor === undefined || beforeCursor === null ? [] : [beforeCursor],
      limit: 20,
    })
    if ("Err" in result) throw new Error("Deposit history limit was rejected")
    const latestIds: Array<Uint8Array | number[]> = [...result.Ok.deposit_ids]
    if (mode === "refresh" && previous?.items.length) {
      const known = new Set(
        previous.items.map((record) => bytesHex(record.deposit_id).toLowerCase()),
      )
      let cursor = result.Ok.next_cursor[0]
      while (
        cursor !== undefined &&
        !latestIds.some((id) => known.has(bytesHex(id).toLowerCase()))
      ) {
        result = await actor.list_deposit_ids({
          owner: Principal.fromText(historyAccount!.owner),
          before_cursor: [cursor],
          limit: 20,
        })
        if ("Err" in result) throw new Error("Deposit history limit was rejected")
        latestIds.push(...result.Ok.deposit_ids)
        cursor = result.Ok.next_cursor[0]
      }
    }
    let nonterminalRefs: NonterminalDepositRef[] = previous?.pendingFunding ?? []
    if (mode === "refresh") {
      nonterminalRefs = []
      let nonterminalCursor: bigint | undefined
      do {
        const open = await actor.list_nonterminal_deposit_refs({
          owner: Principal.fromText(historyAccount!.owner),
          before_cursor: nonterminalCursor === undefined ? [] : [nonterminalCursor],
          limit: 100,
        })
        if ("Err" in open) throw new Error("Open deposit list limit was rejected")
        nonterminalRefs.push(...open.Ok.deposits)
        nonterminalCursor = open.Ok.next_cursor[0]
      } while (nonterminalCursor !== undefined)
    }
    const openIds = nonterminalRefs.map((record) => record.deposit_id)
    const ids =
      mode === "refresh"
        ? depositIdsForRefresh(
            previous,
            [...latestIds, ...openIds],
            (record) => !isDepositTerminal(record.state),
          )
        : result.Ok.deposit_ids
    const records = await fetchInBatches(ids, 20, (batch) =>
      Promise.all(batch.map((id) => actor.get_deposit(id))),
    )
    const resolvedRecords = records.flatMap((record) => record)
    const resolvedIds = new Set(
      resolvedRecords.map((record) => bytesHex(record.deposit_id).toLowerCase()),
    )
    const pendingFunding = nonterminalRefs.filter(
      (record) => !resolvedIds.has(bytesHex(record.deposit_id).toLowerCase()),
    )
    return mergeDepositHistoryPage(
      previous,
      resolvedRecords,
      {
        nextCursor: result.Ok.next_cursor[0] ?? null,
        oldestAvailableCursor: result.Ok.oldest_available_cursor[0] ?? null,
        historyTruncated: result.Ok.history_truncated,
        pendingFunding,
      },
      mode,
    )
  }
  const deposits = useQuery({
    queryKey: depositQueryKey,
    enabled: Boolean(historyAccount),
    queryFn: () =>
      readDepositHistory("refresh", queryClient.getQueryData<DepositHistoryData>(depositQueryKey)),
  })
  const mintRecords = (deposits.data?.items ?? []).filter(
    (record) => record.mint_authorization.length > 0,
  )
  const mintObservations = useQuery({
    queryKey: [
      "deposit-mint-observations",
      deploymentProfile.deploymentInstanceId,
      historyAccount?.owner,
      mintRecords.map((record) => bytesHex(record.deposit_id)).join(":"),
    ],
    enabled: mintRecords.length > 0,
    queryFn: async () =>
      new Map(
        await fetchInBatches(mintRecords, 5, (batch) =>
          Promise.all(
            batch.map(
              async (record) =>
                [bytesHex(record.deposit_id), await observeDeposit(record)] as const,
            ),
          ),
        ),
      ),
    staleTime: 10_000,
    refetchInterval: (query) =>
      [...(query.state.data?.values() ?? [])].some(
        (observation) =>
          Boolean(observation.transactionHash) &&
          !observation.recorded &&
          !(observation.status === "reverted" && observation.finalized) &&
          observation.status !== "conflict",
      )
        ? 10_000
        : false,
    retry: false,
  })
  const finalizedClock = useQuery({
    queryKey: ["history-finalized-time", deploymentProfile.deploymentInstanceId],
    enabled: mintRecords.some((record) => !isDepositTerminal(record.state)),
    queryFn: () => readBaseBlock("finalized"),
    staleTime: 10_000,
    retry: false,
  })
  const withdrawalQueryKey = [
    "withdraw-history",
    deploymentProfile.chainId,
    deploymentProfile.bridgeAddress,
    address,
  ] as const
  const readWithdrawalHistory = async (
    mode: "refresh" | "older",
    previous?: WithdrawalHistoryData,
  ): Promise<WithdrawalHistoryData> => {
    const actor = await createBridgeActor(
      deploymentProfile.icHost,
      deploymentProfile.bridgeCanisterId as string,
    )
    let cursor = mode === "older" ? previous?.nextCursor : undefined
    const items: WithdrawalHistoryItem[] = []
    const previousIds = new Set(
      previous?.items.filter((item) => item.canister).map((item) => item.id),
    )
    let reachedPrevious = false
    let nextCursor: Uint8Array | number[] | null = null
    // Bound each refresh; if a gap remains, pagination continues from that gap.
    for (let page = 0; page < (mode === "refresh" && previous ? 5 : 1); page += 1) {
      const response = await actor.list_withdrawals({
        requester: hexToBytes(address!),
        before_cursor: cursor ? [cursor] : [],
        limit: 20,
      })
      if ("Err" in response)
        throw new Error(
          "IndexNotReady" in response.Err
            ? "Recorded history is being prepared. Please try again shortly."
            : "Recorded Base → IC history could not be loaded.",
        )
      for (const row of response.Ok.items) {
        const id = BigInt(bytesHex(row.withdrawal.withdrawal_id))
        if (previousIds.has(id)) reachedPrevious = true
        items.push({
          id,
          amount: row.withdrawal.amount,
          amountOut: row.withdrawal.amount_out,
          hash: row.transaction_hash[0] ? bytesHex(row.transaction_hash[0]) : undefined,
          createdAtNs: row.observed_at_ns,
          destinationAccount: {
            owner: row.owner.toText(),
            subaccount: Uint8Array.from(row.subaccount),
          },
          canister: row.withdrawal,
        })
      }
      nextCursor = response.Ok.next_cursor[0] ?? null
      if (nextCursor === null || reachedPrevious) break
      cursor = nextCursor
    }
    const olderBoundaryNs = nextCursor === null ? null : (items.at(-1)?.createdAtNs ?? null)
    if (mode === "refresh") {
      const refreshedIds = new Set(items.map((item) => item.id))
      const unresolved = (previous?.items ?? []).filter(
        (item) =>
          item.canister && !isWithdrawalTerminal(item.canister.state) && !refreshedIds.has(item.id),
      )
      const views = await fetchInBatches(unresolved, 20, async (batch) => {
        const response = await actor.get_withdrawals(
          batch.map((item) => item.canister!.withdrawal_id),
        )
        if ("Err" in response) throw new Error("Recorded payout status could not be refreshed.")
        return response.Ok
      })
      for (const [index, view] of views.entries()) {
        if (view[0]) items.push({ ...unresolved[index]!, canister: view[0] })
      }
    }
    const knownHashes = new Set(items.map((item) => item.hash?.toLowerCase()))
    for (const pending of readPendingConfirmations()) {
      if (knownHashes.has(pending.transactionHash.toLowerCase())) continue
      try {
        const item = await withdrawalReceiptDetails(pending.transactionHash, pending.owner)
        if (item.requester.toLowerCase() === address?.toLowerCase()) {
          const block = await readBaseBlock(item.blockNumber)
          const finalized = await readBaseBlock("finalized")
          const receipt = await readBaseReceipt(pending.transactionHash)
          items.push({
            ...item,
            createdAtNs: block.timestamp * 1_000_000_000n,
            baseNeedsReview:
              finalized.number !== null &&
              finalized.number >= item.blockNumber &&
              block.hash !== receipt.blockHash,
          })
        }
      } catch (error) {
        const old = previous?.items.find(
          (item) => item.hash?.toLowerCase() === pending.transactionHash.toLowerCase(),
        )
        const missing =
          error &&
          typeof error === "object" &&
          "name" in error &&
          error.name === "TransactionReceiptNotFoundError"
        if (old && !old.canister && (missing || error instanceof TransactionEvidenceMismatch))
          items.push({ ...old, baseNeedsReview: true })
        // Transport failure keeps the prior row and its confirmed execution state.
      }
    }
    const result = {
      items,
      nextCursor,
      olderBoundaryNs,
    }
    return mode === "refresh" && previous
      ? {
          ...mergeWithdrawalHistoryData(previous, result),
          nextCursor: reachedPrevious ? previous.nextCursor : nextCursor,
          olderBoundaryNs: reachedPrevious ? previous.olderBoundaryNs : olderBoundaryNs,
        }
      : result
  }

  const withdrawals = useQuery({
    queryKey: withdrawalQueryKey,
    enabled: Boolean(address),
    queryFn: () =>
      readWithdrawalHistory(
        "refresh",
        queryClient.getQueryData<WithdrawalHistoryData>(withdrawalQueryKey),
      ),
  })

  const boundaries = useMemo<ActivityBoundaries>(
    () => ({
      deposit: {
        enabled: Boolean(historyAccount) && !deposits.isError,
        hasMore: deposits.data ? deposits.data.nextCursor !== null : Boolean(historyAccount),
        unseenBeforeNs:
          deposits.data?.nextCursor === null
            ? undefined
            : oldestDepositTimestamp(deposits.data?.items),
      },
      withdrawal: {
        enabled: Boolean(address) && !withdrawals.isError,
        hasMore: withdrawals.data ? withdrawals.data.nextCursor !== null : Boolean(address),
        unseenBeforeNs: withdrawals.data?.olderBoundaryNs ?? undefined,
      },
    }),
    [
      address,
      deposits.data,
      deposits.isError,
      historyAccount,
      withdrawals.data,
      withdrawals.isError,
    ],
  )
  const allItems = useMemo(
    () => mergeActivityItems(deposits.data?.items ?? [], withdrawals.data?.items ?? []),
    [deposits.data?.items, withdrawals.data?.items],
  )
  const visibleItems = useMemo(
    () => visibleActivityItems(allItems, "all", boundaries),
    [allItems, boundaries],
  )
  const olderSources = useMemo(() => olderActivitySources("all", boundaries), [boundaries])
  const hasAutomaticProgress =
    Boolean(deposits.data?.pendingFunding.length) ||
    (deposits.data?.items ?? []).some(
      (record) => !isDepositTerminal(record.state) && record.automatic_progress.length > 0,
    ) ||
    (withdrawals.data?.items ?? []).some(
      (item) => !item.canister || !isWithdrawalTerminal(item.canister.state),
    )
  const hasUnresolvedMint = mintRecords.some((record) => !isDepositTerminal(record.state))

  useEffect(() => {
    const onVisibilityChange = () => setPageVisible(document.visibilityState === "visible")
    document.addEventListener("visibilitychange", onVisibilityChange)
    return () => document.removeEventListener("visibilitychange", onVisibilityChange)
  }, [])
  useActivityAutoRefresh(
    activityAutoRefreshEnabled(pageVisible, hasAutomaticProgress || hasUnresolvedMint),
    () => {
      void Promise.all([
        historyAccount ? deposits.refetch() : Promise.resolve(),
        hasUnresolvedMint ? mintObservations.refetch() : Promise.resolve(),
        hasUnresolvedMint ? finalizedClock.refetch() : Promise.resolve(),
        address ? withdrawals.refetch() : Promise.resolve(),
      ])
    },
  )
  useEffect(() => {
    for (const item of withdrawals.data?.items ?? []) {
      if (!item.hash || !item.canister || !("Paid" in item.canister.state)) continue
      completeWithdrawalProgress({
        transactionHash: item.hash,
        owner: item.destinationAccount.owner,
        withdrawalId: bytesHex(item.canister.withdrawal_id),
      })
    }
  }, [completeWithdrawalProgress, withdrawals.data?.items])

  const scanOlderWithdrawals = async () => {
    if (!withdrawals.data || withdrawals.data.nextCursor === null) return
    try {
      setLoadingOlderWithdrawals(true)
      const result = await readWithdrawalHistory("older", withdrawals.data)
      queryClient.setQueryData<WithdrawalHistoryData>(withdrawalQueryKey, (current) =>
        mergeWithdrawalHistoryData(current, result),
      )
    } catch (error) {
      toast.error(
        error instanceof Error
          ? redactRpcUrls(error.message)
          : "Older withdrawal history is unavailable",
      )
    } finally {
      setLoadingOlderWithdrawals(false)
    }
  }
  const scanOlderDeposits = async () => {
    if (!deposits.data || deposits.data.nextCursor === null) return
    try {
      setLoadingOlderDeposits(true)
      const result = await readDepositHistory("older", deposits.data)
      queryClient.setQueryData<DepositHistoryData>(depositQueryKey, (current) =>
        mergeDepositHistoryPage(
          result,
          current?.items ?? [],
          {
            nextCursor: result.nextCursor,
            oldestAvailableCursor: result.oldestAvailableCursor,
            historyTruncated: result.historyTruncated,
            pendingFunding: current?.pendingFunding ?? result.pendingFunding,
          },
          "older",
        ),
      )
    } catch (error) {
      toast.error(
        error instanceof Error
          ? redactRpcUrls(error.message)
          : "Older deposit history is unavailable",
      )
    } finally {
      setLoadingOlderDeposits(false)
    }
  }
  const loadOlderActivity = async () => {
    await Promise.all(
      olderSources.map((source) =>
        source === "to-base" ? scanOlderDeposits() : scanOlderWithdrawals(),
      ),
    )
  }
  const checkAndNotify = async (item: WithdrawalHistoryItem) => {
    try {
      setRetryingHash(item.hash)
      await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      if (!item.hash) throw new Error("Restore this transaction using its Base transaction hash.")
      const { pending, receipt, withdrawalId } = await notifyHistoryWithdrawal(
        { ...item, hash: item.hash },
        undefined,
        (await readBaseBlock("finalized")).number ?? 0n,
      )
      toastWithdrawalNotification(receipt)
      try {
        const result = await continueWithdrawalWithBrowserIdentity(withdrawalId)
        toastSettlement(result)
        if (
          "Complete" in result &&
          "Withdrawal" in result.Complete.state &&
          isWithdrawalTerminal(result.Complete.state.Withdrawal)
        ) {
          completeWithdrawalProgress({
            transactionHash: item.hash,
            owner: item.destinationAccount.owner,
            withdrawalId: bytesHex(withdrawalId),
          })
          await removePendingConfirmation(pending)
        }
      } catch (error) {
        toast.warning(
          error instanceof Error
            ? redactRpcUrls(error.message)
            : "The payout needs another attempt from History.",
        )
      }
      await withdrawals.refetch()
    } catch (error) {
      await withdrawals.refetch()
      toast.error(
        error instanceof Error ? redactRpcUrls(error.message) : "Withdrawal notification failed",
      )
    } finally {
      setRetryingHash(undefined)
    }
  }
  const requestDepositRefund = async (record: DepositView) => {
    const key = bytesHex(record.deposit_id)
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      setActioningId(key)
      if (!ic.adapter)
        throw new Error("Connect any non-anonymous IC wallet to continue this refund")
      closeWalletSession = await ic.adapter.prepare()
      await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      const result = await withBrowserLock(
        `kinic-wallet-prompt:ic:${ic.account?.owner ?? "unknown"}`,
        () => ic.adapter!.requestDepositRefund(Uint8Array.from(record.deposit_id)),
      )
      if ("Refunded" in result.state) toast.success("Refund completed.")
      else if ("Minted" in result.state) toast.success("This deposit was already minted on Base.")
      else toast.info("Refund claim recorded. Run the claim again to continue reconciliation.")
      await deposits.refetch()
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : "The refund could not be claimed. Try again later.",
      )
    } finally {
      await closeWalletSession?.()
      setActioningId(undefined)
    }
  }
  const continueDeposit = async (record: DepositView) => {
    const key = bytesHex(record.deposit_id)
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      setActioningId(key)
      if (!ic.adapter)
        throw new Error("Connect any non-anonymous IC wallet to retry this authorization")
      closeWalletSession = await ic.adapter.prepare()
      await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      const result = await withBrowserLock(
        `kinic-wallet-prompt:ic:${ic.account?.owner ?? "unknown"}`,
        () => ic.adapter!.continueDeposit(Uint8Array.from(record.deposit_id)),
      )
      toastSettlement(result)
      await deposits.refetch()
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : "The authorization could not be retried. Try again later.",
      )
    } finally {
      await closeWalletSession?.()
      setActioningId(undefined)
    }
  }
  const continueWithdrawal = async (item: WithdrawalHistoryItem) => {
    const key = item.id?.toString() ?? item.hash
    try {
      setActioningId(key)
      if (!item.canister) throw new Error("Notify the finalized withdrawal first")
      if (!feeGuardBlocked(item.canister))
        await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      const result = await continueWithdrawalWithBrowserIdentity(
        Uint8Array.from(item.canister.withdrawal_id),
      )
      toastSettlement(result)
      if (
        "Complete" in result &&
        "Withdrawal" in result.Complete.state &&
        isWithdrawalTerminal(result.Complete.state.Withdrawal)
      ) {
        if (item.hash)
          completeWithdrawalProgress({
            transactionHash: item.hash,
            owner: item.destinationAccount.owner,
            withdrawalId: bytesHex(item.canister.withdrawal_id),
          })
        const pending = readPendingConfirmations().find(
          (entry) =>
            entry.kind === "withdrawal" &&
            entry.transactionHash.toLowerCase() === item.hash?.toLowerCase(),
        )
        if (pending) await removePendingConfirmation(pending)
      }
      await withdrawals.refetch()
    } catch (error) {
      toast.error(
        error instanceof Error
          ? error.message
          : "This payout step could not be completed. Try again later.",
      )
    } finally {
      setActioningId(undefined)
    }
  }
  const refresh = async () => {
    {
      await Promise.all([
        historyAccount ? deposits.refetch() : Promise.resolve(),
        hasUnresolvedMint ? mintObservations.refetch() : Promise.resolve(),
        hasUnresolvedMint ? finalizedClock.refetch() : Promise.resolve(),
        address ? withdrawals.refetch() : Promise.resolve(),
      ])
    }
  }
  const refreshing =
    (hasUnresolvedMint && finalizedClock.isFetching) ||
    (Boolean(historyAccount) && (deposits.isFetching || mintObservations.isFetching)) ||
    (Boolean(address) && withdrawals.isFetching)
  const loadingInitial =
    Boolean(historyAccount && !deposits.data && deposits.isFetching) ||
    Boolean(address && !withdrawals.data && withdrawals.isFetching)
  const loadingOlder = loadingOlderDeposits || loadingOlderWithdrawals
  const writesEnabled = !runtime.isFetching && !heartbeat.isFetching
  const sourceStates = {
    deposit: !historyAccount
      ? "disconnected"
      : deposits.isError
        ? "unavailable"
        : !deposits.data
          ? "loading"
          : "ready",
    withdrawal: !address
      ? "disconnected"
      : withdrawals.isError
        ? "unavailable"
        : !withdrawals.data
          ? "loading"
          : "ready",
  } satisfies Record<"deposit" | "withdrawal", HistorySourceState>

  return (
    <div className="route-enter mx-auto max-w-6xl pt-8 md:pt-12">
      <header className="mb-8 flex items-end justify-between gap-4">
        <div>
          <h1 className="font-display text-[42px] leading-[1.1]">Bridge history</h1>
        </div>
        <Button variant="ghost" disabled={refreshing} onClick={() => void refresh()}>
          <RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />
          {refreshing ? "Refreshing…" : "Refresh"}
        </Button>
      </header>

      <form
        className="mb-5 flex flex-wrap gap-2"
        onSubmit={async (event) => {
          event.preventDefault()
          setRecovering(true)
          try {
            toast.success(
              await recoverTransaction(recoveryHash.trim(), {
                evm: address,
                ic: historyAccount?.owner,
              }),
            )
            setRecoveryHash("")
            await refresh()
          } catch (error) {
            toast.error(
              error instanceof Error
                ? redactRpcUrls(error.message)
                : "Transaction recovery failed.",
            )
          } finally {
            setRecovering(false)
          }
        }}
      >
        <input
          aria-label="Base transaction hash"
          placeholder="0x… Base transaction hash"
          value={recoveryHash}
          onChange={(event) => setRecoveryHash(event.target.value)}
          className="min-w-64 flex-1 rounded-lg border p-2"
        />
        <Button type="submit" disabled={recovering || !recoveryHash.trim()}>
          {recovering ? "Restoring…" : "Restore transaction"}
        </Button>
      </form>
      <section
        aria-label="Bridge activity"
        className="min-h-80 rounded-[20px] bg-[var(--panel)] p-4 sm:p-6"
      >
        {!historyAccount && !address ? (
          <Empty
            icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />}
            title="Connect a wallet"
            message="Connect an IC or EVM wallet to load your bridge activity."
          />
        ) : loadingInitial && !allItems.length ? (
          <Empty
            icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />}
            title="Loading activity"
            message="This may take a moment."
          />
        ) : (
          <ActivityList
            items={visibleItems}
            sourceStates={sourceStates}
            writesEnabled={writesEnabled}
            actioningId={actioningId}
            retryingHash={retryingHash}
            historyTruncated={Boolean(deposits.data?.historyTruncated)}
            pendingFunding={deposits.data?.pendingFunding ?? []}
            mintObservations={mintObservations.data ?? new Map()}
            finalizedTimestamp={finalizedClock.data?.timestamp}
            hasOlder={olderSources.length > 0}
            loadingOlder={loadingOlder}
            onRequestDepositRefund={requestDepositRefund}
            onContinueDeposit={continueDeposit}
            onCheckAndNotify={checkAndNotify}
            onContinueWithdrawal={continueWithdrawal}
            onLoadOlder={loadOlderActivity}
            onRefresh={() => void refresh()}
          />
        )}
      </section>
    </div>
  )
}

function Empty({
  icon,
  title,
  message,
}: {
  icon: React.ReactNode
  title: string
  message: string
}) {
  return (
    <div className="grid min-h-64 place-items-center text-center">
      <div>
        {icon}
        <p className="mt-3 font-bold text-black">{title}</p>
        <p className="mt-1 text-sm text-[var(--muted)]">{message}</p>
      </div>
    </div>
  )
}

function ActivityList({
  items,
  sourceStates,
  writesEnabled,
  actioningId,
  retryingHash,
  historyTruncated,
  pendingFunding,
  mintObservations,
  finalizedTimestamp,
  hasOlder,
  loadingOlder,
  onRequestDepositRefund,
  onContinueDeposit,
  onCheckAndNotify,
  onContinueWithdrawal,
  onLoadOlder,
  onRefresh,
}: {
  items: ActivityItem[]
  sourceStates: Record<"deposit" | "withdrawal", HistorySourceState>
  writesEnabled: boolean
  actioningId?: string
  retryingHash?: string
  historyTruncated: boolean
  pendingFunding: NonterminalDepositRef[]
  mintObservations: Map<string, MintObservation>
  finalizedTimestamp?: bigint
  hasOlder: boolean
  loadingOlder: boolean
  onRequestDepositRefund: (record: DepositView) => Promise<void>
  onContinueDeposit: (record: DepositView) => Promise<void>
  onCheckAndNotify: (item: WithdrawalHistoryItem) => Promise<void>
  onContinueWithdrawal: (item: WithdrawalHistoryItem) => Promise<void>
  onLoadOlder: () => Promise<void>
  onRefresh: () => void
}) {
  if (!items.length && pendingFunding.length) {
    return <PendingFundingDeposits deposits={pendingFunding} />
  }
  if (!items.length) {
    const relevantStates = [sourceStates.deposit, sourceStates.withdrawal]
    if (relevantStates.includes("unavailable")) {
      return <HistoryUnavailable sourceStates={sourceStates} onRefresh={onRefresh} />
    }
    if (relevantStates.includes("loading")) {
      return (
        <Empty
          icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />}
          title="Loading activity"
          message="This may take a moment."
        />
      )
    }
    if (relevantStates.every((state) => state === "disconnected")) {
      return (
        <Empty
          icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />}
          title="Connect a wallet"
          message="Connect an IC or EVM wallet to load your bridge activity."
        />
      )
    }
    const emptyMessage = "Your bridge transfers will appear here."
    return (
      <div>
        <Empty
          icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />}
          title={
            relevantStates.includes("disconnected")
              ? "No connected-wallet activity"
              : "No activity yet"
          }
          message={
            relevantStates.includes("disconnected")
              ? `${emptyMessage} Connect the other wallet to include its direction.`
              : emptyMessage
          }
        />
        {hasOlder && <LoadOlder loading={loadingOlder} onClick={onLoadOlder} />}
      </div>
    )
  }
  const unavailable = unavailableSources(sourceStates)
  return (
    <div className="space-y-3">
      {unavailable.length > 0 && (
        <HistoryUnavailable sourceStates={sourceStates} onRefresh={onRefresh} compact />
      )}
      {historyTruncated && (
        <p className="rounded-xl bg-[#fff3e4] px-3 py-2 text-xs font-medium text-[#8a4b08]">
          Some older IC → Base activity is no longer available.
        </p>
      )}
      {pendingFunding.length > 0 && <PendingFundingDeposits deposits={pendingFunding} />}
      <div className="hidden grid-cols-[minmax(6rem,0.7fr)_minmax(7rem,0.8fr)_minmax(7rem,0.8fr)_minmax(9rem,1.3fr)_minmax(7.5rem,1fr)_minmax(6rem,0.7fr)_9rem] gap-4 px-4 pb-1 text-xs font-bold uppercase tracking-[0.08em] text-[var(--muted)] lg:grid">
        <span>Direction</span>
        <span>Base tx</span>
        <span>KINIC tx</span>
        <span>Amount</span>
        <span>Status</span>
        <span>Time</span>
        <span>Next step</span>
      </div>
      {items.map((item) => {
        const observation =
          item.direction === "to-base"
            ? mintObservations.get(bytesHex(item.deposit.deposit_id))
            : undefined
        return item.direction === "to-base" ? (
          <DepositActivityRow
            key={item.key}
            item={item}
            mintFinalization={
              observation?.status === "success"
                ? "minted"
                : observation?.unavailable
                  ? "unavailable"
                  : "absent"
            }
            mintTransactionHash={observation?.transactionHash}
            mintRecording={
              observation?.recorded
                ? "recorded"
                : observation?.notificationError
                  ? "retrying"
                  : observation?.finalized
                    ? "pending"
                    : "confirming"
            }
            processedWithoutReceipt={observation?.status === "processed"}
            finalizedBlockTimestamp={finalizedTimestamp}
            writesEnabled={writesEnabled}
            actioningId={actioningId}
            onRequestRefund={onRequestDepositRefund}
            onContinue={onContinueDeposit}
          />
        ) : (
          <WithdrawalActivityRow
            key={item.key}
            item={item}
            writesEnabled={writesEnabled}
            actioningId={actioningId}
            retryingHash={retryingHash}
            onCheckAndNotify={onCheckAndNotify}
            onContinue={onContinueWithdrawal}
          />
        )
      })}
      {hasOlder && <LoadOlder loading={loadingOlder} onClick={onLoadOlder} />}
    </div>
  )
}

function PendingFundingDeposits({ deposits }: { deposits: NonterminalDepositRef[] }) {
  return (
    <div className="rounded-xl bg-[#fff3e4] px-4 py-3 text-sm text-[#8a4b08]">
      <p className="font-bold">Funding recovery in progress</p>
      <p className="mt-1 text-xs">
        These deposits are preserved by the canister even if browser storage was cleared.
      </p>
      <ul className="mt-2 space-y-1 text-xs">
        {deposits.map((deposit) => (
          <li key={bytesHex(deposit.deposit_id)}>
            Sequence {deposit.owner_sequence.toString()} ·{" "}
            {bytesHex(deposit.deposit_id).slice(0, 14)}…
          </li>
        ))}
      </ul>
    </div>
  )
}

function unavailableSources(
  states: Record<"deposit" | "withdrawal", HistorySourceState>,
): Array<"deposit" | "withdrawal"> {
  return (["deposit", "withdrawal"] as const).filter((source) => states[source] === "unavailable")
}

function HistoryUnavailable({
  sourceStates,
  onRefresh,
  compact = false,
}: {
  sourceStates: Record<"deposit" | "withdrawal", HistorySourceState>
  onRefresh: () => void
  compact?: boolean
}) {
  const unavailable = unavailableSources(sourceStates)
  const direction =
    unavailable.length > 1 ? "Bridge" : unavailable[0] === "deposit" ? "IC → Base" : "Base → IC"
  if (compact) {
    return (
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-white px-4 py-3 text-sm text-[var(--muted)]">
        <span>{direction} activity could not be loaded.</span>
        <Button size="sm" variant="ghost" onClick={onRefresh}>
          Refresh
        </Button>
      </div>
    )
  }
  return (
    <div className="grid min-h-64 place-items-center text-center">
      <div>
        <RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />
        <p className="mt-3 font-bold text-black">Activity could not be loaded</p>
        <p className="mt-1 text-sm text-[var(--muted)]">
          {direction} activity is temporarily unavailable.
        </p>
        <Button className="mt-4" size="sm" variant="ghost" onClick={onRefresh}>
          Refresh
        </Button>
      </div>
    </div>
  )
}

export function DepositActivityRow({
  item,
  mintFinalization,
  processedWithoutReceipt,
  mintTransactionHash,
  mintRecording,
  finalizedBlockTimestamp,
  writesEnabled,
  actioningId,
  onRequestRefund,
  onContinue,
}: {
  item: Extract<ActivityItem, { direction: "to-base" }>
  mintFinalization: DepositMintFinalizationStatus
  processedWithoutReceipt?: boolean
  mintTransactionHash?: `0x${string}`
  mintRecording?: "recorded" | "retrying" | "pending" | "confirming"
  finalizedBlockTimestamp?: bigint
  writesEnabled: boolean
  actioningId?: string
  onRequestRefund: (record: DepositView) => Promise<void>
  onContinue: (record: DepositView) => Promise<void>
}) {
  const record = item.deposit
  const key = bytesHex(record.deposit_id)
  const expectedMint = expectedDepositMint(record)
  const pendingMint = expectedMint
    ? readPendingMint({
        depositId: expectedMint.depositId,
        authorizationDigest: expectedMint.authorizationDigest,
        recipient: expectedMint.recipient,
        grossAmount: expectedMint.grossAmount.toString(),
        chargedServiceFee: expectedMint.serviceFee.toString(),
        mintedAmount: expectedMint.mintedAmount.toString(),
      })
    : undefined
  const transactionHash = mintTransactionHash ?? pendingMint?.transactionHash
  const terminal = isDepositTerminal(record.state)
  const progress = automaticProgressInfo(record.automatic_progress)
  const refund = record.refund[0]
  const kinicTransactions = depositKinicTransactions(record)
  const quote = record.quote[0]
  const continuation = depositContinuation(record)
  const deadlineRefundStatus = authorizationDeadlineRefundStatus(record, finalizedBlockTimestamp)
  const refundPhase = "RefundAvailable" in record.state || "RefundProcessing" in record.state
  const reconciliationMessage = refundPhase
    ? depositReconciliationMessage(record.state, record.last_settlement_stop_reason[0])
    : continuation.message
  const mintedOnBase = mintFinalization === "minted"
  const mintSubmitted = depositUsesPendingMintStatus(
    record.state,
    Boolean(pendingMint),
    mintedOnBase,
  )
  const amountText = refund
    ? `${formatTokenAmount(refund.amount)} KINIC`
    : quote
      ? `${formatTokenAmount(quote.net_amount)} KINIC`
      : `${formatTokenAmount(record.gross_amount)} KINIC`
  return (
    <article className="grid gap-4 rounded-2xl bg-white p-4 lg:grid-cols-[minmax(6rem,0.7fr)_minmax(7rem,0.8fr)_minmax(7rem,0.8fr)_minmax(9rem,1.3fr)_minmax(7.5rem,1fr)_minmax(6rem,0.7fr)_9rem] lg:items-center">
      <div>
        <MobileLabel>Direction</MobileLabel>
        <Badge tone="info">IC → Base</Badge>
      </div>
      <div>
        <MobileLabel>Base tx</MobileLabel>
        {processedWithoutReceipt && (
          <p className="text-xs">
            Processed on Base. Restore with a transaction hash to link the receipt.
          </p>
        )}
        {transactionHash ? (
          <BaseTransactionLink transactionHash={transactionHash} />
        ) : (
          <p className="mt-1 text-xs text-[var(--muted)]">
            {processedWithoutReceipt ? "Receipt not linked" : "Not submitted"}
          </p>
        )}
      </div>
      <div>
        <MobileLabel>KINIC tx</MobileLabel>
        {kinicTransactions.length === 0 ? (
          <p className="mt-1 text-xs text-[var(--muted)]">Not confirmed</p>
        ) : (
          <div>
            {kinicTransactions.map((transaction) => (
              <KinicTransactionLink key={transaction.kind} {...transaction} />
            ))}
          </div>
        )}
      </div>
      <div>
        <MobileLabel>Amount</MobileLabel>
        <p className="text-sm font-bold">{amountText}</p>
      </div>
      <div>
        <MobileLabel>Status</MobileLabel>
        <Badge
          tone={
            mintedOnBase
              ? "good"
              : processedWithoutReceipt || mintSubmitted
                ? "info"
                : depositPhaseTone(record.state)
          }
        >
          {processedWithoutReceipt
            ? "Processed on Base"
            : mintedOnBase
              ? "Success"
              : mintSubmitted
                ? "Mint pending"
                : depositPhaseName(record.state)}
        </Badge>
        {mintedOnBase && mintRecording && (
          <p className="mt-1 text-xs text-[var(--muted)]">
            {mintRecording === "recorded"
              ? "Recorded on IC"
              : mintRecording === "retrying"
                ? "IC recording will retry automatically"
                : mintRecording === "pending"
                  ? "Waiting for IC recording"
                  : "Confirming on Base"}
          </p>
        )}
        {!mintedOnBase && !mintSubmitted && progress && <AutomaticProgress progress={progress} />}
        {!mintedOnBase && refund && "RefundAmountTooSmall" in refund.reason && (
          <p className="mt-1 text-xs font-bold text-[var(--muted)]">
            The amount cannot cover a refund after the service fee. No service fee was charged.
          </p>
        )}
        {!mintedOnBase && refund && "InvalidRecipient" in refund.reason && (
          <p className="mt-1 text-xs font-bold text-[var(--muted)]">
            The recipient cannot receive a mint. No service fee was charged.
          </p>
        )}
        {!mintedOnBase && refund && "ReconciliationRequired" in refund.status && (
          <p className="mt-1 text-xs font-bold text-[#b42318]">
            Ledger result is uncertain — requesting again checks the same transfer.
          </p>
        )}
        {!mintedOnBase && reconciliationMessage && (
          <p
            className={`mt-1 text-xs font-bold ${record.last_settlement_stop_reason[0] ? "text-[#b42318]" : "text-[var(--muted)]"}`}
          >
            {reconciliationMessage}
          </p>
        )}
      </div>
      <div>
        <MobileLabel>Time</MobileLabel>
        <ActivityTime valueNs={item.createdAtNs} />
      </div>
      <div className="min-w-0">
        <MobileLabel>Next step</MobileLabel>
        {processedWithoutReceipt ? (
          <span className="text-sm text-[var(--muted)]">Restore with a transaction hash</span>
        ) : mintedOnBase ? (
          <span className="text-sm text-[var(--muted)]">—</span>
        ) : "AuthorizationAvailable" in record.state ? (
          <MintAuthorizationAction
            key={pendingMint?.transactionHash ?? "unsubmitted"}
            record={record}
            compact
            onRequestRefund={writesEnabled ? () => void onRequestRefund(record) : undefined}
            claimingRefund={actioningId === key}
          />
        ) : continuation.action === "retry-authorization" ? (
          <Button
            size="sm"
            variant="ghost"
            disabled={!writesEnabled || actioningId === key}
            onClick={() => void onContinue(record)}
          >
            {actioningId === key ? "Retrying…" : "Retry authorization"}
          </Button>
        ) : continuation.action === "request-refund" ? (
          deadlineRefundStatus === "ready" ? (
            <Button
              size="sm"
              variant="ghost"
              disabled={!writesEnabled || actioningId === key}
              onClick={() => void onRequestRefund(record)}
            >
              {actioningId === key ? "Requesting…" : "Request refund"}
            </Button>
          ) : (
            <span className="text-sm text-[var(--muted)]">
              {deadlineRefundStatus === "checking-finality"
                ? "Checking Base finality…"
                : "Waiting for Base finality"}
            </span>
          )
        ) : "RefundAvailable" in record.state ||
          ("RefundProcessing" in record.state &&
            Boolean(
              record.last_settlement_stop_reason[0] ||
              (refund && "ReconciliationRequired" in refund.status),
            )) ? (
          <Button
            size="sm"
            variant="ghost"
            disabled={!writesEnabled || actioningId === key}
            onClick={() => void onRequestRefund(record)}
          >
            {actioningId === key ? "Requesting…" : "Request refund"}
          </Button>
        ) : "RefundProcessing" in record.state ? (
          <span className="text-sm text-[var(--muted)]">Refunding…</span>
        ) : terminal ? (
          <span className="text-sm text-[var(--muted)]">—</span>
        ) : continuation.mode === "automatic" ? (
          <span className="text-sm text-[var(--muted)]">Automatic retry</span>
        ) : continuation.mode === "stopped" ? (
          <span className="text-sm text-[var(--muted)]">Review required</span>
        ) : (
          <span className="text-sm text-[var(--muted)]">Automatic processing</span>
        )}
      </div>
    </article>
  )
}

function WithdrawalActivityRow({
  item,
  writesEnabled,
  actioningId,
  retryingHash,
  onCheckAndNotify,
  onContinue,
}: {
  item: Extract<ActivityItem, { direction: "to-ic" }>
  writesEnabled: boolean
  actioningId?: string
  retryingHash?: string
  onCheckAndNotify: (record: WithdrawalHistoryItem) => Promise<void>
  onContinue: (record: WithdrawalHistoryItem) => Promise<void>
}) {
  const record = item.withdrawal
  const key = record.id?.toString() ?? record.hash
  const terminal = record.canister && isWithdrawalTerminal(record.canister.state)
  const pendingNotification = readPendingConfirmations().find(
    (entry) =>
      entry.kind === "withdrawal" &&
      entry.transactionHash.toLowerCase() === record.hash?.toLowerCase(),
  )?.notification
  const pendingAttempt =
    pendingNotification?.status === "awaiting-notification" ? pendingNotification : undefined
  const needsAttention = Boolean(record.canister && !terminal)
  const label = !record.canister
    ? "Committed"
    : "ReleasePending" in record.canister.state
      ? "Payout pending"
      : "ReconciliationHold" in record.canister.state
        ? "Recovery needed"
        : withdrawalPhaseName(record.canister.state)
  const kinicTransactions = withdrawalKinicTransactions(record.canister)
  return (
    <article className="grid gap-4 rounded-2xl bg-white p-4 lg:grid-cols-[minmax(6rem,0.7fr)_minmax(7rem,0.8fr)_minmax(7rem,0.8fr)_minmax(9rem,1.3fr)_minmax(7.5rem,1fr)_minmax(6rem,0.7fr)_9rem] lg:items-center">
      <div>
        <MobileLabel>Direction</MobileLabel>
        <Badge tone="info">Base → IC</Badge>
      </div>
      <div>
        <MobileLabel>Base tx</MobileLabel>
        {record.hash ? (
          <BaseTransactionLink transactionHash={record.hash} />
        ) : (
          <span>Transaction link unavailable</span>
        )}
      </div>
      <div>
        <MobileLabel>KINIC tx</MobileLabel>
        {kinicTransactions.length === 0 ? (
          <p className="mt-1 text-xs text-[var(--muted)]">Not sent yet</p>
        ) : (
          <KinicTransactionLink {...kinicTransactions[0]!} />
        )}
      </div>
      <div>
        <MobileLabel>Amount</MobileLabel>
        <p className="text-sm font-bold">
          {record.amountOut === undefined
            ? "Amount unavailable"
            : `${formatTokenAmount(record.amountOut)} KINIC`}
        </p>
      </div>
      <div>
        <MobileLabel>Status</MobileLabel>
        <p className="mb-1 text-xs text-[var(--muted)]">
          {record.baseNeedsReview ? "Base receipt needs rechecking" : "Base: Success"}
        </p>
        <Badge
          tone={
            needsAttention || pendingAttempt?.failure
              ? "warn"
              : record.canister
                ? withdrawalPhaseTone(record.canister.state)
                : "neutral"
          }
        >
          {label}
        </Badge>
        {needsAttention && (
          <p className="mt-1 text-xs font-bold text-[#b42318]">Continue from History when ready.</p>
        )}
        {pendingAttempt?.failure && (
          <p className="mt-1 text-xs font-bold text-[#b42318]">{pendingAttempt.failure.message}</p>
        )}
      </div>
      <div>
        <MobileLabel>Time</MobileLabel>
        <ActivityTime valueNs={item.createdAtNs} />
      </div>
      <div>
        <MobileLabel>Next step</MobileLabel>
        {!record.canister ? (
          pendingAttempt?.failure?.disposition === "terminal" ? (
            <span className="text-sm text-[var(--muted)]">Operator review required</span>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              disabled={record.baseNeedsReview || !writesEnabled || retryingHash === record.hash}
              onClick={() => void onCheckAndNotify(record)}
            >
              {retryingHash === record.hash
                ? "Checking…"
                : pendingAttempt?.failure
                  ? "Retry IC notification"
                  : "Check status"}
            </Button>
          )
        ) : !terminal ? (
          <Button
            size="sm"
            variant="ghost"
            disabled={(!writesEnabled && !feeGuardBlocked(record.canister)) || actioningId === key}
            onClick={() => void onContinue(record)}
          >
            {actioningId === key ? "Continuing…" : "Continue payout"}
          </Button>
        ) : (
          <span className="text-sm text-[var(--muted)]">—</span>
        )}
      </div>
    </article>
  )
}

function BaseTransactionLink({ transactionHash }: { transactionHash: `0x${string}` }) {
  const href = baseTransactionExplorerUrl(deploymentProfile.chainId, transactionHash)
  if (!href)
    return (
      <p className="mt-1 truncate text-xs text-[var(--muted)]">
        Tx {transactionHash.slice(0, 10)}…
      </p>
    )
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      title={transactionHash}
      aria-label={`Open Base transaction ${transactionHash} in explorer`}
      className="mt-1 block truncate text-xs text-blue-600 underline decoration-current/40 underline-offset-2 transition hover:text-blue-800"
    >
      Tx {transactionHash.slice(0, 10)}…
    </a>
  )
}

type KinicTransactionKind = "deposit" | "refund" | "payout"
type KinicTransaction = { kind: KinicTransactionKind; blockIndex: bigint }

export function depositKinicTransactions(record: DepositView): KinicTransaction[] {
  const fundingBlock = record.funding_ledger_block_index[0]
  if (fundingBlock === undefined) return []
  const transactions: KinicTransaction[] = [{ kind: "deposit", blockIndex: fundingBlock }]
  const refundBlock = record.refund[0]?.refund_ledger_block_index[0]
  if (refundBlock !== undefined) transactions.push({ kind: "refund", blockIndex: refundBlock })
  return transactions
}

export function withdrawalKinicTransactions(record?: WithdrawalView): KinicTransaction[] {
  const releaseBlock = record?.release_ledger_block_index[0]
  return releaseBlock === undefined ? [] : [{ kind: "payout", blockIndex: releaseBlock }]
}

export function KinicTransactionLink({ kind, blockIndex }: KinicTransaction) {
  const label = `${kind[0]!.toUpperCase()}${kind.slice(1)}`
  const text = `${label} #${blockIndex.toLocaleString()}`
  const href = kinicTransactionExplorerUrl(deploymentProfile.snsRootCanisterId, blockIndex)
  if (!href) return <p className="mt-1 truncate text-xs text-[var(--muted)]">{text}</p>
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      aria-label={`Open KINIC ${kind} transaction ${blockIndex.toString()} in explorer`}
      className="mt-1 block truncate text-xs text-blue-600 underline decoration-current/40 underline-offset-2 transition hover:text-blue-800"
    >
      {text}
    </a>
  )
}

function MobileLabel({ children }: { children: React.ReactNode }) {
  return (
    <span className="mb-1 block text-[10px] font-bold uppercase tracking-[0.08em] text-[var(--muted)] lg:hidden">
      {children}
    </span>
  )
}

function LoadOlder({ loading, onClick }: { loading: boolean; onClick: () => Promise<void> }) {
  return (
    <div className="pt-2 text-center">
      <Button size="sm" variant="ghost" disabled={loading} onClick={() => void onClick()}>
        {loading ? "Loading…" : "Load older activity"}
      </Button>
    </div>
  )
}

function ActivityTime({ valueNs }: { valueNs: bigint }) {
  const date = new Date(Number(valueNs / 1_000_000n))
  const exact = date.toLocaleString()
  return (
    <time
      className="text-sm text-[var(--muted)]"
      dateTime={date.toISOString()}
      title={exact}
      aria-label={exact}
    >
      {relativeTime(valueNs)}
    </time>
  )
}

export function relativeTime(valueNs: bigint, nowMs = Date.now()): string {
  const deltaSeconds = Number(valueNs / 1_000_000_000n) - Math.floor(nowMs / 1_000)
  const absolute = Math.abs(deltaSeconds)
  const [divisor, unit]: [number, Intl.RelativeTimeFormatUnit] =
    absolute < 60
      ? [1, "second"]
      : absolute < 3_600
        ? [60, "minute"]
        : absolute < 86_400
          ? [3_600, "hour"]
          : [86_400, "day"]
  return new Intl.RelativeTimeFormat(undefined, { numeric: "auto" }).format(
    Math.round(deltaSeconds / divisor),
    unit,
  )
}

type AutomaticProgressInfo = {
  label: string
  deadlineNs: bigint
  running: boolean
  retryAllowed: boolean
}
export function automaticProgressInfo(
  value: [] | [AutomaticProgressView],
  nowNs = BigInt(Date.now()) * 1_000_000n,
): AutomaticProgressInfo | undefined {
  const progress = value[0]
  if (!progress) return undefined
  if ("Scheduled" in progress.state) {
    const deadlineNs = progress.state.Scheduled.next_run_at_ns
    return {
      label: "Completing automatically",
      deadlineNs,
      running: false,
      retryAllowed: nowNs >= deadlineNs + 300_000_000_000n,
    }
  }
  const deadlineNs = progress.state.Running.lease_until_ns
  return {
    label: "Completing automatically",
    deadlineNs,
    running: true,
    retryAllowed: nowNs >= deadlineNs,
  }
}

function AutomaticProgress({ progress }: { progress: AutomaticProgressInfo }) {
  return <p className="mt-1 text-xs font-medium text-[var(--pink)]">{progress.label}</p>
}

function toastSettlement(result: SettlementActionResult) {
  if ("Stopped" in result) {
    toast.error("This transfer needs attention. Try again from History.")
    return
  }
  if ("ReconciliationProgress" in result) {
    toast.info("Status updated. Continue the payout from History for the next step.")
    return
  }
  if ("Deferred" in result) {
    toast.info("This payout needs another explicit step from History.")
    return
  }
  toast.success(`Transfer ${settlementStateName(result.Complete.state).toLowerCase()}.`)
}

function toastWithdrawalNotification(receipt: NotifyWithdrawalReceipt): void {
  const presentation = withdrawalNotificationPresentation(receipt)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
}

function oldestDepositTimestamp(records?: DepositView[]): bigint | undefined {
  return records?.reduce<bigint | undefined>(
    (oldest, record) =>
      oldest === undefined || record.created_at_ns < oldest ? record.created_at_ns : oldest,
    undefined,
  )
}

function feeGuardBlocked(record?: WithdrawalView): boolean {
  return Boolean(
    record?.last_settlement_stop_reason[0] &&
    "LedgerFeeExceedsServiceFee" in record.last_settlement_stop_reason[0],
  )
}

function expectedDepositMint(record: DepositView): ExpectedDepositMint | undefined {
  const authorization = record.mint_authorization[0]
  const quote = record.quote[0]
  if (!authorization || !quote) return undefined
  return {
    depositId: bytesHex(record.deposit_id),
    recipient: bytesHex(authorization.recipient),
    authorizationDigest: bytesHex(authorization.digest),
    grossAmount: record.gross_amount,
    serviceFee: quote.service_fee,
    mintedAmount: quote.net_amount,
  }
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}
