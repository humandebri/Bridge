import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useEffect, useMemo, useRef, useState } from "react"
import { toast } from "sonner"
import { useAccount, useChainId, useWriteContract } from "wagmi"
import { toHex } from "viem"
import type { DepositView } from "@/generated/bridge.did"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { deploymentProfile } from "@/config/profile"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { useRuntimeHeartbeat, useRuntimeValidation } from "@/features/status/use-status"
import { withBrowserLock } from "@/lib/browser-lock"
import { refetchRuntimeAttestedWriteReady } from "@/lib/runtime-validation"
import { basePublicClient } from "@/lib/evm/client"
import {
  contractAuthorization,
  validateMintAuthorization,
} from "@/lib/mint-authorization"
import { exactMintReceiptFinalization, type ExpectedDepositMint } from "@/lib/deposit-mint-finalization"
import {
  readPendingMint,
  removePendingMint,
  savePendingMint,
  type PendingMintExpectation,
} from "@/lib/pending-confirmations"

const attemptedAutoMintPrompts = new Set<string>()
const FINALIZED_OBSERVATION_REFRESH_MS = 45_000

export interface MintConfirmation {
  transactionHash: `0x${string}`
  recipient: `0x${string}`
  mintedAmount: bigint
}

export type MintProgressEvent =
  | { phase: "awaiting-wallet" }
  | { phase: "submitted"; transactionHash: `0x${string}` }
  | { phase: "included"; transactionHash: `0x${string}`; blockNumber: bigint }
  | { phase: "finalizing"; transactionHash: `0x${string}`; blockNumber: bigint }
  | { phase: "finalized"; transactionHash: `0x${string}`; blockNumber: bigint }
  | { phase: "attention"; message: string; transactionHash?: `0x${string}` }

export function MintAuthorizationAction({
  record,
  compact = false,
  onRequestRefund,
  claimingRefund = false,
  autoPromptOwner,
  mintBlockedReason,
  onMintConfirmed,
  onProgress,
  headless = false,
  registerAction,
}: {
  record: DepositView
  compact?: boolean
  onRequestRefund?: () => void
  claimingRefund?: boolean
  autoPromptOwner?: string
  mintBlockedReason?: string
  onMintConfirmed?: (confirmation: MintConfirmation) => void
  onProgress?: (event: MintProgressEvent) => void
  headless?: boolean
  registerAction?: (action?: { label: string; run: () => void | Promise<void>; pending?: boolean }) => void
}) {
  const { address } = useAccount()
  const chainId = useChainId()
  const write = useWriteContract()
  const queryClient = useQueryClient()
  const runtime = useRuntimeValidation(chainId, { enabled: true, gcTime: Infinity, staleTime: 60_000 })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, {
    enabled: runtime.data?.ready === true,
    refetchInterval: FINALIZED_OBSERVATION_REFRESH_MS,
  })
  const authorization = record.mint_authorization[0]
  const contract = useMemo(
    () => authorization ? contractAuthorization(authorization) : undefined,
    [authorization],
  )
  const pendingExpectation: PendingMintExpectation | undefined = useMemo(
    () => authorization && contract
      ? {
        depositId: contract.depositId,
        authorizationDigest: toHex(Uint8Array.from(authorization.digest)),
        recipient: contract.recipient,
        grossAmount: contract.grossAmount.toString(),
        chargedServiceFee: contract.chargedServiceFee.toString(),
        mintedAmount: (contract.grossAmount - contract.chargedServiceFee).toString(),
      }
      : undefined,
    [authorization, contract],
  )
  const authorizationAvailable = "AuthorizationAvailable" in record.state
  const [pending, setPending] = useState(() => pendingExpectation ? readPendingMint(pendingExpectation) : undefined)
  const [receiptConfirmed, setReceiptConfirmed] = useState(false)
  const [identityConflict, setIdentityConflict] = useState(false)
  const [receiptObservation, setReceiptObservation] = useState<"checking" | "unavailable" | "sequencer-success" | "sequencer-reverted">("checking")
  const [retryDialogOpen, setRetryDialogOpen] = useState(false)
  const notifiedConfirmation = useRef<string | undefined>(undefined)
  const autoPromptKey = autoPromptOwner && authorization
    ? `${autoPromptOwner}:${toHex(Uint8Array.from(authorization.digest))}`
    : undefined
  const [clockNow, setClockNow] = useState(() => Date.now())
  useEffect(() => {
    if (!authorization || !authorizationAvailable) return
    const timer = window.setInterval(() => setClockNow(Date.now()), 1_000)
    return () => window.clearInterval(timer)
  }, [authorization, authorizationAvailable])

  useEffect(() => {
    if (!pendingExpectation || !pending || receiptConfirmed || identityConflict || !authorizationAvailable) return
    let active = true
    const checkReceipt = async () => {
      try {
        const receipt = await basePublicClient.getTransactionReceipt({ hash: pending.transactionHash })
        if (receipt.blockNumber === null || receipt.blockHash === null) return
        if (active) setReceiptObservation(receipt.status === "success" ? "sequencer-success" : "sequencer-reverted")
        if (active) onProgress?.({ phase: "included", transactionHash: pending.transactionHash, blockNumber: receipt.blockNumber })
        const finalized = await basePublicClient.getBlock({ blockTag: "finalized" })
        if (finalized.number === null || receipt.blockNumber > finalized.number) return
        if (active) onProgress?.({ phase: "finalizing", transactionHash: pending.transactionHash, blockNumber: receipt.blockNumber })
        const canonicalReceiptBlock = await basePublicClient.getBlock({ blockNumber: receipt.blockNumber })
        const finalization = exactMintReceiptFinalization({
          expected: expectedDepositMint(pendingExpectation),
          expectedBridgeAddress: deploymentProfile.bridgeAddress as `0x${string}`,
          receipt,
          finalizedBlockNumber: finalized.number,
          canonicalReceiptBlockHash: canonicalReceiptBlock.hash,
        })
        if (active) {
          if (finalization === "finalized") {
            setReceiptConfirmed(true)
            onProgress?.({ phase: "finalized", transactionHash: pending.transactionHash, blockNumber: receipt.blockNumber })
          } else if (finalization === "conflict") {
            setIdentityConflict(true)
            onProgress?.({ phase: "attention", transactionHash: pending.transactionHash, message: "The finalized Base transaction does not match this Mint Authorization. Do not submit another transaction; review it in History." })
          } else if (finalization === "reverted") {
            await removePendingMint(pendingExpectation)
            setPending(undefined)
            setReceiptObservation("checking")
            onProgress?.({ phase: "attention", transactionHash: pending.transactionHash, message: "The Base mint transaction reverted. The authorization can be reviewed before retrying." })
          } else {
            setReceiptObservation("checking")
          }
        }
      } catch {
        if (active) {
          setReceiptObservation((current) => current === "sequencer-success" || current === "sequencer-reverted" ? current : "unavailable")
        }
      }
    }
    void checkReceipt()
    const timer = window.setInterval(() => void checkReceipt(), 10_000)
    return () => {
      active = false
      window.clearInterval(timer)
    }
  }, [authorizationAvailable, identityConflict, onProgress, pending, pendingExpectation, receiptConfirmed])

  useEffect(() => {
    if (!pendingExpectation || !pending || authorizationAvailable) return
    void removePendingMint(pendingExpectation).then(() => setPending(undefined))
  }, [authorizationAvailable, pending, pendingExpectation])

  useEffect(() => {
    if (!receiptConfirmed || !pending || !pendingExpectation) return
    const confirmationKey = `${pendingExpectation.authorizationDigest}:${pending.transactionHash}`
    if (notifiedConfirmation.current === confirmationKey) return
    notifiedConfirmation.current = confirmationKey
    const confirmation: MintConfirmation = {
      transactionHash: pending.transactionHash,
      recipient: pendingExpectation.recipient,
      mintedAmount: BigInt(pendingExpectation.mintedAmount),
    }
    void queryClient.invalidateQueries({ queryKey: ["deposit-history"] })
    if (onMintConfirmed) onMintConfirmed(confirmation)
    else toast.success(`Base mint confirmed (${pending.transactionHash.slice(0, 12)}…).`)
  }, [onMintConfirmed, pending, pendingExpectation, queryClient, receiptConfirmed])

  const mint = useMutation({
    mutationFn: async () => {
      if (!address) throw new Error("Connect a Base wallet to pay gas")
      if (chainId !== deploymentProfile.chainId) throw new Error("Switch the gas-paying wallet to Base")
      const observation = await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      const validated = await validateMintAuthorization(record, observation)
      onProgress?.({ phase: "awaiting-wallet" })
      const hash = await withBrowserLock(
        `kinic-wallet-prompt:base:${address.toLowerCase()}`,
        () => write.writeContractAsync({
          account: address,
          address: deploymentProfile.bridgeAddress as `0x${string}`,
          abi: bridgeAbi,
          functionName: "mintDepositWithAuthorization",
          args: [validated.authorization, validated.signature],
        }),
      )
      const expected = pendingExpectation
      if (!expected || validated.digest.toLowerCase() !== expected.authorizationDigest.toLowerCase()) {
        throw new Error("Mint authorization changed before transaction persistence")
      }
      const pendingMint = { ...expected, transactionHash: hash }
      await savePendingMint(pendingMint)
      onProgress?.({ phase: "submitted", transactionHash: hash })
      setReceiptObservation("checking")
      setPending(pendingMint)
      return { hash, recipient: validated.recipient }
    },
    onError: (error) => {
      onProgress?.({ phase: "attention", message: error instanceof Error ? error.message : "The Base mint could not be submitted." })
      toast.error(error instanceof Error ? error.message : "Base mint could not be submitted")
    },
  })
  const verifyRetry = useMutation({
    mutationFn: async () => {
      if (chainId !== deploymentProfile.chainId) throw new Error("Switch the gas-paying wallet to Base")
      const observation = await refetchRuntimeAttestedWriteReady(runtime.data, runtime.refetch, heartbeat.refetch)
      return validateMintAuthorization(record, observation)
    },
    onSuccess: () => setRetryDialogOpen(true),
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : "Pending mint could not be verified")
    },
  })

  const releasePendingForRetry = async () => {
    if (!pendingExpectation) return
    await removePendingMint(pendingExpectation)
    setPending(undefined)
    setReceiptConfirmed(false)
    setReceiptObservation("checking")
    setRetryDialogOpen(false)
  }

  const recipient = authorization
    ? `0x${Array.from(authorization.recipient, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
    : ""
  const finalizedTimestamp = heartbeat.data?.snapshot?.blockTimestamp
  const estimatedTimestamp = heartbeat.data?.snapshot
    ? heartbeat.data.snapshot.blockTimestamp + BigInt(Math.max(0, Math.floor((clockNow - heartbeat.data.checkedAt) / 1_000)))
    : undefined
  const estimatedDeadlinePassed = authorization !== undefined
    && estimatedTimestamp !== undefined
    && estimatedTimestamp > authorization.deadline
  const finalizedDeadlinePassed = authorization !== undefined
    && finalizedTimestamp !== undefined
    && finalizedTimestamp > authorization.deadline
  const runMint = mint.mutateAsync
  const mintPending = mint.isPending

  useEffect(() => {
    if (!registerAction) return
    if (pending || identityConflict || finalizedDeadlinePassed || mintBlockedReason || !address || finalizedTimestamp === undefined || heartbeat.isError || heartbeat.isStale) {
      registerAction(undefined)
      return
    }
    registerAction({
      label: "Confirm mint in Base wallet",
      pending: mintPending || write.isPending,
      run: async () => { await runMint() },
    })
    return () => registerAction(undefined)
  }, [address, finalizedDeadlinePassed, finalizedTimestamp, heartbeat.isError, heartbeat.isStale, identityConflict, mintBlockedReason, mintPending, pending, registerAction, runMint, write.isPending])

  useEffect(() => {
    if (finalizedDeadlinePassed && !pending) onProgress?.({ phase: "attention", message: "The Mint Authorization expired before a Base transaction was submitted. Open History to confirm the refund path." })
  }, [finalizedDeadlinePassed, onProgress, pending])

  useEffect(() => {
    if (!autoPromptKey
      || attemptedAutoMintPrompts.has(autoPromptKey)
      || !authorizationAvailable
      || !address
      || address.toLowerCase() !== recipient.toLowerCase()
      || chainId !== deploymentProfile.chainId
      || finalizedTimestamp === undefined
      || heartbeat.isError
      || heartbeat.isStale
      || finalizedDeadlinePassed
      || mintBlockedReason
      || identityConflict
      || pending
      || mint.isPending
      || write.isPending) return
    attemptedAutoMintPrompts.add(autoPromptKey)
    mint.mutate()
  }, [address, authorizationAvailable, autoPromptKey, chainId, finalizedDeadlinePassed, finalizedTimestamp, heartbeat.isError, heartbeat.isStale, identityConflict, mint, mintBlockedReason, pending, recipient, write.isPending])

  if (!authorization || !authorizationAvailable) return null
  const remaining = estimatedTimestamp === undefined
    ? undefined
    : authorization.deadline > estimatedTimestamp
      ? authorization.deadline - estimatedTimestamp
      : 0n
  const payerDiffers = Boolean(address && address.toLowerCase() !== recipient.toLowerCase())
  const pendingLabel = receiptObservation === "sequencer-success"
    ? "Included on Base; awaiting finality"
    : receiptObservation === "sequencer-reverted"
      ? "Transaction reverted; awaiting finality"
    : receiptObservation === "unavailable"
      ? "Base receipt unavailable; checking"
      : "Waiting for inclusion"

  if (headless) return null

  return <div className={compact ? "space-y-1" : "mt-4 rounded-2xl border border-[#bfd7ff] bg-[#eef5ff] p-4 text-sm"}>
    {!compact && <>
      <p className="font-bold text-black">{pending ? "Base mint transaction" : "Mint Authorization ready"}</p>
      {!pending && <p className="mt-1 text-[var(--muted)]">Mint recipient {recipient.slice(0, 10)}… · Authorization valid for {remaining === undefined ? "checking Base time" : formatRemaining(remaining)}</p>}
      {payerDiffers && <p className="mt-1 text-[#335f9d]">The connected wallet pays gas only; the signed authorization fixes the mint recipient.</p>}
      {pending && <p className="mt-1 text-[#335f9d]">Submitted {pending.transactionHash.slice(0, 12)}… — {receiptConfirmed ? "Base mint finalized." : identityConflict ? "Transaction does not match this authorization." : receiptObservation === "sequencer-success" ? "Included; waiting for Base finality." : receiptObservation === "sequencer-reverted" ? "Included as reverted; waiting for Base finality." : "Waiting for inclusion."}</p>}
    </>}
    {identityConflict
      ? <p className="font-bold text-[#b42318]">Deposit identity conflict. Do not submit another transaction.</p>
      : receiptConfirmed && pending
      ? <p className="font-bold text-[#176b3a]">Minted on Base</p>
      : finalizedDeadlinePassed && !pending
      ? <div className="space-y-2">
          <p className="text-xs font-bold text-[#8a4b08]">Expired. Claim a refund from History after Base Finalized time passes the deadline.</p>
          {onRequestRefund && <Button size="sm" variant="ghost" disabled={claimingRefund} onClick={onRequestRefund}>
            {claimingRefund ? "Checking Base…" : "Claim refund"}
          </Button>}
        </div>
      : <div className="space-y-1">
          {pending && finalizedDeadlinePassed && receiptObservation === "unavailable" && <p className="text-xs font-bold text-[#8a4b08]">The authorization deadline passed after submission, but no receipt is available yet. The transaction may still be pending or may revert; review the saved transaction before clearing it.</p>}
          {estimatedDeadlinePassed && <p className="text-xs font-bold text-[#8a4b08]">Estimated Base time has passed the deadline. A fresh finalized check will decide whether mint or refund is available.</p>}
          {mintBlockedReason && <p className="text-xs font-bold text-[#8a4b08]">{mintBlockedReason}</p>}
          {pending
            ? <p className={`text-xs font-bold ${receiptObservation === "sequencer-success" ? "text-[#176b3a]" : receiptObservation === "sequencer-reverted" ? "text-[#8a4b08]" : "text-[var(--muted)]"}`}>{pendingLabel}</p>
            : <Button size={compact ? "sm" : "lg"} className={compact ? "" : "mt-3 w-full"} disabled={Boolean(mintBlockedReason) || identityConflict || !address || finalizedTimestamp === undefined || heartbeat.isError || heartbeat.isStale || mint.isPending || write.isPending} onClick={() => {
                if (autoPromptKey) attemptedAutoMintPrompts.add(autoPromptKey)
                mint.mutate()
              }}>
                {mint.isPending ? "Minting…" : "Mint on Base"}
              </Button>}
        </div>}
    {!compact && pending && receiptObservation === "unavailable" && !receiptConfirmed && !identityConflict && <Button
      size="sm"
      variant="ghost"
      disabled={verifyRetry.isPending}
      onClick={() => verifyRetry.mutate()}
    >
      {verifyRetry.isPending ? "Checking saved transaction…" : "Review saved transaction"}
    </Button>}
    <Dialog open={retryDialogOpen} onOpenChange={setRetryDialogOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Clear the saved transaction reference?</DialogTitle>
          <DialogDescription>
            No receipt is currently available and the Deposit ID is still unprocessed on Base. Clearing this
            browser's saved transaction reference enables another submission. If the original transaction is
            mined later, the retry will revert and may cost additional gas.
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <DialogClose asChild><Button variant="ghost">Cancel</Button></DialogClose>
          <Button onClick={() => void releasePendingForRetry()}>Clear and retry</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
}

function expectedDepositMint(expected: PendingMintExpectation): ExpectedDepositMint {
  return {
    depositId: expected.depositId,
    recipient: expected.recipient,
    authorizationDigest: expected.authorizationDigest,
    grossAmount: BigInt(expected.grossAmount),
    serviceFee: BigInt(expected.chargedServiceFee),
    mintedAmount: BigInt(expected.mintedAmount),
  }
}

function formatRemaining(seconds: bigint): string {
  const minutes = seconds / 60n
  const rest = seconds % 60n
  return `${minutes.toString()}:${rest.toString().padStart(2, "0")}`
}
