import { rememberMintRecovery, wasMintRequested } from "@/lib/mint-recovery"
import { loadIcHistoryOwner } from "@/lib/ic-history-owner"
import { readLatestBridgeProgress } from "@/lib/bridge-progress"
import { redactRpcUrls } from "@/lib/transfer-error"
import { observeMint } from "@/lib/mint-observation"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react"
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
import {
  useFinalizedBaseClock,
  useLatestBaseClock,
  useRuntimeHeartbeat,
  useRuntimeValidation,
} from "@/features/status/use-status"
import {
  startMintExecution,
  subscribeMintExecution,
  mintExecutionSnapshot,
  restoreMintExecution,
  mintExecutionBusy,
  mintExecutionMessage,
  mintExecutionDiagnostics,
  recordRecoveredMint,
  releaseFinalizedMintAttempt,
} from "@/lib/mint-execution"
import { prepareMint, checkMintDeadline, mintWalletConnected } from "@/lib/mint-preflight"
import { refetchRuntimeAttestedWriteReady } from "@/lib/runtime-validation"
import { contractAuthorization, validateMintAuthorization } from "@/lib/mint-authorization"
import { mintAuthorizationWindow } from "@/lib/mint-authorization-window"
import {
  readPendingMint,
  removePendingMint,
  savePendingMint,
  type PendingMintExpectation,
} from "@/lib/pending-confirmations"

const attemptedAutoMintPrompts = new Set<string>()

export interface MintConfirmation {
  transactionHash: `0x${string}`
  recipient: `0x${string}`
  mintedAmount: bigint
}

export type MintProgressEvent =
  | { phase: "awaiting-wallet" }
  | { phase: "storage-warning"; message: string }
  | { phase: "preparing" }
  | { phase: "submitted"; transactionHash: `0x${string}` }
  | {
      phase: "included"
      transactionHash: `0x${string}`
      blockNumber: bigint
      outcome: "success" | "reverted"
    }
  | { phase: "finalizing"; transactionHash: `0x${string}`; blockNumber: bigint }
  | { phase: "finalized"; transactionHash: `0x${string}`; blockNumber: bigint }
  | { phase: "attention"; message: string; transactionHash?: `0x${string}` }

export function MintAuthorizationAction({
  record,
  compact = false,
  onRequestRefund,
  claimingRefund = false,
  autoPromptOwner,
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
  onMintConfirmed?: (confirmation: MintConfirmation) => void
  onProgress?: (event: MintProgressEvent) => void
  headless?: boolean
  registerAction?: (action?: {
    label: string
    run: () => void | Promise<void>
    pending?: boolean
  }) => void
}) {
  const { address } = useAccount()
  const [restoredMint] = useState(
    () =>
      wasMintRequested(record) ||
      (autoPromptOwner !== undefined &&
        (readLatestBridgeProgress()?.createdAt ?? Infinity) < performance.timeOrigin),
  )
  const [storageWarning, setStorageWarning] = useState(false)
  const chainId = useChainId()
  const write = useWriteContract()
  const writeContractAsync = write.writeContractAsync
  const queryClient = useQueryClient()
  const runtime = useRuntimeValidation(chainId, {
    enabled: false,
    gcTime: Infinity,
    staleTime: 60_000,
  })
  const heartbeat = useRuntimeHeartbeat(chainId, runtime.data, {
    enabled: false,
  })
  const finalizedBaseClock = useFinalizedBaseClock({
    enabled: true,
    staleTime: 15_000,
    refetchInterval: 15_000,
  })
  const latestBaseClock = useLatestBaseClock({
    enabled: true,
    staleTime: 15_000,
    refetchInterval: 15_000,
  })
  const authorization = record.mint_authorization[0]
  const contract = useMemo(
    () => (authorization ? contractAuthorization(authorization) : undefined),
    [authorization],
  )
  const pendingExpectation: PendingMintExpectation | undefined = useMemo(
    () =>
      authorization && contract
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
  // IC polls recreate the authorization object; that must not cancel a receipt read.
  const pendingAuthorizationDigest = pendingExpectation?.authorizationDigest
  const hasPendingExpectation = pendingExpectation !== undefined
  const executionKey = [
    deploymentProfile.chainId,
    deploymentProfile.bridgeAddress,
    deploymentProfile.bridgeCanisterId,
    deploymentProfile.deploymentInstanceId,
    pendingExpectation?.depositId,
    pendingExpectation?.authorizationDigest,
  ]
    .join(":")
    .toLowerCase()
  const execution = useSyncExternalStore(
    subscribeMintExecution,
    () => mintExecutionSnapshot(executionKey),
    () => mintExecutionSnapshot(executionKey),
  )
  useEffect(() => {
    if (pendingExpectation) restoreMintExecution(executionKey)
  }, [executionKey, pendingExpectation])
  const [storedPending, setPending] = useState(() =>
    pendingExpectation ? readPendingMint(pendingExpectation) : undefined,
  )
  const pending = useMemo(() => {
    if (storedPending) return storedPending
    return execution.phase === "submitted" && execution.transactionHash && pendingExpectation
      ? { ...pendingExpectation, transactionHash: execution.transactionHash }
      : undefined
  }, [storedPending, execution.phase, execution.transactionHash, pendingExpectation])
  const progressCallback = useRef(onProgress)
  useEffect(() => {
    progressCallback.current = onProgress
  }, [onProgress])
  const [receiptConfirmed, setReceiptConfirmed] = useState(false)
  const [mintRecorded, setMintRecorded] = useState(false)
  const [terminalReverted, setTerminalReverted] = useState(false)
  const [notificationUnavailable, setNotificationUnavailable] = useState(false)
  const [identityConflict, setIdentityConflict] = useState(false)
  const [receiptObservation, setReceiptObservation] = useState<
    "checking" | "unavailable" | "sequencer-success" | "sequencer-reverted"
  >("checking")
  const [retryDialogOpen, setRetryDialogOpen] = useState(false)
  const notifiedConfirmation = useRef<string | undefined>(undefined)
  const autoPromptKey =
    autoPromptOwner && authorization
      ? `${autoPromptOwner}:${toHex(Uint8Array.from(authorization.digest))}`
      : undefined
  const [clockNow, setClockNow] = useState(() => Date.now())
  useEffect(() => {
    if (!authorization || !authorizationAvailable) return
    const timer = window.setInterval(() => setClockNow(Date.now()), 1_000)
    return () => window.clearInterval(timer)
  }, [authorization, authorizationAvailable])

  useEffect(() => {
    if (pending || !pendingExpectation) return
    const recovered = readPendingMint(pendingExpectation)
    if (recovered) recordRecoveredMint(executionKey, recovered.transactionHash)
  }, [clockNow, executionKey, pending, pendingExpectation])

  useEffect(() => {
    if (
      !pendingAuthorizationDigest ||
      !pending ||
      mintRecorded ||
      terminalReverted ||
      identityConflict ||
      !authorizationAvailable
    )
      return
    let active = true
    const checkReceipt = async () => {
      if (document.visibilityState !== "visible") return
      const observation = await observeMint(pending)
      if (!active) return
      setReceiptConfirmed(observation.status === "success" && observation.finalized)
      setMintRecorded(observation.recorded)
      setNotificationUnavailable(Boolean(observation.notificationError))
      if (observation.status === "success") {
        setReceiptObservation("sequencer-success")
        onProgress?.({
          phase: "included",
          transactionHash: pending.transactionHash,
          blockNumber: observation.blockNumber!,
          outcome: "success",
        })
        if (observation.finalized) {
          setReceiptConfirmed(true)
          onProgress?.({
            phase: "finalized",
            transactionHash: pending.transactionHash,
            blockNumber: observation.blockNumber!,
          })
        }
      } else if (observation.status === "reverted") {
        setReceiptObservation("sequencer-reverted")
        onProgress?.({
          phase: "included",
          transactionHash: pending.transactionHash,
          blockNumber: observation.blockNumber!,
          outcome: "reverted",
        })
        if (observation.finalized) {
          setTerminalReverted(true)
          onProgress?.({
            phase: "attention",
            transactionHash: pending.transactionHash,
            message: "The Base transaction reverted. Review the authorization before retrying.",
          })
        }
      } else if (observation.status === "conflict") {
        setIdentityConflict(true)
        onProgress?.({
          phase: "attention",
          transactionHash: pending.transactionHash,
          message: "The Base receipt does not match this mint authorization.",
        })
      } else {
        // The observer already preserves success through transport errors. A submitted
        // status here can carry newer reorg evidence even when the next read failed.
        setReceiptObservation(observation.unavailable ? "unavailable" : "checking")
        onProgress?.({ phase: "submitted", transactionHash: pending.transactionHash })
      }
    }

    void checkReceipt()
    const timer = window.setInterval(() => void checkReceipt(), 10_000)
    document.addEventListener("visibilitychange", checkReceipt)
    return () => {
      active = false
      window.clearInterval(timer)
      document.removeEventListener("visibilitychange", checkReceipt)
    }
  }, [
    authorizationAvailable,
    identityConflict,
    onProgress,
    pending,
    pendingAuthorizationDigest,
    mintRecorded,
    terminalReverted,
  ])

  useEffect(() => {
    if (
      !pendingExpectation ||
      !pending ||
      !("Minted" in record.state || "Refunded" in record.state)
    )
      return
    void removePendingMint(pendingExpectation).then(() => setPending(undefined))
  }, [record.state, pending, pendingExpectation])

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

  const executionBusy = mintExecutionBusy(execution)
  const executionBlocked =
    executionBusy || execution.phase === "unknown" || execution.phase === "submitted"
  const executeMint = useCallback(
    async (source: "automatic" | "manual" = "manual") => {
      if (!address || !pendingExpectation || chainId !== deploymentProfile.chainId) {
        toast.error("Connect the gas-paying wallet on Base")
        return
      }
      await startMintExecution({
        key: executionKey,
        wallet: address,
        source,
        connected: () => mintWalletConnected(address, chainId),
        readPending: () => readPendingMint(pendingExpectation)?.transactionHash,
        prepare: async (context) => {
          const result = await prepareMint(record, address, chainId, runtime.data, context)
          if (
            result.validated.digest.toLowerCase() !==
            pendingExpectation.authorizationDigest.toLowerCase()
          )
            throw new Error("Mint authorization changed before submission")
          context.check()
          const stored = await rememberMintRecovery(
            record,
            autoPromptOwner ?? loadIcHistoryOwner()?.account.owner,
            true,
          )
          context.check()
          if (!stored)
            throw new Error("Browser storage is unavailable. Retry after storage is available.")
          return result
        },
        beforeWallet: checkMintDeadline,
        send: ({ validated }) =>
          writeContractAsync({
            account: address,
            address: deploymentProfile.bridgeAddress as `0x${string}`,
            abi: bridgeAbi,
            functionName: "mintDepositWithAuthorization",
            args: [validated.authorization, validated.signature],
          }),
        save: async (hash) => {
          try {
            await savePendingMint({ ...pendingExpectation, transactionHash: hash })
          } catch (error) {
            setStorageWarning(true)
            progressCallback.current?.({
              phase: "storage-warning",
              message:
                "Browser storage is unavailable. Keep this page open while the transaction is checked.",
            })
            throw error
          }
        },
      })
    },
    [
      address,
      pendingExpectation,
      chainId,
      executionKey,
      record,
      runtime.data,
      writeContractAsync,
      autoPromptOwner,
    ],
  )
  useEffect(() => {
    if (pending) recordRecoveredMint(executionKey, pending.transactionHash)
  }, [executionKey, pending])
  useEffect(() => {
    if (execution.phase === "submitted" && execution.transactionHash && hasPendingExpectation) {
      progressCallback.current?.({ phase: "submitted", transactionHash: execution.transactionHash })
    } else if (executionBusy) {
      progressCallback.current?.({
        phase: execution.phase === "wallet" ? "awaiting-wallet" : "preparing",
      })
    } else if (
      execution.phase === "failed" ||
      execution.phase === "rejected" ||
      execution.phase === "unknown"
    ) {
      progressCallback.current?.({
        phase: "attention",
        message: redactRpcUrls(mintExecutionMessage(execution) ?? "Mint preflight failed"),
      })
    }
  }, [execution, executionBusy, hasPendingExpectation])
  const mint = useMemo(
    () => ({
      isPending: executionBusy,
      mutate: () => {
        void executeMint()
      },
      mutateAsync: executeMint,
    }),
    [executionBusy, executeMint],
  )
  const verifyRetry = useMutation({
    mutationFn: async () => {
      if (chainId !== deploymentProfile.chainId)
        throw new Error("Switch the gas-paying wallet to Base")
      const observation = await refetchRuntimeAttestedWriteReady(
        runtime.data,
        runtime.refetch,
        heartbeat.refetch,
      )
      return validateMintAuthorization(record, observation)
    },
    onSuccess: () => setRetryDialogOpen(true),
    onError: (error) => {
      toast.error(
        error instanceof Error
          ? redactRpcUrls(error.message)
          : "Pending mint could not be verified",
      )
    },
  })

  const releasePendingForRetry = async () => {
    if (!pendingExpectation) return
    if (autoPromptKey) attemptedAutoMintPrompts.add(autoPromptKey)
    setTerminalReverted(false)
    await removePendingMint(pendingExpectation)
    releaseFinalizedMintAttempt(executionKey)
    setPending(undefined)
    setReceiptConfirmed(false)
    setReceiptObservation("checking")
    setRetryDialogOpen(false)
  }

  const recipient = authorization
    ? `0x${Array.from(authorization.recipient, (byte) => Number(byte).toString(16).padStart(2, "0")).join("")}`
    : ""
  const finalizedTimestamp = finalizedBaseClock.data?.timestamp
  const estimatedLatestTimestamp = latestBaseClock.data
    ? latestBaseClock.data.timestamp +
      BigInt(Math.max(0, Math.floor((clockNow - latestBaseClock.dataUpdatedAt) / 1_000)))
    : undefined
  const authorizationWindow =
    authorization !== undefined && estimatedLatestTimestamp !== undefined
      ? mintAuthorizationWindow(authorization.deadline, estimatedLatestTimestamp)
      : undefined
  const authorizationExpired = authorizationWindow !== undefined && !authorizationWindow.isUnexpired
  const latestClockUnavailable =
    estimatedLatestTimestamp === undefined || latestBaseClock.isError || latestBaseClock.isStale
  const finalizedDeadlinePassed =
    authorization !== undefined &&
    finalizedTimestamp !== undefined &&
    finalizedTimestamp > authorization.deadline
  const runMint = mint.mutateAsync
  const mintPending = mint.isPending

  useEffect(() => {
    if (!registerAction) return
    if (pending || identityConflict || authorizationExpired || !address || latestClockUnavailable) {
      registerAction(undefined)
      return
    }
    registerAction({
      label: executionBusy
        ? (mintExecutionMessage(execution) ?? "Preparing mint…")
        : "Confirm mint in Base wallet",
      pending: mintPending || write.isPending,
      run: async () => {
        await runMint()
      },
    })
    return () => registerAction(undefined)
  }, [
    address,
    identityConflict,
    latestClockUnavailable,
    execution,
    executionBusy,
    mintPending,
    pending,
    registerAction,
    runMint,
    authorizationExpired,
    write.isPending,
  ])

  useEffect(() => {
    if (finalizedDeadlinePassed && !pending && !mintPending)
      onProgress?.({
        phase: "attention",
        message:
          "The Mint Authorization expired before a Base transaction was submitted. Open History to confirm the refund path.",
      })
  }, [finalizedDeadlinePassed, mintPending, onProgress, pending])

  useEffect(() => {
    if (
      restoredMint ||
      !autoPromptKey ||
      attemptedAutoMintPrompts.has(autoPromptKey) ||
      !authorizationAvailable ||
      !address ||
      address.toLowerCase() !== recipient.toLowerCase() ||
      chainId !== deploymentProfile.chainId ||
      estimatedLatestTimestamp === undefined ||
      latestBaseClock.isError ||
      latestBaseClock.isStale ||
      authorizationExpired ||
      identityConflict ||
      pending ||
      executionBlocked ||
      write.isPending
    )
      return
    attemptedAutoMintPrompts.add(autoPromptKey)
    void executeMint("automatic")
  }, [
    address,
    authorizationAvailable,
    autoPromptKey,
    restoredMint,
    executeMint,
    executionBlocked,
    chainId,
    estimatedLatestTimestamp,
    identityConflict,
    latestBaseClock.isError,
    latestBaseClock.isStale,
    mint,
    pending,
    recipient,
    authorizationExpired,
    write.isPending,
  ])

  if (!authorization || !authorizationAvailable) return null
  const remaining =
    authorizationWindow?.remainingSeconds === undefined
      ? undefined
      : authorizationWindow.remainingSeconds > 0n
        ? authorizationWindow.remainingSeconds
        : 0n
  const payerDiffers = Boolean(address && address.toLowerCase() !== recipient.toLowerCase())
  const pendingLabel =
    receiptObservation === "sequencer-success"
      ? "Success"
      : receiptObservation === "sequencer-reverted"
        ? "Transaction reverted"
        : receiptObservation === "unavailable"
          ? "Submitted on Base; refreshing transaction status."
          : "Waiting for inclusion"

  if (headless) return null

  return (
    <div
      className={
        compact ? "space-y-1" : "mt-4 rounded-2xl border border-[#bfd7ff] bg-[#eef5ff] p-4 text-sm"
      }
    >
      {storageWarning && (
        <p role="alert">
          Browser storage is unavailable. Keep this page open while the transaction is checked.
        </p>
      )}
      {restoredMint && !pending && (
        <p role="status">
          Checking Base for a completed mint. Wallet submission will not restart automatically.
        </p>
      )}
      {execution.phase !== "idle" && (
        <div className="space-y-1" role="status">
          {mintExecutionMessage(execution) && (
            <p>{redactRpcUrls(mintExecutionMessage(execution)!)}</p>
          )}
          {execution.transactionHash && <p className="break-all">{execution.transactionHash}</p>}
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              void navigator.clipboard.writeText(mintExecutionDiagnostics()).then(
                () => toast.success("Diagnostics copied"),
                () => toast.error("Could not copy diagnostics"),
              )
            }}
          >
            Copy mint diagnostics
          </Button>
        </div>
      )}
      {!compact && (
        <>
          <p className="font-bold text-black">
            {pending ? "Base mint transaction" : "Mint Authorization ready"}
          </p>
          {!pending && (
            <p className="mt-1 text-[var(--muted)]">
              Mint recipient {recipient.slice(0, 10)}… · Authorization valid for{" "}
              {remaining === undefined ? "checking Base time" : formatRemaining(remaining)}
            </p>
          )}
          {payerDiffers && (
            <p className="mt-1 text-[#335f9d]">
              The connected wallet pays gas only; the signed authorization fixes the mint recipient.
            </p>
          )}
          {pending && (
            <p className="mt-1 text-[#335f9d]">
              Submitted {pending.transactionHash.slice(0, 12)}… —{" "}
              {receiptConfirmed
                ? "Base mint finalized."
                : identityConflict
                  ? "Transaction does not match this authorization."
                  : receiptObservation === "sequencer-success"
                    ? "Included; waiting for Base finality."
                    : receiptObservation === "sequencer-reverted"
                      ? terminalReverted
                        ? "Finalized as reverted."
                        : "Included as reverted; waiting for Base finality."
                      : "Waiting for inclusion."}
            </p>
          )}
        </>
      )}
      {identityConflict ? (
        <p className="font-bold text-[#b42318]">
          Deposit identity conflict. Do not submit another transaction.
        </p>
      ) : receiptConfirmed && pending ? (
        <div>
          <p className="font-bold text-[#176b3a]">Success</p>
          <p className="text-xs text-[var(--muted)]">
            {mintRecorded
              ? "Recorded on IC"
              : notificationUnavailable
                ? "IC recording will retry automatically"
                : "Waiting for IC recording"}
          </p>
        </div>
      ) : finalizedDeadlinePassed && !pending ? (
        <div className="space-y-2">
          <p className="text-xs font-bold text-[#8a4b08]">
            The authorization deadline passed and Base Finalized time now permits the refund check.
          </p>
          {latestClockUnavailable && (
            <p className="text-xs font-bold text-[#8a4b08]">
              Latest Base time is unavailable, but it is not required for the finalized refund path.
            </p>
          )}
          {onRequestRefund && (
            <Button size="sm" variant="ghost" disabled={claimingRefund} onClick={onRequestRefund}>
              {claimingRefund ? "Checking Base…" : "Claim refund"}
            </Button>
          )}
        </div>
      ) : latestClockUnavailable && !pending ? (
        <p className="text-xs font-bold text-[#8a4b08]">
          Latest Base time could not be refreshed. No Base transaction was sent.
        </p>
      ) : authorizationExpired && !pending ? (
        <div className="space-y-2">
          <p className="text-xs font-bold text-[#8a4b08]">
            The mint authorization has expired, so no Base transaction will be sent. A refund
            becomes available after Base Finalized time passes the deadline.
          </p>
          {finalizedBaseClock.isError && (
            <p className="text-xs font-bold text-[#8a4b08]">
              Base Finalized state could not be refreshed, so refund eligibility cannot be checked
              yet.
            </p>
          )}
          {finalizedDeadlinePassed && onRequestRefund && (
            <Button size="sm" variant="ghost" disabled={claimingRefund} onClick={onRequestRefund}>
              {claimingRefund ? "Checking Base…" : "Claim refund"}
            </Button>
          )}
        </div>
      ) : (
        <div className="space-y-1">
          {pending && finalizedDeadlinePassed && receiptObservation === "unavailable" && (
            <p className="text-xs font-bold text-[#8a4b08]">
              The authorization deadline passed after submission, but no receipt is available yet.
              The transaction may still be pending or may revert; review the saved transaction
              before clearing it.
            </p>
          )}
          {pending ? (
            <p
              className={`text-xs font-bold ${receiptObservation === "sequencer-success" ? "text-[#176b3a]" : receiptObservation === "sequencer-reverted" ? "text-[#8a4b08]" : "text-[var(--muted)]"}`}
            >
              {pendingLabel}
            </p>
          ) : (
            <Button
              size={compact ? "sm" : "lg"}
              className={compact ? "" : "mt-3 w-full"}
              disabled={identityConflict || !address || executionBlocked || write.isPending}
              onClick={() => {
                if (autoPromptKey) attemptedAutoMintPrompts.add(autoPromptKey)
                mint.mutate()
              }}
            >
              {executionBusy
                ? mintExecutionMessage(execution)
                : execution.phase === "failed" || execution.phase === "rejected"
                  ? "Retry mint"
                  : "Mint on Base"}
            </Button>
          )}
        </div>
      )}
      {(!compact || terminalReverted) &&
        pending &&
        (receiptObservation === "unavailable" || terminalReverted) &&
        !receiptConfirmed &&
        !identityConflict && (
          <Button
            size="sm"
            variant="ghost"
            disabled={verifyRetry.isPending}
            onClick={() => {
              // A finalized revert permits clearing the reference without revalidating an
              // expired authorization. Any subsequent mint still validates independently.
              if (terminalReverted) setRetryDialogOpen(true)
              else verifyRetry.mutate()
            }}
          >
            {verifyRetry.isPending ? "Checking saved transaction…" : "Review saved transaction"}
          </Button>
        )}
      <Dialog open={retryDialogOpen} onOpenChange={setRetryDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Clear the saved transaction reference?</DialogTitle>
            <DialogDescription>
              {terminalReverted
                ? "The Base transaction finalized as reverted. Clear this reference to review the available mint or refund action."
                : "No receipt is currently available and the Deposit ID is still unprocessed on Base. Clearing this browser's saved transaction reference enables another submission. If the original transaction is mined later, the retry will revert and may cost additional gas."}
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <DialogClose asChild>
              <Button variant="ghost">Cancel</Button>
            </DialogClose>
            <Button onClick={() => void releasePendingForRetry()}>Clear saved transaction</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

function formatRemaining(seconds: bigint): string {
  const minutes = seconds / 60n
  const rest = seconds % 60n
  return `${minutes.toString()}:${rest.toString().padStart(2, "0")}`
}
