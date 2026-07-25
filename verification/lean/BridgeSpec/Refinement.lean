import BridgeSpec.Claims

namespace BridgeSpec.Refinement

open BridgeSpec

theorem committed_quote_refinement
    {amount serviceFee : Nat} {destination : Account} {w : Withdrawal}
    (accepted : commit amount serviceFee destination = some w) :
    QuoteValid w :=
  Claims.committed_quote_claim accepted

theorem settlement_backing_refinement
    {s next : EconomicState} {amountOut serviceFee ledgerFee : Nat}
    (accepted : checkedSettlement s amountOut serviceFee ledgerFee = some next) :
    Backed s ∧ ledgerFee ≤ serviceFee ∧
      amountOut + serviceFee ≤ s.unpaidLiability ∧
      amountOut + ledgerFee ≤ s.escrow ∧ Backed next :=
  Claims.settlement_backing_claim accepted

theorem withdrawal_finalization_refinement
    {receiptSucceeded : Bool} {receiptBlock finalizedBlock : Nat}
    (accepted : decideWithdrawalFinalization receiptSucceeded receiptBlock
      (some finalizedBlock) = .notify) :
    receiptSucceeded = true ∧ receiptBlock ≤ finalizedBlock :=
  Claims.withdrawal_finalization_claim accepted

theorem pending_queue_refinement
    {queue : PendingQueue} {existing incoming : PendingQueueEntry}
    (blocked : existing.blocked = true)
    (current : queue incoming.key = some existing) :
    (restorePendingQueue queue incoming incoming.key).map
      (fun entry => entry.blocked) = some true :=
  Claims.pending_queue_claim blocked current

theorem canonical_probe_refinement
    {receiptBlock snapshotBlock : Nat} :
    canonicalProbeMatches receiptBlock snapshotBlock = true ↔ receiptBlock = snapshotBlock :=
  Claims.canonical_probe_claim

theorem payment_refinement :
    (∀ {w paid : Withdrawal} {transfer : LedgerTransfer},
      pay w transfer = some paid →
        transfer.amount = w.amountOut ∧ transfer.destination = w.destination ∧
        paid.destination = w.destination ∧ paid.amountOut = w.amountOut) ∧
    (∀ {w : Withdrawal} {transfer : LedgerTransfer},
      w.paid = true → pay w transfer = none) :=
  Claims.payment_claim

theorem deposit_admission_refinement
    {a : DepositAdmission} {net : Nat} (accepted : admitDeposit a = some net) :
    a.serviceFee ≤ a.maximumServiceFee ∧
      a.serviceFee < a.grossAmount ∧ net = a.grossAmount - a.serviceFee ∧
      net > 0 ∧ net ≤ a.perDepositLimit ∧
      a.mintedInWindow + net ≤ a.mintWindowLimit :=
  Claims.deposit_admission_claim accepted

theorem reservation_refinement (reserved candidate : Nat) :
    let next := commitMintReservation reserved candidate
    next.1 + next.2 = reserved + candidate :=
  Claims.reservation_claim reserved candidate

theorem service_fee_refinement
    {serviceFee maximumServiceFee : Nat} :
    serviceFeeChangeAllowed serviceFee maximumServiceFee = true ↔
      serviceFee ≤ maximumServiceFee :=
  Claims.service_fee_claim

theorem fee_rotation_refinement
    {state next : FeeState} {recipient : Nat}
    (rotated : rotateFeeRecipient state recipient = some next) :
    state.pendingPayout = 0 ∧ next.reserve = state.reserve ∧
      next.confirmedDepositFees = state.confirmedDepositFees ∧
      next.confirmedWithdrawalFees = state.confirmedWithdrawalFees ∧
      next.pendingPayout = 0 ∧ next.recipient = recipient :=
  Claims.fee_rotation_claim rotated

theorem fee_payout_refinement
    {reserve pending amount fee : Nat}
    (allowed : feePayoutAllowed reserve pending amount fee = true) :
    pending ≤ reserve ∧ amount + fee ≤ reserve - pending ∧
      payoutDebit false amount fee = 0 ∧
      payoutDebit true amount fee = amount + fee :=
  Claims.fee_payout_claim allowed

theorem hold_refinement
    {success absence : Bool}
    (allowed : holdRetryAllowed success absence = true) :
    success = true ∨ absence = true :=
  Claims.hold_claim allowed

theorem lease_refinement
    {active : Bool} {currentGeneration outcomeGeneration : Nat}
    (accepted : leaseOutcomeCurrent active currentGeneration outcomeGeneration = true) :
    active = true ∧ currentGeneration = outcomeGeneration :=
  Claims.lease_claim accepted

theorem manual_claim_refinement :
    (∀ scheduled active stopped overdue expired,
      manualClaimAllowed true scheduled active stopped overdue expired = false) ∧
    manualClaimAllowed false true true false false false = false :=
  Claims.manual_claim_claim

end BridgeSpec.Refinement
