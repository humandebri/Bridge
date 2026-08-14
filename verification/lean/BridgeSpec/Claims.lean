import BridgeSpec.Theorems

namespace BridgeSpec.Claims

open BridgeSpec

theorem committed_quote_claim
    {amount serviceFee : Nat} {destination : Account} {w : Withdrawal}
    (accepted : commit amount serviceFee destination = some w) :
    QuoteValid w :=
  committed_quote_is_fixed accepted

theorem settlement_backing_claim
    {s next : EconomicState} {amountOut serviceFee ledgerFee : Nat}
    (accepted : checkedSettlement s amountOut serviceFee ledgerFee = some next) :
    Backed s ∧ ledgerFee ≤ serviceFee ∧
      amountOut + serviceFee ≤ s.unpaidLiability ∧
      amountOut + ledgerFee ≤ s.escrow ∧ Backed next :=
  checked_settlement_preserves_backing accepted

theorem withdrawal_finalization_claim
    {receiptSucceeded : Bool} {receiptBlock finalizedBlock : Nat}
    (accepted : decideWithdrawalFinalization receiptSucceeded receiptBlock
      (some finalizedBlock) = .notify) :
    receiptSucceeded = true ∧ receiptBlock ≤ finalizedBlock :=
  withdrawal_notify_requires_finalized_success accepted

theorem withdrawal_admission_boundary_claim
    {observed minimum : Nat}
    (accepted : withdrawalIdAdmissible observed minimum = true) :
    minimum != 0 ∧ minimum ≤ observed :=
  withdrawal_id_admission_requires_nonzero_inclusive_boundary accepted

theorem pending_queue_claim
    {queue : PendingQueue} {existing incoming : PendingQueueEntry}
    (blocked : existing.blocked = true)
    (current : queue incoming.key = some existing) :
    (restorePendingQueue queue incoming incoming.key).map
      (fun entry => entry.blocked) = some true :=
  restore_preserves_blocked_retry blocked current

theorem canonical_probe_claim
    {receiptBlock snapshotBlock : Nat} :
    canonicalProbeMatches receiptBlock snapshotBlock = true ↔ receiptBlock = snapshotBlock :=
  canonical_probe_matches_exactly

theorem withdrawal_finality_quorum_claim
    {first second third : Option Nat} {checkpoint : Nat}
    (selected : withdrawalFinalizedCheckpoint first second third = some checkpoint) :
    twoFinalizedHeadsAttest first second third checkpoint :=
  withdrawal_finality_quorum_selects_two_provider_checkpoint selected

theorem payment_claim :
    (∀ {w paid : Withdrawal} {transfer : LedgerTransfer},
      pay w transfer = some paid →
        transfer.amount = w.amountOut ∧ transfer.destination = w.destination ∧
        paid.destination = w.destination ∧ paid.amountOut = w.amountOut) ∧
    (∀ {w : Withdrawal} {transfer : LedgerTransfer},
      w.paid = true → pay w transfer = none) := by
  constructor
  · intro w paid transfer accepted
    exact payment_uses_committed_destination_and_amount accepted
  · intro w transfer terminal
    exact paid_withdrawal_is_terminal terminal

theorem deposit_admission_claim
    {a : DepositAdmission} {net : Nat} (accepted : admitDeposit a = some net) :
    a.serviceFee ≤ a.maximumServiceFee ∧
      a.serviceFee < a.grossAmount ∧
      net = a.grossAmount - a.serviceFee ∧ net > 0 ∧
      net ≤ a.perDepositLimit ∧
      a.mintedInWindow + net ≤ a.mintWindowLimit := by
  unfold admitDeposit at accepted
  split at accepted
  next admissible =>
    simp only [Option.some.injEq] at accepted
    subst net
    omega
  next => simp at accepted

theorem deposit_identity_preflight_claim (processed : Bool) :
    (decideDepositIdentity processed = .allow ↔ processed = false) ∧
      (decideDepositIdentity processed = .conflict ↔ processed = true) := by
  cases processed <;> simp [decideDepositIdentity]

theorem reservation_claim (reserved candidate : Nat) :
    let next := commitMintReservation reserved candidate
    next.1 + next.2 = reserved + candidate := by
  simp [commitMintReservation]

theorem service_fee_claim
    {serviceFee maximumServiceFee : Nat} :
    serviceFeeChangeAllowed serviceFee maximumServiceFee = true ↔
      serviceFee ≤ maximumServiceFee := by
  simp [serviceFeeChangeAllowed]

theorem governance_transaction_affordability_claim
    {observedWei requiredWei : Nat}
    (insufficient : observedWei < requiredWei) :
    ¬requiredWei ≤ observedWei := by
  omega

theorem signing_cycle_reserve_claim
    {liquid reserve signingCost callMargin charged : Nat}
    (budget : reserve + signingCost + callMargin ≤ liquid)
    (chargeBound : charged ≤ signingCost + callMargin) :
    reserve ≤ liquid - charged := by
  omega

theorem fee_rotation_claim
    {state next : FeeState} {recipient : Nat}
    (rotated : rotateFeeRecipient state recipient = some next) :
    state.pendingPayout = 0 ∧ next.reserve = state.reserve ∧
      next.confirmedDepositFees = state.confirmedDepositFees ∧
      next.confirmedWithdrawalFees = state.confirmedWithdrawalFees ∧
      next.pendingPayout = 0 ∧ next.recipient = recipient := by
  unfold rotateFeeRecipient at rotated
  split at rotated
  next noPending =>
    simp only [Option.some.injEq] at rotated
    subst next
    simp [noPending]
  next => simp at rotated

theorem fee_payout_claim
    {reserve pending amount fee : Nat}
    (allowed : feePayoutAllowed reserve pending amount fee = true) :
    pending ≤ reserve ∧ amount + fee ≤ reserve - pending ∧
      payoutDebit false amount fee = 0 ∧
      payoutDebit true amount fee = amount + fee := by
  simpa [feePayoutAllowed, payoutDebit, Bool.and_eq_true] using allowed

theorem hold_claim
    {success absence : Bool}
    (allowed : holdRetryAllowed success absence = true) :
    success = true ∨ absence = true := by
  simpa [holdRetryAllowed, Bool.or_eq_true] using allowed

theorem lease_claim
    {active : Bool} {currentGeneration outcomeGeneration : Nat}
    (accepted : leaseOutcomeCurrent active currentGeneration outcomeGeneration = true) :
    active = true ∧ currentGeneration = outcomeGeneration := by
  simpa [leaseOutcomeCurrent] using accepted

theorem manual_claim_claim :
    (∀ scheduled stopped overdue,
      manualClaimAllowed scheduled true stopped overdue false = false) ∧
    manualClaimAllowed true false false false false = false := by
  constructor
  · intro scheduled stopped overdue
    simp [manualClaimAllowed]
  · rfl

theorem notification_admission_claim
    {globalCount callerCount globalLimit callerLimit ingestionCount ingestionLimit : Nat}
    (verificationAccepted :
      notificationAdmissionAllowed globalCount callerCount globalLimit callerLimit = true)
    (ingestionAccepted :
      notificationIngestionAllowed ingestionCount ingestionLimit = true) :
    globalCount < globalLimit ∧ callerCount < callerLimit ∧ ingestionCount < ingestionLimit := by
  have verification : globalCount < globalLimit ∧ callerCount < callerLimit := by
    simpa [notificationAdmissionAllowed, Bool.and_eq_true] using verificationAccepted
  have ingestion : ingestionCount < ingestionLimit := by
    simpa [notificationIngestionAllowed] using ingestionAccepted
  exact ⟨verification.1, verification.2, ingestion⟩

theorem lease_lane_claim
    {targetActive targetAutomatic : Bool} {activeInLane capacity : Nat}
    (allowed :
      decideLeaseLaneClaim targetActive targetAutomatic activeInLane capacity = .allow) :
    targetActive = false ∧ activeInLane < capacity := by
  cases targetActive with
  | false => simpa [decideLeaseLaneClaim] using allowed
  | true =>
      cases targetAutomatic <;> simp [decideLeaseLaneClaim] at allowed

theorem funding_attempt_claim :
    decideFundingAttempt .definitiveFailure = .release ∧
      decideFundingAttempt .success = .promoteSuccess ∧
      decideFundingAttempt .duplicate = .promoteSuccess ∧
      decideFundingAttempt .ambiguous = .promoteAmbiguous ∧
      decideFundingAttempt .retryableFailure = .retain := by
  decide

theorem funding_reconciliation_claim :
    decideFundingReconciliation false false false = .wait ∧
      decideFundingReconciliation false false true = .wait ∧
      decideFundingReconciliation false true false = .wait ∧
      decideFundingReconciliation false true true = .wait ∧
      decideFundingReconciliation true false false = .restartFresh ∧
      decideFundingReconciliation true false true = .restartFresh ∧
      decideFundingReconciliation true true false = .wait ∧
      decideFundingReconciliation true true true = .release := by
  decide

end BridgeSpec.Claims
