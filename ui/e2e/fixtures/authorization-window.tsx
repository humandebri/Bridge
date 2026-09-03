import { useState } from "react"
import { createRoot } from "react-dom/client"
import type { DepositView, SettlementStopReason } from "@/generated/bridge.did"
import { validatedDepositWriteGate } from "@/features/bridge/bridge-page"
import type { ActivityItem } from "@/lib/activity-history"
import { DepositActivityRow } from "@/routes/history"
import "@/styles.css"

const ADDRESS = `0x${"11".repeat(20)}` as const

function Harness() {
  const [finalizedTimestamp, setFinalizedTimestamp] = useState(1_000n)
  const [refundRequests, setRefundRequests] = useState(0)
  const [walletWrites, setWalletWrites] = useState(0)
  const [ledgerPulls, setLedgerPulls] = useState(0)
  const [intentWrites, setIntentWrites] = useState(0)
  const [boundaryError, setBoundaryError] = useState("")
  const item = depositItem({ AuthorizationWindowTooShort: null })

  const attemptBoundaryDeposit = () => {
    try {
      validatedDepositWriteGate({
        amount: 100n,
        expectedSequence: 1n,
        sequence: 1n,
        ledger: { balance: 1_000n, fee: 1n, allowance: 0n },
        observation: {
          ready: true,
          blockers: [],
          checkedAt: 1,
          snapshot: {
            serviceFee: 10n,
            maxServiceFee: 20n,
            perDepositLimit: 1_000n,
            minted: 0n,
            limit: 1_000n,
            startedAt: 1_000n,
            duration: 300n,
            depositsPaused: false,
            withdrawalsPaused: false,
            bridgeSigner: ADDRESS,
            mintAuthorizationEpoch: 1n,
            blockTimestamp: 1_300n,
          },
        },
      })
      setWalletWrites((count) => count + 1)
      setLedgerPulls((count) => count + 1)
      setIntentWrites((count) => count + 1)
    } catch (error) {
      setBoundaryError(error instanceof Error ? error.message : "Boundary check failed")
    }
  }

  return (
    <main className="mx-auto grid max-w-6xl gap-6 p-6">
      <h1 className="text-xl font-bold">Authorization window browser fixture</h1>
      <DepositActivityRow
        item={item}
        mintFinalization="absent"
        finalizedBlockTimestamp={finalizedTimestamp}
        writesEnabled
        onRequestRefund={() => {
          setRefundRequests((count) => count + 1)
          return Promise.resolve()
        }}
        onContinue={() => Promise.resolve()}
      />
      <button type="button" onClick={() => setFinalizedTimestamp(1_001n)}>
        Advance finalized Base time
      </button>
      <p data-testid="refund-requests">Refund requests: {refundRequests}</p>
      <button type="button" onClick={attemptBoundaryDeposit}>
        Attempt boundary deposit
      </button>
      <p role="alert">{boundaryError}</p>
      <dl>
        <div>
          <dt>Wallet writes</dt>
          <dd data-testid="wallet-writes">{walletWrites}</dd>
        </div>
        <div>
          <dt>Ledger pulls</dt>
          <dd data-testid="ledger-pulls">{ledgerPulls}</dd>
        </div>
        <div>
          <dt>Intent writes</dt>
          <dd data-testid="intent-writes">{intentWrites}</dd>
        </div>
      </dl>
    </main>
  )
}

function depositItem(
  reason: SettlementStopReason,
): Extract<ActivityItem, { direction: "to-base" }> {
  const deposit: DepositView = {
    base_recipient: new Uint8Array(20).fill(3),
    deposit_id: new Uint8Array(32).fill(1),
    quote: [{ net_amount: 90n, service_fee: 10n }],
    max_service_fee: 10n,
    funding_ledger_block_index: [1n],
    from_subaccount: [],
    last_settlement_stop_reason: [reason],
    created_at_ns: 1n,
    state: { AuthorizationPending: null },
    available_refund_amount: [100n],
    owner_sequence: 1n,
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
  return {
    key: "deposit:browser-history-deadline",
    direction: "to-base",
    createdAtNs: deposit.created_at_ns,
    deposit,
  }
}

createRoot(document.getElementById("root")!).render(<Harness />)
