import { useState } from "react"
import { deploymentProfile } from "@/config/profile"
import { DepositActivityRow } from "@/routes/history"
import type { DepositView } from "@/generated/bridge.did"
import { createRoot } from "react-dom/client"
import {
  BridgeProgressProvider,
  useBridgeProgress,
} from "@/features/bridge/bridge-progress-provider"
import { useTransferPresentation } from "@/features/bridge/use-transfer-presentation"
import { initialTransferFacts, transferIdentity } from "@/lib/transfer-state"
import "@/styles.css"
deploymentProfile.snsRootCanisterId = "aaaaa-aa"
const depositId = `0x${"11".repeat(32)}` as const
const historyDeposit: DepositView = {
  base_recipient: new Uint8Array(20).fill(3),
  deposit_id: new Uint8Array(32).fill(1),
  quote: [{ net_amount: 90n, service_fee: 10n }],
  max_service_fee: 10n,
  funding_ledger_block_index: [1n],
  from_subaccount: [],
  last_settlement_stop_reason: [],
  created_at_ns: 1n,
  state: { AuthorizationAvailable: null },
  available_refund_amount: [100n],
  owner_sequence: 1n,
  mint_receipt: [],
  mint_authorization: [
    {
      finalized_block_number: 10n,
      signature: [],
      deposit_id: new Uint8Array(32).fill(1),
      issued_at_timestamp: 900n,
      domain_name: "KINIC Bridge",
      charged_service_fee: 10n,
      recipient: new Uint8Array(20).fill(3),
      domain_version: "1",
      authorization_epoch: 1n,
      max_service_fee: 10n,
      deadline: 1_000n,
      signature_dispatch_attempt: 1,
      chain_id: 84_532n,
      finalized_block_hash: new Uint8Array(32).fill(2),
      finalized_block_timestamp: 900n,
      verifying_contract: new Uint8Array(20).fill(4),
      digest: new Uint8Array(32).fill(5),
      gross_amount: 100n,
    },
  ],
  automatic_progress: [],
  gross_amount: 100n,
  refund: [],
}
function Harness() {
  const [historyComplete, setHistoryComplete] = useState(false)
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
      <section className="mx-auto max-w-6xl" aria-label="Included mint history">
        <DepositActivityRow
          item={{
            key: "history-included",
            direction: "to-base",
            createdAtNs: 1n,
            deposit: historyComplete
              ? { ...historyDeposit, state: { Minted: null } }
              : historyDeposit,
          }}
          mintFinalization="minted"
          mintRecording={historyComplete ? "recorded" : "confirming"}
          mintTransactionHash={`0x${"ab".repeat(32)}`}
          writesEnabled={false}
          onRequestRefund={async () => {}}
          onContinue={async () => {}}
        />
      </section>
      <button onClick={() => setHistoryComplete(true)}>Complete history mint</button>
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
              transactionHash: `0x${"22".repeat(32)}`,
              receiptBlockNumber: "123",
              baseTransactionOutcome: "success",
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
