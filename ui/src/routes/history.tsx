import { Principal } from "@dfinity/principal"
import { useQuery, useQueryClient } from "@tanstack/react-query"
import { createFileRoute } from "@tanstack/react-router"
import { Clock3, RefreshCcw } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import { toast } from "sonner"
import { hexToBytes, numberToHex } from "viem"
import { useAccount, useChainId } from "wagmi"
import { Alert } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { deploymentProfile } from "@/config/profile"
import { MintAuthorizationAction } from "@/features/bridge/mint-authorization-action"
import { useRuntimeValidation, useRuntimeWriteReadiness } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import type { AutomaticProgressView, DepositView, SettlementActionResult, WithdrawalView } from "@/generated/bridge.did"
import {
  activityAutoRefreshEnabled,
  mergeActivityItems,
  olderActivitySources,
  visibleActivityItems,
  type ActivityBoundaries,
  type ActivityFilter,
  type ActivityItem,
  type WithdrawalHistoryItem,
} from "@/lib/activity-history"
import { formatTokenAmount } from "@/lib/amounts"
import { withBrowserLock } from "@/lib/browser-lock"
import { depositIdsForRefresh, mergeDepositHistoryPage, type DepositHistoryData } from "@/lib/deposit-history"
import { basePublicClient } from "@/lib/evm/client"
import { createBridgeActor } from "@/lib/ic/bridge"
import type { IcWalletAdapter } from "@/lib/ic/wallet"
import { removePendingConfirmation } from "@/lib/pending-confirmations"
import { refetchRuntimeWriteReady } from "@/lib/runtime-validation"
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

function HistoryPage() {
  const { address } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const runtime = useRuntimeValidation(chainId)
  const runtimeReadiness = useRuntimeWriteReadiness(runtime.data)
  const queryClient = useQueryClient()
  const [filter, setFilter] = useState<ActivityFilter>("all")
  const [retryingHash, setRetryingHash] = useState<string>()
  const [actioningId, setActioningId] = useState<string>()
  const [loadingOlderWithdrawals, setLoadingOlderWithdrawals] = useState(false)
  const [loadingOlderDeposits, setLoadingOlderDeposits] = useState(false)
  const [pageVisible, setPageVisible] = useState(() => document.visibilityState === "visible")

  const depositQueryKey = ["deposit-history", ic.account?.owner] as const
  const readDepositHistory = async (mode: "refresh" | "older", previous?: DepositHistoryData): Promise<DepositHistoryData> => {
    const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
    const beforeCursor = mode === "older" ? previous?.nextCursor : undefined
    let result = await actor.list_deposit_ids({ owner: Principal.fromText(ic.account!.owner), before_cursor: beforeCursor === undefined || beforeCursor === null ? [] : [beforeCursor], limit: 20 })
    if ("Err" in result) throw new Error("Deposit history limit was rejected")
    const latestIds: Array<Uint8Array | number[]> = [...result.Ok.deposit_ids]
    if (mode === "refresh" && previous?.items.length) {
      const known = new Set(previous.items.map((record) => bytesHex(record.deposit_id).toLowerCase()))
      let cursor = result.Ok.next_cursor[0]
      while (cursor !== undefined && !latestIds.some((id) => known.has(bytesHex(id).toLowerCase()))) {
        result = await actor.list_deposit_ids({ owner: Principal.fromText(ic.account!.owner), before_cursor: [cursor], limit: 20 })
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
    enabled: Boolean(ic.account),
    queryFn: () => readDepositHistory("refresh", queryClient.getQueryData<DepositHistoryData>(depositQueryKey)),
  })

  const withdrawalQueryKey = ["withdraw-history", deploymentProfile.chainId, deploymentProfile.bridgeAddress, address] as const
  const blockTimestampNs = async (blockNumber: bigint): Promise<bigint> => queryClient.fetchQuery({
    queryKey: ["base-block-timestamp", deploymentProfile.chainId, blockNumber.toString()],
    staleTime: Number.POSITIVE_INFINITY,
    queryFn: async () => (await basePublicClient.getBlock({ blockNumber })).timestamp * 1_000_000_000n,
  })
  const readWithdrawalHistory = async (mode: "refresh" | "older", previous?: WithdrawalHistoryData): Promise<WithdrawalHistoryData> => {
    const client = basePublicClient
    const finalized = await client.getBlock({ blockTag: "finalized" })
    if (finalized.number === null || finalized.hash === null) throw new Error("finalized Base block is unavailable")
    let usablePrevious = previous
    if (previous) {
      const checkpoint = await client.getBlock({ blockNumber: previous.lastFinalizedBlock })
      if (finalized.number < previous.lastFinalizedBlock || checkpoint.hash !== previous.lastFinalizedBlockHash) usablePrevious = undefined
    }
    const scan = await scanWithdrawalLogs<WithdrawalEventLog>({
      deploymentBlock: deploymentProfile.deploymentBlock as bigint,
      finalizedBlock: finalized.number,
      finalizedBlockHash: finalized.hash,
      previous: usablePrevious,
      mode,
      fetchLogs: async (fromBlock, toBlock) => client.getContractEvents({ address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, eventName: "WithdrawalCommitted", args: { requester: address }, fromBlock, toBlock, strict: true }),
      fetchBlockHash: async (blockNumber) => (await client.getBlock({ blockNumber })).hash,
    })
    const bridge = deploymentProfile.bridgeCanisterId ? await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId) : undefined
    const views = bridge ? await fetchInBatches(scan.logs, 20, async (logs) => {
      const result = await bridge.get_withdrawals(logs.map((log) => hexToBytes(numberToHex(log.args.withdrawalId, { size: 32 }))))
      if ("Err" in result) throw new Error("Canister rejected the withdrawal history batch")
      return result.Ok
    }) : undefined
    const blockNumbers = scan.logs.map((log) => log.blockNumber).filter((value): value is bigint => value !== null)
    const timestamps = await fetchUniqueBlockTimestamps(blockNumbers, blockTimestampNs)
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
    const olderBoundaryNs = scan.olderCursor === null ? null : await blockTimestampNs(scan.olderCursor)
    return { ...scan, items, olderBoundaryNs }
  }
  const withdrawals = useQuery({
    queryKey: withdrawalQueryKey,
    enabled: Boolean(address),
    queryFn: () => readWithdrawalHistory("refresh", queryClient.getQueryData<WithdrawalHistoryData>(withdrawalQueryKey)),
  })

  const boundaries = useMemo<ActivityBoundaries>(() => ({
    deposit: {
      enabled: Boolean(ic.account) && !deposits.isError,
      hasMore: deposits.data ? deposits.data.nextCursor !== null : Boolean(ic.account),
      unseenBeforeNs: deposits.data?.nextCursor === null ? undefined : oldestDepositTimestamp(deposits.data?.items),
    },
    withdrawal: {
      enabled: Boolean(address) && !withdrawals.isError,
      hasMore: withdrawals.data ? withdrawals.data.olderCursor !== null : Boolean(address),
      unseenBeforeNs: withdrawals.data?.olderBoundaryNs ?? undefined,
    },
  }), [address, deposits.data, deposits.isError, ic.account, withdrawals.data, withdrawals.isError])
  const allItems = useMemo(
    () => mergeActivityItems(deposits.data?.items ?? [], withdrawals.data?.items ?? []),
    [deposits.data?.items, withdrawals.data?.items],
  )
  const visibleItems = useMemo(() => visibleActivityItems(allItems, filter, boundaries), [allItems, boundaries, filter])
  const olderSources = useMemo(() => olderActivitySources(filter, boundaries), [boundaries, filter])
  useEffect(() => {
    const onVisibilityChange = () => setPageVisible(document.visibilityState === "visible")
    document.addEventListener("visibilitychange", onVisibilityChange)
    return () => document.removeEventListener("visibilitychange", onVisibilityChange)
  }, [])
  useEffect(() => {
    if (!activityAutoRefreshEnabled(pageVisible, Boolean(ic.account), Boolean(address))) return
    const timer = window.setInterval(() => {
      void Promise.all([
        ic.account ? deposits.refetch() : Promise.resolve(),
        address ? withdrawals.refetch() : Promise.resolve(),
      ])
    }, 60_000)
    return () => window.clearInterval(timer)
  }, [address, deposits, ic.account, pageVisible, withdrawals])

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
      await refetchRuntimeWriteReady(() => runtime.refetch())
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
  const continueDeposit = async (record: DepositView) => {
    const key = bytesHex(record.deposit_id)
    let closeWalletSession: (() => Promise<void>) | undefined
    try {
      setActioningId(key)
      if (!ic.adapter) throw new Error("Connect the deposit owner IC wallet")
      closeWalletSession = await ic.adapter.prepare()
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const result = await withBrowserLock(`kinic-wallet-prompt:ic:${ic.account?.owner ?? "unknown"}`, () => ic.adapter!.continueDeposit(Uint8Array.from(record.deposit_id)))
      toastSettlement(result)
      await deposits.refetch()
    } catch {
      toast.error("This transfer could not be retried. Try again later.")
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
      if (!feeGuardBlocked(item.canister)) await refetchRuntimeWriteReady(() => runtime.refetch())
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
  const refresh = () => {
    void Promise.all([
      runtime.refetch(),
      ic.account ? deposits.refetch() : Promise.resolve(),
      address ? withdrawals.refetch() : Promise.resolve(),
    ])
  }
  const refreshing = runtime.isFetching || (Boolean(ic.account) && deposits.isFetching) || (Boolean(address) && withdrawals.isFetching)
  const loadingInitial = Boolean(ic.account && !deposits.data && deposits.isFetching) || Boolean(address && !withdrawals.data && withdrawals.isFetching)
  const loadingOlder = loadingOlderDeposits || loadingOlderWithdrawals
  const writesEnabled = runtimeReadiness.ready && !runtime.isFetching
  const runtimeReason = runtime.isFetching
    ? "Checking availability…"
    : runtime.data
      ? "Bridge actions are temporarily unavailable. Try Refresh."
      : "Refresh before continuing."

  return <div className="route-enter mx-auto max-w-5xl pt-8 md:pt-12">
    <header className="mb-8 flex items-end justify-between gap-4">
      <div>
        <p className="text-sm font-medium text-[var(--pink)]">Activity</p>
        <h1 className="font-display mt-2 text-[42px] leading-[1.1]">Bridge history</h1>
        <p className="mt-3 max-w-xl text-base leading-6 text-[var(--muted)]">Your bridge activity updates automatically.</p>
      </div>
      <Button variant="ghost" disabled={refreshing} onClick={refresh}>
        <RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />
        {refreshing ? "Refreshing…" : "Refresh"}
      </Button>
    </header>

    {!writesEnabled && <Alert className="mb-5" tone="warning"><strong>Actions unavailable.</strong><span className="ml-1">{runtimeReason}</span></Alert>}
    {Boolean(ic.account) !== Boolean(address) && <Alert className="mb-5">
      {ic.account ? "Connect an EVM wallet to include Base → IC activity." : "Connect an IC wallet to include IC → Base activity."}
    </Alert>}
    {(deposits.isError || withdrawals.isError) && <Alert className="mb-5" tone="warning">
      Some activity is unavailable. The visible order may be incomplete. Choose Refresh to try again.
    </Alert>}

    <div className="mb-5 inline-flex rounded-2xl bg-[var(--panel)] p-1" role="group" aria-label="Filter activity">
      <FilterButton active={filter === "all"} onClick={() => setFilter("all")}>All</FilterButton>
      <FilterButton active={filter === "to-base"} onClick={() => setFilter("to-base")}>To Base</FilterButton>
      <FilterButton active={filter === "to-ic"} onClick={() => setFilter("to-ic")}>To IC</FilterButton>
    </div>

    <section aria-label="Bridge activity" className="min-h-80 rounded-[20px] bg-[var(--panel)] p-4 sm:p-6">
      {!ic.account && !address
        ? <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Connect a wallet" message="Connect an IC or EVM wallet to load your bridge activity." />
        : loadingInitial && !allItems.length
          ? <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading activity" message="This may take a moment." />
          : <ActivityList
              items={visibleItems}
              filter={filter}
              writesEnabled={writesEnabled}
              actioningId={actioningId}
              retryingHash={retryingHash}
              historyTruncated={Boolean(deposits.data?.historyTruncated)}
              hasOlder={olderSources.length > 0}
              loadingOlder={loadingOlder}
              onContinueDeposit={continueDeposit}
              onCheckAndNotify={checkAndNotify}
              onContinueWithdrawal={continueWithdrawal}
              onLoadOlder={loadOlderActivity}
            />}
    </section>
  </div>
}

function FilterButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return <button type="button" aria-pressed={active} className={`rounded-xl px-4 py-2.5 text-sm font-bold transition ${active ? "bg-black text-white" : "text-[var(--muted)] hover:text-[var(--pink)]"}`} onClick={onClick}>{children}</button>
}

function Empty({ icon, title, message }: { icon: React.ReactNode; title: string; message: string }) {
  return <div className="grid min-h-64 place-items-center text-center"><div>{icon}<p className="mt-3 font-bold text-black">{title}</p><p className="mt-1 text-sm text-[var(--muted)]">{message}</p></div></div>
}

function ActivityList({
  items,
  filter,
  writesEnabled,
  actioningId,
  retryingHash,
  historyTruncated,
  hasOlder,
  loadingOlder,
  onContinueDeposit,
  onCheckAndNotify,
  onContinueWithdrawal,
  onLoadOlder,
}: {
  items: ActivityItem[]
  filter: ActivityFilter
  writesEnabled: boolean
  actioningId?: string
  retryingHash?: string
  historyTruncated: boolean
  hasOlder: boolean
  loadingOlder: boolean
  onContinueDeposit: (record: DepositView) => Promise<void>
  onCheckAndNotify: (item: WithdrawalHistoryItem) => Promise<void>
  onContinueWithdrawal: (item: WithdrawalHistoryItem) => Promise<void>
  onLoadOlder: () => Promise<void>
}) {
  if (!items.length) {
    const filtered = filter === "to-base" ? "No IC → Base activity in the loaded history." : filter === "to-ic" ? "No Base → IC activity in the loaded history." : "Your bridge transfers will appear here."
    return <div>
      <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title={filter === "all" ? "No activity yet" : "No matching activity"} message={filtered} />
      {hasOlder && <LoadOlder loading={loadingOlder} onClick={onLoadOlder} />}
    </div>
  }
  return <div className="space-y-3">
    {historyTruncated && <p className="rounded-xl bg-[#fff3e4] px-3 py-2 text-xs font-medium text-[#8a4b08]">Some older IC → Base activity is no longer available.</p>}
    <div className="hidden grid-cols-[minmax(7rem,0.8fr)_minmax(12rem,1.8fr)_minmax(8rem,1fr)_minmax(7rem,0.9fr)_10.5rem] gap-4 px-4 pb-1 text-xs font-bold uppercase tracking-[0.08em] text-[var(--muted)] md:grid">
      <span>Direction</span><span>Amount</span><span>Status</span><span>Time</span><span>Action</span>
    </div>
    {items.map((item) => item.direction === "to-base"
      ? <DepositActivityRow key={item.key} item={item} writesEnabled={writesEnabled} actioningId={actioningId} onContinue={onContinueDeposit} />
      : <WithdrawalActivityRow key={item.key} item={item} writesEnabled={writesEnabled} actioningId={actioningId} retryingHash={retryingHash} onCheckAndNotify={onCheckAndNotify} onContinue={onContinueWithdrawal} />)}
    {hasOlder && <LoadOlder loading={loadingOlder} onClick={onLoadOlder} />}
  </div>
}

function DepositActivityRow({ item, writesEnabled, actioningId, onContinue }: {
  item: Extract<ActivityItem, { direction: "to-base" }>
  writesEnabled: boolean
  actioningId?: string
  onContinue: (record: DepositView) => Promise<void>
}) {
  const record = item.deposit
  const key = bytesHex(record.deposit_id)
  const terminal = isDepositTerminal(record.state)
  const progress = automaticProgressInfo(record.automatic_progress)
  const refund = record.refund[0]
  const quote = record.quote[0]
  const reconciliationMessage = depositReconciliationMessage(record.state, record.last_settlement_stop_reason[0])
  const amountText = refund
    ? `${formatTokenAmount(refund.amount)} KINIC returned to IC`
    : quote
      ? `${formatTokenAmount(quote.net_amount)} KINIC on Base`
      : `${formatTokenAmount(record.gross_amount)} KINIC awaiting quote`
  return <article className="grid gap-4 rounded-2xl bg-white p-4 md:grid-cols-[minmax(7rem,0.8fr)_minmax(12rem,1.8fr)_minmax(8rem,1fr)_minmax(7rem,0.9fr)_10.5rem] md:items-center">
    <div><MobileLabel>Direction</MobileLabel><Badge tone="info">IC → Base</Badge><p className="mt-1 truncate text-xs text-[var(--muted)]">Deposit {key.slice(0, 10)}…</p></div>
    <div><MobileLabel>Amount</MobileLabel><p className="text-sm font-bold">{amountText}</p>{refund && <p className="mt-1 text-xs text-[var(--muted)]">Refund ledger fee: {formatTokenAmount(refund.ledger_fee)} KINIC</p>}</div>
    <div><MobileLabel>Status</MobileLabel><Badge tone={depositPhaseTone(record.state)}>{depositPhaseName(record.state)}</Badge>{progress && <AutomaticProgress progress={progress} />}{reconciliationMessage && <p className={`mt-1 text-xs font-bold ${record.last_settlement_stop_reason[0] ? "text-[#b42318]" : "text-[var(--muted)]"}`}>{reconciliationMessage}</p>}</div>
    <div><MobileLabel>Time</MobileLabel><ActivityTime valueNs={item.createdAtNs} /></div>
    <div className="md:text-right"><MobileLabel>Action</MobileLabel>{"AuthorizationAvailable" in record.state || "ExpiryReconciliation" in record.state ? <MintAuthorizationAction record={record} compact onExpiredReconcile={writesEnabled ? () => void onContinue(record) : undefined} reconciling={actioningId === key} /> : !terminal && (!progress || progress.retryAllowed) ? <Button size="sm" variant="ghost" disabled={!writesEnabled || actioningId === key} onClick={() => void onContinue(record)}>{actioningId === key ? "Retrying…" : "Retry"}</Button> : <span className="text-sm text-[var(--muted)]">—</span>}</div>
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
  return <article className="grid gap-4 rounded-2xl bg-white p-4 md:grid-cols-[minmax(7rem,0.8fr)_minmax(12rem,1.8fr)_minmax(8rem,1fr)_minmax(7rem,0.9fr)_10.5rem] md:items-center">
    <div><MobileLabel>Direction</MobileLabel><Badge tone="info">Base → IC</Badge><p className="mt-1 truncate text-xs text-[var(--muted)]">{record.id === undefined ? `Withdrawal ${record.hash.slice(0, 10)}…` : `Withdrawal #${record.id}`}</p></div>
    <div><MobileLabel>Amount</MobileLabel><p className="text-sm font-bold">{record.amountOut === undefined ? "Amount unavailable" : `${formatTokenAmount(record.amountOut)} KINIC to IC`}</p></div>
    <div><MobileLabel>Status</MobileLabel><Badge tone={needsAttention ? "warn" : record.canister ? withdrawalPhaseTone(record.canister.state) : "neutral"}>{label}</Badge>{progress && <AutomaticProgress progress={progress} />}{needsAttention && <p className="mt-1 text-xs font-bold text-[#b42318]">Needs attention</p>}</div>
    <div><MobileLabel>Time</MobileLabel><ActivityTime valueNs={item.createdAtNs} /></div>
    <div className="md:text-right"><MobileLabel>Action</MobileLabel>{!record.canister ? <Button size="sm" variant="ghost" disabled={!writesEnabled || retryingHash === record.hash} onClick={() => void onCheckAndNotify(record)}>{retryingHash === record.hash ? "Checking…" : "Check status"}</Button> : !terminal && (!progress || progress.retryAllowed) ? <Button size="sm" variant="ghost" disabled={(!writesEnabled && !feeGuardBlocked(record.canister)) || actioningId === key} onClick={() => void onContinue(record)}>{actioningId === key ? "Retrying…" : "Retry"}</Button> : <span className="text-sm text-[var(--muted)]">—</span>}</div>
  </article>
}

function MobileLabel({ children }: { children: React.ReactNode }) {
  return <span className="mb-1 block text-[10px] font-bold uppercase tracking-[0.08em] text-[var(--muted)] md:hidden">{children}</span>
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

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` {
  return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}`
}
