import { createFileRoute } from "@tanstack/react-router"
import { RefreshCcw } from "lucide-react"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { useChainId } from "wagmi"
import { formatEther } from "viem"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { useBridgeStatus, useRuntimeHeartbeat, useRuntimeValidation, useRuntimeWriteReadiness } from "@/features/status/use-status"
import { formatTokenAmount } from "@/lib/amounts"
import { bridgeAvailability, displayCyclesSufficient, STATUS_FRESHNESS_MS, statusDataIsFresh } from "@/lib/bridge-availability"

export const Route = createFileRoute("/status")({ component: StatusPage })

function StatusPage() {
  const chainId = useChainId()
  const validation = useRuntimeValidation(chainId)
  const validationReadiness = useRuntimeWriteReadiness(validation.data)
  const base = useRuntimeHeartbeat(chainId, validation.data)
  const canister = useBridgeStatus()
  const runtime = validation.data
  const baseData = base.data?.snapshot && base.data.finalizedBlock !== undefined && base.data.finalizedBlockHash
    ? {
        ...base.data.snapshot,
        observedBlock: base.data.finalizedBlock,
        observedBlockHash: base.data.finalizedBlockHash,
        observedTimestamp: base.data.snapshot.blockTimestamp,
      }
    : undefined
  const canisterData = canister.data
  const [now, setNow] = useState(0)
  const { refetch: refetchValidation } = validation
  const { refetch: refetchBase } = base
  const { refetch: refetchCanister } = canister
  const initialRefreshChainId = useRef<number | null>(null)

  const refresh = useCallback(() => {
    void (async () => {
      if (!validationReadiness.ready) {
        const checked = await refetchValidation()
        if (checked.data?.ready !== true && !(checked.data && "status" in checked.data && checked.data.status)) {
          await refetchCanister()
        }
        return
      }
      const checked = await refetchBase()
      if (!(checked.data && "status" in checked.data && checked.data.status)) {
        await refetchCanister()
      }
    })()
  }, [refetchBase, refetchCanister, refetchValidation, validationReadiness.ready])

  useEffect(() => {
    if (initialRefreshChainId.current === chainId) return
    initialRefreshChainId.current = chainId
    refresh()
  }, [chainId, refresh])

  useEffect(() => {
    const timestamps = [runtime?.checkedAt, base.dataUpdatedAt, canister.dataUpdatedAt]
      .filter((value): value is number => value !== undefined && value > 0)
    const syncTimeout = window.setTimeout(() => setNow(Date.now()), 0)
    const expiresAt = timestamps.length === 3 ? Math.min(...timestamps) + STATUS_FRESHNESS_MS + 1 : 0
    const remaining = expiresAt - Date.now()
    const expiryTimeout = remaining > 0
      ? window.setTimeout(() => setNow(Date.now()), remaining)
      : undefined
    return () => {
      window.clearTimeout(syncTimeout)
      if (expiryTimeout !== undefined) window.clearTimeout(expiryTimeout)
    }
  }, [base.dataUpdatedAt, canister.dataUpdatedAt, runtime?.checkedAt])

  const observationsFresh = statusDataIsFresh({
    runtimeCheckedAt: runtime?.checkedAt,
    baseUpdatedAt: base.dataUpdatedAt,
    canisterUpdatedAt: canister.dataUpdatedAt,
    now,
  }) && !base.isError && !canister.isError
  const cyclesSufficient = canisterData
    ? displayCyclesSufficient({
        cyclesBalance: canisterData.reserve.cycles_balance,
        requiredCycles: canisterData.reserve.required_cycles,
      })
    : undefined
  const { available, toBase, toIc } = bridgeAvailability({
    runtimeReady: runtime?.ready === true && observationsFresh,
    baseStatus: baseData,
    icDepositsPaused: canisterData?.deposits_paused,
    cyclesSufficient,
  })
  const remaining = baseData ? (baseData.limit > baseData.minted ? baseData.limit - baseData.minted : 0n) : undefined
  const refreshing = validation.isFetching || base.isFetching || canister.isFetching
  const lastUpdatedAt = useMemo(() => {
    const timestamps = [runtime?.checkedAt, base.dataUpdatedAt, canister.dataUpdatedAt]
      .filter((value): value is number => value !== undefined && value > 0)
    return timestamps.length === 3 ? Math.min(...timestamps) : undefined
  }, [base.dataUpdatedAt, canister.dataUpdatedAt, runtime?.checkedAt])
  const errors = [
    ...(runtime?.ready === false ? runtime.blockers : []),
    base.isError ? errorMessage(base.error, "Base RPC status could not be refreshed") : undefined,
    canister.isError ? errorMessage(canister.error, "IC status could not be refreshed") : undefined,
  ].filter((value): value is string => value !== undefined)

  return <div className="route-enter mx-auto max-w-4xl pt-8 md:pt-12">
    <header className="mb-8 flex items-end justify-between gap-4">
      <div>
        <p className="text-sm font-medium text-[var(--pink)]">Current state</p>
        <h1 className="font-display mt-2 text-[42px] leading-[1.1]">Bridge status</h1>
        <p className="mt-3 max-w-xl text-base leading-6 text-[var(--muted)]">Current availability across Internet Computer and Base.</p>
      </div>
      <Button variant="ghost" disabled={refreshing} onClick={refresh}>
        <RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />
        {refreshing ? "Refreshing…" : "Refresh"}
      </Button>
    </header>

    {(!observationsFresh || errors.length > 0) && <div className="mb-5 rounded-2xl border border-amber-300/60 bg-amber-50 p-4 text-sm text-amber-950">
      <p className="font-semibold">Availability is fail-closed until fresh status checks succeed.</p>
      {errors.length > 0 && <p className="mt-1 break-words">{errors.join(" · ")}</p>}
      <p className="mt-1 text-amber-900/70">Last updated: {lastUpdatedAt ? new Date(lastUpdatedAt).toLocaleString() : "Never"}</p>
    </div>}

    <section className="grid gap-5 md:grid-cols-2">
      <div className="rounded-[20px] bg-black p-6 text-white">
        <div className="flex items-start justify-between gap-4">
          <div><h2 className="text-xl font-bold">Availability</h2><p className="mt-1 text-sm text-white/60">Whether transfers can start right now.</p></div>
          <Badge tone={available ? "good" : "warn"}>{available ? "Available" : "Unavailable"}</Badge>
        </div>
        <div className="mt-8 grid grid-cols-2 gap-4"><Metric label="To Base" value={toBase} inverse /><Metric label="To Internet Computer" value={toIc} inverse /></div>
      </div>
      <div className="rounded-[20px] bg-[var(--panel)] p-6">
        <h2 className="text-xl font-bold">Current terms</h2>
        <div className="mt-8 grid grid-cols-2 gap-4">
          <Metric label="Service fee" value={baseData ? `${formatTokenAmount(baseData.serviceFee)} KINIC` : "—"} />
          <Metric label="Per transfer" value={baseData ? `${formatTokenAmount(baseData.perDepositLimit)} KINIC` : "—"} />
          <Metric label="Available this period" value={remaining === undefined ? "—" : `${formatTokenAmount(remaining)} KINIC`} />
        </div>
      </div>
    </section>
    <section className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
      <Stat label="Deposits" value={canisterData?.counts.deposits.toString() ?? "—"} />
      <Stat label="Withdrawals" value={canisterData?.counts.withdrawals.toString() ?? "—"} />
      <Stat label="Unpaid withdrawals" value={canisterData?.unpaid_withdrawal_count.toString() ?? "—"} />
      <Stat label="Unpaid amount" value={canisterData ? `${formatTokenAmount(canisterData.unpaid_withdrawal_amount_out)} KINIC` : "—"} />
    </section>
    <section className="mt-5 rounded-[20px] bg-[var(--panel)] p-6">
      <h2 className="text-xl font-bold">Governance gas lane</h2>
      <p className="mt-1 text-sm text-[var(--muted)]">Mint gas is paid by the submitting Base wallet. Canister ETH is reserved only for governance operations.</p>
      <div className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <Metric label="Governance floor" value={canisterData ? `${formatEther(canisterData.reserve.governance_eth_floor_wei)} ETH` : "—"} />
        <Metric label="Total required" value={canisterData ? `${formatEther(canisterData.reserve.required_eth_wei)} ETH` : "—"} />
        <Metric label="Observed balance" value={canisterData ? `${formatEther(canisterData.reserve.eth_balance_wei)} ETH` : "—"} />
      </div>
    </section>
    <section className="mt-5 rounded-[20px] bg-[var(--panel)] p-6">
      <h2 className="text-xl font-bold">Withdrawal operations</h2>
      <div className="mt-5 grid gap-4 sm:grid-cols-2">
        <Metric label="Oldest unpaid observation" value={canisterData?.oldest_unpaid_withdrawal_observed_at_ns[0] !== undefined ? new Date(Number(canisterData.oldest_unpaid_withdrawal_observed_at_ns[0] / 1_000_000n)).toLocaleString() : "—"} />
        <Metric label="Ledger stop reasons" value={canisterData?.withdrawal_stop_reasons.join(", ") || "None"} />
      </div>
    </section>
  </div>
}

function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? error.message : fallback
}

function Metric({ label, value, inverse = false }: { label: string; value: string; inverse?: boolean }) {
  return <div><p className={`text-xs ${inverse ? "text-white/55" : "text-[var(--muted)]"}`}>{label}</p><p className="mt-1 text-lg font-bold">{value}</p></div>
}

function Stat({ label, value }: { label: string; value: string }) {
  return <div className="rounded-2xl bg-[var(--panel)] p-5"><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-2 text-3xl font-bold">{value}</p></div>
}
