import { createFileRoute, useNavigate } from "@tanstack/react-router"
import { useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query"
import { Clock3, RefreshCcw } from "lucide-react"
import { hexToBytes, numberToHex } from "viem"
import { useAccount, useChainId } from "wagmi"
import { Principal } from "@dfinity/principal"
import { useEffect, useState, type KeyboardEvent } from "react"
import { toast } from "sonner"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Alert } from "@/components/ui/alert"
import { deploymentProfile } from "@/config/profile"
import { useRuntimeValidation, useRuntimeWriteReadiness } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import type { AutomaticProgressView, BaseConfirmationView, DepositView, SettlementActionResult, WithdrawalView } from "@/generated/bridge.did"
import { formatTokenAmount } from "@/lib/amounts"
import { depositIdsForRefresh, mergeDepositHistoryPage, type DepositHistoryData } from "@/lib/deposit-history"
import { createBridgeActor } from "@/lib/ic/bridge"
import type { IcWalletAdapter } from "@/lib/ic/wallet"
import { refetchRuntimeWriteReady } from "@/lib/runtime-validation"
import { basePublicClient } from "@/lib/evm/client"
import { depositPhaseName, depositPhaseTone, isDepositTerminal, isWithdrawalTerminal, settlementStateName, withdrawalPhaseName, withdrawalPhaseTone } from "@/lib/settlement-phase"
import { fetchInBatches, scanWithdrawalLogs, type FinalizedEventLog, type WithdrawalLogScan } from "@/lib/withdrawal-history"
import { removePendingConfirmation, restorePendingConfirmation, savePendingConfirmation } from "@/lib/pending-confirmations"
import { withdrawalNotificationPresentation } from "@/lib/withdrawal-notification"

type HistoryTab = "deposit" | "withdraw"
export const Route = createFileRoute("/history")({
  validateSearch: (search: Record<string, unknown>): { tab: HistoryTab } => ({ tab: search.tab === "withdraw" ? "withdraw" : "deposit" }),
  component: HistoryPage,
})

function HistoryPage() {
  const { tab } = Route.useSearch()
  const navigate = useNavigate({ from: "/history" })
  const { address } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const runtime = useRuntimeValidation(chainId)
  const runtimeReadiness = useRuntimeWriteReadiness(runtime.data)
  const queryClient = useQueryClient()
  const [retryingHash, setRetryingHash] = useState<string>()
  const [actioningId, setActioningId] = useState<string>()
  const [loadingOlder, setLoadingOlder] = useState(false)
  const [loadingOlderDeposits, setLoadingOlderDeposits] = useState(false)
  const [pageVisible, setPageVisible] = useState(() => document.visibilityState === "visible")
  const depositQueryKey = ["deposit-history", ic.account?.owner] as const
  const readDepositHistory = async (mode: "refresh" | "older", previous?: DepositHistoryData): Promise<DepositHistoryData> => {
    const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
    const beforeCursor = mode === "older" ? previous?.nextCursor : undefined
    const result = await actor.list_deposit_ids({ owner: Principal.fromText(ic.account!.owner), before_cursor: beforeCursor === undefined || beforeCursor === null ? [] : [beforeCursor], limit: 20 })
    if ("Err" in result) throw new Error("Deposit history limit was rejected")
    const ids = mode === "refresh" ? depositIdsForRefresh(previous, result.Ok.deposit_ids) : result.Ok.deposit_ids
    const records = await fetchInBatches(ids, 20, (batch) => Promise.all(batch.map((id) => actor.get_deposit(id))))
    const additions = records.flatMap((record) => record)
    const returnedCursor = result.Ok.next_cursor[0] ?? null
    return mergeDepositHistoryPage(previous, additions, {
      nextCursor: returnedCursor,
      oldestAvailableCursor: result.Ok.oldest_available_cursor[0] ?? null,
      historyTruncated: result.Ok.history_truncated,
    }, mode)
  }
  const deposits = useQuery({
    queryKey: depositQueryKey,
    enabled: false,
    queryFn: () => readDepositHistory("refresh", queryClient.getQueryData<DepositHistoryData>(depositQueryKey)),
  })
  const withdrawalQueryKey = ["withdraw-history", deploymentProfile.chainId, deploymentProfile.bridgeAddress, address] as const
  const readWithdrawalHistory = async (mode: "refresh" | "older", previous?: WithdrawalHistoryData): Promise<WithdrawalHistoryData> => {
    const client = basePublicClient
    const finalized = await client.getBlock({ blockTag: "finalized" })
    if (finalized.number === null || finalized.hash === null) throw new Error("finalized Base block is unavailable")
    let usablePrevious = previous
    if (previous) {
      const checkpoint = await client.getBlock({ blockNumber: previous.lastFinalizedBlock })
      if (finalized.number < previous.lastFinalizedBlock || checkpoint.hash !== previous.lastFinalizedBlockHash) {
        usablePrevious = undefined
      }
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
    const items: WithdrawalHistoryItem[] = scan.logs.map((log, index) => ({ id: log.args.withdrawalId, amount: log.args.amount, amountOut: log.args.amountOut, hash: log.transactionHash as `0x${string}`, canister: views?.[index]?.[0] }))
    return { ...scan, items }
  }
  const withdrawals = useQuery({
    queryKey: withdrawalQueryKey,
    enabled: false,
    queryFn: async () => {
      const previous = queryClient.getQueryData<WithdrawalHistoryData>(withdrawalQueryKey)
      return readWithdrawalHistory("refresh", previous)
    },
  })
  const scheduledDepositVisible = deposits.data?.items.some((record) => record.automatic_progress.length > 0) ?? false
  const scheduledWithdrawalVisible = withdrawals.data?.items.some((item) => (item.canister?.automatic_progress.length ?? 0) > 0) ?? false
  const scheduledRecordVisible = tab === "deposit" ? scheduledDepositVisible : scheduledWithdrawalVisible
  useEffect(() => {
    if (!ic.account) return
    for (const record of deposits.data?.items ?? []) {
      const submitted = submittedTransaction(record.base_confirmation)
      if (submitted) restorePendingConfirmation({ kind: "deposit", settlementId: bytesHex(record.deposit_id), transactionHash: bytesHex(submitted.transaction_hash), owner: ic.account.owner })
    }
  }, [deposits.data, ic.account, withdrawals.data])
  useEffect(() => {
    const onVisibilityChange = () => setPageVisible(document.visibilityState === "visible")
    document.addEventListener("visibilitychange", onVisibilityChange)
    return () => document.removeEventListener("visibilitychange", onVisibilityChange)
  }, [])
  useEffect(() => {
    if (!(scheduledRecordVisible && pageVisible)) return
    const timer = window.setInterval(() => void (tab === "deposit" ? deposits.refetch() : withdrawals.refetch()), 60_000)
    return () => window.clearInterval(timer)
  }, [deposits, pageVisible, scheduledRecordVisible, tab, withdrawals])
  const scanOlder = async () => {
    if (!withdrawals.data) return
    try {
      setLoadingOlder(true)
      queryClient.setQueryData(withdrawalQueryKey, await readWithdrawalHistory("older", withdrawals.data))
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Older withdrawal history is unavailable")
    } finally {
      setLoadingOlder(false)
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
  const checkAndNotify = async (item: WithdrawalHistoryItem) => {
    try {
      setRetryingHash(item.hash)
      if (!ic.adapter) throw new Error("Connect the destination IC wallet before retrying")
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const receipt = await ic.adapter.notifyWithdrawal(hexToBytes(item.hash))
      removePendingConfirmation({ kind: "withdrawal", transactionHash: item.hash, owner: ic.account?.owner ?? "" })
      toastWithdrawalNotification(receipt)
      await withdrawals.refetch()
    } catch (error) {
      await withdrawals.refetch()
      toast.error(error instanceof Error ? error.message : "Withdrawal notification failed")
    } finally {
      setRetryingHash(undefined)
    }
  }
  const continueDeposit = async (record: DepositView) => {
    const key = bytesHex(record.deposit_id)
    try {
      setActioningId(key)
      if (!ic.adapter) throw new Error("Connect the deposit owner IC wallet")
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const result = await ic.adapter.continueDeposit(Uint8Array.from(record.deposit_id))
      toastSettlement(result)
      await deposits.refetch()
    } catch { toast.error("This transfer could not be retried. Try again later.") }
    finally { setActioningId(undefined) }
  }
  const continueWithdrawal = async (item: WithdrawalHistoryItem) => {
    const key = item.id?.toString() ?? item.hash
    try {
      setActioningId(key)
      if (!ic.adapter || !item.canister) throw new Error("Connect the withdrawal owner IC wallet")
      if (!feeGuardBlocked(item.canister)) await refetchRuntimeWriteReady(() => runtime.refetch())
      const result = await ic.adapter.continueWithdrawal(Uint8Array.from(item.canister.withdrawal_id))
      toastSettlement(result)
      await withdrawals.refetch()
    } catch { toast.error("This transfer could not be retried. Try again later.") }
    finally { setActioningId(undefined) }
  }
  const confirmDeposit = (record: DepositView) => {
    const submitted = submittedTransaction(record.base_confirmation)
    if (!submitted || !ic.account) return
    savePendingConfirmation({ kind: "deposit", settlementId: bytesHex(record.deposit_id), transactionHash: bytesHex(submitted.transaction_hash), owner: ic.account.owner, blocked: false })
    toast.info("Waiting for Base finalized confirmation. Your IC wallet will request approval when ready.")
  }
  const refresh = () => { void Promise.all([runtime.refetch(), tab === "deposit" ? deposits.refetch() : withdrawals.refetch()]) }
  const refreshing = runtime.isFetching || (tab === "deposit" ? deposits.isFetching : withdrawals.isFetching)
  const writesEnabled = runtimeReadiness.ready && !runtime.isFetching
  const runtimeReason = runtime.isFetching
    ? "Checking availability…"
    : runtime.data
      ? "Bridge actions are temporarily unavailable. Try Refresh."
      : "Refresh before continuing."
  return <div className="route-enter mx-auto max-w-3xl pt-8 md:pt-12">
    <header className="mb-8 flex items-end justify-between gap-4"><div><p className="text-sm font-medium text-[var(--pink)]">Activity</p><h1 className="font-display mt-2 text-[42px] leading-[1.1]">Bridge history</h1><p className="mt-3 max-w-xl text-base leading-6 text-[var(--muted)]">Your deposits and withdrawals update automatically.</p></div><Button variant="ghost" disabled={refreshing} onClick={refresh}><RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />{refreshing ? "Refreshing…" : "Refresh"}</Button></header>
    <div className="mb-5 inline-flex rounded-2xl bg-[var(--panel)] p-1" role="tablist" aria-label="History type"><Tab tab="deposit" active={tab === "deposit"} onSelect={(next) => void navigate({ search: { tab: next }, replace: true })}>Deposits</Tab><Tab tab="withdraw" active={tab === "withdraw"} onSelect={(next) => void navigate({ search: { tab: next }, replace: true })}>Withdrawals</Tab></div>
    {!writesEnabled && <Alert className="mb-5" tone="warning"><strong>Actions unavailable.</strong><span className="ml-1">{runtimeReason}</span></Alert>}
    <section id={`history-${tab}-panel`} role="tabpanel" aria-labelledby={`history-${tab}-tab`} className="min-h-80 rounded-[20px] bg-[var(--panel)] p-5 sm:p-7">
      {tab === "deposit" ? <DepositHistory query={deposits} connected={Boolean(ic.account)} writesEnabled={writesEnabled} actioningId={actioningId} loadingOlder={loadingOlderDeposits} onConfirm={confirmDeposit} onContinue={continueDeposit} onScanOlder={scanOlderDeposits} /> : <WithdrawalHistory query={withdrawals} connected={Boolean(address)} writesEnabled={writesEnabled} actioningId={actioningId} retryingHash={retryingHash} loadingOlder={loadingOlder} onCheckAndNotify={checkAndNotify} onContinue={continueWithdrawal} onScanOlder={scanOlder} />}
    </section>
  </div>
}

function Tab({ tab, active, onSelect, children }: { tab: HistoryTab; active: boolean; onSelect: (tab: HistoryTab) => void; children: React.ReactNode }) {
  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return
    event.preventDefault()
    const next = event.key === "ArrowLeft" || event.key === "Home" ? "deposit" : "withdraw"
    onSelect(next)
    document.getElementById(`history-${next}-tab`)?.focus()
  }
  return <button id={`history-${tab}-tab`} role="tab" aria-selected={active} aria-controls={`history-${tab}-panel`} tabIndex={active ? 0 : -1} className={`rounded-xl px-5 py-2.5 text-sm font-bold transition ${active ? "bg-black text-white" : "text-[var(--muted)] hover:text-[var(--pink)]"}`} onClick={() => onSelect(tab)} onKeyDown={onKeyDown}>{children}</button>
}
function Empty({ icon, title, message }: { icon: React.ReactNode; title: string; message: string }) { return <div className="grid min-h-64 place-items-center text-center"><div>{icon}<p className="mt-3 font-bold text-black">{title}</p><p className="mt-1 text-sm text-[var(--muted)]">{message}</p></div></div> }
function DepositHistory({ query, connected, writesEnabled, actioningId, loadingOlder, onConfirm, onContinue, onScanOlder }: { query: UseQueryResult<DepositHistoryData>; connected: boolean; writesEnabled: boolean; actioningId?: string; loadingOlder: boolean; onConfirm: (record: DepositView) => void; onContinue: (record: DepositView) => Promise<void>; onScanOlder: () => Promise<void> }) { if (!connected) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Connect an IC wallet" message="Connect the wallet used for your deposits." />; if (query.isFetching) return <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading deposits" message="This may take a moment." />; if (query.isError) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Deposit history is unavailable" message="Choose Refresh to try again." />; if (!query.data) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="History has not been loaded" message="Choose Refresh to load it." />; if (!query.data.items.length) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="No deposits yet" message="Your deposits will appear here." />; return <div className="space-y-3">{query.data.historyTruncated && <p className="rounded-xl bg-[#fff3e4] px-3 py-2 text-xs font-medium text-[#8a4b08]">Some older deposits are no longer available.</p>}{query.data.items.map((record) => { const key = bytesHex(record.deposit_id); const terminal = isDepositTerminal(record.state); const progress = automaticProgressInfo(record.automatic_progress); const submitted = submittedTransaction(record.base_confirmation); const stateName = depositPhaseName(record.state); return <div key={key} className="flex items-center justify-between gap-4 rounded-2xl bg-white p-4"><div><p className="text-sm font-bold">Deposit {key.slice(0, 12)}…</p><p className="mt-1 text-sm text-[var(--muted)]">{formatTokenAmount(record.net_amount)} KINIC on Base</p>{submitted && <p className="mt-1 text-xs font-medium text-[var(--pink)]">Waiting for wallet-confirmed finalized verification</p>}{progress && <AutomaticProgress progress={progress} />}{record.last_settlement_stop_reason[0] && <p className="mt-1 text-xs font-bold text-[#b42318]">This deposit needs attention.</p>}</div><div className="flex items-center gap-2">{submitted ? <Button size="sm" variant="ghost" disabled={!writesEnabled} onClick={() => onConfirm(record)}>Confirm finalized tx</Button> : !terminal && (!progress || progress.retryAllowed) && <Button size="sm" variant="ghost" disabled={!writesEnabled || actioningId === key} onClick={() => void onContinue(record)}>{actioningId === key ? "Retrying…" : "Retry"}</Button>}<Badge tone={depositPhaseTone(record.state)}>{stateName}</Badge></div></div> })}{query.data.nextCursor !== null && <div className="pt-2 text-center"><Button size="sm" variant="ghost" disabled={loadingOlder} onClick={() => void onScanOlder()}>{loadingOlder ? "Loading…" : "Load older deposits"}</Button></div>}</div> }
export interface WithdrawalHistoryItem { id?: bigint; amount?: bigint; amountOut?: bigint; hash: `0x${string}`; canister?: WithdrawalView }
interface WithdrawalEventLog extends FinalizedEventLog { args: { withdrawalId: bigint; amount: bigint; maxServiceFee: bigint; chargedServiceFee: bigint; amountOut: bigint } }
export interface WithdrawalHistoryData extends WithdrawalLogScan<WithdrawalEventLog> { items: WithdrawalHistoryItem[] }
function WithdrawalHistory({ query, connected, writesEnabled, actioningId, retryingHash, loadingOlder, onCheckAndNotify, onContinue, onScanOlder }: { query: UseQueryResult<WithdrawalHistoryData>; connected: boolean; writesEnabled: boolean; actioningId?: string; retryingHash?: string; loadingOlder: boolean; onCheckAndNotify: (item: WithdrawalHistoryItem) => Promise<void>; onContinue: (item: WithdrawalHistoryItem) => Promise<void>; onScanOlder: () => Promise<void> }) { if (!connected) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="Connect a Base wallet" message="Connect the wallet used for your withdrawals." />; if (query.isFetching) return <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading withdrawals" message="This may take a moment." />; if (query.isError) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="Withdrawal history is unavailable" message="Choose Refresh to try again." />; if (!query.data) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="History has not been loaded" message="Choose Refresh to load it." />; if (!query.data.items.length) return <div className="grid min-h-64 place-items-center text-center"><div><RefreshCcw className="mx-auto size-6 text-[var(--pink)]" /><p className="mt-3 font-bold text-black">No withdrawals yet</p>{query.data.olderCursor !== null && <Button className="mt-4" size="sm" variant="ghost" disabled={loadingOlder} onClick={() => void onScanOlder()}>{loadingOlder ? "Loading…" : "Load older withdrawals"}</Button>}</div></div>; return <div className="space-y-3">{query.data.items.map((item) => { const key = item.id?.toString() ?? item.hash; const terminal = item.canister && isWithdrawalTerminal(item.canister.state); const progress = item.canister ? automaticProgressInfo(item.canister.automatic_progress) : undefined; const needsAttention = Boolean(item.canister?.last_settlement_stop_reason[0]) || item.canister?.state !== undefined && "ReconciliationHold" in item.canister.state; const label = !item.canister ? "Committed" : needsAttention ? "Needs attention" : withdrawalPhaseName(item.canister.state); return <div key={key} className="flex items-center justify-between gap-4 rounded-2xl bg-white p-4"><div><p className="text-sm font-bold">{item.id === undefined ? `Withdrawal ${item.hash.slice(0, 12)}…` : `Withdrawal #${item.id}`}</p>{item.amountOut !== undefined && <p className="mt-1 text-sm text-[var(--muted)]">{formatTokenAmount(item.amountOut)} KINIC to IC</p>}{progress && <AutomaticProgress progress={progress} />}{needsAttention && <p className="mt-1 text-xs font-bold text-[#b42318]">This withdrawal needs attention.</p>}</div><div className="flex items-center gap-2">{!item.canister && <Button size="sm" variant="ghost" disabled={!writesEnabled || retryingHash === item.hash} onClick={() => void onCheckAndNotify(item)}>{retryingHash === item.hash ? "Checking…" : "Check status"}</Button>}{item.canister && !terminal && (!progress || progress.retryAllowed) && <Button size="sm" variant="ghost" disabled={(!writesEnabled && !feeGuardBlocked(item.canister)) || actioningId === key} onClick={() => void onContinue(item)}>{actioningId === key ? "Retrying…" : "Retry"}</Button>}<Badge tone={needsAttention ? "warn" : item.canister ? withdrawalPhaseTone(item.canister.state) : "neutral"}>{label}</Badge></div></div> })}{query.data.olderCursor !== null && <div className="pt-2 text-center"><Button size="sm" variant="ghost" disabled={loadingOlder} onClick={() => void onScanOlder()}>{loadingOlder ? "Loading…" : "Load older withdrawals"}</Button></div>}</div> }

type AutomaticProgressInfo = { label: string; deadlineNs: bigint; running: boolean; retryAllowed: boolean }
export function automaticProgressInfo(value: [] | [AutomaticProgressView], nowNs = BigInt(Date.now()) * 1_000_000n): AutomaticProgressInfo | undefined { const progress = value[0]; if (!progress) return undefined; const confirmation = "Confirmation" in progress.phase; if ("Scheduled" in progress.state) { const deadlineNs = progress.state.Scheduled.next_run_at_ns; return { label: confirmation ? "Waiting for confirmation" : "Completing automatically", deadlineNs, running: false, retryAllowed: nowNs >= deadlineNs + 300_000_000_000n } } const deadlineNs = progress.state.Running.lease_until_ns; return { label: confirmation ? "Verifying confirmation" : "Completing automatically", deadlineNs, running: true, retryAllowed: nowNs >= deadlineNs } }
function AutomaticProgress({ progress }: { progress: AutomaticProgressInfo }) { return <p className="mt-1 text-xs font-medium text-[var(--pink)]">{progress.label}</p> }

function toastSettlement(result: SettlementActionResult) { if ("Stopped" in result) { toast.error("This transfer needs attention. Try again from History."); return } if ("WaitingForConfirmation" in result || "Submitted" in result) { toast.info("Submitted. The status will update automatically."); return } if ("ReconciliationProgress" in result) { toast.info("Status updated. Processing will continue automatically."); return } toast.success(`Transfer ${settlementStateName(result.Complete.state).toLowerCase()}.`) }
function toastWithdrawalNotification(receipt: Awaited<ReturnType<IcWalletAdapter["notifyWithdrawal"]>>): void { const presentation = withdrawalNotificationPresentation(receipt); if (presentation.tone === "success") toast.success(presentation.message); else if (presentation.tone === "warning") toast.warning(presentation.message); else toast.info(presentation.message) }
function feeGuardBlocked(record?: WithdrawalView): boolean {
  return record?.last_settlement_stop_reason[0] === "LedgerFeeExceedsServiceFee"
}
function bytesHex(bytes: Uint8Array | number[]): `0x${string}` { return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}` }
function submittedTransaction(value: [] | [BaseConfirmationView]) { const confirmation = value[0]; return confirmation && "Submitted" in confirmation ? confirmation.Submitted : undefined }
