import { createFileRoute, useNavigate } from "@tanstack/react-router"
import { useQuery, useQueryClient, type UseQueryResult } from "@tanstack/react-query"
import { Clock3, RefreshCcw } from "lucide-react"
import { createPublicClient, defineChain, hexToBytes, http, numberToHex } from "viem"
import { useAccount } from "wagmi"
import { Principal } from "@dfinity/principal"
import { Badge } from "@/components/ui/badge"
import { deploymentProfile } from "@/config/profile"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import type { DepositView, WithdrawalView } from "@/generated/bridge.did"
import { formatTokenAmount } from "@/lib/amounts"
import { createBridgeActor } from "@/lib/ic/bridge"
import { scanWithdrawalLogs, type FinalizedEventLog, type WithdrawalLogScan } from "@/lib/withdrawal-history"

type HistoryTab = "deposit" | "withdraw"
export const Route = createFileRoute("/history")({
  validateSearch: (search: Record<string, unknown>): { tab: HistoryTab } => ({ tab: search.tab === "withdraw" ? "withdraw" : "deposit" }),
  component: HistoryPage,
})

function HistoryPage() {
  const { tab } = Route.useSearch()
  const navigate = useNavigate({ from: "/history" })
  const { address } = useAccount()
  const ic = useIcWallet()
  const queryClient = useQueryClient()
  const deposits = useQuery({
    queryKey: ["deposit-history", ic.account?.owner],
    enabled: tab === "deposit" && Boolean(ic.account && deploymentProfile.bridgeCanisterId),
    queryFn: async () => {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      const result = await actor.list_deposit_ids({ owner: Principal.fromText(ic.account!.owner), before_sequence: [], limit: 20 })
      if ("Err" in result) throw new Error("Deposit history limit was rejected")
      return (await Promise.all(result.Ok.deposit_ids.map((id) => actor.get_deposit(id)))).flatMap((record) => record)
    }, refetchInterval: 15_000,
  })
  const withdrawalQueryKey = ["withdraw-history", deploymentProfile.chainId, deploymentProfile.bridgeAddress, address] as const
  const withdrawals = useQuery({
    queryKey: withdrawalQueryKey,
    enabled: tab === "withdraw" && Boolean(address && deploymentProfile.bridgeAddress && deploymentProfile.deploymentBlock !== null),
    queryFn: async () => {
      const client = publicClient()
      const finalized = await client.getBlock({ blockTag: "finalized" })
      if (finalized.number === null) throw new Error("Finalized Base block number is unavailable")
      const previous = queryClient.getQueryData<WithdrawalHistoryData>(withdrawalQueryKey)
      const scan = await scanWithdrawalLogs<WithdrawalEventLog>({
        deploymentBlock: deploymentProfile.deploymentBlock as bigint,
        finalizedBlock: finalized.number,
        previous,
        fetchLogs: async (fromBlock, toBlock) => client.getContractEvents({ address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, eventName: "WithdrawalCreated", args: { requester: address }, fromBlock, toBlock, strict: true }),
      })
      const bridge = deploymentProfile.bridgeCanisterId ? await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId) : undefined
      const views = bridge ? await bridge.get_withdrawals(scan.logs.map((log) => hexToBytes(numberToHex(log.args.withdrawalId, { size: 32 })))) : undefined
      if (views && "Err" in views) throw new Error("Canister rejected the withdrawal history batch")
      return { ...scan, items: scan.logs.map((log, index) => ({ id: log.args.withdrawalId, amount: log.args.amount, canister: views && "Ok" in views ? views.Ok[index]?.[0] : undefined })) }
    }, refetchInterval: 15_000,
  })
  return <div className="route-enter mx-auto max-w-3xl pt-8 md:pt-12">
    <header className="mb-8"><p className="text-sm font-medium text-[var(--pink)]">Activity</p><h1 className="font-display mt-2 text-[42px] leading-[1.1]">Bridge history</h1><p className="mt-3 max-w-xl text-base leading-6 text-[var(--muted)]">View transfers associated with the connected source wallet.</p></header>
    <div className="mb-5 inline-flex rounded-2xl bg-[var(--panel)] p-1" role="tablist" aria-label="History type"><Tab active={tab === "deposit"} onClick={() => void navigate({ search: { tab: "deposit" }, replace: true })}>Deposits</Tab><Tab active={tab === "withdraw"} onClick={() => void navigate({ search: { tab: "withdraw" }, replace: true })}>Withdrawals</Tab></div>
    <section className="min-h-80 rounded-[20px] bg-[var(--panel)] p-5 sm:p-7">
      {tab === "deposit" ? <DepositHistory query={deposits} connected={Boolean(ic.account)} /> : <WithdrawalHistory query={withdrawals} connected={Boolean(address)} />}
    </section>
    <p className="mt-4 text-xs leading-5 text-[var(--muted)]">Deposit IDs are available through a public owner index. Anyone who knows an IC Principal may correlate its records with Base recipients.</p>
  </div>
}

function Tab({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) { return <button role="tab" aria-selected={active} className={`rounded-xl px-5 py-2.5 text-sm font-bold transition ${active ? "bg-black text-white" : "text-[var(--muted)] hover:text-[var(--pink)]"}`} onClick={() => onClick()}>{children}</button> }
function Empty({ icon, title, message }: { icon: React.ReactNode; title: string; message: string }) { return <div className="grid min-h-64 place-items-center text-center"><div>{icon}<p className="mt-3 font-bold text-black">{title}</p><p className="mt-1 text-sm text-[var(--muted)]">{message}</p></div></div> }
function DepositHistory({ query, connected }: { query: UseQueryResult<DepositView[]>; connected: boolean }) { if (!connected) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title="Connect an IC wallet" message="Deposit history follows the connected IC account." />; if (query.isLoading) return <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading deposits" message="Reading the public canister index." />; if (query.isError || !query.data?.length) return <Empty icon={<Clock3 className="mx-auto size-6 text-[var(--pink)]" />} title={query.isError ? "Deposit history is unavailable" : "No deposits yet"} message="Completed and pending deposits will appear here." />; return <div className="space-y-3">{query.data.map((record) => <div key={bytesHex(record.deposit_id)} className="flex items-center justify-between gap-4 rounded-2xl bg-white p-4"><div><p className="text-sm font-bold">{bytesHex(record.deposit_id).slice(0, 18)}…</p><p className="mt-1 text-sm text-[var(--muted)]">{formatTokenAmount(record.net_amount)} KINIC on Base</p></div><Badge tone={record.state === "Minted" ? "good" : "neutral"}>{record.state}</Badge></div>)}</div> }
interface WithdrawalHistoryItem { id: bigint; amount: bigint; canister?: WithdrawalView }
interface WithdrawalEventLog extends FinalizedEventLog { args: { withdrawalId: bigint; amount: bigint } }
interface WithdrawalHistoryData extends WithdrawalLogScan<WithdrawalEventLog> { items: WithdrawalHistoryItem[] }
function WithdrawalHistory({ query, connected }: { query: UseQueryResult<WithdrawalHistoryData>; connected: boolean }) { if (!connected) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title="Connect a Base wallet" message="Withdrawal history follows the connected Base account." />; if (query.isLoading) return <Empty icon={<RefreshCcw className="mx-auto size-6 animate-spin text-[var(--pink)]" />} title="Loading withdrawals" message="Scanning finalized contract events." />; if (query.isError || !query.data?.items.length) return <Empty icon={<RefreshCcw className="mx-auto size-6 text-[var(--pink)]" />} title={query.isError ? "Withdrawal history is unavailable" : "No withdrawals yet"} message="Completed and pending withdrawals will appear here." />; return <div className="space-y-3">{query.data.items.map((item) => <div key={item.id.toString()} className="flex items-center justify-between gap-4 rounded-2xl bg-white p-4"><div><p className="text-sm font-bold">Withdrawal #{item.id.toString()}</p><p className="mt-1 text-sm text-[var(--muted)]">{formatTokenAmount(item.amount)} KINIC burned</p></div><Badge tone={item.canister?.state === "Released" ? "good" : "neutral"}>{item.canister?.state ?? "Awaiting notification"}</Badge></div>)}</div> }
function bytesHex(bytes: Uint8Array | number[]) { return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}` }
function publicClient() { return createPublicClient({ chain: defineChain({ id: deploymentProfile.chainId, name: deploymentProfile.label, nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: { default: { http: [deploymentProfile.baseRpcUrl] } } }), transport: http(deploymentProfile.baseRpcUrl) }) }
