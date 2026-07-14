import { Link } from "@tanstack/react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ArrowDownUp, ArrowRight, ChevronDown, Clock3, Flame, LockKeyhole, RotateCcw, Settings } from "lucide-react"
import { Principal } from "@dfinity/principal"
import { useEffect, useMemo, useState } from "react"
import { toast } from "sonner"
import { createPublicClient, defineChain, hexToBytes, http } from "viem"
import { useAccount, useChainId, useWriteContract } from "wagmi"
import { Alert } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { deploymentProfile } from "@/config/profile"
import { useBaseStatus, useRuntimeValidation } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { useWalletDialog } from "@/features/wallet/wallet-controls"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { estimatedAmountOut, formatTokenAmount, parseTokenAmount } from "@/lib/amounts"
import { createLedgerActor, ledgerAccount } from "@/lib/ic/ledger"
import type { IcWalletAdapter } from "@/lib/ic/wallet"
import { currentInjectedWallet, sameIcAccount } from "@/lib/wallet-snapshot"
import { accountSubaccountHex, addPendingWithdrawal, assertPendingAccount, matchesPendingContext, readPendingWithdrawals, removePendingWithdrawal, type PendingWithdrawalNotification } from "@/lib/withdrawal-notifications"

export type BridgeDirection = "deposit" | "withdraw"

interface PendingDeposit { clientRequestId: string; baseRecipient: string; grossAmount: string; maxServiceFee: string; owner: string; subaccount: string }
const PENDING_KEY = "kinic-bridge.pending-deposit.v1"

export function BridgePage({ direction, onDirectionChange }: { direction: BridgeDirection; onDirectionChange: (direction: BridgeDirection) => void }) {
  const [depositAmount, setDepositAmount] = useState("")
  const [withdrawAmount, setWithdrawAmount] = useState("")
  const [minimum, setMinimum] = useState("")
  const [minimumEdited, setMinimumEdited] = useState(false)
  const [detailsOpen, setDetailsOpen] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const [disclosed, setDisclosed] = useState(false)
  const [notifying, setNotifying] = useState(false)
  const [pending, setPending] = useState<PendingDeposit | null>(() => {
    const stored = sessionStorage.getItem(PENDING_KEY)
    if (!stored) return null
    try { return JSON.parse(stored) as PendingDeposit } catch { return null }
  })
  const { address, isConnected } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const wallets = useWalletDialog()
  const queryClient = useQueryClient()
  const write = useWriteContract()
  const base = useBaseStatus()
  const runtime = useRuntimeValidation(chainId)
  const baseData = !base.isError && !base.isStale ? base.data : undefined
  const depositParsed = useMemo(() => parseTokenAmount(depositAmount), [depositAmount])
  const withdrawParsed = useMemo(() => parseTokenAmount(withdrawAmount), [withdrawAmount])

  useEffect(() => { if (pending) sessionStorage.setItem(PENDING_KEY, JSON.stringify(pending)); else sessionStorage.removeItem(PENDING_KEY) }, [pending])
  useEffect(() => {
    if (!ic.adapter || !ic.account) return
    for (const item of readPendingWithdrawals().filter((entry) => matchesPendingContext(entry, ic.account!, deploymentProfile))) {
      void finalizeAndNotify(item, ic.adapter).then(() => { removePendingWithdrawal(item.hash); void queryClient.invalidateQueries({ queryKey: ["withdraw-history"] }) }).catch(() => undefined)
    }
  }, [ic.account, ic.adapter, queryClient])

  const ledger = useQuery({
    queryKey: ["deposit-ledger", ic.account?.owner, bytesHex(ic.account?.subaccount ?? new Uint8Array())],
    enabled: direction === "deposit" && Boolean(ic.account && deploymentProfile.ledgerCanisterId && deploymentProfile.bridgeCanisterId),
    queryFn: async () => {
      const actor = await createLedgerActor(deploymentProfile.icHost, deploymentProfile.ledgerCanisterId as string)
      const account = ledgerAccount(ic.account!.owner, ic.account!.subaccount)
      const spender = ledgerAccount(deploymentProfile.bridgeCanisterId as string)
      const [balance, fee, allowance] = await Promise.all([actor.icrc1_balance_of(account), actor.icrc1_fee(), actor.icrc2_allowance({ account, spender })])
      return { balance, fee, allowance: allowance.allowance }
    },
  })
  const ledgerFee = useQuery({
    queryKey: ["withdraw-ledger-fee", deploymentProfile.ledgerCanisterId],
    enabled: direction === "withdraw" && Boolean(deploymentProfile.ledgerCanisterId),
    queryFn: async () => (await createLedgerActor(deploymentProfile.icHost, deploymentProfile.ledgerCanisterId as string)).icrc1_fee(),
  })
  const bsnsBalance = useQuery({
    queryKey: ["bsns-balance", address],
    enabled: direction === "withdraw" && Boolean(address && deploymentProfile.bsnsAddress),
    queryFn: () => publicClient().readContract({ address: deploymentProfile.bsnsAddress as `0x${string}`, abi: bsnsAbi, functionName: "balanceOf", args: [address!] }),
  })
  const ledgerData = !ledger.isError && !ledger.isStale ? ledger.data : undefined
  const ledgerFeeData = !ledgerFee.isError && !ledgerFee.isStale ? ledgerFee.data : undefined
  const bsnsBalanceData = !bsnsBalance.isError && !bsnsBalance.isStale ? bsnsBalance.data : undefined
  const estimate = withdrawParsed.ok && baseData && ledgerFeeData !== undefined ? estimatedAmountOut(withdrawParsed.value, baseData.serviceFee, ledgerFeeData) : 0n
  const displayedMinimum = minimumEdited ? minimum : estimate > 0n ? formatTokenAmount(estimate) : ""
  const parsedMinimum = parseTokenAmount(displayedMinimum)

  const deposit = useMutation({
    mutationFn: async () => {
      if (!runtime.data?.ready) throw new Error("Runtime verification has not passed")
      if (!address || !isConnected) throw new Error("Connect the Base recipient wallet")
      if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
      if (!depositParsed.ok) throw new Error(depositParsed.reason)
      if (!disclosed) throw new Error("Acknowledge the bSNS governance disclosure")
      if (!ledgerData || !baseData) throw new Error("Fee and balance data are unavailable or stale")
      if (ledgerData.balance < depositParsed.value + ledgerData.fee * 2n) throw new Error("KINIC balance does not cover the deposit and ledger fees")
      const confirmedAccount = { owner: ic.account.owner, subaccount: ic.account.subaccount }
      const confirmedRecipient = address
      const activeEvm = await currentInjectedWallet()
      const activeIc = await ic.adapter.getAccount()
      if (activeEvm.chainId !== deploymentProfile.chainId || activeEvm.address.toLowerCase() !== confirmedRecipient.toLowerCase() || !sameIcAccount(activeIc, confirmedAccount)) throw new Error("A connected account or chain changed; review and submit again")
      const retry = pending && pending.owner === confirmedAccount.owner && pending.subaccount === subaccountKey(confirmedAccount.subaccount) && pending.baseRecipient.toLowerCase() === confirmedRecipient.toLowerCase() && pending.grossAmount === depositParsed.value.toString() && pending.maxServiceFee === baseData.serviceFee.toString()
      const clientRequestId = retry ? hexToBytes(pending.clientRequestId as `0x${string}`) : randomRequestId()
      setPending({ clientRequestId: bytesHex(clientRequestId), baseRecipient: confirmedRecipient, grossAmount: depositParsed.value.toString(), maxServiceFee: baseData.serviceFee.toString(), owner: confirmedAccount.owner, subaccount: subaccountKey(confirmedAccount.subaccount) })
      const requiredAllowance = depositParsed.value + ledgerData.fee
      if (ledgerData.allowance < requiredAllowance) await ic.adapter.approve({ amount: requiredAllowance, currentAllowance: ledgerData.allowance, ledgerFee: ledgerData.fee })
      const finalEvm = await currentInjectedWallet()
      const finalIc = await ic.adapter.getAccount()
      if (finalEvm.chainId !== deploymentProfile.chainId || finalEvm.address.toLowerCase() !== confirmedRecipient.toLowerCase() || !sameIcAccount(finalIc, confirmedAccount)) throw new Error("A connected account or chain changed; review and submit again")
      return ic.adapter.requestDeposit({ clientRequestId, baseRecipient: hexToBytes(confirmedRecipient), grossAmount: depositParsed.value, maxServiceFee: baseData.serviceFee })
    },
    onSuccess: (receipt) => { setPending(null); setDisclosed(false); setDepositAmount(""); toast.success(`Deposit ${bytesHex(receipt.deposit_id).slice(0, 14)}… accepted`); void queryClient.invalidateQueries({ queryKey: ["deposit-history"] }); void queryClient.invalidateQueries({ queryKey: ["deposit-ledger"] }) },
    onError: (error) => toast.error(error instanceof Error ? error.message : "Deposit failed"),
  })

  const submitWithdrawal = async () => {
    try {
      if (!runtime.data?.ready) throw new Error("Runtime verification has not passed")
      if (!address) throw new Error("Connect the Base wallet that owns bSNS")
      if (!ic.account || !ic.adapter) throw new Error("Connect the destination IC wallet")
      if (!withdrawParsed.ok) throw new Error(withdrawParsed.reason)
      if (!parsedMinimum.ok) throw new Error(parsedMinimum.reason)
      if (parsedMinimum.value > estimate) throw new Error("Minimum amount out can only be lowered from the current estimate")
      if (baseData === undefined || ledgerFeeData === undefined || bsnsBalanceData === undefined) throw new Error("Fee or balance data is unavailable or stale")
      if (bsnsBalanceData < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
      const confirmedIcAccount = { owner: ic.account.owner, subaccount: ic.account.subaccount }
      const snapshotAddress = address
      const activeEvm = await currentInjectedWallet()
      const activeIc = await ic.adapter.getAccount()
      if (activeEvm.chainId !== deploymentProfile.chainId || activeEvm.address.toLowerCase() !== snapshotAddress.toLowerCase() || !sameIcAccount(activeIc, confirmedIcAccount)) throw new Error("A connected account or chain changed; review and submit again")
      const owner = Principal.fromText(confirmedIcAccount.owner).toUint8Array()
      const subaccount = confirmedIcAccount.subaccount ?? new Uint8Array(32)
      const hash = await write.writeContractAsync({ account: snapshotAddress, address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, functionName: "createWithdrawal", args: [withdrawParsed.value, parsedMinimum.value, bytesToHex(owner), bytesToHex(subaccount)] })
      const item: PendingWithdrawalNotification = { hash, owner: confirmedIcAccount.owner, subaccount: accountSubaccountHex(confirmedIcAccount), requester: snapshotAddress, chainId: deploymentProfile.chainId, bridgeAddress: deploymentProfile.bridgeAddress as `0x${string}` }
      addPendingWithdrawal(item); setWithdrawAmount(""); setMinimum(""); setMinimumEdited(false); setNotifying(true)
      toast.success(`Withdrawal submitted: ${hash.slice(0, 12)}…`)
      await finalizeAndNotify(item, ic.adapter)
      removePendingWithdrawal(hash); toast.success("Finalized withdrawal was queued for IC settlement"); await queryClient.invalidateQueries({ queryKey: ["withdraw-history"] })
    } catch (error) { toast.error(error instanceof Error ? error.message : "Withdrawal failed") }
    finally { setNotifying(false) }
  }

  const depositBlockers = [!address && "Connect both wallets", !ic.account && "Connect both wallets", !runtime.data?.ready && "Bridge checks have not passed", (!baseData || !ledgerData) && "Fee and balance data is unavailable", !depositParsed.ok && (depositParsed.reason ?? "Enter an amount")].filter(Boolean) as string[]
  const withdrawalBlockers = [!address && "Connect both wallets", !ic.account && "Connect both wallets", !runtime.data?.ready && "Bridge checks have not passed", (!baseData || ledgerFeeData === undefined || bsnsBalanceData === undefined) && "Fee and balance data is unavailable", !withdrawParsed.ok && (withdrawParsed.reason ?? "Enter an amount"), !parsedMinimum.ok && "Set a valid minimum received", parsedMinimum.ok && parsedMinimum.value > estimate && "Minimum received exceeds the estimate"].filter(Boolean) as string[]
  const blockers = direction === "deposit" ? depositBlockers : withdrawalBlockers
  const amount = direction === "deposit" ? depositAmount : withdrawAmount
  const balance = direction === "deposit" ? ledgerData?.balance : bsnsBalanceData
  const fee = baseData?.serviceFee
  const receive = direction === "deposit" ? (depositParsed.ok && fee !== undefined ? (depositParsed.value > fee ? depositParsed.value - fee : 0n) : undefined) : (estimate > 0n ? estimate : undefined)
  const source = direction === "deposit" ? { network: "Internet Computer", wallet: ic.account?.owner ?? "Connect IC wallet" } : { network: "Base", wallet: address ?? "Connect Base wallet" }
  const destination = direction === "deposit" ? { network: "Base", wallet: address ?? "Connect Base wallet" } : { network: "Internet Computer", wallet: ic.account?.owner ?? "Connect IC wallet" }

  const changeDirection = () => { setConfirming(false); setDisclosed(false); onDirectionChange(direction === "deposit" ? "withdraw" : "deposit") }
  return <div className="route-enter mx-auto max-w-[620px] pt-2 md:pt-4">
    <div className="mb-5 text-center md:mb-2"><p className="text-sm font-bold text-[var(--pink)]">IC ↔ Base</p><h1 className="font-display mt-1 text-[40px] leading-[1.1] text-black md:text-[32px]">Bridge KINIC</h1><p className="mx-auto mt-2 max-w-md text-[15px] leading-6 text-[var(--muted)] md:hidden">Move KINIC between IC and Base with both wallets verified.</p></div>
    <nav className="mb-2 hidden items-center justify-end gap-2 md:flex" aria-label="Bridge shortcuts">
      <ShortcutLink to="/history" label="History"><Clock3 className="size-5" /></ShortcutLink>
      <ShortcutLink to="/status" label="Status"><Settings className="size-5" /></ShortcutLink>
    </nav>
    <section className="overflow-hidden rounded-[20px] bg-[var(--panel)] p-4 sm:p-5" aria-label="KINIC bridge">
      <div className={`kinic-lines mb-4 ${direction === "withdraw" ? "is-withdraw" : ""}`} aria-hidden="true"><i /><i /><i /></div>
      <div className="relative grid gap-2 sm:grid-cols-2">
        <EndpointCard label="From" network={source.network} wallet={source.wallet} onClick={() => wallets.openFor(direction === "deposit" ? "ic" : "base")} />
        <EndpointCard label="To" network={destination.network} wallet={destination.wallet} onClick={() => wallets.openFor(direction === "deposit" ? "base" : "ic")} />
        <button type="button" onClick={changeDirection} className="absolute left-1/2 top-1/2 z-10 grid size-8 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border-2 border-[var(--panel)] bg-black text-white transition duration-300 hover:rotate-180 hover:bg-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]" aria-label="Reverse bridge direction"><ArrowDownUp className="size-3.5 sm:rotate-90" /></button>
      </div>
      <div className="mt-3 rounded-2xl bg-white p-4">
        <div className="flex items-center justify-between gap-4"><Label htmlFor="bridge-amount">You send</Label><span className="text-sm text-[var(--muted)]">Balance {balance !== undefined ? formatTokenAmount(balance) : "—"} KINIC</span></div>
        <div className="mt-1 flex items-center gap-3"><Input id="bridge-amount" className="h-14 border-0 px-0 text-3xl font-medium focus:ring-0" inputMode="decimal" placeholder="0.00000000" value={amount} onChange={(event) => { if (direction === "deposit") setDepositAmount(event.target.value); else { setWithdrawAmount(event.target.value); setMinimumEdited(false) } }} /><span className="rounded-xl bg-[var(--panel)] px-3 py-2 text-sm font-bold">KINIC</span></div>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3 rounded-2xl bg-white p-4 text-sm"><Quote label="Bridge fee" value={fee !== undefined ? `${formatTokenAmount(fee)} KINIC` : "—"} /><Quote label="You receive" value={receive !== undefined ? `${formatTokenAmount(receive)} KINIC` : "—"} /></div>
      {direction === "withdraw" && <div className="mt-3 rounded-2xl bg-white"><button type="button" className="flex w-full items-center justify-between p-4 text-sm font-bold" onClick={() => setDetailsOpen((value) => !value)} aria-expanded={detailsOpen}>Minimum received <ChevronDown className={`size-4 transition ${detailsOpen ? "rotate-180" : ""}`} /></button>{detailsOpen && <div className="border-t border-[var(--line)] p-4"><Input id="minimum-out" inputMode="decimal" value={displayedMinimum} onChange={(event) => { setMinimum(event.target.value); setMinimumEdited(true) }} /><p className="mt-2 text-xs leading-5 text-[var(--muted)]">Lower this only if you accept fee movement. A lower result is refunded on Base.</p></div>}</div>}
      {pending && direction === "deposit" && <Alert className="mt-3"><div className="flex gap-2"><RotateCcw className="mt-1 size-4 shrink-0" /><span>An uncertain previous response will reuse request <span className="font-mono">{pending.clientRequestId.slice(0, 14)}…</span>.</span></div></Alert>}
      {!runtime.data?.ready && <div className="mt-4 flex items-center justify-between gap-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] px-4 py-3 text-sm text-[#d5691b]"><span>Transfers are locked during preflight.</span><Link to="/status" className="font-bold underline underline-offset-4">View status</Link></div>}
      <Button className="mt-3 h-14 w-full" size="lg" disabled={blockers.length > 0 || deposit.isPending || write.isPending || notifying} onClick={() => setConfirming(true)}>{direction === "deposit" ? "Bridge to Base" : "Bridge to IC"}<ArrowRight className="size-4" /></Button>
      {blockers.length > 0 && <p className="mt-3 text-center text-xs text-[var(--muted)]">Next: {blockers[0]}</p>}
    </section>
    <BridgeConfirmationDialog direction={direction} open={confirming} setOpen={(open) => { setConfirming(open); if (!open) setDisclosed(false) }} disclosed={disclosed} setDisclosed={(value) => setDisclosed(value)} source={source.wallet} destination={destination.wallet} amount={amount} receive={receive} pending={deposit.isPending || write.isPending || notifying} onConfirm={() => { setConfirming(false); if (direction === "deposit") deposit.mutate(); else void submitWithdrawal() }} />
  </div>
}

function ShortcutLink({ to, label, children }: { to: "/history" | "/status"; label: string; children: React.ReactNode }) { return <Link to={to} search={to === "/history" ? { tab: "deposit" } : undefined} aria-label={`Open ${label.toLowerCase()}`} className="group relative grid size-12 place-items-center rounded-full border border-[var(--line-strong)] bg-[#ededed] text-black transition duration-300 hover:-translate-y-0.5 hover:border-black hover:bg-black hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]"><span className="pointer-events-none absolute right-full mr-2 hidden rounded-lg bg-black px-2.5 py-1.5 text-xs font-bold text-white group-hover:block group-focus-visible:block">{label}</span>{children}</Link> }
function EndpointCard({ label, network, wallet, onClick }: { label: string; network: string; wallet: string; onClick: () => void }) { return <button type="button" onClick={() => onClick()} className="min-w-0 rounded-2xl border border-[var(--line)] bg-white p-3.5 text-left transition duration-300 hover:-translate-y-[2px] hover:border-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]"><span className="text-xs font-medium text-[var(--muted)]">{label}</span><span className="mt-0.5 flex items-center justify-between gap-3"><strong className="text-base text-black">{network}</strong><LockKeyhole className="size-4 text-[var(--pink)]" /></span><span className="mt-1 block truncate text-xs text-[var(--muted)]">{wallet}</span></button> }
function Quote({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-1 font-bold text-black">{value}</p></div> }
function ConfirmRow({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-1 break-all text-sm font-bold text-black">{value}</p></div> }

export function BridgeConfirmationDialog({ direction, open, setOpen, disclosed, setDisclosed, source, destination, amount, receive, pending, onConfirm }: { direction: BridgeDirection; open: boolean; setOpen: (open: boolean) => void; disclosed: boolean; setDisclosed: (value: boolean) => void; source: string; destination: string; amount: string; receive?: bigint; pending: boolean; onConfirm: () => void }) {
  return <Dialog open={open} onOpenChange={(value) => setOpen(value)}><DialogContent><DialogHeader><DialogTitle>{direction === "deposit" ? "Confirm bridge to Base" : "Confirm irreversible burn"}</DialogTitle><DialogDescription>Review both wallets and the amount before opening the wallet prompt.</DialogDescription></DialogHeader><div className="mt-5 space-y-4 rounded-2xl bg-[var(--panel)] p-4"><ConfirmRow label="Source" value={source} /><ConfirmRow label="Destination" value={destination} /><ConfirmRow label="Send / receive" value={`${amount || "—"} / ${receive !== undefined ? formatTokenAmount(receive) : "—"} KINIC`} />{direction === "withdraw" && <Alert tone="danger">Burning KINIC on Base is irreversible. If fees move below your minimum, the bridge prepares a Base refund.</Alert>}</div>{direction === "deposit" && <label className="mt-4 flex cursor-pointer items-start gap-3 rounded-2xl border border-[var(--line)] p-4"><Checkbox checked={disclosed} onCheckedChange={(value) => setDisclosed(value === true)} /><span className="text-sm leading-5"><strong className="text-black">I understand the Base token has no SNS governance power.</strong><span className="mt-1 block text-[var(--muted)]">The wallet will show a verified ICRC-21 consent message.</span></span></label>}<DialogFooter><DialogClose asChild><Button variant="ghost">Cancel</Button></DialogClose><Button variant={direction === "withdraw" ? "danger" : "default"} disabled={pending || (direction === "deposit" && !disclosed)} onClick={() => onConfirm()}>{direction === "withdraw" && <Flame className="size-4" />}{direction === "deposit" ? "Confirm and open wallet" : "Confirm burn"}</Button></DialogFooter></DialogContent></Dialog>
}

function bytesHex(bytes: Uint8Array | number[]) { return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}` }
function randomRequestId() { const bytes = new Uint8Array(32); crypto.getRandomValues(bytes); return bytes }
function subaccountKey(subaccount?: Uint8Array) { return bytesHex(subaccount ?? new Uint8Array(32)) }
function bytesToHex(bytes: Uint8Array): `0x${string}` { return `0x${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}` }
function publicClient() { return createPublicClient({ chain: defineChain({ id: deploymentProfile.chainId, name: deploymentProfile.label, nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: { default: { http: [deploymentProfile.baseRpcUrl] } } }), transport: http(deploymentProfile.baseRpcUrl) }) }
const activeNotifications = new Set<string>()
async function finalizeAndNotify(item: PendingWithdrawalNotification, adapter: IcWalletAdapter) { if (activeNotifications.has(item.hash)) return; activeNotifications.add(item.hash); try { const client = publicClient(); const receipt = await client.waitForTransactionReceipt({ hash: item.hash }); if (receipt.status !== "success") throw new Error("Base withdrawal transaction reverted"); for (;;) { const finalized = await client.getBlock({ blockTag: "finalized" }); if (finalized.number >= receipt.blockNumber) break; await new Promise((resolve) => setTimeout(resolve, 12_000)) } const currentAccount = await adapter.getAccount(); assertPendingAccount(item, currentAccount); await adapter.notifyWithdrawal(hexToBytes(item.hash)) } finally { activeNotifications.delete(item.hash) } }
