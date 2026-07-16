import { createFileRoute } from "@tanstack/react-router"
import { RefreshCcw } from "lucide-react"
import { useChainId } from "wagmi"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { useBridgeStatus, useConfirmedBaseStatus, useRuntimeValidation } from "@/features/status/use-status"
import { formatTokenAmount } from "@/lib/amounts"
import { bridgeAvailability } from "@/lib/bridge-availability"

export const Route = createFileRoute("/status")({ component: StatusPage })

function StatusPage() {
  const chainId = useChainId()
  const validation = useRuntimeValidation(chainId)
  const base = useConfirmedBaseStatus()
  const canister = useBridgeStatus()
  const runtime = validation.data
  const baseData = !base.isError && !base.isStale ? base.data : undefined
  const canisterData = !canister.isError && !canister.isStale ? canister.data : undefined
  const { available, toBase, toIc } = bridgeAvailability({
    runtimeReady: runtime?.ready === true,
    baseStatus: baseData,
    reserveSufficient: canisterData?.reserve.sufficient,
  })
  const remaining = baseData ? (baseData.limit > baseData.minted ? baseData.limit - baseData.minted : 0n) : undefined
  const refreshing = validation.isFetching || base.isFetching || canister.isFetching
  const refresh = () => { void validation.refetch().then(() => Promise.all([base.refetch(), canister.refetch()])) }
  return <div className="route-enter mx-auto max-w-4xl pt-8 md:pt-12">
    <header className="mb-8 flex items-end justify-between gap-4"><div><p className="text-sm font-medium text-[var(--pink)]">Current state</p><h1 className="font-display mt-2 text-[42px] leading-[1.1]">Bridge status</h1><p className="mt-3 max-w-xl text-base leading-6 text-[var(--muted)]">Current availability across Internet Computer and Base.</p></div><Button variant="ghost" disabled={refreshing} onClick={refresh}><RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />{refreshing ? "Refreshing…" : "Refresh"}</Button></header>
    <section className="grid gap-5 md:grid-cols-2">
      <div className="rounded-[20px] bg-black p-6 text-white"><div className="flex items-start justify-between gap-4"><div><h2 className="text-xl font-bold">Availability</h2><p className="mt-1 text-sm text-white/60">Whether transfers can start right now.</p></div><Badge tone={available ? "good" : "warn"}>{available ? "Available" : "Unavailable"}</Badge></div><div className="mt-8 grid grid-cols-2 gap-4"><Metric label="To Base" value={toBase} inverse /><Metric label="To Internet Computer" value={toIc} inverse /></div></div>
      <div className="rounded-[20px] bg-[var(--panel)] p-6"><h2 className="text-xl font-bold">Current terms</h2><div className="mt-8 grid grid-cols-2 gap-4"><Metric label="Service fee" value={baseData ? `${formatTokenAmount(baseData.serviceFee)} KINIC` : "—"} /><Metric label="Per transfer" value={baseData ? `${formatTokenAmount(baseData.perDepositLimit)} KINIC` : "—"} /><Metric label="Available this period" value={remaining === undefined ? "—" : `${formatTokenAmount(remaining)} KINIC`} /></div></div>
    </section>
    <section className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-4"><Stat label="Deposits" value={canisterData?.counts.deposits.toString() ?? "—"} /><Stat label="Withdrawals" value={canisterData?.counts.withdrawals.toString() ?? "—"} /><Stat label="Unpaid withdrawals" value={canisterData?.unpaid_withdrawal_count.toString() ?? "—"} /><Stat label="Unpaid amount" value={canisterData ? `${formatTokenAmount(canisterData.unpaid_withdrawal_amount_out)} KINIC` : "—"} /></section>
    <section className="mt-5 rounded-[20px] bg-[var(--panel)] p-6"><h2 className="text-xl font-bold">Withdrawal operations</h2><div className="mt-5 grid gap-4 sm:grid-cols-2"><Metric label="Oldest unpaid observation" value={canisterData?.oldest_unpaid_withdrawal_observed_at_ns[0] !== undefined ? new Date(Number(canisterData.oldest_unpaid_withdrawal_observed_at_ns[0] / 1_000_000n)).toLocaleString() : "—"} /><Metric label="Ledger stop reasons" value={canisterData?.withdrawal_stop_reasons.join(", ") || "None"} /></div></section>
  </div>
}

function Metric({ label, value, inverse = false }: { label: string; value: string; inverse?: boolean }) { return <div><p className={`text-xs ${inverse ? "text-white/55" : "text-[var(--muted)]"}`}>{label}</p><p className="mt-1 text-lg font-bold">{value}</p></div> }
function Stat({ label, value }: { label: string; value: string }) { return <div className="rounded-2xl bg-[var(--panel)] p-5"><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-2 text-3xl font-bold">{value}</p></div> }
