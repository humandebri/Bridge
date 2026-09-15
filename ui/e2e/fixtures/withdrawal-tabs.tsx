import { useState } from "react"
import { createRoot } from "react-dom/client"
import {
  BridgeProgressProvider,
  useBridgeProgress,
} from "@/features/bridge/bridge-progress-provider"
import { SettlementConfirmationCoordinator } from "@/features/bridge/settlement-confirmation-coordinator"
import {
  savePendingConfirmation,
  markPendingConfirmationNotified,
  readPendingConfirmations,
} from "@/lib/pending-confirmations"
import "@/styles.css"
const hash = `0x${"33".repeat(32)}` as const
const withdrawalId = `0x${"07".repeat(32)}` as const
function Harness() {
  const bridge = useBridgeProgress()
  const [observing, setObserving] = useState(false)
  return (
    <main>
      <button
        onClick={async () => {
          await savePendingConfirmation({
            kind: "withdrawal",
            transactionHash: hash,
            owner: "aaaaa-aa",
          })
          await markPendingConfirmationNotified(readPendingConfirmations()[0]!, withdrawalId)
          bridge.start({
            direction: "withdraw",
            phase: "ledger-payout",
            source: `0x${"11".repeat(20)}`,
            destination: "aaaaa-aa",
            transactionHash: hash,
            sendAmount: "2",
            receiveAmount: "1.5",
            sendSymbol: "KINIC",
            receiveSymbol: "KINIC",
            withdrawal: { owner: "aaaaa-aa", withdrawalId },
          })
        }}
      >
        Track withdrawal
      </button>
      <button onClick={() => setObserving(true)}>Observe</button>
      <p data-testid="transport-warning">{bridge.progress?.transfer?.warnings.transport ?? ""}</p>
      <p data-testid="phase">{bridge.progress?.phase ?? "none"}</p>
      {observing && <SettlementConfirmationCoordinator />}
    </main>
  )
}
createRoot(document.getElementById("root")!).render(
  <BridgeProgressProvider>
    <Harness />
  </BridgeProgressProvider>,
)
