import { Link } from "@tanstack/react-router"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { ArrowDownUp, ArrowRight, LockKeyhole, RefreshCcw } from "lucide-react"
import { Principal } from "@dfinity/principal"
import { useEffect, useMemo, useState } from "react"
import { toast } from "sonner"
import { hexToBytes } from "viem"
import { useAccount, useChainId, useWriteContract } from "wagmi"
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
import { basePublicClient } from "@/lib/evm/client"
import { refetchRuntimeWriteReady } from "@/lib/runtime-validation"
import { currentInjectedWallet, requireWalletSnapshot, sameIcAccount } from "@/lib/wallet-snapshot"
import { createWithdrawalAfterRevalidation } from "@/lib/withdrawal-submit"
import { savePendingConfirmation } from "@/lib/pending-confirmations"
import { readDepositIntent, removeDepositIntent, saveDepositIntent } from "@/lib/deposit-intents"

export type BridgeDirection = "deposit" | "withdraw"

interface UnresolvedDepositAttempt {
  call: DepositCall
  account: IcAccount
  recipient: `0x${string}`
}

export function BridgePage({ direction, onDirectionChange }: { direction: BridgeDirection; onDirectionChange: (direction: BridgeDirection) => void }) {
  const [depositAmount, setDepositAmount] = useState("")
  const [withdrawAmount, setWithdrawAmount] = useState("")
  const [confirming, setConfirming] = useState(false)
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
  const bsnsBalance = useQuery({
    queryKey: ["bsns-balance", address],
    enabled: false,
    queryFn: () => basePublicClient.readContract({ address: deploymentProfile.bsnsAddress as `0x${string}`, abi: bsnsAbi, functionName: "balanceOf", args: [address!] }),
  })
  const ledgerData = !ledger.isError && !ledger.isStale ? ledger.data : undefined
  const bsnsBalanceData = !bsnsBalance.isError && !bsnsBalance.isStale ? bsnsBalance.data : undefined
  const estimate = withdrawParsed.ok && baseData ? estimatedAmountOut(withdrawParsed.value, baseData.serviceFee) : 0n
  const ownerSequenceData = !ownerSequence.isError && !ownerSequence.isStale ? ownerSequence.data : undefined
  const refreshing = runtime.isFetching || base.isFetching || ledger.isFetching || bsnsBalance.isFetching || (!unresolvedDeposit && ownerSequence.isFetching)
  const refreshBridgeData = () => {
    const calls: Promise<unknown>[] = [runtime.refetch(), base.refetch()]
    if (direction === "deposit" && ic.account) {
      calls.push(ledger.refetch())
      if (!unresolvedDeposit) calls.push(ownerSequence.refetch())
    }
    if (direction === "withdraw" && address) calls.push(bsnsBalance.refetch())
    void Promise.all(calls)
  }

  useEffect(() => {
    const account = ic.account
    let active = true
    queueMicrotask(() => {
      if (active) setUnresolvedDeposit(account ? readDepositIntent(account) : undefined)
    })
    return () => { active = false }
  }, [ic.account])

  const deposit = useMutation({
    mutationFn: async (attempt: UnresolvedDepositAttempt) => {
      if (!address || !isConnected || !ic.account || !ic.adapter) throw new Error("Reconnect the wallets used for this deposit")
      const activeEvm = await currentInjectedWallet()
      const activeIc = await ic.adapter.getAccount()
      requireWalletSnapshot(
        { address: attempt.recipient, chainId: deploymentProfile.chainId, icAccount: attempt.account },
        { ...activeEvm, icAccount: activeIc },
        "before submitting this deposit",
      )
      await refetchRuntimeWriteReady(() => runtime.refetch())
      saveDepositIntent({ ...attempt, state: "submitted" })
      setUnresolvedDeposit(attempt)
      return ic.adapter.requestDeposit(attempt.call)
    },
    onSuccess: async (receipt, attempt) => {
      queryClient.setQueryData(["deposit-owner-sequence", attempt.account.owner], receipt.owner_sequence + 1n)
      const settlement = receipt.settlement[0]
      let submittedTransactionHash = settlement && "Submitted" in settlement
        ? settlement.Submitted.transaction_hash
        : undefined
      if (!submittedTransactionHash) {
        try {
          const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
          const [canonical] = await actor.get_deposit(receipt.deposit_id)
          const confirmation = canonical?.base_confirmation[0]
          if (confirmation && "Submitted" in confirmation) submittedTransactionHash = confirmation.Submitted.transaction_hash
        } catch {
          toast.warning("The deposit was accepted, but its canonical settlement could not be restored yet. Check it again before starting another deposit.")
          return
        }
      }
      if (submittedTransactionHash) {
        savePendingConfirmation({ kind: "deposit", settlementId: bytesHex(receipt.deposit_id), transactionHash: bytesHex(submittedTransactionHash), owner: attempt.account.owner })
      }
      removeDepositIntent(attempt.account)
      setUnresolvedDeposit(undefined)
      setDepositAmount("")
      toast.success(`Deposit ${bytesHex(receipt.deposit_id).slice(0, 14)}… accepted. finalized confirmation will be requested through your IC wallet.`)
    },
    onError: (error) => {
      toast.error(error instanceof Error ? `${error.message}. Retry the same deposit or check whether it was accepted.` : "Deposit response is unresolved")
    },
  })

  const submitDeposit = async () => {
    try {
      if (unresolvedDeposit) {
        deposit.mutate(unresolvedDeposit)
        return
      }
      if (!address || !isConnected) throw new Error("Connect the Base recipient wallet")
      if (!ic.account || !ic.adapter) throw new Error("Connect OISY or Plug")
      if (!depositParsed.ok) throw new Error(depositParsed.reason)
      if (!ledgerData || !baseData || ownerSequenceData === undefined) throw new Error("Balance or fee information is unavailable. Choose Refresh.")
      if (ledgerData.balance < requiredDepositBalance(depositParsed.value, ledgerData.fee, ledgerData.allowance)) throw new Error(`${deploymentProfile.icToken.symbol} balance does not cover the deposit and required ledger fees`)
      const confirmedAccount = { owner: ic.account.owner, subaccount: ic.account.subaccount }
      const confirmedRecipient = address
      const activeEvm = await currentInjectedWallet()
      const activeIc = await ic.adapter.getAccount()
      const expectedWallets = { address: confirmedRecipient, chainId: deploymentProfile.chainId, icAccount: confirmedAccount }
      requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc })
      const requiredAllowance = depositParsed.value + ledgerData.fee
      if (ledgerData.allowance < requiredAllowance) {
        await refetchRuntimeWriteReady(() => runtime.refetch())
        await ic.adapter.approve({ amount: requiredAllowance, currentAllowance: ledgerData.allowance, ledgerFee: ledgerData.fee })
      }
      const finalEvm = await currentInjectedWallet()
      const finalIc = await ic.adapter.getAccount()
      requireWalletSnapshot(expectedWallets, { ...finalEvm, icAccount: finalIc }, "during approval")
      const attempt: UnresolvedDepositAttempt = {
        call: { ownerSequence: ownerSequenceData, baseRecipient: hexToBytes(confirmedRecipient), grossAmount: depositParsed.value, maxServiceFee: baseData.serviceFee },
        account: { owner: confirmedAccount.owner, subaccount: confirmedAccount.subaccount?.slice() },
        recipient: confirmedRecipient,
      }
      saveDepositIntent({ ...attempt, state: "prepared" })
      setUnresolvedDeposit(attempt)
      deposit.mutate(attempt)
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "Deposit failed")
    }
  }

  const checkUnresolvedDeposit = async () => {
    if (!unresolvedDeposit) return
    setCheckingDeposit(true)
    try {
      const actor = await createBridgeActor(deploymentProfile.icHost, deploymentProfile.bridgeCanisterId as string)
      const nextSequence = await actor.get_next_deposit_sequence(Principal.fromText(unresolvedDeposit.account.owner))
      const status = classifyDepositRecoverySequence(unresolvedDeposit.call.ownerSequence, nextSequence)
      if (status === "not-accepted") {
        queryClient.setQueryData(["deposit-owner-sequence", unresolvedDeposit.account.owner], nextSequence)
        removeDepositIntent(unresolvedDeposit.account)
        setUnresolvedDeposit(undefined)
        toast.info("The deposit was not accepted. You can edit the form or submit a new request.")
      } else if (status === "accepted-or-conflicted") {
        const record = await actor.get_deposit_by_owner_sequence(
          Principal.fromText(unresolvedDeposit.account.owner),
          unresolvedDeposit.call.ownerSequence,
        )
        if (!record[0]
          || record[0].gross_amount !== unresolvedDeposit.call.grossAmount
          || record[0].max_service_fee !== unresolvedDeposit.call.maxServiceFee
          || bytesHex(record[0].base_recipient).toLowerCase() !== bytesHex(unresolvedDeposit.call.baseRecipient).toLowerCase()
          || bytesHex(record[0].from_subaccount[0] ?? new Uint8Array(32)) !== bytesHex(unresolvedDeposit.account.subaccount ?? new Uint8Array(32))) {
          throw new Error("Canonical deposit does not match the saved intent")
        }
        const confirmation = record[0].base_confirmation[0]
        if (confirmation && "Submitted" in confirmation) {
          savePendingConfirmation({
            kind: "deposit",
            settlementId: bytesHex(record[0].deposit_id),
            transactionHash: bytesHex(confirmation.Submitted.transaction_hash),
            owner: unresolvedDeposit.account.owner,
          })
        }
        queryClient.setQueryData(["deposit-owner-sequence", unresolvedDeposit.account.owner], nextSequence)
        removeDepositIntent(unresolvedDeposit.account)
        setUnresolvedDeposit(undefined)
        toast.success("The accepted deposit was recovered from canonical history.")
      } else {
        toast.error("This deposit needs attention. Check History before continuing.")
      }
    } catch (error) {
      toast.error(error instanceof Error ? error.message : "The deposit could not be checked. Try again from History.")
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
      if (baseData === undefined || bsnsBalanceData === undefined) throw new Error("Fee or balance data is unavailable or stale")
      if (withdrawParsed.value <= baseData.serviceFee) throw new Error("Amount must be greater than the current service fee")
      if (bsnsBalanceData < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
      const confirmedIcAccount = { owner: ic.account.owner, subaccount: ic.account.subaccount }
      const snapshotAddress = address
      const activeEvm = await currentInjectedWallet()
      const activeIc = await ic.adapter.getAccount()
      const expectedWallets = { address: snapshotAddress, chainId: deploymentProfile.chainId, icAccount: confirmedIcAccount }
      requireWalletSnapshot(expectedWallets, { ...activeEvm, icAccount: activeIc })
      const owner = Principal.fromText(confirmedIcAccount.owner).toUint8Array()
      const subaccount = confirmedIcAccount.subaccount ?? new Uint8Array(32)
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const client = basePublicClient
      const allowance = await client.readContract({
        address: deploymentProfile.bsnsAddress as `0x${string}`,
        abi: bsnsAbi,
        functionName: "allowance",
        args: [snapshotAddress, deploymentProfile.bridgeAddress as `0x${string}`],
      })
      if (allowance < withdrawParsed.value) {
        const approvalHash = await write.writeContractAsync({
          account: snapshotAddress,
          address: deploymentProfile.bsnsAddress as `0x${string}`,
          abi: bsnsAbi,
          functionName: "approve",
          args: [deploymentProfile.bridgeAddress as `0x${string}`, withdrawParsed.value],
        })
        const approvalReceipt = await client.waitForTransactionReceipt({ hash: approvalHash })
        if (approvalReceipt.status !== "success") throw new Error("Token approval failed")
      }
      const broadcast = await createWithdrawalAfterRevalidation({
        expectedWallets,
        refetchRuntime: () => runtime.refetch(),
        currentEvmWallet: currentInjectedWallet,
        currentIcAccount: () => ic.adapter!.getAccount(),
        refetchFinancials: async () => {
          const [quote, balanceResult] = await Promise.all([base.refetch(), bsnsBalance.refetch()])
          if (quote.isError || quote.isStale || !quote.data || balanceResult.isError || balanceResult.isStale || balanceResult.data === undefined) throw new Error("Fee or balance data changed and could not be verified")
          return { serviceFee: quote.data.serviceFee, balance: balanceResult.data }
        },
        validateFinancials: ({ serviceFee, balance: finalBalance }) => {
          if (withdrawParsed.value <= serviceFee) throw new Error("Amount must be greater than the current service fee")
          if (finalBalance < withdrawParsed.value) throw new Error("bSNS balance is insufficient")
        },
        createWithdrawal: ({ serviceFee }) => write.writeContractAsync({ account: snapshotAddress, address: deploymentProfile.bridgeAddress as `0x${string}`, abi: bridgeAbi, functionName: "createWithdrawal", args: [withdrawParsed.value, serviceFee, bytesToHex(owner), bytesToHex(subaccount)] }),
        onBroadcast: (transactionHash) => savePendingConfirmation({
          kind: "withdrawal",
          transactionHash,
          owner: confirmedIcAccount.owner,
        }),
      })
      setWithdrawAmount("")
      if (broadcast.pendingSaved) {
        toast.success(`Withdrawal submitted: ${broadcast.transactionHash.slice(0, 12)}…. Confirmation is pending and its status will update automatically.`)
      } else {
        toast.warning(`Withdrawal ${broadcast.transactionHash} was submitted, but this browser could not save it. Copy the transaction hash; after it succeeds, recover it from History.`)
      }
    } catch (error) { toast.error(error instanceof Error ? error.message : "Withdrawal failed") }
    finally { setSubmittingWithdrawal(false) }
  }

  const retryAccountMatches = unresolvedDeposit && ic.account ? sameIcAccount(ic.account, unresolvedDeposit.account) : false
  const retryRecipientMatches = unresolvedDeposit && address ? address.toLowerCase() === unresolvedDeposit.recipient.toLowerCase() : false
  const runtimeReason = runtimeReadiness.ready
    ? undefined
    : runtime.isFetching
      ? "Checking availability…"
      : runtime.data
        ? "Bridge is temporarily unavailable. Try Refresh."
        : "Refresh before continuing."
  const depositBlockers = unresolvedDeposit
    ? [runtimeReason, !ic.account && "Reconnect the original IC wallet", !address && "Reconnect the original Base wallet", ic.account && !retryAccountMatches && "Reconnect the original IC wallet", address && !retryRecipientMatches && "Reconnect the original Base wallet"].filter(Boolean) as string[]
    : [!address && "Connect both wallets", !ic.account && "Connect both wallets", runtimeReason, (!baseData || !ledgerData || ownerSequenceData === undefined) && "Balance or fee information is unavailable", !depositParsed.ok && (depositParsed.reason ?? "Enter an amount")].filter(Boolean) as string[]
  const withdrawalBlockers = [!address && "Connect both wallets", !ic.account && "Connect both wallets", runtimeReason, (!baseData || bsnsBalanceData === undefined) && "Fee and balance data is unavailable", !withdrawParsed.ok && (withdrawParsed.reason ?? "Enter an amount"), withdrawParsed.ok && baseData && withdrawParsed.value <= baseData.serviceFee && "Amount must exceed the service fee"].filter(Boolean) as string[]
  const blockers = direction === "deposit" ? depositBlockers : withdrawalBlockers
  const amountError = !unresolvedDeposit && (direction === "deposit" ? (!depositParsed.ok ? depositParsed.reason : undefined) : (!withdrawParsed.ok ? withdrawParsed.reason : undefined))
  const amount = direction === "deposit" ? (unresolvedDeposit ? formatTokenAmount(unresolvedDeposit.call.grossAmount) : depositAmount) : withdrawAmount
  const balance = direction === "deposit" ? ledgerData?.balance : bsnsBalanceData
  const fee = unresolvedDeposit?.call.maxServiceFee ?? baseData?.serviceFee
  const receive = direction === "deposit" ? (unresolvedDeposit ? (unresolvedDeposit.call.grossAmount > unresolvedDeposit.call.maxServiceFee ? unresolvedDeposit.call.grossAmount - unresolvedDeposit.call.maxServiceFee : 0n) : depositParsed.ok && fee !== undefined ? (depositParsed.value > fee ? depositParsed.value - fee : 0n) : undefined) : (estimate > 0n ? estimate : undefined)
  const source = direction === "deposit" ? { network: "Internet Computer", wallet: unresolvedDeposit?.account.owner ?? ic.account?.owner ?? "Connect IC wallet" } : { network: "Base", wallet: address ?? "Connect Base wallet" }
  const destination = direction === "deposit" ? { network: "Base", wallet: unresolvedDeposit?.recipient ?? address ?? "Connect Base wallet" } : { network: "Internet Computer", wallet: ic.account?.owner ?? "Connect IC wallet" }

  const changeDirection = () => { if (unresolvedDeposit) return; setConfirming(false); onDirectionChange(direction === "deposit" ? "withdraw" : "deposit") }
  return <div className="route-enter grid items-start gap-8 pb-6 pt-4 lg:grid-cols-[minmax(0,1fr)_minmax(560px,620px)] lg:gap-16 lg:pb-12 lg:pt-14 xl:gap-20">
    <div className="lg:sticky lg:top-28 lg:pt-12" data-testid="bridge-intro">
      <p className="text-xs font-bold uppercase tracking-[.18em] text-[var(--pink)]">IC ↔ Base</p>
      <h1 className="font-display mt-4 max-w-[460px] text-[42px] leading-[1.02] text-black sm:text-[52px] lg:text-[58px]">Bridge KINIC</h1>
      <p className="mt-5 max-w-[460px] text-[16px] leading-7 text-[var(--muted)] sm:text-[17px]">Move tokens between IC and Base with both wallets verified.</p>
      <div className="mt-8 hidden items-center gap-3 text-xs font-bold uppercase tracking-[.12em] text-[var(--support)] lg:flex"><span className="h-px w-12 bg-[var(--pink)]" />1:1 across both networks</div>
    </div>
    <section className="overflow-hidden rounded-[24px] border border-[var(--line)] bg-[var(--panel)] p-4 shadow-[0_24px_70px_rgba(20,34,53,.09)] sm:p-5" aria-label="KINIC bridge" data-testid="bridge-panel">
      <div className="mb-5 flex items-center justify-between gap-4">
        <div className={`kinic-rail ${direction === "withdraw" ? "is-withdraw" : ""}`} aria-hidden="true"><i /><i /><i /><i /></div>
        <Button size="sm" variant="ghost" disabled={refreshing} onClick={refreshBridgeData}><RefreshCcw className={refreshing ? "size-4 animate-spin" : "size-4"} />{refreshing ? "Refreshing…" : "Refresh"}</Button>
      </div>
      <div className="relative grid gap-2 sm:grid-cols-2">
        <EndpointCard label="From" network={source.network} wallet={source.wallet} disabled={Boolean(unresolvedDeposit)} onClick={() => wallets.openFor(direction === "deposit" ? "ic" : "base")} />
        <EndpointCard label="To" network={destination.network} wallet={destination.wallet} disabled={Boolean(unresolvedDeposit)} onClick={() => wallets.openFor(direction === "deposit" ? "base" : "ic")} />
        <button type="button" disabled={Boolean(unresolvedDeposit)} onClick={changeDirection} className="absolute left-1/2 top-1/2 z-10 grid size-8 -translate-x-1/2 -translate-y-1/2 place-items-center rounded-full border-2 border-[var(--panel)] bg-black text-white transition duration-300 hover:rotate-180 hover:bg-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:bg-[var(--muted)] disabled:hover:rotate-0" aria-label="Reverse bridge direction"><ArrowDownUp className="size-3.5 sm:rotate-90" /></button>
      </div>
      <div className="mt-3 rounded-2xl bg-white p-4">
        <div className="flex items-center justify-between gap-4"><Label htmlFor="bridge-amount">You send</Label><span className="text-sm text-[var(--muted)]">Balance {balance !== undefined ? formatTokenAmount(balance) : "—"} {sendToken.symbol}</span></div>
        <div className="mt-1 flex items-center gap-3"><Input id="bridge-amount" disabled={Boolean(unresolvedDeposit)} aria-invalid={Boolean(amountError)} aria-describedby="bridge-amount-feedback" className="font-numeric h-14 border-0 px-0 text-3xl font-semibold focus:ring-0" inputMode="decimal" placeholder="0.00000000" value={amount} onChange={(event) => { if (direction === "deposit") setDepositAmount(event.target.value); else setWithdrawAmount(event.target.value) }} /><span className="rounded-xl bg-[var(--panel)] px-3 py-2 text-sm font-bold">{sendToken.symbol}</span></div>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-3 rounded-2xl bg-white p-4 text-sm"><Quote label="Current bridge fee" value={fee !== undefined ? `${formatTokenAmount(fee)} ${sendToken.symbol}` : "—"} /><Quote label="Estimated receive" value={receive !== undefined ? `${formatTokenAmount(receive)} ${receiveToken.symbol}` : "—"} /></div>
      {direction === "withdraw" && <div className="mt-3 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] p-4 text-sm leading-5 text-[#8a4b08]"><strong className="text-black">Base refund is not available after burn.</strong><p className="mt-1">If delivery is interrupted, the bridge retries the same fixed amount to this same IC account.</p></div>}
      {unresolvedDeposit && <div className="mt-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] p-4 text-sm text-[#8a4b08]"><p className="font-bold text-black">Deposit status unavailable</p><p className="mt-1 leading-5">Check whether the deposit was accepted before starting another one.</p><div className="mt-3 flex flex-wrap gap-2"><Button size="sm" variant="ghost" disabled={checkingDeposit || deposit.isPending} onClick={() => void checkUnresolvedDeposit()}>{checkingDeposit ? "Checking…" : "Check status"}</Button><Link to="/history" search={{ tab: "deposit" }} className="inline-flex h-9 items-center rounded-xl px-3 text-sm font-bold underline underline-offset-4">Open History</Link></div></div>}
      {runtimeReason && <div className="mt-4 flex items-center justify-between gap-4 rounded-2xl border border-[#ffd19b] bg-[#fff3e4] px-4 py-3 text-sm text-[#d5691b]"><span>{runtimeReason}</span><Link to="/status" className="font-bold underline underline-offset-4">View status</Link></div>}
      <Button className="mt-3 h-14 w-full" size="lg" disabled={blockers.length > 0 || deposit.isPending || write.isPending || submittingWithdrawal} onClick={() => setConfirming(true)}>{direction === "deposit" ? (unresolvedDeposit ? "Retry same deposit" : "Bridge to Base") : "Bridge to IC"}<ArrowRight className="size-4" /></Button>
      <p id="bridge-amount-feedback" className="mt-3 min-h-4 text-center text-xs text-[var(--muted)]" aria-live="polite">{blockers.length > 0 ? `Next: ${blockers[0]}` : "Ready to review"}</p>
    </section>
    <BridgeConfirmationDialog direction={direction} open={confirming} setOpen={setConfirming} source={source.wallet} destination={destination.wallet} amount={amount} receive={receive} sendSymbol={sendToken.symbol} receiveSymbol={receiveToken.symbol} pending={deposit.isPending || write.isPending || submittingWithdrawal} onConfirm={() => { setConfirming(false); if (direction === "deposit") void submitDeposit(); else void submitWithdrawal() }} />
  </div>
}

function EndpointCard({ label, network, wallet, disabled, onClick }: { label: string; network: string; wallet: string; disabled?: boolean; onClick: () => void }) { return <button type="button" disabled={disabled} onClick={() => onClick()} className="min-w-0 rounded-2xl border border-[var(--line)] bg-white p-3.5 text-left transition duration-300 hover:-translate-y-[2px] hover:border-[var(--pink)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] disabled:cursor-not-allowed disabled:hover:translate-y-0 disabled:hover:border-[var(--line)]"><span className="text-xs font-medium text-[var(--muted)]">{label}</span><span className="mt-0.5 flex items-center justify-between gap-3"><strong className="text-base text-black">{network}</strong><LockKeyhole className="size-4 text-[var(--pink)]" /></span><span className="mt-1 block truncate text-xs text-[var(--muted)]">{wallet}</span></button> }
function Quote({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="font-numeric mt-1 font-bold text-black">{value}</p></div> }
function ConfirmRow({ label, value }: { label: string; value: string }) { return <div><p className="text-xs text-[var(--muted)]">{label}</p><p className="mt-1 break-all text-sm font-bold text-black">{value}</p></div> }

export function BridgeConfirmationDialog({ direction, open, setOpen, source, destination, amount, receive, sendSymbol, receiveSymbol, pending, onConfirm }: { direction: BridgeDirection; open: boolean; setOpen: (open: boolean) => void; source: string; destination: string; amount: string; receive?: bigint; sendSymbol: string; receiveSymbol: string; pending: boolean; onConfirm: () => void }) {
  const [burnAcknowledged, setBurnAcknowledged] = useState(false)
  const close = (value: boolean) => {
    if (!value) setBurnAcknowledged(false)
    setOpen(value)
  }
  return <Dialog open={open} onOpenChange={close}><DialogContent><DialogHeader><DialogTitle>{direction === "deposit" ? "Confirm bridge to Base" : "Confirm bridge to IC"}</DialogTitle><DialogDescription>Review both wallets and the amount before opening the wallet prompt.</DialogDescription></DialogHeader><div className="mt-5 space-y-4 rounded-2xl bg-[var(--panel)] p-4"><ConfirmRow label="Source" value={source} /><ConfirmRow label="Destination" value={destination} /><ConfirmRow label="Send / receive" value={`${amount || "—"} ${sendSymbol} / ${receive !== undefined ? formatTokenAmount(receive) : "—"} ${receiveSymbol}`} /></div>{direction === "withdraw" && <label className="mt-4 flex items-start gap-3 text-sm leading-5"><Checkbox aria-label="Acknowledge irreversible burn" checked={burnAcknowledged} onCheckedChange={(checked) => setBurnAcknowledged(checked === true)} /><span>I understand that confirming burns the Base tokens and no Base refund is available.</span></label>}<DialogFooter><DialogClose asChild><Button variant="ghost">Cancel</Button></DialogClose><Button disabled={pending || (direction === "withdraw" && !burnAcknowledged)} onClick={() => { setBurnAcknowledged(false); onConfirm() }}>Confirm and open wallet</Button></DialogFooter></DialogContent></Dialog>
}

function bytesHex(bytes: Uint8Array | number[]): `0x${string}` { return `0x${Array.from(bytes, (value) => Number(value).toString(16).padStart(2, "0")).join("")}` }
function bytesToHex(bytes: Uint8Array): `0x${string}` { return `0x${Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("")}` }
