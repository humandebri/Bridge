import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect, useState } from "react"
import { toast } from "sonner"
import { useAccount, useChainId, useWriteContract } from "wagmi"
import type { DepositView } from "@/generated/bridge.did"
import { bridgeAbi } from "@/generated/abi/bridge.generated"
import { deploymentProfile } from "@/config/profile"
import { Button } from "@/components/ui/button"
import { useRuntimeValidation } from "@/features/status/use-status"
import { refetchRuntimeWriteReady } from "@/lib/runtime-validation"
import { basePublicClient } from "@/lib/evm/client"
import {
  contractAuthorization,
  readPendingMint,
  removePendingMint,
  savePendingMint,
  validateMintAuthorization,
} from "@/lib/mint-authorization"

export function MintAuthorizationAction({
  record,
  compact = false,
  onExpiredReconcile,
  reconciling = false,
}: {
  record: DepositView
  compact?: boolean
  onExpiredReconcile?: () => void
  reconciling?: boolean
}) {
  const { address } = useAccount()
  const chainId = useChainId()
  const write = useWriteContract()
  const queryClient = useQueryClient()
  const runtime = useRuntimeValidation(chainId, { enabled: true, gcTime: Infinity, staleTime: Infinity })
  const authorization = record.mint_authorization[0]
  const depositId = authorization ? contractAuthorization(authorization).depositId : undefined
  const [pending, setPending] = useState(() => depositId ? readPendingMint(depositId) : undefined)
  const [receiptConfirmed, setReceiptConfirmed] = useState(false)
  const baseClock = useQuery({
    queryKey: ["mint-authorization-base-clock", deploymentProfile.chainId, deploymentProfile.bridgeAddress],
    enabled: Boolean(authorization && "AuthorizationAvailable" in record.state),
    queryFn: async () => (await basePublicClient.getBlock({ blockTag: "latest" })).timestamp,
    refetchInterval: 5_000,
    refetchIntervalInBackground: false,
    staleTime: 4_000,
  })

  useEffect(() => {
    if (!depositId || !pending || receiptConfirmed || !("AuthorizationAvailable" in record.state)) return
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
  }, [depositId, pending, receiptConfirmed, record.state])

  useEffect(() => {
    if (!depositId || !pending || "AuthorizationAvailable" in record.state) return
    void removePendingMint(depositId).then(() => setPending(undefined))
  }, [depositId, pending, record.state])

  const mint = useMutation({
    mutationFn: async () => {
      if (!address) throw new Error("Connect a Base wallet to pay gas")
      if (chainId !== deploymentProfile.chainId) throw new Error("Switch the gas-paying wallet to Base")
      await refetchRuntimeWriteReady(() => runtime.refetch())
      const validated = await validateMintAuthorization(record)
      const hash = await write.writeContractAsync({
        account: address,
        address: deploymentProfile.bridgeAddress as `0x${string}`,
        abi: bridgeAbi,
        functionName: "mintDepositWithAuthorization",
        args: [validated.authorization, validated.signature],
      })
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
      toast.success(`Base Mint済み (${hash.slice(0, 12)}…)。Canisterの期限後reconciliationを待っています。`)
      await queryClient.invalidateQueries({ queryKey: ["deposit-history"] })
    },
    onError: (error) => {
      toast.error(error instanceof Error ? error.message : "Base mint could not be submitted")
    },
  })

  if (!authorization || !("AuthorizationAvailable" in record.state)) return null
  const baseTimestamp = baseClock.data
  const expired = baseTimestamp !== undefined && baseTimestamp > authorization.deadline
  const remaining = baseTimestamp === undefined
    ? undefined
    : authorization.deadline > baseTimestamp
      ? authorization.deadline - baseTimestamp
      : 0n
  const recipient = `0x${Array.from(authorization.recipient, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
  const payerDiffers = Boolean(address && address.toLowerCase() !== recipient.toLowerCase())

  return <div className={compact ? "space-y-1" : "mt-4 rounded-2xl border border-[#bfd7ff] bg-[#eef5ff] p-4 text-sm"}>
    {!compact && <>
      <p className="font-bold text-black">Mint Authorization ready</p>
      <p className="mt-1 text-[var(--muted)]">Mint先 {recipient.slice(0, 10)}… · 残り {remaining === undefined ? "Base時刻確認中" : formatRemaining(remaining)}</p>
      {payerDiffers && <p className="mt-1 text-[#335f9d]">接続walletはgasだけを支払い、Mint先は署名済みrecipientです。</p>}
      {pending && <p className="mt-1 text-[#335f9d]">送信済み {pending.slice(0, 12)}… — {receiptConfirmed ? "Base Mint済み。Canister reconciliation待ちです。" : "Base receiptを確認中です。"}</p>}
    </>}
    {expired
      ? <div className="space-y-2">
          <p className="text-xs font-bold text-[#8a4b08]">期限切れ。Base Finalized上の未Mint証拠を確認できた場合だけ返金します。この操作は強制返金ではありません。</p>
          {onExpiredReconcile && <Button size="sm" variant="ghost" disabled={reconciling} onClick={onExpiredReconcile}>
            {reconciling ? "確認中…" : "安全な照合を開始"}
          </Button>}
        </div>
      : <Button size={compact ? "sm" : "lg"} className={compact ? "" : "mt-3 w-full"} disabled={!address || baseTimestamp === undefined || baseClock.isError || baseClock.isStale || Boolean(pending) || mint.isPending || write.isPending} onClick={() => mint.mutate()}>
          {pending ? "Base transaction pending" : mint.isPending ? "Minting…" : "Mint on Base"}
        </Button>}
  </div>
}

function formatRemaining(seconds: bigint): string {
  const minutes = seconds / 60n
  const rest = seconds % 60n
  return `${minutes.toString()}:${rest.toString().padStart(2, "0")}`
}
