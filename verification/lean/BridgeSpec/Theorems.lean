import BridgeSpec.Model

namespace BridgeSpec

theorem committed_quote_is_fixed
    {amount serviceFee : Nat} {destination : Account} {w : Withdrawal}
    (h : commit amount serviceFee destination = some w) :
    QuoteValid w := by
  unfold commit at h
  split at h
  next feeLt =>
    simp only [Option.some.injEq] at h
    subst w
    simp only [QuoteValid]
    omega
  next => simp at h

theorem payment_uses_committed_destination_and_amount
    {w paid : Withdrawal} {transfer : LedgerTransfer}
    (h : pay w transfer = some paid) :
    transfer.amount = w.amountOut ∧ transfer.destination = w.destination ∧
      paid.destination = w.destination ∧ paid.amountOut = w.amountOut := by
  unfold pay at h
  split at h
  next valid =>
    simp only [Option.some.injEq] at h
    subst paid
    exact ⟨valid.2.2.1, valid.2.2.2, rfl, rfl⟩
  next => simp at h

theorem excessive_ledger_fee_stops
    {w : Withdrawal} {transfer : LedgerTransfer}
    (h : w.chargedServiceFee < transfer.ledgerFee) :
    pay w transfer = none := by
  simp [pay, Nat.not_le.mpr h]

theorem paid_withdrawal_is_terminal
    {w : Withdrawal} {transfer : LedgerTransfer} (paid : w.paid = true) :
    pay w transfer = none := by
  simp [pay, paid]

theorem observe_burn_preserves_backing
    {s : EconomicState} {amount : Nat}
    (backed : Backed s) (available : amount ≤ s.baseSupply) :
    Backed (observeBurn s amount) := by
  unfold Backed at backed ⊢
  simp only [observeBurn]
  omega

theorem paid_debt_preserves_backing
    {s : EconomicState} {amountOut serviceFee ledgerFee : Nat}
    (backed : Backed s)
    (feeBound : ledgerFee ≤ serviceFee)
    (liability : amountOut + serviceFee ≤ s.unpaidLiability)
    (escrow : amountOut + ledgerFee ≤ s.escrow) :
    Backed (settleDebt s amountOut serviceFee ledgerFee) := by
  unfold Backed at backed ⊢
  simp only [settleDebt]
  omega

theorem outbound_settlement_preserves_backing
    {s : EconomicState}
    {amountOut ledgerFee serviceFee escrowDebit reserveCredit liabilityDebit : Nat}
    (accepted : outboundSettlement amountOut ledgerFee serviceFee =
      some (escrowDebit, reserveCredit, liabilityDebit))
    (backed : Backed s)
    (liability : amountOut + serviceFee ≤ s.unpaidLiability)
    (escrow : amountOut + ledgerFee ≤ s.escrow) :
    escrowDebit = amountOut + ledgerFee ∧
      reserveCredit = serviceFee - ledgerFee ∧
      liabilityDebit = amountOut + serviceFee ∧
      Backed (settleDebt s amountOut serviceFee ledgerFee) := by
  unfold outboundSettlement at accepted
  split at accepted
  next feeBound =>
    simp only [Option.some.injEq, Prod.mk.injEq] at accepted
    obtain ⟨escrowDebitEq, reserveCreditEq, liabilityDebitEq⟩ := accepted
    exact ⟨escrowDebitEq.symm, reserveCreditEq.symm, liabilityDebitEq.symm,
      paid_debt_preserves_backing backed feeBound liability escrow⟩
  next => simp at accepted

theorem checked_settlement_preserves_backing
    {s next : EconomicState} {amountOut serviceFee ledgerFee : Nat}
    (accepted : checkedSettlement s amountOut serviceFee ledgerFee = some next) :
    Backed s ∧ ledgerFee ≤ serviceFee ∧
      amountOut + serviceFee ≤ s.unpaidLiability ∧
      amountOut + ledgerFee ≤ s.escrow ∧ Backed next := by
  unfold checkedSettlement at accepted
  split at accepted
  next admissible =>
    simp only [Option.some.injEq] at accepted
    subst next
    exact ⟨admissible.1, admissible.2.1, admissible.2.2.1, admissible.2.2.2,
      paid_debt_preserves_backing admissible.1 admissible.2.1
        admissible.2.2.1 admissible.2.2.2⟩
  next => simp at accepted

theorem withdrawal_notify_requires_finalized_success
    {receiptSucceeded : Bool} {receiptBlock finalizedBlock : Nat}
    (h : decideWithdrawalFinalization receiptSucceeded receiptBlock (some finalizedBlock) =
      .notify) :
    receiptSucceeded = true ∧ receiptBlock ≤ finalizedBlock := by
  simp only [decideWithdrawalFinalization] at h
  split at h
  next notFinalized => contradiction
  next finalized =>
    split at h
    next succeeded => exact ⟨succeeded, Nat.le_of_not_gt finalized⟩
    next => contradiction

theorem finalized_revert_is_never_notified
    {receiptBlock finalizedBlock : Nat} (finalized : receiptBlock ≤ finalizedBlock) :
    decideWithdrawalFinalization false receiptBlock (some finalizedBlock) =
      .discardReverted := by
  simp [decideWithdrawalFinalization, Nat.not_lt.mpr finalized]

theorem unfinalized_receipt_remains_retryable
    {receiptSucceeded : Bool} {receiptBlock finalizedBlock : Nat}
    (unfinalized : finalizedBlock < receiptBlock) :
    decideWithdrawalFinalization receiptSucceeded receiptBlock (some finalizedBlock) = .retry := by
  simp [decideWithdrawalFinalization, unfinalized]

theorem withdrawal_id_admission_requires_nonzero_inclusive_boundary
    {observed minimum : Nat}
    (accepted : withdrawalIdAdmissible observed minimum = true) :
    minimum != 0 ∧ minimum ≤ observed := by
  simpa [withdrawalIdAdmissible] using accepted

theorem serialized_upsert_preserves_different_entry
    {queue : PendingQueue} {incoming : PendingQueueEntry} {key : Nat}
    (different : Not (key = incoming.key)) :
    upsertPendingQueue queue incoming key = queue key := by
  simp [upsertPendingQueue, different]

theorem restore_preserves_blocked_retry
    {queue : PendingQueue} {existing incoming : PendingQueueEntry}
    (blocked : existing.blocked = true)
    (current : queue incoming.key = some existing) :
    (restorePendingQueue queue incoming incoming.key).map (fun entry => entry.blocked) = some true := by
  simp [restorePendingQueue, current, blocked, upsertPendingQueue]

theorem storage_failure_retains_session_queue (queue : PendingQueue) :
    (recordPendingQueueWrite queue false).session = queue := by
  rfl

theorem canonical_probe_matches_exactly
    {receiptBlock snapshotBlock : Nat} :
    canonicalProbeMatches receiptBlock snapshotBlock = true ↔ receiptBlock = snapshotBlock := by
  simp [canonicalProbeMatches]

theorem withdrawal_finality_quorum_selects_two_provider_checkpoint
    {first second third : Option Nat} {checkpoint : Nat}
    (selected : withdrawalFinalizedCheckpoint first second third = some checkpoint) :
    twoFinalizedHeadsAttest first second third checkpoint := by
  rcases first with _ | first <;> rcases second with _ | second <;>
    rcases third with _ | third <;>
    simp [withdrawalFinalizedCheckpoint, twoFinalizedHeadsAttest, finalizedHeadAttests,
      Nat.min_def] at selected ⊢
  · split at selected <;> simp_all <;> omega
  · split at selected <;> simp_all <;> omega
  · split at selected <;> simp_all <;> omega
  · split at selected
    · split at selected
      · simp_all <;> omega
      · split at selected <;> simp_all <;> omega
    · split at selected
      · simp_all <;> omega
      · split at selected <;> simp_all <;> omega

end BridgeSpec
