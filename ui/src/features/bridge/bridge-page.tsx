import { Link } from "@tanstack/react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ArrowDownUp, ArrowRight, ChevronDown, Clock3, Flame, LockKeyhole, RefreshCcw, Settings } from "lucide-react"
import { Principal } from "@dfinity/principal"
import { useMemo, useState } from "react"
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
import { useCurrentBaseQuote, useRuntimeValidation, useRuntimeWriteReadiness } from "@/features/status/use-status"
import { useIcWallet } from "@/features/wallet/ic-wallet-provider"
import { useWalletDialog } from "@/features/wallet/wallet-controls"
import { bsnsAbi } from "@/generated/abi/bsns.generated"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { estimatedAmountOut, formatTokenAmount, parseTokenAmount, requiredDepositBalance } from "@/lib/amounts"
import { classifyDepositRecoverySequence } from "@/lib/deposit-recovery"
import { createLedgerActor, ledgerAccount } from "@/lib/ic/ledger"
import { createBridgeActor } from "@/lib/ic/bridge"
import type { DepositCall, IcAccount } from "@/lib/ic/wallet"
import { refetchRuntimeWriteReady } from "@/lib/runtime-validation"
import { currentInjectedWallet, sameIcAccount } from "@/lib/wallet-snapshot"

export type BridgeDirection = "deposit" | "withdraw"

interface UnresolvedDepositAttempt {
  call: DepositCall
  account: IcAccount
  recipient: `0x${string}`
}

export function BridgePage({ direction, onDirectionChange }: { direction: BridgeDirection; onDirectionChange: (direction: BridgeDirection) => void }) {
  const [depositAmount, setDepositAmount] = useState("")
  const [withdrawAmount, setWithdrawAmount] = useState("")
  const [minimum, setMinimum] = useState("")
  const [minimumEdited, setMinimumEdited] = useState(false)
  const [detailsOpen, setDetailsOpen] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const [disclosed, setDisclosed] = useState(false)
  const [unresolvedDeposit, setUnresolvedDeposit] = useState<UnresolvedDepositAttempt>()
  const [checkingDeposit, setCheckingDeposit] = useState(false)
  const [submittingWithdrawal, setSubmittingWithdrawal] = useState(false)
  const queryClient = useQueryClient()
  const { address, isConnected } = useAccount()
  const chainId = useChainId()
  const ic = useIcWallet()
  const wallets = useWalletDialog()
  const write = useWriteContract()
  const base = useCurrentBaseQuote()
  const runtime = useRuntimeValidation(chainId)
  const runtimeReadiness = useRuntimeWriteReadiness(runtime.data)
  const sendToken = direction === "deposit" ? deploymentProfile.icToken : deploymentProfile.baseToken
  const receiveToken = direction === "deposit" ? deploymentProfile.baseToken : deploymentProfile.icToken
  const baseData = !base.isError && !base.isStale ? base.data : undefined
  const depositParsed = useMemo(() => parseTokenAmount(depositAmount), [depositAmount])
  const withdrawParsed = useMemo(() => parseTokenAmount(withdrawAmount), [withdrawAmount])

  const ownerSequenceKey = ["deposit-owner-sequence", ic.account?.owner] as const
  const ownerSequence = useQuery({
    queryKey: ownerSequenceKey,
    enabled: false,
    queryFn: async () => {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      return actor.get_next_deposit_sequence(Principal.fromText(ic.account!.owner))
    },
  })
  const ledger = useQuery({
    queryKey: ["deposit-ledger", ic.account?.owner, bytesHex(ic.account?.subaccount ?? new Uint8Array())],
    enabled: false,
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
    enabled: false,
    queryFn: async () => (await createLedgerActor(deploymentProfile.icHost, deploymentProfile.ledgerCanisterId as string)).icrc1_fee(),
  })
  const bsnsBalance = useQuery({
    queryKey: ["bsns-balance", address],
    enabled: false,
    queryFn: () => publicClient().readContract({ address: deploymentProfile.bsnsAddress as `0x${string}`, abi: bsnsAbi, functionName: "balanceOf", args: [address!] }),
  })
  const ledgerData = !ledger.isError && !ledger.isStale ? ledger.data : undefined
  const ledgerFeeData = !ledgerFee.isError && !ledgerFee.isStale ? ledgerFee.data : undefined
  const bsnsBalanceData = !bsnsBalance.isError && !bsnsBalance.isStale ? bsnsBalance.data : undefined
  const estimate = withdrawParsed.ok && baseData && ledgerFeeData !== undefined ? estimatedAmountOut(withdrawParsed.value, baseData.serviceFee, ledgerFeeData) : 0n
  const displayedMinimum = minimumEdited ? minimum : estimate > 0n ? formatTokenAmount(estimate) : ""
  const parsedMinimum = parseTokenAmount(displayedMinimum)
  const ownerSequenceData = !ownerSequence.isError && !ownerSequence.isStale ? ownerSequence.data : undefined
  const refreshing = runtime.isFetching || base.isFetching || ledger.isFetching || ledgerFee.isFetching || bsnsBalance.isFetching || (!unresolvedDeposit && ownerSequence.isFetching)
  const refreshBridgeData = () => {
    const calls: Promise<unknown>[] = [runtime.refetch(), base.refetch()]
    if (direction === "deposit" && ic.account) {
      calls.push(ledger.refetch())
      if (!unresolvedDeposit) calls.push(ownerSequence.refetch())
    }
    if (direction === "withdraw" && address) calls.push(ledgerFee.refetch(), bsnsBalance.refetch())
    void Promise.all(calls)
  }

  const deposit = useMutation({
    mutationFn: async () => {
      if (unresolvedDeposit) {
        if (!address || !isConnected || !ic.account || !ic.adapter) throw new Error("Reconnect the wallets used for this deposit")
        const activeEvm = await currentInjectedWallet()
        const activeIc = await ic.adapter.getAccount()
        if (activeEvm.chainId !== deploymentProfile.chainId || activeEvm.address.toLowerCase() !== unresolvedDeposit.recipient.toLowerCase() || !sameIcAccount(activeIc, unresolvedDeposit.account)) throw new Error("Reconnect the original wallets before retrying this deposit")
        await refetchRuntimeWriteReady(() => runtime.refetch())
        return ic.adapter.requestDeposit(unresolvedDeposit.call)
      }
      if (!address || !isConnected) throw new Error("Connect the Base recipient wallet")
      if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
      if (!depositParsed.ok) throw new Error(depositParsed.reason)
      if (!disclosed) throw new Error("Acknowledge the bSNS governance disclosure")
      if (!ledgerData || !baseData || ownerSequenceData === undefined) throw new Error("Fee, balance, or owner sequence is unavailable or stale. Choose Refresh bridge data.")
      if (ledgerData.balance < requiredDepositBalance(depositParsed.value, ledgerData.fee, ledgerData.allowance)) throw new Error(`${deploymentProfile.icToken.symbol} balance does not cover the deposit and required ledger fees`)
      const confirmedAccount = { owner: ic.account.owner, subaccount: ic.account.subaccount }
      const confirmedRecipient = address
      const activeEvm = await currentInjectedWallet()
      const activeIc = await ic.adapter.getAccount()
      if (activeEvm.chainId !== deploymentProfile.chainId || activeEvm.address.toLowerCase() !== confirmedRecipient.toLowerCase() || !sameIcAccount(activeIc, confirmedAccount)) throw new Error("A connected account or chain changed; review and submit again")
      const requiredAllowance = depositParsed.value + ledgerData.fee
      if (ledgerData.allowance < requiredAllowance) {
        await refetchRuntimeWriteReady(() => runtime.refetch())
        await ic.adapter.approve({ amount: requiredAllowance, currentAllowance: ledgerData.allowance, ledgerFee: ledgerData.fee })
      }
      const finalEvm = await currentInjectedWallet()
      const finalIc = await ic.adapter.getAccount()
      if (finalEvm.chainId !== deploymentProfile.chainId || finalEvm.address.toLowerCase() !== confirmedRecipient.toLowerCase() || !sameIcAccount(finalIc, confirmedAccount)) throw new Error("A connected account or chain changed; review and submit again")
      const attempt: UnresolvedDepositAttempt = {
        call: { ownerSequence: ownerSequenceData, baseRecipient: hexToBytes(confirmedRecipient), grossAmount: depositParsed.value, maxServiceFee: baseData.serviceFee },
        account: { owner: confirmedAccount.owner, subaccount: confirmedAccount.subaccount?.slice() },
        recipient: confirmedRecipient,
      }
      await refetchRuntimeWriteReady(() => runtime.refetch())
      setUnresolvedDeposit(attempt)
      return ic.adapter.requestDeposit(attempt.call)
    },
    onSuccess: (receipt) => {
      queryClient.setQueryData(ownerSequenceKey, receipt.owner_sequence + 1n)
      setUnresolvedDeposit(undefined)
      setDisclosed(false); setDepositAmount("")
      toast.success(`Deposit ${bytesHex(receipt.deposit_id).slice(0, 14)}… accepted. Settlement confirmation will continue automatically; inspect it in History.`)
    },
    onError: (error) => {
      toast.error(error instanceof Error ? `${error.message}. Retry the same deposit or check whether it was accepted.` : "Deposit response is unresolved")
    },
  })

  const checkUnresolvedDeposit = async () => {
    if (!unresolvedDeposit) return
    setCheckingDeposit(true)
    try {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      const nextSequence = await actor.get_next_deposit_sequence(Principal.fromText(unresolvedDeposit.account.owner))
      const status = classifyDepositRecoverySequence(unresolvedDeposit.call.ownerSequence, nextSequence)
      if (status === "not-accepted") {
        queryClient.setQueryData(ownerSequenceKey, nextSequence)
        setUnresolvedDeposit(undefined)
        setDisclosed(false)
        toast.info("The deposit was not accepted. You can edit the form or submit a new request.")
      } else if (status === "accepted-or-conflicted") {
        toast.error("The owner sequence advanced. Retry the exact deposit to recover its receipt, or inspect History.")
      } else {
        toast.error("The owner sequence is inconsistent. This deposit remains locked; inspect History before continuing.")
      }
    } catch {
      toast.error("The owner sequence could not be checked. This deposit remains locked.")
    } finally {
      setCheckingDeposit(false)
    }
  }

  const submitWithdrawal = async () => {
    try {
      setSubmittingWithdrawal(true)
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
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const client = publicClient()
      const allowance = await client.readContract({
        address: deploymentProfile.bsnsAddress as `0x${string}`,
        abi: bsnsAbi,
        functionName: "allowance",
        args: [snapshotAddress, deploymentProfile.bridgeAddress as `0x${string}`],
      })
      if (allowance !== withdrawParsed.value) {
        const approvalHash = await write.writeContractAsync({
          account: snapshotAddress,
          address: deploymentProfile.bsnsAddress as `0x${string}`,
          abi: bsnsAbi,
          functionName: "approve",
          args: [deploymentProfile.bridgeAddress as `0x${string}`, withdrawParsed.value],
        })
        const approvalReceipt = await client.waitForTransactionReceipt({ hash: approvalHash })
        if (approvalReceipt.status !== "success") throw new Error("The exact bSNS approval reverted")
        const afterApprovalEvm = await currentInjectedWallet()
        const afterApprovalIc = await ic.adapter.getAccount()
        if (afterApprovalEvm.chainId !== deploymentProfile.chainId || afterApprovalEvm.address.toLowerCase() !== snapshotAddress.toLowerCase() || !sameIcAccount(afterApprovalIc, confirmedIcAccount)) throw new Error("A connected account or chain changed after approval; review and submit again")
      }
      const hash = await write.writeContractAsync({ account: snapshotAddress, address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, functionName: "createWithdrawal", args: [withdrawParsed.value, parsedMinimum.value, bytesToHex(owner), bytesToHex(subaccount)] })
      const withdrawalReceipt = await client.waitForTransactionReceipt({ hash })
      if (withdrawalReceipt.status !== "success") {
        throw new Error("The withdrawal transaction reverted")
      }
      setWithdrawAmount(""); setMinimum(""); setMinimumEdited(false)
      toast.success(`Withdrawal submitted: ${hash.slice(0, 12)}…. Waiting for Base safe confirmation.`)
      void (async () => {
        try {
          await waitUntilSafe(client, withdrawalReceipt.blockNumber)
          await ic.adapter!.notifyWithdrawal(hexToBytes(hash))
          toast.success("Withdrawal is safe and the ICP release was started automatically")
        } catch {
          toast.warning("The withdrawal transaction succeeded, but automatic notification did not finish. Reconnect the destination IC wallet and use Check and notify in History.")
        }
      })()
    } catch (error) { toast.error(error instanceof Error ? error.message : "Withdrawal failed") }
    finally { setSubmittingWithdrawal(false) }
  }

  const retryAccountMatches = unresolvedDeposit && ic.account ? sameIcAccount(ic.account, unresolvedDeposit.account) : false
  const retryRecipientMatches = unresolvedDeposit && address ? address.toLowerCase() === unresolvedDeposit.recipient.toLowerCase() : false
  const runtimeReason = runtime.isFetching ? "Runtime verification is in progress." : runtimeReadiness.reason
  const depositBlockers = unresolvedDeposit
    ? [runtimeReason, !ic.account && "Reconnect the original IC wallet", !address && "Reconnect the original Base wallet", ic.account && !retryAccountMatches && "Reconnect the original IC wallet", address && !retryRecipientMatches && "Reconnect the original Base wallet"].filter(Boolean) as string[]
    : [!address && "Connect both wallets", !ic.account && "Connect both wallets", runtimeReason, (!baseData || !ledgerData || ownerSequenceData === undefined) && "Fee, balance, or owner sequence is unavailable", !depositParsed.ok && (depositParsed.reason ?? "Enter an amount")].filter(Boolean) as string[]
  const withdrawalBlockers = [!address && "Connect both wallets", !ic.account && "Connect both wallets", runtimeReason, (!baseData || ledgerFeeData === undefined || bsnsBalanceData === undefined) && "Fee and balance data is unavailable", !withdrawParsed.ok && (withdrawParsed.reason ?? "Enter an amount"), !parsedMinimum.ok && "Set a valid minimum received", parsedMinimum.ok && parsedMinimum.value > estimate && "Minimum received exceeds the estimate"].filter(Boolean) as string[]
  const blockers = direction === "deposit" ? depositBlockers : withdrawalBlockers
  const amountError = !unresolvedDeposit && (direction === "deposit" ? (!depositParsed.ok ? depositParsed.reason : undefined) : (!withdrawParsed.ok ? withdrawParsed.reason : undefined))
  const amount = direction === "deposit" ? (unresolvedDeposit ? formatTokenAmount(unresolvedDeposit.call.grossAmount) : depositAmount) : withdrawAmount
  const balance = direction === "deposit" ? ledgerData?.balance : bsnsBalanceData
  const fee = unresolvedDeposit?.call.maxServiceFee ?? baseData?.serviceFee
  const receive = direction === "deposit" ? (unresolvedDeposit ? (unresolvedDeposit.call.grossAmount > unresolvedDeposit.call.maxServiceFee ? unresolvedDeposit.call.grossAmount - unresolvedDeposit.call.maxServiceFee : 0n) : depositParsed.ok && fee !== undefined ? (depositParsed.value > fee ? depositParsed.value - fee : 0n) : undefined) : (estimate > 0n ? estimate : undefined)
  const source = direction === "deposit" ? { network: "Internet Computer", wallet: unresolvedDeposit?.account.owner ?? ic.account?.owner ?? "Connect IC wallet" } : { network: "Base", wallet: address ?? "Connect Base wallet" }
  const destination = direction === "deposit" ? { network: "Base", wallet: unresolvedDeposit?.recipient ?? address ?? "Connect Base wallet" } : { network: "Internet Computer", wallet: ic.account?.owner ?? "Connect IC wallet" }

  const changeDirection = () => { if (unresolvedDeposit) return; setConfirming(false); setDisclosed(false); onDirectionChange(direction === "deposit" ? "withdraw" : "deposit") }
  return <div className="route-enter mx-auto max-w-[620px] pt-2 md:pt-4">
    <div className="mb-5 text-center md:mb-2"><p className="text-sm font-bold text-[var(--pink)]">IC ↔ Base</p><h1 className="font-display mt-1 text-[40px] leading-[1.1] text-black md:text-[32px]">Bridge KINIC</h1><p className="mx-auto mt-2 max-w-md text-[15px] leading-6 text-[var(--muted)] md:hidden">Move tokens between IC and Base with both wallets verified.</p></div>
    <nav className="mb-2 hidden items-center justify-end gap-2 md:flex" aria-label="Bridge shortcuts">
      <ShortcutLink to="/history" label="History"><Clock3 className="size-5" /></ShortcutLink>
      <ShortcutLink to="/status" label="Status"><Settings className="size-5" /></ShortcutLink>
    </nav>
    <section className="overflow-hidden rounded-[20px] bg-[var(--panel)] p-4 sm:p-5" aria-label="KINIC bridge">
      <div className={`kinic-lines mb-4 ${direction === "withdraw" ? "is-withdraw" : ""}`} aria-hidden="true"><i /><i /><i /></div>
      <div className="mb-3 flex justify-end"><Button size="sm" variant="ghost" disabled={refreshing} onClick={refreshBridgeData}><RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />{refreshing ? "Refreshing…" : "Refresh bridge data"}</Button></div>
      <div className="relative grid gap-2 sm:grid-cols-2">
        <EndpointCard label="From" network={source.network} wallet={source.wallet} disabled={Boolean(unresolvedDeposit)} onClick={() => wallets.openFor(direction === "deposit" ? "ic" : "base")} />
        <EndpointCard label="To" network={destination.network} wallet={destination.wallet} disabled={Boolean(unresolvedDeposit)} onClick={() => wallets.openFor(direction === "deposit" ? "base" : "ic")} />
        <button type="button" disabled={Boolean(unresolvedDeposit)} onClick={changeDirection} className="absolute left-1/2 top-1/2 z-10 grid size-8 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border-2 border-[var(--panel)] bg-black text-white transition duration-300 hover:rotate-180 hover:bg-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:bg-[var(--muted)] disabled:hover:rotate-0" aria-label="Reverse bridge direction"><ArrowDownUp className="size-3.5 sm:rotate-90" /></button>
      </div>
      <div className="mt-3 rounded-2xl bg-white p-4">
        <div className="flex items-center justify-between gap-4"><Label htmlFor="bridge-amount">You send</Label><span className="text-sm text-[var(--muted)]">Balance {balance !== undefined ? formatTokenAmount(balance) : "—"} {sendToken.symbol}</span></div>
        <div className="mt-1 flex items-center gap-3"><Input id="bridge-amount" disabled={Boolean(unresolvedDeposit)} aria-invalid={Boolean(amountError)} aria-describedby="bridge-amount-feedback" className="h-14 border-0 px-0 text-3xl font-medium focus:ring-0" inputMode="decimal" placeholder="0.00000000" value={amount} onChange={(event) => { if (direction === "deposit") setDepositAmount(event.target.value); else { setWithdrawAmount(event.target.value); setMinimumEdited(false) } }} /><span className="rounded-xl bg-[var(--panel)] px-3 py-2 text-sm font-bold">{sendToken.symbol}</span></div>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3 rounded-2xl bg-white p-4 text-sm"><Quote label="Current bridge fee" value={fee !== undefined ? `${formatTokenAmount(fee)} ${sendToken.symbol}` : "—"} /><Quote label="Estimated receive" value={receive !== undefined ? `${formatTokenAmount(receive)} ${receiveToken.symbol}` : "—"} /></div>
      {direction === "withdraw" && <div className="mt-3 rounded-2xl bg-white"><button type="button" className="flex w-full items-center justify-between p-4 text-sm font-bold" onClick={() => setDetailsOpen((value) => !value)} aria-expanded={detailsOpen}>Minimum received <ChevronDown className={`size-4 transition ${detailsOpen ? "rotate-180" : ""}`} /></button>{detailsOpen && <div className="border-t border-[var(--line)] p-4"><Input id="minimum-out" inputMode="decimal" value={displayedMinimum} onChange={(event) => { setMinimum(event.target.value); setMinimumEdited(true) }} /><p className="mt-2 text-xs leading-5 text-[var(--muted)]">Lower this only if you accept fee movement. A lower result is refunded on Base.</p></div>}</div>}
      {unresolvedDeposit && <div className="mt-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] p-4 text-sm text-[#8a4b08]"><p className="font-bold text-black">Deposit response unresolved</p><p className="mt-1 leading-5">Owner sequence {unresolvedDeposit.call.ownerSequence.toString()} is locked to this amount and Base recipient. Retry sends the exact same request and never creates a new sequence.</p><div className="mt-3 flex flex-wrap gap-2"><Button size="sm" variant="ghost" disabled={checkingDeposit || deposit.isPending} onClick={() => void checkUnresolvedDeposit()}>{checkingDeposit ? "Checking…" : "Check whether accepted"}</Button><Link to="/history" search={{ tab: "deposit" }} className="inline-flex h-9 items-center rounded-xl px-3 text-sm font-bold underline underline-offset-4">Open History</Link></div></div>}
      {runtimeReason && <div className="mt-4 flex items-center justify-between gap-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] px-4 py-3 text-sm text-[#d5691b]"><span>{runtimeReason}</span><Link to="/status" className="font-bold underline underline-offset-4">View status</Link></div>}
      <Button className="mt-3 h-14 w-full" size="lg" disabled={blockers.length > 0 || deposit.isPending || write.isPending || submittingWithdrawal} onClick={() => setConfirming(true)}>{direction === "deposit" ? (unresolvedDeposit ? "Retry same deposit" : "Bridge to Base") : "Bridge to IC"}<ArrowRight className="size-4" /></Button>
      <p id="bridge-amount-feedback" className="mt-3 min-h-4 text-center text-xs text-[var(--muted)]" aria-live="polite">{blockers.length > 0 ? `Next: ${blockers[0]}` : "Ready to review"}</p>
    </section>
    <BridgeConfirmationDialog direction={direction} open={confirming} setOpen={(open) => { setConfirming(open); if (!open) setDisclosed(false) }} disclosed={disclosed} setDisclosed={(value) => setDisclosed(value)} source={source.wallet} destination={destination.wallet} amount={amount} receive={receive} sendSymbol={sendToken.symbol} receiveSymbol={receiveToken.symbol} pending={deposit.isPending || write.isPending || submittingWithdrawal} onConfirm={() => { setConfirming(false); if (direction === "deposit") deposit.mutate(); else void submitWithdrawal() }} />
  </div>
}

function ShortcutLink({ to, label, children }: { to: "/history" | "/status"; label: string; children: React.ReactNode }) { return <Link to={to} search={to === "/history" ? { tab: "deposit" } : undefined} aria-label={`Open ${label.toLowerCase()}`} className="group relative grid size-12 place-items-center rounded-full border border-[var(--line-strong)] bg-[#ededed] text-black transition duration-300 hover:-translate-y-0.5 hover:border-black hover:bg-black hover:text-white focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)]"><span className="pointer-events-none absolute right-full mr-2 hidden rounded-lg bg-black px-2.5 py-1.5 text-xs font-bold text-white group-hover:block group-focus-visible:block">{label}</span>{children}</Link> }
function EndpointCard({ label, network, wallet, disabled, onClick }: { label: string; network: string; wallet: string; disabled?: boolean; onClick: () => void }) { return <button type="button" disabled={disabled} onClick={() => onClick()} className="min-w-0 rounded-2xl border border-[var(--line)] bg-white p-3.5 text-left transition duration-300 hover:-translate-y-[2px] hover:border-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:hover:translate-y-0 disabled:hover:border-[var(--line)]"><span className="text-xs font-medium text-[var(--muted)]">{label}</span><span className="mt-0.5 flex items-center justify-between gap-3"><strong className="text-base text-black">{network}</strong><LockKeyhole className="size-4 text-[var(--pink)]" /></span><span className="mt-1 block truncate text-xs text-[var(--muted)]">{wallet}</span></button> }
function Quote({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-1 font-bold text-black">{value}</p></div> }
function ConfirmRow({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-1 break-all text-sm font-bold text-black">{value}</p></div> }

export function BridgeConfirmationDialog({ direction, open, setOpen, disclosed, setDisclosed, source, destination, amount, receive, sendSymbol, receiveSymbol, pending, onConfirm }: { direction: BridgeDirection; open: boolean; setOpen: (open: boolean) => void; disclosed: boolean; setDisclosed: (value: boolean) => void; source: string; destination: string; amount: string; receive?: bigint; sendSymbol: string; receiveSymbol: string; pending: boolean; onConfirm: () => void }) {
  return <Dialog open={open} onOpenChange={(value) => setOpen(value)}><DialogContent><DialogHeader><DialogTitle>{direction === "deposit" ? "Confirm bridge to Base" : "Confirm irreversible burn"}</DialogTitle><DialogDescription>Review both wallets and the amount before opening the wallet prompt.</DialogDescription></DialogHeader><div className="mt-5 space-y-4 rounded-2xl bg-[var(--panel)] p-4"><ConfirmRow label="Source" value={source} /><ConfirmRow label="Destination" value={destination} /><ConfirmRow label="Send / receive" value={`${amount || "—"} ${sendSymbol} / ${receive !== undefined ? formatTokenAmount(receive) : "—"} ${receiveSymbol}`} />{direction === "withdraw" && <Alert tone="danger">Burning {sendSymbol} on Base is irreversible. If fees move below your minimum, the bridge prepares a Base refund.</Alert>}</div>{direction === "deposit" && <label className="mt-4 flex cursor-pointer items-start gap-3 rounded-2xl border border-[var(--line)] p-4"><Checkbox checked={disclosed} onCheckedChange={(value) => setDisclosed(value === true)} /><span className="text-sm leading-5"><strong className="text-black">I understand the Base token has no SNS governance power.</strong><span className="mt-1 block text-[var(--muted)]">The wallet will show a verified ICRC-21 consent message.</span></span></label>}<DialogFooter><DialogClose asChild><Button variant="ghost">Cancel</Button></DialogClose><Button variant={direction === "withdraw" ? "danger" : "default"} disabled={pending || (direction === "deposit" && !disclosed)} onClick={() => onConfirm()}>{direction === "withdraw" && <Flame className="size-4" />}{direction === "deposit" ? "Confirm and open wallet" : "Confirm burn"}</Button></DialogFooter></DialogContent></Dialog>
}

function bytesHex(bytes: Uint8Array | number[]) { return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}` }
function bytesToHex(bytes: Uint8Array): `0x${string}` { return `0x${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}` }
async function waitUntilSafe(client: ReturnType<typeof publicClient>, receiptBlock: bigint): Promise<void> {
  for (;;) {
    const safe = await client.getBlock({ blockTag: "safe" })
    if (safe.number !== null && safe.number >= receiptBlock) return
    await new Promise((resolve) => window.setTimeout(resolve, 15_000))
  }
}
function publicClient() { return createPublicClient({ chain: defineChain({ id: deploymentProfile.chainId, name: deploymentProfile.label, nativeCurrency: { name: "Ether", symbol: "ETH", decimals: 18 }, rpcUrls: { default: { http: [deploymentProfile.baseRpcUrl] } } }), transport: http(deploymentProfile.baseRpcUrl) }) }
