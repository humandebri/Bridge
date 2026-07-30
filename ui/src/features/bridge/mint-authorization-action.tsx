import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect, useState } from "react"
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
import { useRuntimeValidation } from "@/features/status/use-status"
import { withBrowserLock } from "@/lib/browser-lock"
import { refetchRuntimeWriteReady } from "@/lib/runtime-validation"
import { basePublicClient } from "@/lib/evm/client"
import {
  contractAuthorization,
  validateMintAuthorization,
} from "@/lib/mint-authorization"
import {
  readPendingMint,
  removePendingMint,
  savePendingMint,
} from "@/lib/pending-confirmations"

const attemptedAutoMintPrompts = new Set<string>()

export function MintAuthorizationAction({
  record,
  compact = false,
  onRequestRefund,
  claimingRefund = false,
  autoPromptOwner,
  mintBlockedReason,
}: {
  record: DepositView
  compact?: boolean
  onRequestRefund?: () => void
  claimingRefund?: boolean
  autoPromptOwner?: string
  mintBlockedReason?: string
}) {
  const { address } = useAccount()
  const chainId = useChainId()
  const write = useWriteContract()
  const queryClient = useQueryClient()
  const runtime = useRuntimeValidation(chainId, { enabled: true, gcTime: Infinity, staleTime: Infinity })
  const authorization = record.mint_authorization[0]
  const depositId = authorization ? contractAuthorization(authorization).depositId : undefined
  const authorizationAvailable = "AuthorizationAvailable" in record.state
  const [pending, setPending] = useState(() => depositId ? readPendingMint(depositId) : undefined)
  const [receiptConfirmed, setReceiptConfirmed] = useState(false)
  const [retryDialogOpen, setRetryDialogOpen] = useState(false)
  const autoPromptKey = autoPromptOwner && authorization
    ? `${autoPromptOwner}:${toHex(Uint8Array.from(authorization.digest))}`
    : undefined
  const baseClock = useQuery({
    queryKey: ["mint-authorization-base-clock", deploymentProfile.chainId, deploymentProfile.bridgeAddress],
    enabled: Boolean(authorization && authorizationAvailable),
    queryFn: async () => (await basePublicClient.getBlock({ blockTag: "latest" })).timestamp,
    refetchInterval: 5_000,
    refetchIntervalInBackground: false,
    staleTime: 4_000,
  })

  useEffect(() => {
    if (!depositId || !pending || receiptConfirmed || !authorizationAvailable) return
    let active = true
    const checkReceipt = async () => {
      try {
        const receipt = await basePublicClient.getTransactionReceipt({ hash: pending })
        if (active) {
          if (receipt.status === "success") {
            setReceiptConfirmed(true)
          } else {
            await removePendingMint(depositId)
            setPending(undefined)
          }
        }
      } catch { /* A submitted transaction may remain pending or temporarily unavailable. */ }
    }
    void checkReceipt()
    const timer = window.setInterval(() => void checkReceipt(), 10_000)
    return () => {
      active = false
      window.clearInterval(timer)
    }
  }, [authorizationAvailable, depositId, pending, receiptConfirmed])

  useEffect(() => {
    if (!depositId || !pending || authorizationAvailable) return
    void removePendingMint(depositId).then(() => setPending(undefined))
  }, [authorizationAvailable, depositId, pending])

  const mint = useMutation({
    mutationFn: async () => {
      if (!address) throw new Error("Connect a Base wallet to pay gas")
      if (chainId !== deploymentProfile.chainId) throw new Error("Switch the gas-paying wallet to Base")
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const validated = await validateMintAuthorization(record)
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
      await savePendingMint(validated.authorization.depositId, hash)
      setPending(hash)
      const receipt = await basePublicClient.waitForTransactionReceipt({ hash })
      if (receipt.status !== "success") {
        await removePendingMint(validated.authorization.depositId)
        setPending(undefined)
        throw new Error("Base mint transaction reverted")
      }
      setReceiptConfirmed(true)
      return { hash, recipient: validated.recipient }
    },
    onSuccess: async ({ hash }) => {
      toast.success(`Base mint confirmed (${hash.slice(0, 12)}…).`)
      await queryClient.invalidateQueries({ queryKey: ["deposit-history"] })
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : "Base mint could not be submitted")
    },
  })
  const verifyRetry = useMutation({
    mutationFn: async () => {
      if (chainId !== deploymentProfile.chainId) throw new Error("Switch the gas-paying wallet to Base")
      await refetchRuntimeWriteReady(() => runtime.refetch())
      return validateMintAuthorization(record)
    },
    onSuccess: () => setRetryDialogOpen(true),
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : "Pending mint could not be verified")
    },
  })

  const releasePendingForRetry = async () => {
    if (!depositId) return
    await removePendingMint(depositId)
    setPending(undefined)
    setReceiptConfirmed(false)
    setRetryDialogOpen(false)
  }

  const recipient = authorization
    ? `0x${Array.from(authorization.recipient, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
    : ""
  const baseTimestamp = baseClock.data
  const expired = authorization !== undefined
    && baseTimestamp !== undefined
    && baseTimestamp > authorization.deadline

  useEffect(() => {
    if (!autoPromptKey
      || attemptedAutoMintPrompts.has(autoPromptKey)
      || !authorizationAvailable
      || !address
      || address.toLowerCase() !== recipient.toLowerCase()
      || chainId !== deploymentProfile.chainId
      || baseTimestamp === undefined
      || baseClock.isError
      || baseClock.isStale
      || expired
      || mintBlockedReason
      || pending
      || mint.isPending
      || write.isPending) return
    attemptedAutoMintPrompts.add(autoPromptKey)
    mint.mutate()
  }, [address, authorizationAvailable, autoPromptKey, baseClock.isError, baseClock.isStale, baseTimestamp, chainId, expired, mint, mintBlockedReason, pending, recipient, write.isPending])

  if (!authorization || !authorizationAvailable) return null
  const remaining = baseTimestamp === undefined
    ? undefined
    : authorization.deadline > baseTimestamp
      ? authorization.deadline - baseTimestamp
      : 0n
  const payerDiffers = Boolean(address && address.toLowerCase() !== recipient.toLowerCase())

  return <div className={compact ? "space-y-1" : "mt-4 rounded-2xl border border-[#bfd7ff] bg-[#eef5ff] p-4 text-sm"}>
    {!compact && <>
      <p className="font-bold text-black">Mint Authorization ready</p>
      <p className="mt-1 text-[var(--muted)]">Mint recipient {recipient.slice(0, 10)}… · Time remaining {remaining === undefined ? "Checking Base time" : formatRemaining(remaining)}</p>
      {payerDiffers && <p className="mt-1 text-[#335f9d]">The connected wallet pays gas only; the signed authorization fixes the mint recipient.</p>}
      {pending && <p className="mt-1 text-[#335f9d]">Submitted {pending.slice(0, 12)}… — {receiptConfirmed ? "Base mint confirmed." : "Checking Base receipt."}</p>}
    </>}
    {receiptConfirmed && pending
      ? <p className="font-bold text-[#176b3a]">Minted on Base</p>
      : expired
      ? <div className="space-y-2">
          <p className="text-xs font-bold text-[#8a4b08]">Expired. Claim a refund from History after Base Finalized time passes the deadline.</p>
          {onRequestRefund && <Button size="sm" variant="ghost" disabled={claimingRefund} onClick={onRequestRefund}>
            {claimingRefund ? "Checking Base…" : "Claim refund"}
          </Button>}
        </div>
      : <div className="space-y-1">
          {mintBlockedReason && <p className="text-xs font-bold text-[#8a4b08]">{mintBlockedReason}</p>}
          <Button size={compact ? "sm" : "lg"} className={compact ? "" : "mt-3 w-full"} disabled={Boolean(mintBlockedReason) || !address || baseTimestamp === undefined || baseClock.isError || baseClock.isStale || Boolean(pending) || mint.isPending || write.isPending} onClick={() => {
            if (autoPromptKey) attemptedAutoMintPrompts.add(autoPromptKey)
            mint.mutate()
          }}>
              {pending ? "Base transaction pending" : mint.isPending ? "Minting…" : "Mint on Base"}
            </Button>
        </div>}
    {pending && !receiptConfirmed && <Button
      size="sm"
      variant="ghost"
      disabled={verifyRetry.isPending}
      onClick={() => verifyRetry.mutate()}
    >
      {verifyRetry.isPending ? "Checking status…" : "Check status and retry"}
    </Button>}
    <Dialog open={retryDialogOpen} onOpenChange={setRetryDialogOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Clear the saved transaction?</DialogTitle>
          <DialogDescription>
            The Mint Authorization is still valid on Base. If the original transaction is mined later,
            the retry will revert and may cost additional gas.
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

function formatRemaining(seconds: bigint): string {
  const minutes = seconds / 60n
  const rest = seconds % 60n
  return `${minutes.toString()}:${rest.toString().padStart(2, "0")}`
}
