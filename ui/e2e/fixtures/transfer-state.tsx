import { createRoot } from "react-dom/client"
import {
  BridgeProgressProvider,
  useBridgeProgress,
} from "@/features/bridge/bridge-progress-provider"
import { useTransferPresentation } from "@/features/bridge/use-transfer-presentation"
import { initialTransferFacts, transferIdentity } from "@/lib/transfer-state"
import "@/styles.css"
const depositId = `0x${"11".repeat(32)}` as const
function Harness() {
  const bridge = useBridgeProgress()
  const history = useTransferPresentation(
    initialTransferFacts(
      bridge.progress ? transferIdentity(bridge.progress) : `deposit:${depositId}`,
      bridge.progress?.direction ?? "deposit",
      bridge.progress?.direction === "withdraw"
        ? "base-withdrawal-submitted"
        : "awaiting-base-mint",
    ),
  )
  return (
    <main className="p-8">
      <h1>Transfer state fixture</h1>
      <p data-testid="history-title">{history.title}</p>
      <button
        onClick={() =>
          bridge.start({
            direction: "deposit",
            phase: "awaiting-base-mint",
            source: "aaaaa-aa",
            destination: "0x1111111111111111111111111111111111111111",
            sendAmount: "2",
            receiveAmount: "1.5",
            sendSymbol: "KINIC",
            receiveSymbol: "KINIC",
            deposit: { owner: "aaaaa-aa", ownerSequence: "1", depositId },
          })
        }
      >
        Start deposit
      </button>
      <button
        onClick={() =>
          bridge.start({
            direction: "withdraw",
            phase: "base-withdrawal-submitted",
            source: "0x1111111111111111111111111111111111111111",
            destination: "aaaaa-aa",
            sendAmount: "2",
            receiveAmount: "1.5",
            sendSymbol: "KINIC",
            receiveSymbol: "KINIC",
            transactionHash: `0x${"22".repeat(32)}`,
            withdrawal: { owner: "aaaaa-aa" },
          })
        }
      >
        Start withdrawal
      </button>
      <button
        onClick={() =>
          bridge.progress &&
          bridge.update(bridge.progress.id, {
            observationSource: "base",
            phase: "awaiting-ic-notification",
            observationError: undefined,
          })
        }
      >
        Notify IC
      </button>
      <button
        onClick={() =>
          bridge.progress &&
          bridge.update(bridge.progress.id, { phase: "ledger-payout", observationError: undefined })
        }
      >
        Payout pending
      </button>
      <button
        onClick={() =>
          bridge.progress &&
          bridge.update(bridge.progress.id, { phase: "complete", outcome: "paid" })
        }
      >
        Paid
      </button>
      <button
        onClick={() => {
          if (bridge.progress)
            bridge.update(bridge.progress.id, {
              observationSource: "base",
              observationError: "Base status could not be refreshed. Retrying.",
            })
        }}
      >
        Fail read
      </button>
      <button
        onClick={() => {
          if (bridge.progress)
            bridge.update(bridge.progress.id, {
              observationSource: "base",
              phase: "base-mint-included",
              observationError: undefined,
            })
        }}
      >
        Recover
      </button>
      <button
        onClick={() => {
          if (bridge.progress)
            bridge.update(bridge.progress.id, { phase: "complete", outcome: "minted" })
        }}
      >
        Finalize
      </button>
      <button
        onClick={() => {
          if (bridge.progress) bridge.update(bridge.progress.id, { phase: "base-mint-submitted" })
        }}
      >
        Late inclusion
      </button>
      <button
        onClick={() => {
          const id = bridge.progress?.id
          if (!id) return
          bridge.update(id, { phase: "attention", issue: "refund-ready" })
          bridge.setAction(id, {
            label: "Check refund",
            run: async () => {
              bridge.update(id, { phase: "refund-checking" })
              const response = await fetch("/transfer-test/refund", { method: "POST" })
              if (!response.ok) throw new Error("Fixture refund failed")
              bridge.update(id, { phase: "complete", outcome: "refunded" })
            },
          })
        }}
      >
        Refund ready
      </button>
    </main>
  )
}
createRoot(document.getElementById("root")!).render(
  <BridgeProgressProvider>
    <Harness />
  </BridgeProgressProvider>,
)
