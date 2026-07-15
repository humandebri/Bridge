import { createFileRoute } from "@tanstack/react-router"
import { CheckCircle2, RefreshCcw, ShieldAlert, TimerReset } from "lucide-react"
import { useChainId } from "wagmi"
import { Alert } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { Button } from "@/components/ui/button"
import { useBridgeStatus, useConfirmedBaseStatus, useRuntimeValidation } from "@/features/status/use-status"
import { formatTokenAmount } from "@/lib/amounts"
import { deploymentProfile } from "@/config/profile"
import { bytesHex } from "@/lib/runtime-validation"

export const Route = createFileRoute("/status")({ component: StatusPage })

function StatusPage() {
  const chainId = useChainId()
  const validation = useRuntimeValidation(chainId)
  const base = useConfirmedBaseStatus()
  const canister = useBridgeStatus()
  const runtime = validation.data
  const baseData = !base.isError && !base.isStale ? base.data : undefined
  const canisterData = !canister.isError && !canister.isStale ? canister.data : undefined
  const safeHash = canisterData ? bytesHex(canisterData.last_safe_base_block_hash, 32) : undefined
  const observedSigner = canisterData ? bytesHex(canisterData.observed_bridge_signer, 20) : undefined
  const observedRuntime = canisterData ? bytesHex(canisterData.observed_bridge_runtime_sha256, 32) : undefined
  const signerMatches = observedSigner?.toLowerCase() === deploymentProfile.expected_bridge_signer?.toLowerCase()
  const runtimeMatches = observedRuntime?.toLowerCase() === deploymentProfile.bridgeRuntimeHash?.toLowerCase()
  const observedChain = canisterData?.observed_base_chain_id[0]
  const quorumTime = canisterData?.last_safe_observation_ns ? new Date(Number(canisterData.last_safe_observation_ns / 1_000_000n)).toLocaleString() : "—"
  const mintedPercent = baseData && baseData.limit > 0n ? Number((baseData.minted * 10_000n) / baseData.limit) / 100 : 0
  const refreshing = validation.isFetching || base.isFetching || canister.isFetching
  const refresh = () => { void validation.refetch().then(() => Promise.all([base.refetch(), canister.refetch()])) }
  return <div className="route-enter mx-auto max-w-4xl pt-8 md:pt-12">
    <header className="mb-8 flex items-end justify-between gap-4"><div><p className="text-sm font-medium text-[var(--pink)]">Verified state</p><h1 className="font-display mt-2 text-[42px] leading-[1.1]">Bridge status</h1><p className="mt-3 max-w-xl text-base leading-6 text-[var(--muted)]">Evidence is read only when you explicitly refresh.</p></div><Button variant="ghost" disabled={refreshing} onClick={refresh}><RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />{refreshing ? "Refreshing…" : "Refresh"}</Button></header>
    {!runtime?.ready && <Alert tone="warning"><div className="flex gap-3"><ShieldAlert className="mt-1 size-5 shrink-0" /><div><strong>Bridge checks have not passed.</strong><ul className="mt-1 list-disc pl-5">{(runtime?.blockers ?? ["Checking the reviewed deployment profile…"]).map((item) => <li key={item}>{item}</li>)}</ul></div></div></Alert>}
    <div className="mt-5"><Alert tone="warning"><div className="flex gap-3"><ShieldAlert className="mt-1 size-5 shrink-0" /><div><strong>Safe is not finality.</strong><p className="mt-1">ICP release may start from this quorum-backed Safe observation. A reorg before finalization remains an accepted protocol risk.</p></div></div></Alert></div>
    <section className="mt-5 grid gap-5 md:grid-cols-[1.4fr_.6fr]">
      <div className="rounded-[20px] bg-[var(--panel)] p-6"><div className="flex items-start justify-between gap-4"><div><h2 className="text-xl font-bold">Mint window</h2><p className="mt-1 text-sm text-[var(--muted)]">Confirmed Base contract state.</p></div><Badge tone={baseData?.depositsPaused ? "warn" : "good"}>{baseData?.depositsPaused ? "Paused" : baseData ? "Open" : "Unavailable"}</Badge></div><div className="mt-8 flex items-end justify-between"><span className="text-4xl font-bold">{baseData ? formatTokenAmount(baseData.minted) : "—"}</span><span className="text-sm text-[var(--muted)]">of {baseData ? formatTokenAmount(baseData.limit) : "—"} KINIC</span></div><Progress value={mintedPercent} /><div className="mt-6 grid grid-cols-2 gap-4"><Metric label="Service fee" value={baseData ? `${formatTokenAmount(baseData.serviceFee)} KINIC` : "—"} /><Metric label="Per deposit" value={baseData ? `${formatTokenAmount(baseData.perDepositLimit)} KINIC` : "—"} /></div></div>
      <div className="rounded-[20px] bg-black p-6 text-white"><h2 className="text-xl font-bold">Confirmation</h2><div className="mt-8 space-y-6"><div className="flex items-center gap-3"><TimerReset className="size-5 text-[var(--pink)]" /><Metric label="Quorum Safe block" value={canisterData?.last_safe_base_block.toString() ?? "—"} inverse /></div><div className="flex items-center gap-3"><TimerReset className="size-5 text-[var(--pink)]" /><Metric label="Quorum observed at" value={quorumTime} inverse /></div><div className="flex items-center gap-3"><CheckCircle2 className="size-5 text-[var(--pink)]" /><Metric label="Reserve gate" value={canisterData ? (canisterData.reserve.sufficient ? "Sufficient" : "Insufficient") : "Unavailable"} inverse /></div></div></div>
    </section>
    <section className="mt-5 rounded-[20px] bg-[var(--panel)] p-6"><div className="flex items-start justify-between gap-4"><div><h2 className="text-xl font-bold">Quorum-bound Base identity</h2><p className="mt-1 text-sm text-[var(--muted)]">Canister 2-of-3 observation used for write readiness. Browser RPC checks canonical membership but does not replace this snapshot.</p></div><Badge tone={runtime?.ready ? "good" : "warn"}>{runtime?.ready ? "Profile matched" : "Check required"}</Badge></div><div className="mt-6 grid gap-4 sm:grid-cols-2"><Metric label="Safe block hash" value={compactHex(safeHash)} /><Metric label="Observed chain ID" value={observedChain?.toString() ?? "—"} /><Metric label="Bridge signer" value={`${compactHex(observedSigner)} · ${signerMatches ? "profile match" : "mismatch"}`} /><Metric label="Bridge runtime SHA-256" value={`${compactHex(observedRuntime)} · ${runtimeMatches ? "profile match" : "mismatch"}`} /></div></section>
    <section className="mt-5 grid gap-4 sm:grid-cols-2 lg:grid-cols-4"><Stat label="Deposits" value={canisterData?.counts.deposits.toString() ?? "—"} /><Stat label="Withdrawals" value={canisterData?.counts.withdrawals.toString() ?? "—"} /><Stat label="Pending EVM actions" value={canisterData?.counts.pending_evm_operations.toString() ?? "—"} /><Stat label="Reconciliation holds" value={canisterData?.counts.reconciliation_holds.toString() ?? "—"} /></section>
  </div>
}

function Metric({ label, value, inverse = false }: { label: string; value: string; inverse?: boolean }) { return <div><p className={`text-xs ${inverse ? "text-white/55" : "text-[var(--muted)]"}`}>{label}</p><p className="mt-1 text-lg font-bold">{value}</p></div> }
function Stat({ label, value }: { label: string; value: string }) { return <div className="rounded-2xl bg-[var(--panel)] p-5"><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-2 text-3xl font-bold">{value}</p></div> }
function compactHex(value?: string) { return value ? `${value.slice(0, 12)}…${value.slice(-8)}` : "—" }
