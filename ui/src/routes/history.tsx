import { Principal } from "@dfinity/principal"
import { useQueries, useQuery, useQueryClient } from "@tanstack/react-query"
import { createFileRoute } from "@tanstack/react-router"
import { Clock3, RefreshCcw } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import { toast } from "sonner"
import { hexToBytes, numberToHex } from "viem"
import { useAccount, useChainId } from "wagmi"
import { Alert } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { deploymentProfile } from "@/config/profile"
import { MintAuthorizationAction } from "@/features/bridge/mint-authorization-action"
import { useRuntimeHeartbeat, useRuntimeValidation, useRuntimeWriteReadiness } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import type { AutomaticProgressView, DepositView, SettlementActionResult, WithdrawalView } from "@/generated/bridge.did"
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
import { depositIdsForRefresh, mergeDepositHistoryPage, type DepositHistoryData } from "@/lib/deposit-history"
import {
  depositMintEventMatches,
  depositMintFinalizationStatus,
  DEPOSIT_MINT_SCAN_CHUNKS_PER_MANUAL_REFRESH,
  scanDepositMintLogs,
  type DepositMintFinalizationStatus,
  type DepositMintLogScan,
  type ExpectedDepositMint,
} from "@/lib/deposit-mint-finalization"
import { baseHistoryClients, baseTransactionExplorerUrl, withHistoryClientFailover } from "@/lib/evm/client"
import { finalizedCheckpointMatches } from "@/lib/finalized-checkpoint"
import { sameIcAccount } from "@/lib/ic-history-owner"
import { createBridgeActor } from "@/lib/ic/bridge"
import type { IcWalletAdapter } from "@/lib/ic/wallet"
import { readPendingMint, removePendingConfirmation } from "@/lib/pending-confirmations"
import { refetchRuntimeAttestedWriteReady } from "@/lib/runtime-validation"
import { depositPhaseName, depositPhaseTone, depositReconciliationMessage, isDepositTerminal, isWithdrawalTerminal, settlementStateName, withdrawalPhaseName, withdrawalPhaseTone } from "@/lib/settlement-phase"
import { fetchInBatches, fetchUniqueBlockTimestamps, scanWithdrawalLogs, type FinalizedEventLog, type WithdrawalLogScan } from "@/lib/withdrawal-history"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"

export const Route = createFileRoute("/history")({ component: HistoryPage })

interface WithdrawalEventLog extends FinalizedEventLog {
  args: { withdrawalId: bigint; amount: bigint; maxServiceFee: bigint; chargedServiceFee: bigint; amountOut: bigint }
}

export interface WithdrawalHistoryData extends WithdrawalLogScan<WithdrawalEventLog> {
  items: WithdrawalHistoryItem[]
  olderBoundaryNs: bigint | null
}

type HistorySourceState = "disconnected" | "loading" | "ready" | "unavailable"
type DepositMintScanState = { scan?: DepositMintLogScan; state: "ready" | "checking" | "unavailable" }

function HistoryPage() {
  const { address } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const historyAccount = ic.account ?? ic.historyAccount
  const runtime = useRuntimeValidation(chainId, { enabled: true })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, { enabled: runtime.data?.ready === true, refetchInterval: 45_000 })
  const attestationReadiness = useRuntimeWriteReadiness(runtime.data)
  const heartbeatReadiness = useRuntimeWriteReadiness(heartbeat.data)
  const runtimeReadiness = { ready: attestationReadiness.ready && heartbeatReadiness.ready }
  const queryClient = useQueryClient()
  const [retryingHash, setRetryingHash] = useState<string>()
  const [actioningId, setActioningId] = useState<string>()
  const [loadingOlderWithdrawals, setLoadingOlderWithdrawals] = useState(false)
  const [loadingOlderDeposits, setLoadingOlderDeposits] = useState(false)
  const [pageVisible, setPageVisible] = useState(() => document.visibilityState === "visible")
  const failedHistoryClients = useRef(new Set<number>())
  const manualMintScan = useRef(false)

  const depositQueryKey = ["deposit-history", historyAccount?.owner] as const
  const readDepositHistory = async (mode: "refresh" | "older", previous?: DepositHistoryData): Promise<DepositHistoryData> => {
    const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
    const beforeCursor = mode === "older" ? previous?.nextCursor : undefined
    let result = await actor.list_deposit_ids({ owner: Principal.fromText(historyAccount!.owner), before_cursor: beforeCursor === undefined || beforeCursor === null ? [] : [beforeCursor], limit: 20 })
    if ("Err" in result) throw new Error("Deposit history limit was rejected")
    const latestIds: Array<Uint8Array | number[]> = [...result.Ok.deposit_ids]
    if (mode === "refresh" && previous?.items.length) {
      const known = new Set(previous.items.map((record) => bytesHex(record.deposit_id).toLowerCase()))
      let cursor = result.Ok.next_cursor[0]
      while (cursor !== undefined && !latestIds.some((id) => known.has(bytesHex(id).toLowerCase()))) {
        result = await actor.list_deposit_ids({ owner: Principal.fromText(historyAccount!.owner), before_cursor: [cursor], limit: 20 })
        if ("Err" in result) throw new Error("Deposit history limit was rejected")
        latestIds.push(...result.Ok.deposit_ids)
        cursor = result.Ok.next_cursor[0]
      }
    }
    const ids = mode === "refresh" ? depositIdsForRefresh(previous, latestIds, (record) => !isDepositTerminal(record.state)) : result.Ok.deposit_ids
    const records = await fetchInBatches(ids, 20, (batch) => Promise.all(batch.map((id) => actor.get_deposit(id))))
    return mergeDepositHistoryPage(previous, records.flatMap((record) => record), {
      nextCursor: result.Ok.next_cursor[0] ?? null,
      oldestAvailableCursor: result.Ok.oldest_available_cursor[0] ?? null,
      historyTruncated: result.Ok.history_truncated,
    }, mode)
  }
  const deposits = useQuery({
    queryKey: depositQueryKey,
    enabled: Boolean(historyAccount),
    queryFn: () => readDepositHistory("refresh", queryClient.getQueryData<DepositHistoryData>(depositQueryKey)),
  })
  const mintRecords = (deposits.data?.items ?? []).filter((record) => record.mint_authorization.length > 0)
  const depositMintScans = useQueries({
    queries: mintRecords.map((record) => {
      const authorization = record.mint_authorization[0]!
      const depositId = bytesHex(record.deposit_id)
      const queryKey = [
        "deposit-mint-events",
        deploymentProfile.chainId,
        deploymentProfile.bridgeAddress,
        depositId,
        authorization.finalized_block_number.toString(),
        bytesHex(authorization.digest),
      ] as const
      return {
        queryKey,
        queryFn: async () => withHistoryClientFailover(baseHistoryClients, failedHistoryClients.current, async (client) => {
          const finalized = await client.getBlock({ blockTag: "finalized" })
          if (finalized.number === null || finalized.hash === null) throw new Error("finalized Base block is unavailable")
          let previous = queryClient.getQueryData<DepositMintLogScan>(queryKey)
          if (previous && !await finalizedCheckpointMatches({
            finalizedBlock: finalized.number,
            finalizedBlockHash: finalized.hash,
            checkpointBlock: previous.lastFinalizedBlock,
            checkpointBlockHash: previous.lastFinalizedBlockHash,
            fetchCheckpointBlockHash: async (blockNumber) => (await client.getBlock({ blockNumber })).hash,
          })) previous = undefined
          return scanDepositMintLogs({
            deploymentBlock: authorization.finalized_block_number,
            finalizedBlock: finalized.number,
            finalizedBlockHash: finalized.hash,
            previous,
            maxChunks: manualMintScan.current ? DEPOSIT_MINT_SCAN_CHUNKS_PER_MANUAL_REFRESH : undefined,
            fetchLogs: (fromBlock, toBlock) => client.getContractEvents({
              address: deploymentProfile.bridgeAddress as `0x${string}`,
              abi: bridgeAbi,
              eventName: "DepositMinted",
              args: { depositId },
              fromBlock,
              toBlock,
              strict: true,
            }),
            fetchBlockHash: async (blockNumber) => {
              const block = await client.getBlock({ blockNumber })
              if (block.hash === null) throw new Error("finalized Base checkpoint hash is unavailable")
              return block.hash
            },
          })
        }),
        staleTime: 15_000,
      }
    }),
  })
  const depositMintScanById = new Map(mintRecords.map((record, index) => [
    bytesHex(record.deposit_id),
    {
      scan: depositMintScans[index]?.data,
      state: depositMintScans[index]?.isError
        ? "unavailable" as const
        : depositMintScans[index]?.isFetching ? "checking" as const : "ready" as const,
    },
  ]))
  const depositMintScanError = depositMintScans.some((query) => query.isError)
  const depositMintScanFetching = depositMintScans.some((query) => query.isFetching)

  const withdrawalQueryKey = ["withdraw-history", deploymentProfile.chainId, deploymentProfile.bridgeAddress, address] as const
  const readWithdrawalHistory = async (mode: "refresh" | "older", previous?: WithdrawalHistoryData): Promise<WithdrawalHistoryData> => {
    const evmHistory = await withHistoryClientFailover(baseHistoryClients, failedHistoryClients.current, async (client) => {
      const finalized = await client.getBlock({ blockTag: "finalized" })
      if (finalized.number === null || finalized.hash === null) throw new Error("finalized Base block is unavailable")
      let usablePrevious = previous
      if (previous && !await finalizedCheckpointMatches({
        finalizedBlock: finalized.number,
        finalizedBlockHash: finalized.hash,
        checkpointBlock: previous.lastFinalizedBlock,
        checkpointBlockHash: previous.lastFinalizedBlockHash,
        fetchCheckpointBlockHash: async (blockNumber) => (await client.getBlock({ blockNumber })).hash,
      })) usablePrevious = undefined
      const scan = await scanWithdrawalLogs<WithdrawalEventLog>({
        deploymentBlock: deploymentProfile.deploymentBlock as bigint,
        finalizedBlock: finalized.number,
        finalizedBlockHash: finalized.hash,
        previous: usablePrevious,
        mode,
        fetchLogs: async (fromBlock, toBlock) => client.getContractEvents({ address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, eventName: "WithdrawalCommitted", args: { requester: address }, fromBlock, toBlock, strict: true }),
        fetchBlockHash: async (blockNumber) => (await client.getBlock({ blockNumber })).hash,
      })
      const blockNumbers = scan.logs.map((log) => log.blockNumber).filter((value): value is bigint => value !== null)
      const timestamps = await fetchUniqueBlockTimestamps(blockNumbers, async (blockNumber) => (await client.getBlock({ blockNumber })).timestamp * 1_000_000_000n)
      const olderBoundaryNs = scan.olderCursor === null
        ? null
        : (await client.getBlock({ blockNumber: scan.olderCursor })).timestamp * 1_000_000_000n
      return { scan, timestamps, olderBoundaryNs }
    })
    const { scan, timestamps, olderBoundaryNs } = evmHistory
    const bridge = deploymentProfile.bridgeCanisterId ? await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId) : undefined
    const views = bridge ? await fetchInBatches(scan.logs, 20, async (logs) => {
      const result = await bridge.get_withdrawals(logs.map((log) => hexToBytes(numberToHex(log.args.withdrawalId, { size: 32 }))))
      if ("Err" in result) throw new Error("Canister rejected the withdrawal history batch")
      return result.Ok
    }) : undefined
    const items: WithdrawalHistoryItem[] = scan.logs.map((log, index) => {
      if (log.blockNumber === null || log.logIndex === null || log.transactionHash === null) throw new Error("Finalized withdrawal log metadata is incomplete")
      const createdAtNs = timestamps.get(log.blockNumber)
      if (createdAtNs === undefined) throw new Error("Withdrawal block timestamp is unavailable")
      return {
        id: log.args.withdrawalId,
        amount: log.args.amount,
        amountOut: log.args.amountOut,
        hash: log.transactionHash,
        blockNumber: log.blockNumber,
        logIndex: log.logIndex,
        createdAtNs,
        canister: views?.[index]?.[0],
      }
    })
    return { ...scan, items, olderBoundaryNs }
  }
  const withdrawals = useQuery({
    queryKey: withdrawalQueryKey,
    enabled: Boolean(address),
    queryFn: () => readWithdrawalHistory("refresh", queryClient.getQueryData<WithdrawalHistoryData>(withdrawalQueryKey)),
  })

  const boundaries = useMemo<ActivityBoundaries>(() => ({
    deposit: {
      enabled: Boolean(historyAccount) && !deposits.isError,
      hasMore: deposits.data ? deposits.data.nextCursor !== null : Boolean(historyAccount),
      unseenBeforeNs: deposits.data?.nextCursor === null ? undefined : oldestDepositTimestamp(deposits.data?.items),
    },
    withdrawal: {
      enabled: Boolean(address) && !withdrawals.isError,
      hasMore: withdrawals.data ? withdrawals.data.olderCursor !== null : Boolean(address),
      unseenBeforeNs: withdrawals.data?.olderBoundaryNs ?? undefined,
    },
  }), [address, deposits.data, deposits.isError, historyAccount, withdrawals.data, withdrawals.isError])
  const allItems = useMemo(
    () => mergeActivityItems(deposits.data?.items ?? [], withdrawals.data?.items ?? []),
    [deposits.data?.items, withdrawals.data?.items],
  )
  const visibleItems = useMemo(() => visibleActivityItems(allItems, "all", boundaries), [allItems, boundaries])
  const olderSources = useMemo(() => olderActivitySources("all", boundaries), [boundaries])
  useEffect(() => {
    const onVisibilityChange = () => setPageVisible(document.visibilityState === "visible")
    document.addEventListener("visibilitychange", onVisibilityChange)
    return () => document.removeEventListener("visibilitychange", onVisibilityChange)
  }, [])
  useActivityAutoRefresh(
    activityAutoRefreshEnabled(pageVisible, Boolean(historyAccount), Boolean(address)),
    () => {
      void Promise.all([
        historyAccount ? deposits.refetch() : Promise.resolve(),
        ...depositMintScans.map((scan) => scan.refetch()),
        address ? withdrawals.refetch() : Promise.resolve(),
      ])
    },
  )

  const scanOlderWithdrawals = async () => {
    if (!withdrawals.data || withdrawals.data.olderCursor === null) return
    try {
      setLoadingOlderWithdrawals(true)
      queryClient.setQueryData(withdrawalQueryKey, await readWithdrawalHistory("older", withdrawals.data))
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Older withdrawal history is unavailable")
    } finally {
      setLoadingOlderWithdrawals(false)
    }
  }
  const scanOlderDeposits = async () => {
    if (!deposits.data || deposits.data.nextCursor === null) return
    try {
      setLoadingOlderDeposits(true)
      queryClient.setQueryData(depositQueryKey, await readDepositHistory("older", deposits.data))
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Older deposit history is unavailable")
    } finally {
      setLoadingOlderDeposits(false)
    }
  }
  const loadOlderActivity = async () => {
    await Promise.all(olderSources.map((source) => source === "to-base" ? scanOlderDeposits() : scanOlderWithdrawals()))
  }
  const checkAndNotify = async (item: WithdrawalHistoryItem) => {
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      setRetryingHash(item.hash)
      if (!ic.adapter) throw new Error("Connect the destination IC wallet before retrying")
      closeWalletSession = await ic.adapter.prepare()
      await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      const receipt = await withBrowserLock(`kinic-wallet-prompt:ic:${ic.account?.owner ?? "unknown"}`, () => ic.adapter!.notifyWithdrawal(hexToBytes(item.hash)))
      await removePendingConfirmation({ kind: "withdrawal", transactionHash: item.hash, owner: ic.account?.owner ?? "" })
      toastWithdrawalNotification(receipt)
      await withdrawals.refetch()
    } catch (error) {
      await withdrawals.refetch()
      toast.error(error instanceof Error ? error.message : "Withdrawal notification failed")
    } finally {
      await closeWalletSession?.()
      setRetryingHash(undefined)
    }
  }
  const requestDepositRefund = async (record: DepositView) => {
    const key = bytesHex(record.deposit_id)
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      setActioningId(key)
      if (!ic.adapter) throw new Error("Connect the deposit owner IC wallet")
      if (!sameIcAccount(ic.account, historyAccount)) throw new Error("Connect the IC wallet that owns this deposit")
      closeWalletSession = await ic.adapter.prepare()
      await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      const result = await withBrowserLock(`kinic-wallet-prompt:ic:${ic.account?.owner ?? "unknown"}`, () => ic.adapter!.requestDepositRefund(Uint8Array.from(record.deposit_id)))
      if ("Refunded" in result.state) toast.success("Refund completed.")
      else if ("Minted" in result.state) toast.success("This deposit was already minted on Base.")
      else toast.info("Refund claim recorded. Run the claim again to continue reconciliation.")
      await deposits.refetch()
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "The refund could not be claimed. Try again later.")
    } finally {
      await closeWalletSession?.()
      setActioningId(undefined)
    }
  }
  const continueWithdrawal = async (item: WithdrawalHistoryItem) => {
    const key = item.id?.toString() ?? item.hash
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      setActioningId(key)
      if (!ic.adapter || !item.canister) throw new Error("Connect the withdrawal owner IC wallet")
      closeWalletSession = await ic.adapter.prepare()
      if (!feeGuardBlocked(item.canister)) await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      const result = await withBrowserLock(`kinic-wallet-prompt:ic:${ic.account?.owner ?? "unknown"}`, () => ic.adapter!.continueWithdrawal(Uint8Array.from(item.canister!.withdrawal_id)))
      toastSettlement(result)
      await withdrawals.refetch()
    } catch {
      toast.error("This transfer could not be retried. Try again later.")
    } finally {
      await closeWalletSession?.()
      setActioningId(undefined)
    }
  }
  const refresh = async () => {
    manualMintScan.current = true
    try {
      await Promise.all([
        runtime.refetch(),
        heartbeat.refetch(),
        historyAccount ? deposits.refetch() : Promise.resolve(),
        ...depositMintScans.map((scan) => scan.refetch()),
        address ? withdrawals.refetch() : Promise.resolve(),
      ])
    } finally {
      manualMintScan.current = false
    }
  }
  const refreshing = runtime.isFetching || heartbeat.isFetching || (Boolean(historyAccount) && (deposits.isFetching || depositMintScanFetching)) || (Boolean(address) && withdrawals.isFetching)
  const loadingInitial = Boolean(historyAccount && !deposits.data && deposits.isFetching) || Boolean(address && !withdrawals.data && withdrawals.isFetching)
  const loadingOlder = loadingOlderDeposits || loadingOlderWithdrawals
  const writesEnabled = runtimeReadiness.ready && !runtime.isFetching
  const sourceStates = {
    deposit: !historyAccount ? "disconnected" : deposits.isError ? "unavailable" : !deposits.data ? "loading" : "ready",
    withdrawal: !address ? "disconnected" : withdrawals.isError ? "unavailable" : !withdrawals.data ? "loading" : "ready",
  } satisfies Record<"deposit" | "withdrawal", HistorySourceState>

  return <div className="route-enter mx-auto max-w-5xl pt-8 md:pt-12">
    <header className="mb-8 flex items-end justify-between gap-4">
      <div>
        <h1 className="font-display text-[42px] leading-[1.1]">Bridge history</h1>
      </div>
      <Button variant="ghost" disabled={refreshing} onClick={() => void refresh()}>
        <RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />
        {refreshing ? "Refreshing…" : "Refresh"}
      </Button>
    </header>

    {depositMintScanError && <Alert className="mb-5" tone="warning">
      Finalized Base mint history is unavailable. New Base mint submissions are paused; refund claims remain available.
    </Alert>}

    <section aria-label="Bridge activity" className="min-h-80 rounded-[20px] bg-[var(--panel)] p-4 sm:p-6">
      {!historyAccount && !address
        ? <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Connect a wallet" message="Connect an IC or EVM wallet to load your bridge activity." />
        : loadingInitial && !allItems.length
          ? <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading activity" message="This may take a moment." />
          : <ActivityList
              items={visibleItems}
              sourceStates={sourceStates}
              writesEnabled={writesEnabled}
              actioningId={actioningId}
              retryingHash={retryingHash}
              historyTruncated={Boolean(deposits.data?.historyTruncated)}
              depositMintScans={depositMintScanById}
              hasOlder={olderSources.length > 0}
              loadingOlder={loadingOlder}
              onRequestDepositRefund={requestDepositRefund}
              onCheckAndNotify={checkAndNotify}
              onContinueWithdrawal={continueWithdrawal}
              onLoadOlder={loadOlderActivity}
              onRefresh={() => void refresh()}
            />}
    </section>
  </div>
}

function Empty({ icon, title, message }: { icon: React.ReactNode; title: string; message: string }) {
  return <div className="grid min-h-64 place-items-center text-center"><div>{icon}<p className="mt-3 font-bold text-black">{title}</p><p className="mt-1 text-sm text-[var(--muted)]">{message}</p></div></div>
}

function ActivityList({
  items,
  sourceStates,
  writesEnabled,
  actioningId,
  retryingHash,
  historyTruncated,
  depositMintScans,
  hasOlder,
  loadingOlder,
  onRequestDepositRefund,
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
  depositMintScans: Map<string, DepositMintScanState>
  hasOlder: boolean
  loadingOlder: boolean
  onRequestDepositRefund: (record: DepositView) => Promise<void>
  onCheckAndNotify: (item: WithdrawalHistoryItem) => Promise<void>
  onContinueWithdrawal: (item: WithdrawalHistoryItem) => Promise<void>
  onLoadOlder: () => Promise<void>
  onRefresh: () => void
}) {
  if (!items.length) {
    const relevantStates = [sourceStates.deposit, sourceStates.withdrawal]
    if (relevantStates.includes("unavailable")) {
      return <HistoryUnavailable sourceStates={sourceStates} onRefresh={onRefresh} />
    }
    if (relevantStates.includes("loading")) {
      return <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading activity" message="This may take a moment." />
    }
    if (relevantStates.every((state) => state === "disconnected")) {
      return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Connect a wallet" message="Connect an IC or EVM wallet to load your bridge activity." />
    }
    const emptyMessage = "Your bridge transfers will appear here."
    return <div>
      <Empty
        icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />}
        title={relevantStates.includes("disconnected") ? "No connected-wallet activity" : "No activity yet"}
        message={relevantStates.includes("disconnected") ? `${emptyMessage} Connect the other wallet to include its direction.` : emptyMessage}
      />
      {hasOlder && <LoadOlder loading={loadingOlder} onClick={onLoadOlder} />}
    </div>
  }
  const unavailable = unavailableSources(sourceStates)
  return <div className="space-y-3">
    {unavailable.length > 0 && <HistoryUnavailable sourceStates={sourceStates} onRefresh={onRefresh} compact />}
    {historyTruncated && <p className="rounded-xl bg-[#fff3e4] px-3 py-2 text-xs font-medium text-[#8a4b08]">Some older IC → Base activity is no longer available.</p>}
    <div className="hidden grid-cols-[minmax(6.5rem,0.7fr)_minmax(7rem,0.8fr)_minmax(10.5rem,1.6fr)_minmax(8rem,1fr)_minmax(6.5rem,0.8fr)_10rem] gap-4 px-4 pb-1 text-xs font-bold uppercase tracking-[0.08em] text-[var(--muted)] lg:grid">
      <span>Direction</span><span>Tx ID</span><span>Amount</span><span>Status</span><span>Time</span><span>Action</span>
    </div>
    {items.map((item) => {
      const mintScan = item.direction === "to-base" ? depositMintScans.get(bytesHex(item.deposit.deposit_id)) : undefined
      return item.direction === "to-base"
      ? <DepositActivityRow key={item.key} item={item} mintFinalization={depositMintStatus(item.deposit, mintScan?.scan, mintScan?.state ?? "ready")} mintTransactionHash={depositMintTransactionHash(item.deposit, mintScan?.scan)} mintScan={mintScan?.scan} writesEnabled={writesEnabled} actioningId={actioningId} onRequestRefund={onRequestDepositRefund} />
      : <WithdrawalActivityRow key={item.key} item={item} writesEnabled={writesEnabled} actioningId={actioningId} retryingHash={retryingHash} onCheckAndNotify={onCheckAndNotify} onContinue={onContinueWithdrawal} />
    })}
    {hasOlder && <LoadOlder loading={loadingOlder} onClick={onLoadOlder} />}
  </div>
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
  const direction = unavailable.length > 1 ? "Bridge" : unavailable[0] === "deposit" ? "IC → Base" : "Base → IC"
  if (compact) {
    return <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-white px-4 py-3 text-sm text-[var(--muted)]">
      <span>{direction} activity could not be loaded.</span>
      <Button size="sm" variant="ghost" onClick={onRefresh}>Refresh</Button>
    </div>
  }
  return <div className="grid min-h-64 place-items-center text-center">
    <div>
      <RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />
      <p className="mt-3 font-bold text-black">Activity could not be loaded</p>
      <p className="mt-1 text-sm text-[var(--muted)]">{direction} activity is temporarily unavailable.</p>
      <Button className="mt-4" size="sm" variant="ghost" onClick={onRefresh}>Refresh</Button>
    </div>
  </div>
}

function DepositActivityRow({ item, mintFinalization, mintTransactionHash, mintScan, writesEnabled, actioningId, onRequestRefund }: {
  item: Extract<ActivityItem, { direction: "to-base" }>
  mintFinalization: DepositMintFinalizationStatus
  mintTransactionHash?: `0x${string}`
  mintScan?: DepositMintLogScan
  writesEnabled: boolean
  actioningId?: string
  onRequestRefund: (record: DepositView) => Promise<void>
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
  const quote = record.quote[0]
  const availableRefund = record.available_refund_amount[0]
  const reconciliationMessage = depositReconciliationMessage(record.state, record.last_settlement_stop_reason[0])
  const mintedOnBase = mintFinalization === "minted"
  const mintBlockedReason = mintFinalization === "unavailable"
    ? "Finalized Base mint history is unavailable. Refresh before minting."
    : mintFinalization === "checking"
      ? `Checking finalized Base mint history before minting${mintScan?.olderCursor === null ? "" : ` (${mintScan?.olderCursor === undefined ? "starting" : `${mintScan.olderCursor.toString()} is the next older block`})`}.`
      : undefined
  const amountText = refund
    ? `${formatTokenAmount(refund.amount)} KINIC returned to IC`
    : quote
      ? `${formatTokenAmount(quote.net_amount)} KINIC on Base`
      : `${formatTokenAmount(record.gross_amount)} KINIC awaiting quote`
  return <article className="grid gap-4 rounded-2xl bg-white p-4 lg:grid-cols-[minmax(6.5rem,0.7fr)_minmax(7rem,0.8fr)_minmax(10.5rem,1.6fr)_minmax(8rem,1fr)_minmax(6.5rem,0.8fr)_10rem] lg:items-center">
    <div><MobileLabel>Direction</MobileLabel><Badge tone="info">IC → Base</Badge></div>
    <div><MobileLabel>Tx ID</MobileLabel>{transactionHash
      ? <BaseTransactionLink transactionHash={transactionHash} />
      : <p className="mt-1 text-xs text-[var(--muted)]">Base transaction not submitted</p>}</div>
    <div><MobileLabel>Amount</MobileLabel><p className="text-sm font-bold">{amountText}</p>{availableRefund !== undefined && <p className="mt-1 text-xs text-[var(--muted)]">Available after non-refundable fees: {formatTokenAmount(availableRefund)} KINIC</p>}{refund && <p className="mt-1 text-xs text-[var(--muted)]">Non-refundable refund Ledger fee: {formatTokenAmount(refund.ledger_fee)} KINIC</p>}</div>
    <div><MobileLabel>Status</MobileLabel><Badge tone={mintedOnBase ? "good" : depositPhaseTone(record.state)}>{mintedOnBase ? "Minted on Base (finalized)" : depositPhaseName(record.state)}</Badge>{!mintedOnBase && progress && <AutomaticProgress progress={progress} />}{!mintedOnBase && refund && "ReconciliationRequired" in refund.status && <p className="mt-1 text-xs font-bold text-[#b42318]">Ledger result is uncertain — requesting again checks the same transfer.</p>}{!mintedOnBase && reconciliationMessage && <p className={`mt-1 text-xs font-bold ${record.last_settlement_stop_reason[0] ? "text-[#b42318]" : "text-[var(--muted)]"}`}>{reconciliationMessage}</p>}</div>
    <div><MobileLabel>Time</MobileLabel><ActivityTime valueNs={item.createdAtNs} /></div>
    <div className="lg:text-right"><MobileLabel>Action</MobileLabel>{mintedOnBase
      ? <span className="text-sm text-[var(--muted)]">—</span>
      : "AuthorizationAvailable" in record.state
        ? <MintAuthorizationAction record={record} compact mintBlockedReason={mintBlockedReason} onRequestRefund={writesEnabled ? () => void onRequestRefund(record) : undefined} claimingRefund={actioningId === key} />
      : "RefundAvailable" in record.state || ("RefundProcessing" in record.state && Boolean(record.last_settlement_stop_reason[0] || (refund && "ReconciliationRequired" in refund.status)))
        ? <Button size="sm" variant="ghost" disabled={!writesEnabled || actioningId === key} onClick={() => void onRequestRefund(record)}>{actioningId === key ? "Requesting…" : "Request refund"}</Button>
        : "RefundProcessing" in record.state
          ? <span className="text-sm text-[var(--muted)]">Refunding…</span>
        : terminal ? <span className="text-sm text-[var(--muted)]">—</span> : <span className="text-sm text-[var(--muted)]">Automatic processing</span>}</div>
  </article>
}

function WithdrawalActivityRow({ item, writesEnabled, actioningId, retryingHash, onCheckAndNotify, onContinue }: {
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
  const progress = record.canister ? automaticProgressInfo(record.canister.automatic_progress) : undefined
  const needsAttention = Boolean(record.canister?.last_settlement_stop_reason[0]) || record.canister?.state !== undefined && "ReconciliationHold" in record.canister.state
  const label = !record.canister ? "Committed" : needsAttention ? "Needs attention" : withdrawalPhaseName(record.canister.state)
  return <article className="grid gap-4 rounded-2xl bg-white p-4 lg:grid-cols-[minmax(6.5rem,0.7fr)_minmax(7rem,0.8fr)_minmax(10.5rem,1.6fr)_minmax(8rem,1fr)_minmax(6.5rem,0.8fr)_10rem] lg:items-center">
    <div><MobileLabel>Direction</MobileLabel><Badge tone="info">Base → IC</Badge></div>
    <div><MobileLabel>Tx ID</MobileLabel><BaseTransactionLink transactionHash={record.hash} /></div>
    <div><MobileLabel>Amount</MobileLabel><p className="text-sm font-bold">{record.amountOut === undefined ? "Amount unavailable" : `${formatTokenAmount(record.amountOut)} KINIC to IC`}</p></div>
    <div><MobileLabel>Status</MobileLabel><Badge tone={needsAttention ? "warn" : record.canister ? withdrawalPhaseTone(record.canister.state) : "neutral"}>{label}</Badge>{progress && <AutomaticProgress progress={progress} />}{needsAttention && <p className="mt-1 text-xs font-bold text-[#b42318]">Needs attention</p>}</div>
    <div><MobileLabel>Time</MobileLabel><ActivityTime valueNs={item.createdAtNs} /></div>
    <div className="lg:text-right"><MobileLabel>Action</MobileLabel>{!record.canister ? <Button size="sm" variant="ghost" disabled={!writesEnabled || retryingHash === record.hash} onClick={() => void onCheckAndNotify(record)}>{retryingHash === record.hash ? "Checking…" : "Check status"}</Button> : !terminal && (!progress || progress.retryAllowed) ? <Button size="sm" variant="ghost" disabled={(!writesEnabled && !feeGuardBlocked(record.canister)) || actioningId === key} onClick={() => void onContinue(record)}>{actioningId === key ? "Retrying…" : "Retry"}</Button> : <span className="text-sm text-[var(--muted)]">—</span>}</div>
  </article>
}

function BaseTransactionLink({ transactionHash }: { transactionHash: `0x${string}` }) {
  const href = baseTransactionExplorerUrl(deploymentProfile.chainId, transactionHash)
  if (!href) return <p className="mt-1 truncate text-xs text-[var(--muted)]">Tx {transactionHash.slice(0, 10)}…</p>
  return <a
    href={href}
    target="_blank"
    rel="noreferrer"
    title={transactionHash}
    aria-label={`Open Base transaction ${transactionHash} in explorer`}
    className="mt-1 block truncate text-xs text-[var(--muted)] underline decoration-current/40 underline-offset-2 transition hover:text-[var(--pink)]"
  >
    Tx {transactionHash.slice(0, 10)}…
  </a>
}

function MobileLabel({ children }: { children: React.ReactNode }) {
  return <span className="mb-1 block text-[10px] font-bold uppercase tracking-[0.08em] text-[var(--muted)] lg:hidden">{children}</span>
}

function LoadOlder({ loading, onClick }: { loading: boolean; onClick: () => Promise<void> }) {
  return <div className="pt-2 text-center"><Button size="sm" variant="ghost" disabled={loading} onClick={() => void onClick()}>{loading ? "Loading…" : "Load older activity"}</Button></div>
}

function ActivityTime({ valueNs }: { valueNs: bigint }) {
  const date = new Date(Number(valueNs / 1_000_000n))
  const exact = date.toLocaleString()
  return <time className="text-sm text-[var(--muted)]" dateTime={date.toISOString()} title={exact} aria-label={exact}>{relativeTime(valueNs)}</time>
}

export function relativeTime(valueNs: bigint, nowMs = Date.now()): string {
  const deltaSeconds = Number(valueNs / 1_000_000_000n) - Math.floor(nowMs / 1_000)
  const absolute = Math.abs(deltaSeconds)
  const [divisor, unit]: [number, Intl.RelativeTimeFormatUnit] = absolute < 60
    ? [1, "second"]
    : absolute < 3_600
      ? [60, "minute"]
      : absolute < 86_400
        ? [3_600, "hour"]
        : [86_400, "day"]
  return new Intl.RelativeTimeFormat(undefined, { numeric: "auto" }).format(Math.round(deltaSeconds / divisor), unit)
}

type AutomaticProgressInfo = { label: string; deadlineNs: bigint; running: boolean; retryAllowed: boolean }
export function automaticProgressInfo(value: [] | [AutomaticProgressView], nowNs = BigInt(Date.now()) * 1_000_000n): AutomaticProgressInfo | undefined {
  const progress = value[0]
  if (!progress) return undefined
  if ("Scheduled" in progress.state) {
    const deadlineNs = progress.state.Scheduled.next_run_at_ns
    return { label: "Completing automatically", deadlineNs, running: false, retryAllowed: nowNs >= deadlineNs + 300_000_000_000n }
  }
  const deadlineNs = progress.state.Running.lease_until_ns
  return { label: "Completing automatically", deadlineNs, running: true, retryAllowed: nowNs >= deadlineNs }
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
    toast.info("Status updated. Processing will continue automatically.")
    return
  }
  if ("Deferred" in result) {
    toast.info("Status updated. Processing will resume automatically.")
    return
  }
  toast.success(`Transfer ${settlementStateName(result.Complete.state).toLowerCase()}.`)
}

function toastWithdrawalNotification(receipt: Awaited<ReturnType<IcWalletAdapter["notifyWithdrawal"]>>): void {
  const presentation = withdrawalNotificationPresentation(receipt)
  if (presentation.tone === "success") toast.success(presentation.message)
  else if (presentation.tone === "warning") toast.warning(presentation.message)
  else toast.info(presentation.message)
}

function oldestDepositTimestamp(records?: DepositView[]): bigint | undefined {
  return records?.reduce<bigint | undefined>((oldest, record) => oldest === undefined || record.created_at_ns < oldest ? record.created_at_ns : oldest, undefined)
}

function feeGuardBlocked(record?: WithdrawalView): boolean {
  return record?.last_settlement_stop_reason[0] === "LedgerFeeExceedsServiceFee"
}

function depositMintStatus(
  record: DepositView,
  scan: DepositMintLogScan | undefined,
  queryState: "ready" | "checking" | "unavailable",
): DepositMintFinalizationStatus {
  const expected = expectedDepositMint(record)
  const authorization = record.mint_authorization[0]
  if (!authorization || !expected) return "absent"
  return depositMintFinalizationStatus({
    expected,
    authorizationBlock: authorization.finalized_block_number,
    scan,
    queryState,
  })
}

function depositMintTransactionHash(record: DepositView, scan?: DepositMintLogScan): `0x${string}` | undefined {
  const expected = expectedDepositMint(record)
  if (!expected) return undefined
  return scan?.logs.find((log) => depositMintEventMatches(expected, log.args))?.transactionHash ?? undefined
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
