import { createFileRoute, useNavigate } from "@tanstack/react-router"
import { useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query"
import { Clock3, RefreshCcw } from "lucide-react"
import { createPublicClient, defineChain, hexToBytes, http, numberToHex } from "viem"
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
import type { DepositView, SettlementActionResult, WithdrawalView } from "@/generated/bridge.did"
import { formatTokenAmount } from "@/lib/amounts"
import { mergeDepositHistoryPage, type DepositHistoryData } from "@/lib/deposit-history"
import { createBridgeActor } from "@/lib/ic/bridge"
import { automaticConfirmationCheckDate, hasScheduledConfirmation, nextAutomaticConfirmationCheck, shouldPollScheduledHistory } from "@/lib/confirmation-schedule"
import { refetchRuntimeWriteReady } from "@/lib/runtime-validation"
import { fetchInBatches, scanWithdrawalLogs, type SafeEventLog, type WithdrawalLogScan } from "@/lib/withdrawal-history"

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
    const additions = (await Promise.all(result.Ok.deposit_ids.map((id) => actor.get_deposit(id)))).flatMap((record) => record)
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
    const client = publicClient()
    const safe = await client.getBlock({ blockTag: "safe" })
    if (safe.number === null || safe.hash === null) throw new Error("Safe Base block is unavailable")
    let usablePrevious = previous
    if (previous) {
      const checkpoint = await client.getBlock({ blockNumber: previous.lastSafeBlock })
      if (safe.number < previous.lastSafeBlock || checkpoint.hash !== previous.lastSafeBlockHash) {
        usablePrevious = undefined
      }
    }
    const scan = await scanWithdrawalLogs<WithdrawalEventLog>({
      deploymentBlock: deploymentProfile.deploymentBlock as bigint,
      safeBlock: safe.number,
      safeBlockHash: safe.hash,
      previous: usablePrevious,
      mode,
      fetchLogs: async (fromBlock, toBlock) => client.getContractEvents({ address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, eventName: "WithdrawalCreated", args: { requester: address }, fromBlock, toBlock, strict: true }),
      fetchBlockHash: async (blockNumber) => (await client.getBlock({ blockNumber })).hash,
    })
    const bridge = deploymentProfile.bridgeCanisterId ? await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId) : undefined
    const views = bridge ? await fetchInBatches(scan.logs, 20, async (logs) => {
      const result = await bridge.get_withdrawals(logs.map((log) => hexToBytes(numberToHex(log.args.withdrawalId, { size: 32 }))))
      if ("Err" in result) throw new Error("Canister rejected the withdrawal history batch")
      return result.Ok
    }) : undefined
    const items: WithdrawalHistoryItem[] = scan.logs.map((log, index) => ({ id: log.args.withdrawalId, amount: log.args.amount, minAmountOut: log.args.minAmountOut, hash: log.transactionHash as `0x${string}`, canister: views?.[index]?.[0] }))
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
  const scheduledDepositVisible = hasScheduledConfirmation(deposits.data?.items.map((record) => record.next_automatic_confirmation_check_at_ns) ?? [])
  const scheduledWithdrawalVisible = hasScheduledConfirmation(withdrawals.data?.items.flatMap((item) => item.canister ? [item.canister.next_automatic_confirmation_check_at_ns] : []) ?? [])
  const scheduledRecordVisible = tab === "deposit" ? scheduledDepositVisible : scheduledWithdrawalVisible
  useEffect(() => {
    const onVisibilityChange = () => setPageVisible(document.visibilityState === "visible")
    document.addEventListener("visibilitychange", onVisibilityChange)
    return () => document.removeEventListener("visibilitychange", onVisibilityChange)
  }, [])
  useEffect(() => {
    if (!shouldPollScheduledHistory(scheduledRecordVisible, pageVisible)) return
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
      const duplicate = "Duplicate" in receipt
      const value = duplicate ? receipt.Duplicate : receipt.Ingested
      const settlement = value.settlement[0]
      if (settlement) toastSettlement(settlement)
      toast.success(duplicate ? "Withdrawal was already recorded" : "Withdrawal notification succeeded")
      await withdrawals.refetch()
    } catch (error) {
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
    } catch (error) { toast.error(error instanceof Error ? error.message : "Settlement action failed") }
    finally { setActioningId(undefined) }
  }
  const continueWithdrawal = async (item: WithdrawalHistoryItem) => {
    const key = item.id?.toString() ?? item.hash
    try {
      setActioningId(key)
      if (!ic.adapter || !item.canister) throw new Error("Connect the withdrawal owner IC wallet")
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const result = await ic.adapter.continueWithdrawal(Uint8Array.from(item.canister.withdrawal_id))
      toastSettlement(result)
      await withdrawals.refetch()
    } catch (error) { toast.error(error instanceof Error ? error.message : "Settlement action failed") }
    finally { setActioningId(undefined) }
  }
  const refresh = () => { void Promise.all([runtime.refetch(), tab === "deposit" ? deposits.refetch() : withdrawals.refetch()]) }
  const refreshing = runtime.isFetching || (tab === "deposit" ? deposits.isFetching : withdrawals.isFetching)
  const writesEnabled = runtimeReadiness.ready && !runtime.isFetching
  const runtimeReason = runtime.isFetching ? "Runtime verification is in progress." : runtimeReadiness.reason
  return <div className="route-enter mx-auto max-w-3xl pt-8 md:pt-12">
    <header className="mb-8 flex items-end justify-between gap-4"><div><p className="text-sm font-medium text-[var(--pink)]">Activity</p><h1 className="font-display mt-2 text-[42px] leading-[1.1]">Bridge history</h1><p className="mt-3 max-w-xl text-base leading-6 text-[var(--muted)]">Submitted Base transactions are confirmed automatically at the Safe head. Retry is available here only when automatic progress has stopped.</p></div><Button variant="ghost" disabled={refreshing} onClick={refresh}><RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />{refreshing ? "Refreshing…" : "Refresh"}</Button></header>
    <div className="mb-5 inline-flex rounded-2xl bg-[var(--panel)] p-1" role="tablist" aria-label="History type"><Tab tab="deposit" active={tab === "deposit"} onSelect={(next) => void navigate({ search: { tab: next }, replace: true })}>Deposits</Tab><Tab tab="withdraw" active={tab === "withdraw"} onSelect={(next) => void navigate({ search: { tab: next }, replace: true })}>Withdrawals</Tab></div>
    {!writesEnabled && <Alert className="mb-5" tone="warning"><strong>Settlement actions are locked.</strong><span className="ml-1">{runtimeReason}</span></Alert>}
    <section id={`history-${tab}-panel`} role="tabpanel" aria-labelledby={`history-${tab}-tab`} className="min-h-80 rounded-[20px] bg-[var(--panel)] p-5 sm:p-7">
      {tab === "deposit" ? <DepositHistory query={deposits} connected={Boolean(ic.account)} writesEnabled={writesEnabled} actioningId={actioningId} loadingOlder={loadingOlderDeposits} onContinue={continueDeposit} onScanOlder={scanOlderDeposits} /> : <WithdrawalHistory query={withdrawals} connected={Boolean(address)} writesEnabled={writesEnabled} actioningId={actioningId} retryingHash={retryingHash} loadingOlder={loadingOlder} onCheckAndNotify={checkAndNotify} onContinue={continueWithdrawal} onScanOlder={scanOlder} />}
    </section>
    <p className="mt-4 text-xs leading-5 text-[var(--muted)]">Deposit IDs are available through a public owner index. Anyone who knows an IC Principal may correlate its records with Base recipients.</p>
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
function DepositHistory({ query, connected, writesEnabled, actioningId, loadingOlder, onContinue, onScanOlder }: { query: UseQueryResult<DepositHistoryData>; connected: boolean; writesEnabled: boolean; actioningId?: string; loadingOlder: boolean; onContinue: (record: DepositView) => Promise<void>; onScanOlder: () => Promise<void> }) { if (!connected) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Connect an IC wallet" message="Deposit history follows the connected IC account." />; if (query.isFetching) return <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading deposits" message="Reading the public canister index once." />; if (query.isError) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Deposit history is unavailable" message="Choose Refresh to try one more time." />; if (!query.data) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="History has not been loaded" message="Choose Refresh to read it once." />; if (!query.data.items.length) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title={query.data.historyTruncated ? "No retained deposits" : "No deposits yet"} message={query.data.historyTruncated ? "Older deposit index entries were pruned by the canister." : "Completed and pending deposits will appear here."} />; return <div className="space-y-3">{query.data.historyTruncated && <p className="rounded-xl bg-[#fff3e4] px-3 py-2 text-xs font-medium text-[#8a4b08]">The canister has pruned older deposit index entries, so records beyond the retained index are unavailable.</p>}{query.data.items.map((record) => { const key = bytesHex(record.deposit_id); const terminal = ["Minted", "MintReverted", "Cancelled"].includes(record.state); const scheduledAt = nextAutomaticConfirmationCheck(record.next_automatic_confirmation_check_at_ns); return <div key={key} className="flex items-center justify-between gap-4 rounded-2xl bg-white p-4"><div><p className="text-sm font-bold">{key.slice(0, 18)}…</p><p className="mt-1 text-xs text-[var(--muted)]">Owner sequence {record.owner_sequence.toString()}</p><p className="mt-1 text-sm text-[var(--muted)]">{formatTokenAmount(record.net_amount)} KINIC on Base</p>{scheduledAt !== undefined && <AutomaticConfirmation scheduledAtNs={scheduledAt} />}{record.last_settlement_stop_reason[0] && <p className="mt-1 text-xs font-bold text-[#b42318]">Stopped: {record.last_settlement_stop_reason[0]}</p>}</div><div className="flex items-center gap-2">{!terminal && scheduledAt === undefined && <Button size="sm" variant="ghost" disabled={!writesEnabled || actioningId === key} onClick={() => void onContinue(record)}>{actioningId === key ? "Retrying…" : "Retry settlement"}</Button>}<Badge tone={record.state === "Minted" ? "good" : terminal ? "warn" : "neutral"}>{record.state}</Badge></div></div> })}{query.data.nextCursor !== null && <div className="pt-2 text-center"><Button size="sm" variant="ghost" disabled={loadingOlder} onClick={() => void onScanOlder()}>{loadingOlder ? "Loading…" : "Load older deposits"}</Button></div>}</div> }
interface WithdrawalHistoryItem { id?: bigint; amount?: bigint; minAmountOut?: bigint; hash: `0x${string}`; canister?: WithdrawalView }
interface WithdrawalEventLog extends SafeEventLog { args: { withdrawalId: bigint; amount: bigint; minAmountOut: bigint } }
interface WithdrawalHistoryData extends WithdrawalLogScan<WithdrawalEventLog> { items: WithdrawalHistoryItem[] }
function WithdrawalHistory({ query, connected, writesEnabled, actioningId, retryingHash, loadingOlder, onCheckAndNotify, onContinue, onScanOlder }: { query: UseQueryResult<WithdrawalHistoryData>; connected: boolean; writesEnabled: boolean; actioningId?: string; retryingHash?: string; loadingOlder: boolean; onCheckAndNotify: (item: WithdrawalHistoryItem) => Promise<void>; onContinue: (item: WithdrawalHistoryItem) => Promise<void>; onScanOlder: () => Promise<void> }) { if (!connected) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="Connect a Base wallet" message="Withdrawal history follows the connected Base account." />; if (query.isFetching) return <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading withdrawals" message="Scanning safe contract events once." />; if (query.isError) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="Withdrawal history is unavailable" message="Choose Refresh to try one more time." />; if (!query.data) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="History has not been loaded" message="Choose Refresh to read it once." />; if (!query.data.items.length) return <div className="grid min-h-64 place-items-center text-center"><div><RefreshCcw className="mx-auto size-6 text-[var(--pink)]" /><p className="mt-3 font-bold text-black">{query.data.olderCursor === null ? "No safe withdrawals yet" : "No withdrawals in the scanned range"}</p>{query.data.olderCursor !== null && <Button className="mt-4" size="sm" variant="ghost" disabled={loadingOlder} onClick={() => void onScanOlder()}>{loadingOlder ? "Scanning…" : "Scan older"}</Button>}</div></div>; return <div className="space-y-3">{query.data.items.map((item) => { const key = item.id?.toString() ?? item.hash; const terminal = item.canister && ["Released", "Refunded", "AcknowledgeReverted", "RefundReverted"].includes(item.canister.state); const scheduledAt = item.canister?.next_automatic_confirmation_check_at_ns[0]; return <div key={key} className="flex items-center justify-between gap-4 rounded-2xl bg-white p-4"><div><p className="text-sm font-bold">{item.id === undefined ? `Pending withdrawal ${item.hash.slice(0, 12)}…` : `Withdrawal #${item.id}`}</p>{item.amount !== undefined && <p className="mt-1 text-sm text-[var(--muted)]">{formatTokenAmount(item.amount)} KINIC burned</p>}{scheduledAt !== undefined && <AutomaticConfirmation scheduledAtNs={scheduledAt} />}{item.canister?.last_settlement_stop_reason[0] && <p className="mt-1 text-xs font-bold text-[#b42318]">Stopped: {item.canister.last_settlement_stop_reason[0]}</p>}</div><div className="flex items-center gap-2">{!item.canister && <Button size="sm" variant="ghost" disabled={!writesEnabled || retryingHash === item.hash} onClick={() => void onCheckAndNotify(item)}>{retryingHash === item.hash ? "Checking…" : "Check and notify"}</Button>}{item.canister && !terminal && scheduledAt === undefined && <Button size="sm" variant="ghost" disabled={!writesEnabled || actioningId === key} onClick={() => void onContinue(item)}>{actioningId === key ? "Retrying…" : "Retry settlement"}</Button>}<Badge tone={item.canister?.state === "Released" || item.canister?.state === "Refunded" ? "good" : terminal ? "warn" : "neutral"}>{item.canister?.state ?? "Awaiting safe notification"}</Badge></div></div> })}{query.data.olderCursor !== null && <div className="pt-2 text-center"><Button size="sm" variant="ghost" disabled={loadingOlder} onClick={() => void onScanOlder()}>{loadingOlder ? "Scanning…" : "Scan older"}</Button></div>}</div> }

function AutomaticConfirmation({ scheduledAtNs }: { scheduledAtNs: bigint }) { return <p className="mt-1 text-xs font-medium text-[var(--pink)]">Confirming automatically · next check {automaticConfirmationCheckDate(scheduledAtNs).toLocaleString()}</p> }

function toastSettlement(result: SettlementActionResult) { if ("Stopped" in result) { toast.error(`Settlement stopped in ${result.Stopped.state}: ${Object.keys(result.Stopped.reason)[0] ?? "Unknown error"}`); return } if ("WaitingForConfirmation" in result) { toast.info("Transaction is submitted and confirmation will be checked automatically."); return } if ("ReconciliationProgress" in result) { toast.info("Reconciliation progressed. The canister will continue at the next automatic confirmation step."); return } if ("Submitted" in result) { toast.success("Transaction submitted. Base confirmation will be checked automatically."); return } toast.success(`Settlement is ${result.Complete.state}`) }
function bytesHex(bytes: Uint8Array | number[]) { return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}` }
function publicClient() { return createPublicClient({ chain: defineChain({ id: deploymentProfile.chainId, name: deploymentProfile.label, nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: { default: { http: [deploymentProfile.baseRpcUrl] } } }), transport: http(deploymentProfile.baseRpcUrl) }) }
