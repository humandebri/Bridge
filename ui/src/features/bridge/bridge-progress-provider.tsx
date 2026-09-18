import {
  assertTransferActionAllowed,
  initialTransferFacts,
  reduceTransfer,
  transferIdentity,
  transferPresentation,
  publishTransferFacts,
  readTransferFacts,
  subscribeTransfers,
} from "@/lib/transfer-state"
import { mintExecutionDiagnostics } from "@/lib/mint-execution"
import { toast } from "sonner"
import { Check, ChevronUp, Circle, LoaderCircle, Minus, TriangleAlert } from "lucide-react"
import {
  createContext,
  useCallback,
  useEffect,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  bridgeProgressDetail,
  bridgeProgressLabel,
  bridgeProgressSteps,
  createBridgeProgress,
  isDepositInteractionComplete,
  readLatestBridgeProgress,
  removeLatestBridgeProgress,
  saveLatestBridgeProgress,
  type BridgeProgressRecord,
  withdrawalFinalityProgress,
} from "@/lib/bridge-progress"

interface ProgressAction {
  label: string
  run: () => void | Promise<void>
  pending?: boolean
}

interface RegisteredProgressAction extends ProgressAction {
  progressId: string
}

interface BridgeProgressContextValue {
  progress?: BridgeProgressRecord
  start: (input: Parameters<typeof createBridgeProgress>[0]) => BridgeProgressRecord
  update: (
    id: string,
    patch: Partial<Omit<BridgeProgressRecord, "id" | "version" | "createdAt">>,
  ) => void
  minimize: () => void
  restore: () => void
  dismiss: () => void
  setAction: (progressId: string, action?: ProgressAction) => void
  completeWithdrawal: (input: {
    transactionHash: `0x${string}`
    owner: string
    withdrawalId?: `0x${string}`
  }) => boolean
}

const BridgeProgressContext = createContext<BridgeProgressContextValue | undefined>(undefined)

function persistProgress(record: BridgeProgressRecord) {
  if (saveLatestBridgeProgress(record)) return
  const transfer =
    record.transfer ??
    initialTransferFacts(transferIdentity(record), record.direction, record.phase)
  record.transfer = {
    ...transfer,
    warnings: {
      ...transfer.warnings,
      storage: "Browser storage unavailable. Keep this page open.",
    },
  }
}

export function BridgeProgressProvider({ children }: { children: ReactNode }) {
  const [restored] = useState<BridgeProgressRecord | undefined>(() => {
    const record = readLatestBridgeProgress()
    if (!record) return undefined
    const phase =
      record.phase === "attention"
        ? "attention"
        : record.transactionHash
          ? record.direction === "deposit"
            ? "base-mint-submitted"
            : "base-withdrawal-submitted"
          : record.withdrawal?.withdrawalId
            ? "ledger-payout"
            : record.deposit?.depositId
              ? "authorization-generating"
              : "attention"
    const transfer = initialTransferFacts(transferIdentity(record), record.direction, phase)
    if (phase === "attention") {
      const submissionUnknown =
        record.direction === "deposit" &&
        record.attentionPhase === "awaiting-base-mint" &&
        !record.transactionHash
      transfer.issue = submissionUnknown ? "unknown" : "stopped"
      transfer.message = submissionUnknown
        ? "Check the saved transfer before continuing."
        : (record.attentionMessage ?? "Check the saved transfer before continuing.")
    }
    return { ...record, phase, transfer, attentionMessage: transfer.message }
  })
  const [progress, setProgress] = useState<BridgeProgressRecord | undefined>(restored)
  const progressRef = useRef(progress)
  const [minimized, setMinimized] = useState(Boolean(restored))
  const [action, setAction] = useState<RegisteredProgressAction>()

  const setProgressAction = useCallback<BridgeProgressContextValue["setAction"]>(
    (progressId, nextAction) => {
      setAction((current) => {
        if (!nextAction) return current?.progressId === progressId ? undefined : current
        if (progressRef.current?.id !== progressId) return current
        return { ...nextAction, progressId }
      })
    },
    [],
  )

  const start = useCallback<BridgeProgressContextValue["start"]>((input) => {
    if (progressRef.current)
      throw new Error("Complete or close the current transfer before starting another one")
    const next = createBridgeProgress(input)
    persistProgress(next)
    progressRef.current = next
    setProgress(next)
    setMinimized(false)
    setAction(undefined)
    return next
  }, [])
  const update = useCallback<BridgeProgressContextValue["update"]>(
    (id, patch) => {
      if (patch.phase === "complete") setProgressAction(id, undefined)
      const current = progressRef.current
      if (!current || current.id !== id) return
      const nextPhase = patch.phase ?? current.phase
      const attentionPhase =
        nextPhase === "attention"
          ? (patch.attentionPhase ??
            (current.phase === "attention"
              ? current.attentionPhase
              : current.phase === "complete"
                ? undefined
                : current.phase))
          : undefined
      const updatedAt = Date.now()
      const candidate = { ...current, ...patch, attentionPhase, updatedAt }
      const identity = transferIdentity(candidate)
      const facts =
        readTransferFacts(identity) ??
        (current.transfer?.identity === identity
          ? current.transfer
          : initialTransferFacts(identity, current.direction, current.phase))
      const source =
        patch.observationSource ??
        (patch.phase?.startsWith("base-") || patch.transactionHash || patch.receiptBlockNumber
          ? "base"
          : patch.phase === "authorization-generating" ||
              patch.phase === "ledger-payout" ||
              patch.phase === "ic-notification-recorded" ||
              (patch.phase === "awaiting-base-mint" &&
                patch.attentionPhase === "authorization-generating")
            ? "ic"
            : "operation")
      const hasObservation = [
        "phase",
        "issue",
        "outcome",
        "transactionHash",
        "recordingPending",
        "attentionMessage",
        "completionMessage",
      ].some((key) => key in patch)
      const unchanged = Object.entries(patch).every(([key, value]) => {
        if (key === "observationSource") return true
        if (key === "observationError") return facts.transportErrors?.[source] === value
        if (key === "walletWarning") return facts.warnings.wallet === value
        if (key === "storageWarning") return facts.warnings.storage === value
        return current[key as keyof BridgeProgressRecord] === value
      })
      if (unchanged && !(hasObservation && facts.transportErrors?.[source])) return
      const revision = (facts.revisions[source] ?? 0) + 1
      let transfer = hasObservation
        ? reduceTransfer(facts, {
            identity,
            generation: facts.generation,
            source,
            revision,
            type: "observed",
            phase: candidate.phase,
            issue:
              patch.issue ??
              (patch.phase === undefined
                ? facts.issue
                : patch.phase === "attention"
                  ? "stopped"
                  : undefined),
            outcome:
              patch.outcome ??
              (patch.phase === "complete"
                ? current.direction === "deposit"
                  ? "minted"
                  : "paid"
                : undefined),
            message: patch.attentionMessage ?? patch.completionMessage,
            transactionHash: patch.transactionHash,
            recordingPending: patch.recordingPending,
          })
        : facts
      for (const [field, cause] of [
        ["observationError", "transport"],
        ["storageWarning", "storage"],
        ["walletWarning", "wallet"],
      ] as const) {
        if (field in patch)
          transfer = reduceTransfer(transfer, {
            identity,
            generation: transfer.generation,
            source,
            revision: (transfer.revisions[source] ?? 0) + 1,
            type: "warning",
            cause,
            message: patch[field],
          })
      }
      const next = {
        ...candidate,
        phase: transfer.phase,
        transfer,
        attentionMessage: transfer.phase === "attention" ? transfer.message : undefined,
      }

      persistProgress(next)
      progressRef.current = next
      setProgress(next)
      publishTransferFacts(next.transfer)
    },
    [setProgressAction],
  )
  useEffect(() => {
    if (progress?.transfer && readTransferFacts(progress.transfer.identity) !== progress.transfer)
      publishTransferFacts(progress.transfer)
  }, [progress])
  useEffect(
    () =>
      subscribeTransfers(() => {
        const current = progressRef.current
        if (!current) return
        const transfer = readTransferFacts(transferIdentity(current))
        if (!transfer || transfer === current.transfer) return
        const next = {
          ...current,
          transfer,
          phase: transfer.phase,
          attentionMessage: transfer.phase === "attention" ? transfer.message : undefined,
        }
        progressRef.current = next
        persistProgress(next)
        setProgress(next)
      }),
    [],
  )
  const walletProgressId = progress?.id
  const walletProgressPhase = progress?.phase
  useEffect(() => {
    if (
      !walletProgressId ||
      !walletProgressPhase ||
      ![
        "awaiting-base-mint",
        "awaiting-base-withdrawal",
        "awaiting-base-allowance",
        "awaiting-ic-allowance",
        "awaiting-ic-deposit",
      ].includes(walletProgressPhase)
    )
      return
    const timer = window.setTimeout(
      () => update(walletProgressId, { walletWarning: "Check your wallet. Do not submit again." }),
      60_000,
    )
    return () => window.clearTimeout(timer)
  }, [walletProgressId, walletProgressPhase, update])
  const minimize = useCallback(() => setMinimized(true), [])
  const restore = useCallback(() => setMinimized(false), [])
  const dismiss = useCallback(() => {
    setProgress((current) => {
      if (current) removeLatestBridgeProgress(current.id)
      progressRef.current = undefined
      return undefined
    })
    setAction(undefined)
    setMinimized(false)
  }, [])
  const completeWithdrawal = useCallback<BridgeProgressContextValue["completeWithdrawal"]>(
    (input) => {
      const current = progressRef.current
      if (
        current?.direction !== "withdraw" ||
        current.transactionHash?.toLowerCase() !== input.transactionHash.toLowerCase()
      )
        return false
      if (current.phase !== "complete")
        update(current.id, {
          phase: "complete",
          withdrawal: { owner: input.owner, withdrawalId: input.withdrawalId },
          completionMessage: `${current.receiveAmount} ${current.receiveSymbol} was paid to ${shortDestination(current.destination)}.`,
        })
      return true
    },
    [update],
  )

  const value = useMemo<BridgeProgressContextValue>(
    () => ({
      progress,
      start,
      update,
      minimize,
      restore,
      dismiss,
      setAction: setProgressAction,
      completeWithdrawal,
    }),
    [completeWithdrawal, dismiss, minimize, progress, restore, setProgressAction, start, update],
  )

  return (
    <BridgeProgressContext.Provider value={value}>
      {children}
      {progress && minimized && (
        <button
          type="button"
          onClick={() => setMinimized(false)}
          className="fixed bottom-5 right-4 z-50 flex max-w-[calc(100vw-2rem)] items-center gap-3 rounded-2xl border border-[#bfd7ff] bg-white px-4 py-3 text-left shadow-[0_18px_55px_rgba(20,34,53,.2)] transition hover:-translate-y-0.5 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus)] sm:right-6"
          aria-label={`Open transfer progress: ${bridgeProgressLabel(progress)}`}
        >
          <span
            className={`grid size-9 shrink-0 place-items-center rounded-full ${progress.phase === "attention" ? "bg-[#fff0ec] text-[#b42318]" : progress.phase === "complete" ? "bg-[#eaf8ef] text-[#157347]" : "bg-[var(--pink-soft)] text-[var(--pink)]"}`}
          >
            {progress.phase === "attention" ||
            (progress.transfer && transferPresentation(progress.transfer).icon === "warning") ? (
              <TriangleAlert className="size-4" />
            ) : progress.phase === "complete" ? (
              <Check className="size-4" />
            ) : (
              <LoaderCircle className="size-4 animate-spin" />
            )}
          </span>
          <span className="min-w-0">
            <span className="block text-xs font-semibold text-[var(--muted)]">
              {progress.direction === "deposit" ? "Bridge to Base" : "Bridge to IC"}
            </span>
            <span className="block truncate text-sm font-bold text-black">
              {bridgeProgressLabel(progress)}
            </span>
          </span>
          <ChevronUp className="size-4 shrink-0 text-[var(--muted)]" />
        </button>
      )}
      {progress && !minimized && (
        <ProgressDialog
          progress={progress}
          action={action}
          onMinimize={() => setMinimized(true)}
          onDismiss={value.dismiss}
        />
      )}
    </BridgeProgressContext.Provider>
  )
}

export function useBridgeProgress(): BridgeProgressContextValue {
  const value = useContext(BridgeProgressContext)
  if (!value) throw new Error("useBridgeProgress must be used inside BridgeProgressProvider")
  return value
}

function ProgressDialog({
  progress,
  action,
  onMinimize,
  onDismiss,
}: {
  progress: BridgeProgressRecord
  action?: ProgressAction
  onMinimize: () => void
  onDismiss: () => void
}) {
  const presentation = progress.transfer ? transferPresentation(progress.transfer) : undefined
  const needsAttention = progress.phase === "attention" || presentation?.icon === "warning"
  const canonicalTerminal =
    progress.phase === "complete" ||
    progress.phase === "attention" ||
    Boolean(presentation?.terminal)
  const depositInteractionComplete = isDepositInteractionComplete(progress)
  const dismissible = canonicalTerminal || depositInteractionComplete
  const closeProgress =
    depositInteractionComplete ||
    presentation?.terminal ||
    progress.phase === "complete" ||
    progress.phase === "attention"
      ? onDismiss
      : onMinimize
  const handleOutsidePointerDown = dismissible ? closeProgress : onMinimize
  const steps = bridgeProgressSteps(progress)
  const finalityProgress = withdrawalFinalityProgress(progress)
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && dismissible) closeProgress()
      }}
    >
      <DialogContent
        dismissible={dismissible}
        aria-describedby={canonicalTerminal ? "bridge-progress-description" : undefined}
        onOverlayPointerDown={handleOutsidePointerDown}
        onPointerDownOutside={(event) => {
          event.preventDefault()
          handleOutsidePointerDown()
        }}
        className="max-h-[min(760px,calc(100vh-2rem))] max-w-[560px] overflow-y-auto"
      >
        <DialogHeader>
          <div className="flex items-start justify-between gap-4 pr-7">
            <div>
              <DialogTitle>
                {progress.direction === "deposit" ? "Bridge to Base" : "Bridge to IC"}
              </DialogTitle>
              {canonicalTerminal && (
                <DialogDescription id="bridge-progress-description">
                  Review the result below.
                </DialogDescription>
              )}
            </div>
            {!dismissible && (
              <Button size="sm" variant="ghost" onClick={onMinimize}>
                <Minus className="size-4" />
                Minimize
              </Button>
            )}
          </div>
        </DialogHeader>
        {progress.phase !== "attention" && progress.attentionMessage && (
          <p role="alert">{progress.attentionMessage}</p>
        )}
        {needsAttention && (
          <div className="mt-5 rounded-2xl border border-[#ffbdad] bg-[#fff0ec] p-4" role="alert">
            <p className="font-bold text-black">{bridgeProgressLabel(progress)}</p>
            <p className="mt-1 text-sm leading-6 text-[var(--muted)]">
              {bridgeProgressDetail(progress)}
            </p>
          </div>
        )}
        {(progress.phase === "complete" || depositInteractionComplete) && (
          <div className="mt-5 rounded-2xl border border-[#9ed8b3] bg-[#eaf8ef] p-4" role="status">
            <p className="font-bold text-black">
              {progress.phase === "complete" ? bridgeProgressLabel(progress) : "Mint included"}
            </p>
            <p className="mt-1 text-sm leading-6 text-[var(--muted)]">
              {progress.phase === "complete"
                ? bridgeProgressDetail(progress)
                : "No further action is needed. Final confirmation will continue in History."}
            </p>
          </div>
        )}
        {progress.transfer?.warnings.storage && (
          <p role="alert">{progress.transfer.warnings.storage}</p>
        )}
        {progress.direction === "deposit" && (
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
        )}
        <ol className="mt-5 space-y-1" aria-label="Transfer progress">
          {steps.map((step, index) => (
            <li
              key={step.label}
              aria-current={
                step.status === "current" || step.status === "attention" ? "step" : undefined
              }
              aria-label={
                depositInteractionComplete && step.label === "Base mint transaction"
                  ? "Base mint transaction complete"
                  : undefined
              }
              className="relative flex min-h-11 items-start gap-3"
            >
              {index < steps.length - 1 && (
                <span
                  aria-hidden="true"
                  className="absolute left-[15px] top-8 h-[calc(100%-1rem)] w-px bg-[var(--line)]"
                />
              )}
              <span
                className={`relative z-10 grid size-8 shrink-0 place-items-center rounded-full border ${step.status === "attention" ? "border-[#ffbdad] bg-[#fff0ec] text-[#b42318]" : step.status === "complete" ? "border-[#9ed8b3] bg-[#eaf8ef] text-[#157347]" : step.status === "current" ? "border-[var(--pink)] bg-[var(--pink-soft)] text-[var(--pink)]" : "border-[var(--line)] bg-white text-[var(--muted)]"}`}
              >
                {step.status === "attention" ? (
                  <TriangleAlert className="size-4" />
                ) : step.status === "complete" ? (
                  <Check className="size-4" />
                ) : step.status === "current" ? (
                  needsAttention ? (
                    <TriangleAlert className="size-4" />
                  ) : (
                    <LoaderCircle className="size-4 animate-spin" />
                  )
                ) : (
                  <Circle className="size-3" />
                )}
              </span>
              <span
                className={`min-w-0 pt-1 text-sm font-bold ${step.status === "waiting" ? "text-[var(--muted)]" : "text-black"}`}
              >
                <span className="block">{step.label}</span>
                {step.note && (
                  <span className="mt-0.5 block text-xs font-normal leading-5 text-[var(--muted)]">
                    {step.note}
                  </span>
                )}
                {step.label === "Base finality" && step.status === "current" && (
                  <span className="mt-1 block text-xs font-normal leading-5 text-[var(--muted)]">
                    <span className="block">Usually takes about 20 minutes.</span>
                    {finalityProgress ? (
                      <>
                        <span className="block">
                          Finalized block #{finalityProgress.finalizedBlockNumber} / Target block #
                          {finalityProgress.targetBlockNumber}
                        </span>
                        <span className="block">
                          {finalityProgress.remainingBlocks} blocks remaining
                        </span>
                      </>
                    ) : (
                      <span className="block">Checking finalized block…</span>
                    )}
                  </span>
                )}
              </span>
            </li>
          ))}
        </ol>
        <DialogFooter>
          {action &&
            !depositInteractionComplete &&
            !presentation?.terminal &&
            presentation?.code !== "conflict" &&
            presentation?.code !== "processed" && (
              <Button
                disabled={action.pending}
                onClick={() => {
                  assertTransferActionAllowed(transferIdentity(progress))
                  void action.run()
                }}
              >
                {action.pending && progress.direction !== "deposit" ? "Working…" : action.label}
              </Button>
            )}
          {progress.phase === "attention" && (
            <Button asChild>
              <a href="/history" onClick={onMinimize}>
                Open History
              </a>
            </Button>
          )}
          {dismissible && (
            <Button onClick={closeProgress}>
              {depositInteractionComplete ? "Finish" : "Close"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function shortDestination(value: string): string {
  return value.length > 18 ? `${value.slice(0, 10)}…${value.slice(-6)}` : value
}
