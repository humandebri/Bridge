import BridgeSpec.Claims
import BridgeSpec.FiniteWidthModel

namespace BridgeSpec.ModelRefinement

open BridgeSpec
open BridgeSpec.FiniteWidthModel

theorem committed_quote_model_refinement
    (amount serviceFee : U128) (destination : Account) :
    commitImpl amount serviceFee destination =
      commit amount.val serviceFee.val destination := by
  rfl

theorem settlement_backing_model_refinement
    (state : EconomicState) (amountOut serviceFee ledgerFee : U128)
    (bounded : state.escrow ≤ maxU128 ∧ state.baseSupply ≤ maxU128 ∧
      state.feeReserve ≤ maxU128 ∧ state.unpaidLiability ≤ maxU128 ∧
      amountOut.val + serviceFee.val ≤ maxU128 ∧
      amountOut.val + ledgerFee.val ≤ maxU128) :
    settlementImpl state amountOut serviceFee ledgerFee =
      checkedSettlement state amountOut.val serviceFee.val ledgerFee.val := by
  simp [settlementImpl, bounded]

theorem withdrawal_finalization_model_refinement
    (receiptSucceeded : Bool) (receiptBlock : U64) (finalizedBlock : Option U64)
    (canonical : Bool) :
    finalizationImpl receiptSucceeded receiptBlock finalizedBlock canonical =
      decideWithdrawalFinalization receiptSucceeded receiptBlock.val
        (finalizedBlock.map U64.val) canonical := by
  rfl

theorem pending_queue_model_refinement
    (queue : PendingQueue) (incoming : PendingQueueEntry) :
    pendingQueueImpl queue incoming = restorePendingQueue queue incoming := by
  rfl

theorem canonical_probe_model_refinement
    (receiptBlock snapshotBlock : U64) :
    canonicalProbeImpl receiptBlock snapshotBlock =
      canonicalProbeMatches receiptBlock.val snapshotBlock.val := by
  rfl

theorem withdrawal_finality_quorum_model_refinement
    (first second third : Option U64) :
    withdrawalFinalityCheckpointImpl first second third =
      withdrawalFinalizedCheckpoint (first.map U64.val) (second.map U64.val) (third.map U64.val) := by
  rfl

theorem ledger_block_provenance_model_refinement (current : Option U128) (block : U128) :
    ledgerBlockImpl current block =
      ledgerBlockProvenance (current.map U128.val) block.val := by
  rfl

theorem payment_model_refinement
    (withdrawal : Withdrawal) (transfer : LedgerTransfer)
    (bounded : withdrawal.amount ≤ maxU128 ∧ withdrawal.amountOut ≤ maxU128 ∧
      withdrawal.chargedServiceFee ≤ maxU128 ∧ transfer.amount ≤ maxU128 ∧
      transfer.ledgerFee ≤ maxU128) :
    paymentImpl withdrawal transfer = pay withdrawal transfer := by
  simp [paymentImpl, bounded]

theorem deposit_admission_model_refinement
    (admission : DepositAdmission)
    (bounded : admission.serviceFee ≤ maxU128 ∧
      admission.maximumServiceFee ≤ maxU128 ∧ admission.grossAmount ≤ maxU128 ∧
      admission.perDepositLimit ≤ maxU128 ∧ admission.mintedInWindow ≤ maxU128 ∧
      admission.mintWindowLimit ≤ maxU128 ∧
      admission.mintedInWindow +
        (admission.grossAmount - admission.serviceFee) ≤ maxU128) :
    depositAdmissionImpl admission = admitDeposit admission := by
  simp [depositAdmissionImpl, bounded]

theorem deposit_identity_preflight_model_refinement (processed : Bool) :
    depositIdentityImpl processed = decideDepositIdentity processed := by
  cases processed <;> rfl

theorem reservation_model_refinement
    (reserved candidate : U128) (bounded : reserved.val + candidate.val ≤ maxU128) :
    reservationImpl reserved candidate =
      some (commitMintReservation reserved.val candidate.val) := by
  simp [reservationImpl, checkedAdd128, bounded, commitMintReservation]

theorem service_fee_model_refinement
    (serviceFee minimumServiceFee maximumServiceFee : U128) :
    serviceFeeImpl serviceFee minimumServiceFee maximumServiceFee =
      serviceFeeChangeAllowed serviceFee.val minimumServiceFee.val maximumServiceFee.val := by
  rfl

theorem fee_rotation_model_refinement
    (state : FeeState) (recipient : U64) :
    feeRotationImpl state recipient = rotateFeeRecipient state recipient.val := by
  rfl

theorem fee_payout_model_refinement
    (reserve pending amount fee : U128)
    (bounded : amount.val + fee.val ≤ maxU128) :
    feePayoutImpl reserve pending amount fee =
      feePayoutAllowed reserve.val pending.val amount.val fee.val := by
  simp [feePayoutImpl, checkedAdd128, bounded]

theorem hold_model_refinement
    (success absence : Bool) :
    holdImpl success absence = holdRetryAllowed success absence := by
  rfl

theorem lease_model_refinement
    (active : Bool) (currentGeneration outcomeGeneration : U64) :
    leaseImpl active currentGeneration outcomeGeneration =
      leaseOutcomeCurrent active currentGeneration.val outcomeGeneration.val := by
  rfl

theorem manual_claim_model_refinement
    (scheduled active stopped overdue expired : Bool) :
    manualClaimImpl scheduled active stopped overdue expired =
      manualClaimAllowed scheduled active stopped overdue expired := by
  rfl

theorem refund_request_identity_model_refinement
    (authenticated : Bool) :
    refundRequestIdentityImpl authenticated =
      decideRefundRequestIdentity authenticated := by
  rfl

theorem deposit_nonterminal_index_model_refinement (state : U16) :
    depositNonterminalIndexImpl state = depositNonterminalIndexed state.val := by
  rfl

theorem notification_admission_model_refinement
    (globalCount callerCount globalLimit callerLimit ingestionCount ingestionLimit : U16)
    (hashMatches : Bool) (nowNs retryAfterNs : U64) :
    notificationAdmissionImpl globalCount callerCount globalLimit callerLimit
        ingestionCount ingestionLimit hashMatches nowNs retryAfterNs =
      (notificationAdmissionAllowed globalCount.val callerCount.val globalLimit.val callerLimit.val,
        notificationIngestionAllowed ingestionCount.val ingestionLimit.val,
        notificationFailureCooldownActive hashMatches nowNs.val retryAfterNs.val) := by
  rfl

theorem lease_lane_model_refinement
    (targetActive targetAutomatic : Bool) (activeInLane capacity : U64) :
    leaseLaneClaimImpl targetActive targetAutomatic activeInLane capacity =
      decideLeaseLaneClaim targetActive targetAutomatic activeInLane.val capacity.val := by
  rfl

theorem funding_attempt_model_refinement (outcome : FundingOutcomeKind) :
    fundingAttemptImpl outcome = decideFundingAttempt outcome := by
  rfl

theorem funding_reconciliation_model_refinement
    (completeAbsence finalScan dedupExpired : Bool) :
    fundingReconciliationImpl completeAbsence finalScan dedupExpired =
      decideFundingReconciliation completeAbsence finalScan dedupExpired := by
  rfl

end BridgeSpec.ModelRefinement
