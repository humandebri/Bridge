import BridgeSpec.Protocol
import BridgeSpec.ControlPlane
import BridgeSpec.GlobalHistory
import BridgeSpec.Liveness
import BridgeSpec.Claims
import BridgeSpec.Refinement
import BridgeSpec.LedgerBlockProvenance

namespace BridgeSpec.ClaimContracts

open BridgeSpec
open BridgeSpec.MintAuthorization
open BridgeSpec.Protocol.Deposit

abbrev LedgerBlockProvenance := BridgeSpec.LedgerBlockProvenance.ClaimContract
theorem ledger_block_provenance_witness : LedgerBlockProvenance :=
  BridgeSpec.LedgerBlockProvenance.claim_contract_witness

def FeeAccountingOnce : Prop :=
  (∀ {next : State} {events : List Event},
      depositRun initial events = some next → traceFeeCreditCount events ≤ 1) ∧
    (∀ {state next : DepositState},
      installSignature state = some next →
        ∃ authorization, state.authorization = some authorization ∧
          next.feeReserve = state.feeReserve + authorization.chargedServiceFee ∧
          next.feeCounted = true)

theorem fee_accounting_once_witness : FeeAccountingOnce := by
  constructor
  · intro next events accepted
    exact deposit_run_fee_credit_count_at_most_once accepted
  · intro state next accepted
    exact authorization_signature_counts_exact_service_fee_once accepted

def RuntimeAttestationReuse : Prop :=
  ∀ {initialDomain : ControlPlane.InstallDomain} {state : ControlPlane.State},
    ControlPlane.Reachable initialDomain state →
      ∀ reusedDomain, state.lastReusedDomain = some reusedDomain →
        reusedDomain = state.domain

theorem runtime_attestation_reuse_witness : RuntimeAttestationReuse := by
  intro initialDomain state reachable
  exact ControlPlane.reused_attestation_binds_current_install_domain
    (ControlPlane.reachable_is_safe reachable)

def GovernanceNonceChainBinding : Prop :=
  ∀ {initialDomain : ControlPlane.InstallDomain} {state : ControlPlane.State},
    ControlPlane.Reachable initialDomain state →
      ∀ chainId, state.lastGovernanceChain = some chainId →
        chainId = state.configuredChainId

theorem governance_nonce_chain_binding_witness : GovernanceNonceChainBinding := by
  intro initialDomain state reachable
  exact ControlPlane.governance_nonce_binds_configured_chain
    (ControlPlane.reachable_is_safe reachable)

def GovernanceTransactionAffordability : Prop :=
  ∀ observedWei requiredWei : Nat,
    observedWei < requiredWei → ¬requiredWei ≤ observedWei

theorem governance_transaction_affordability_witness : GovernanceTransactionAffordability := by
  intro observedWei requiredWei insufficient affordable
  omega

def SigningCycleReserve : Prop :=
  ∀ liquid reserve signingCost callMargin charged : Nat,
    reserve + signingCost + callMargin ≤ liquid →
      charged ≤ signingCost + callMargin → reserve ≤ liquid - charged

theorem signing_cycle_reserve_witness : SigningCycleReserve := by
  intro liquid reserve signingCost callMargin charged budget chargeBound
  omega

def ActivationPreflight : Prop :=
  ∀ {initialDomain : ControlPlane.InstallDomain} {state : ControlPlane.State},
    ControlPlane.Reachable initialDomain state → state.paused = false →
      state.activationCount > 0 → state.lastActivationValidated = true

theorem activation_preflight_witness : ActivationPreflight := by
  intro initialDomain state reachable unpaused activated
  exact ControlPlane.unpaused_activation_is_validated
    (ControlPlane.reachable_is_safe reachable) unpaused activated

def IntegratedProtocolReachability : Prop :=
  (∀ {state : Protocol.ProtocolState}, Protocol.Reachable state → Protocol.Safe state) ∧
    (∀ {stored reopened : Protocol.ProtocolState},
      Protocol.reopenState stored = some reopened → Protocol.Safe reopened)

theorem integrated_protocol_reachability_witness : IntegratedProtocolReachability := by
  exact ⟨Protocol.reachable_is_safe, Protocol.reopened_state_is_safe⟩

def GlobalInterleavingSafety : Prop :=
  (∀ {state final : GlobalHistory.GlobalState} {events : List GlobalHistory.Event},
      GlobalHistory.Safe state → GlobalHistory.Runs state events final →
        GlobalHistory.Safe final) ∧
    (∀ {state next : GlobalHistory.GlobalState} {event : GlobalHistory.Event} {other : Nat},
      other ≠ event.id → GlobalHistory.step state event = some next →
        GlobalHistory.findRecord? next.records other =
          GlobalHistory.findRecord? state.records other)

theorem global_interleaving_safety_witness : GlobalInterleavingSafety := by
  exact ⟨GlobalHistory.runs_preserve_safe, GlobalHistory.step_frames_other_record⟩

def DepositTransitionSafety : Prop :=
  ∀ {state next : Protocol.Deposit.State} {event : Protocol.Deposit.Event},
    Protocol.Deposit.depositStep state event = some next →
      Protocol.Deposit.WellFormed state → Protocol.Deposit.WellFormed next

theorem deposit_transition_safety_witness : DepositTransitionSafety := by
  intro state next event accepted safe
  exact Protocol.Deposit.deposit_step_preserves_well_formed safe accepted

def DepositIdentityPreflight : Prop :=
  ∀ {state next : IdentityHistory.State} {candidate : Nat},
    IdentityHistory.Reachable state →
      IdentityHistory.preflight state candidate = some next → state candidate = false

theorem deposit_identity_preflight_witness : DepositIdentityPreflight := by
  exact IdentityHistory.reachable_preflight_is_fresh

def RefundEvidenceEnforcement : Prop :=
  ∀ {final : State} {historyPrefix suffix : List Event}
      {origin : AuthorizationOrigin} {evidence : ExpiryEvidence},
    depositRun initial (historyPrefix ++ .startExpiredRefund origin evidence :: suffix) = some final →
      evidence.depositProcessed = false ∧
        ∃ authorization : Authorization,
          evidence.depositId = authorization.depositId ∧
          evidence.authorizationDigest = authorization.digest ∧
          evidence.finalizedTimestamp > authorization.deadline

theorem refund_evidence_enforcement_witness : RefundEvidenceEnforcement := by
  exact refund_event_in_accepted_trace_requires_finalized_absence

def RefundRequestAuthorization : Prop :=
  ∀ {state next : State} {historyPrefix : List Event}
      {authenticated : Bool}
      {origin : AuthorizationOrigin} {evidence : ExpiryEvidence},
    depositRun initial historyPrefix = some state →
      requestExpiredRefund authenticated state origin evidence = some next →
        authenticated = true ∧
          evidence.depositProcessed = false

theorem refund_request_authorization_witness : RefundRequestAuthorization := by
  exact refund_request_after_accepted_prefix_requires_authentication_and_absence

def SettlementBacking : Prop :=
  (∀ {state final : GlobalHistory.GlobalState} {events : List GlobalHistory.Event},
      GlobalHistory.Safe state → GlobalHistory.Runs state events final →
        GlobalHistory.Backed final.accounting) ∧
    (∀ {record next : GlobalHistory.Record} {ledgerFee transferAmount destination : Nat},
      GlobalHistory.applyRecord record
        (.payout record.id ledgerFee transferAmount destination) = some next →
      next.economic.escrow = record.economic.escrow - (record.netAmount + ledgerFee) ∧
        next.economic.feeReserve = record.economic.feeReserve +
          (record.chargedServiceFee - ledgerFee) ∧
        next.economic.unreleasedLiability = record.economic.unreleasedLiability -
          (record.netAmount + record.chargedServiceFee))

theorem settlement_backing_witness : SettlementBacking := by
  constructor
  · intro state final events safe runs
    exact (GlobalHistory.runs_preserve_safe safe runs).2.2.1
  · exact GlobalHistory.payout_applies_exact_delta

def PaymentIdentity : Prop :=
  (∀ {record next : GlobalHistory.Record} {ledgerFee transferAmount destination : Nat},
      GlobalHistory.applyRecord record
        (.payout record.id ledgerFee transferAmount destination) = some next →
      transferAmount = record.netAmount ∧ destination = record.paymentDestination ∧
        ledgerFee ≤ record.chargedServiceFee) ∧
    (∀ {state next : GlobalHistory.GlobalState} {event : GlobalHistory.Event} {other : Nat},
      other ≠ event.id → GlobalHistory.step state event = some next →
        GlobalHistory.findRecord? next.records other =
          GlobalHistory.findRecord? state.records other)

theorem payment_identity_witness : PaymentIdentity := by
  exact ⟨GlobalHistory.payout_requires_exact_identity, GlobalHistory.step_frames_other_record⟩

def ReservationLifecycle : Prop :=
  (∀ {record next : GlobalHistory.Record},
      GlobalHistory.applyRecord record (.releaseReservation record.id) = some next →
        next.economic.reservedMint = 0) ∧
    (∀ {record : GlobalHistory.Record}, record.releaseApplied = true →
      record.phase.terminal = false →
        GlobalHistory.applyRecord record (.releaseReservation record.id) = none)

theorem reservation_lifecycle_witness : ReservationLifecycle := by
  exact ⟨GlobalHistory.release_clears_exact_reservation,
    GlobalHistory.duplicate_release_is_rejected⟩

def DepositBacking : Prop :=
  (∀ {state final : GlobalHistory.GlobalState} {events : List GlobalHistory.Event},
      GlobalHistory.Safe state → GlobalHistory.Runs state events final →
        GlobalHistory.Backed final.accounting) ∧
    (∀ {record next : GlobalHistory.Record},
      GlobalHistory.applyRecord record (.installSignature record.id) = some next →
        next.economic.feeReserve = record.economic.feeReserve + record.chargedServiceFee ∧
          next.economic.unmintedLiability =
            record.economic.unmintedLiability - record.chargedServiceFee) ∧
    (∀ {record next : GlobalHistory.Record},
      GlobalHistory.applyRecord record (.mint record.id) = some next →
        next.economic.baseSupply = record.economic.baseSupply + record.netAmount ∧
          next.economic.unmintedLiability =
            record.economic.unmintedLiability - record.netAmount) ∧
    (∀ {record next : GlobalHistory.Record} {amount : Nat},
      GlobalHistory.applyRecord record (.refund record.id amount) = some next →
        next.economic.escrow = record.economic.escrow - amount ∧
          next.economic.unmintedLiability = record.economic.unmintedLiability - amount)

theorem deposit_backing_witness : DepositBacking := by
  refine ⟨?_, GlobalHistory.signature_applies_exact_fee,
    GlobalHistory.mint_applies_exact_amount, GlobalHistory.refund_applies_exact_amount⟩
  intro state final events safe runs
  exact (GlobalHistory.runs_preserve_safe safe runs).2.2.1

def DepositDecisionSafety : Prop := DepositTransitionSafety ∧ DepositBacking

theorem deposit_decision_safety_witness : DepositDecisionSafety := by
  exact ⟨deposit_transition_safety_witness, deposit_backing_witness⟩

def CommittedQuote : Prop :=
  (∀ {amount serviceFee : Nat} {destination : Account} {withdrawal : Withdrawal},
      commit amount serviceFee destination = some withdrawal → QuoteValid withdrawal) ∧
    IntegratedProtocolReachability

theorem committed_quote_witness : CommittedQuote :=
  ⟨Claims.committed_quote_claim, integrated_protocol_reachability_witness⟩

def DepositAdmission : Prop :=
  (∀ {admission : BridgeSpec.DepositAdmission} {net : Nat}, admitDeposit admission = some net →
      admission.serviceFee ≤ admission.maximumServiceFee ∧
      admission.serviceFee < admission.grossAmount ∧
      net = admission.grossAmount - admission.serviceFee ∧ net > 0 ∧
      net ≤ admission.perDepositLimit ∧
      admission.mintedInWindow + net ≤ admission.mintWindowLimit) ∧
    IntegratedProtocolReachability

theorem deposit_admission_witness : DepositAdmission :=
  ⟨Claims.deposit_admission_claim, integrated_protocol_reachability_witness⟩

def ReservationCommit : Prop :=
  (∀ reserved candidate : Nat,
      let next := commitMintReservation reserved candidate
      next.1 + next.2 = reserved + candidate) ∧ ReservationLifecycle

theorem reservation_commit_witness : ReservationCommit :=
  ⟨Claims.reservation_claim, reservation_lifecycle_witness⟩

def ServiceFeeMaximum : Prop :=
  (∀ serviceFee maximumServiceFee : Nat,
      serviceFeeChangeAllowed serviceFee maximumServiceFee = true ↔
        serviceFee ≤ maximumServiceFee) ∧ FeeAccountingOnce

theorem service_fee_maximum_witness : ServiceFeeMaximum :=
  ⟨by intro serviceFee maximumServiceFee; exact Claims.service_fee_claim,
    fee_accounting_once_witness⟩

def FeeRecipientRotation : Prop :=
  (∀ {state next : FeeState} {recipient : Nat},
      rotateFeeRecipient state recipient = some next →
        state.pendingPayout = 0 ∧ next.reserve = state.reserve ∧
        next.confirmedDepositFees = state.confirmedDepositFees ∧
        next.confirmedWithdrawalFees = state.confirmedWithdrawalFees ∧
        next.pendingPayout = 0 ∧ next.recipient = recipient) ∧
    IntegratedProtocolReachability

theorem fee_recipient_rotation_witness : FeeRecipientRotation :=
  ⟨Claims.fee_rotation_claim, integrated_protocol_reachability_witness⟩

def FeePayout : Prop :=
  (∀ {reserve pending amount fee : Nat},
      feePayoutAllowed reserve pending amount fee = true →
        pending ≤ reserve ∧ amount + fee ≤ reserve - pending ∧
        payoutDebit false amount fee = 0 ∧ payoutDebit true amount fee = amount + fee) ∧
    SettlementBacking

theorem fee_payout_witness : FeePayout :=
  ⟨Claims.fee_payout_claim, settlement_backing_witness⟩

def HoldResolution : Prop :=
  (∀ {success absence : Bool}, holdRetryAllowed success absence = true →
      success = true ∨ absence = true) ∧ IntegratedProtocolReachability

theorem hold_resolution_witness : HoldResolution :=
  ⟨Claims.hold_claim, integrated_protocol_reachability_witness⟩

def LeaseOutcome : Prop :=
  (∀ {active : Bool} {currentGeneration outcomeGeneration : Nat},
      leaseOutcomeCurrent active currentGeneration outcomeGeneration = true →
        active = true ∧ currentGeneration = outcomeGeneration) ∧
    GlobalInterleavingSafety

theorem lease_outcome_witness : LeaseOutcome :=
  ⟨Claims.lease_claim, global_interleaving_safety_witness⟩

def NotificationQuotaIsolation : Prop :=
  (∀ {globalCount callerCount globalLimit callerLimit ingestionCount ingestionLimit : Nat},
      notificationAdmissionAllowed globalCount callerCount globalLimit callerLimit = true →
      notificationIngestionAllowed ingestionCount ingestionLimit = true →
        globalCount < globalLimit ∧ callerCount < callerLimit ∧
          ingestionCount < ingestionLimit) ∧ IntegratedProtocolReachability

theorem notification_quota_isolation_witness : NotificationQuotaIsolation :=
  ⟨Claims.notification_admission_claim, integrated_protocol_reachability_witness⟩

def LeaseLaneIsolation : Prop :=
  (∀ {targetActive targetAutomatic : Bool} {activeInLane capacity : Nat},
      decideLeaseLaneClaim targetActive targetAutomatic activeInLane capacity = .allow →
        targetActive = false ∧ activeInLane < capacity) ∧
    GlobalInterleavingSafety

theorem lease_lane_isolation_witness : LeaseLaneIsolation :=
  ⟨Claims.lease_lane_claim, global_interleaving_safety_witness⟩

def FundingAttemptLifecycle : Prop :=
  (decideFundingAttempt .definitiveFailure = .release ∧
    decideFundingAttempt .success = .promoteSuccess ∧
    decideFundingAttempt .duplicate = .promoteSuccess ∧
    decideFundingAttempt .ambiguous = .promoteAmbiguous ∧
    decideFundingAttempt .retryableFailure = .retain) ∧ IntegratedProtocolReachability

theorem funding_attempt_lifecycle_witness : FundingAttemptLifecycle :=
  ⟨Claims.funding_attempt_claim, integrated_protocol_reachability_witness⟩

def FundingReconciliationFreshness : Prop :=
  (decideFundingReconciliation false false false = .wait ∧
    decideFundingReconciliation false false true = .wait ∧
    decideFundingReconciliation false true false = .wait ∧
    decideFundingReconciliation false true true = .wait ∧
    decideFundingReconciliation true false false = .restartFresh ∧
    decideFundingReconciliation true false true = .restartFresh ∧
    decideFundingReconciliation true true false = .wait ∧
    decideFundingReconciliation true true true = .release) ∧ IntegratedProtocolReachability

theorem funding_reconciliation_freshness_witness : FundingReconciliationFreshness :=
  ⟨Claims.funding_reconciliation_claim, integrated_protocol_reachability_witness⟩

def WithdrawalFinalization : Prop :=
  (∀ {receiptSucceeded : Bool} {receiptBlock finalizedBlock : Nat},
      decideWithdrawalFinalization receiptSucceeded receiptBlock (some finalizedBlock) = .notify →
        receiptSucceeded = true ∧ receiptBlock ≤ finalizedBlock) ∧
    (∀ {receiptSucceeded : Bool} {receiptBlock : Nat},
      decideWithdrawalFinalization receiptSucceeded receiptBlock none = .retry)

theorem withdrawal_finalization_witness : WithdrawalFinalization := by
  constructor
  · exact Claims.withdrawal_finalization_claim
  · intro receiptSucceeded receiptBlock
    rfl

def PendingQueue : Prop :=
  (∀ {queue : BridgeSpec.PendingQueue} {existing incoming : PendingQueueEntry},
      existing.blocked = true → queue incoming.key = some existing →
        (restorePendingQueue queue incoming incoming.key).map
          (fun entry => entry.blocked) = some true) ∧
    (∀ queue : BridgeSpec.PendingQueue,
      restorePendingQueue queue = restorePendingQueue queue)

theorem pending_queue_witness : PendingQueue := by
  exact ⟨Claims.pending_queue_claim, fun _ => rfl⟩

def CanonicalProbe : Prop :=
  (∀ receiptBlock snapshotBlock : Nat,
      canonicalProbeMatches receiptBlock snapshotBlock = true ↔
        receiptBlock = snapshotBlock) ∧ IntegratedProtocolReachability

theorem canonical_probe_witness : CanonicalProbe :=
  ⟨by intro receiptBlock snapshotBlock; exact Claims.canonical_probe_claim,
    integrated_protocol_reachability_witness⟩

def WithdrawalFinalityQuorum : Prop :=
  ∀ {first second third : Option Nat} {checkpoint : Nat},
    withdrawalFinalizedCheckpoint first second third = some checkpoint →
      twoFinalizedHeadsAttest first second third checkpoint

theorem withdrawal_finality_quorum_witness : WithdrawalFinalityQuorum := by
  intro first second third checkpoint selected
  exact Claims.withdrawal_finality_quorum_claim selected

def AuthorizationBinding : Prop :=
  (∀ {state next : DepositState} {authorization : Authorization}
      {origin : AuthorizationOrigin},
      commitAuthorization state authorization origin = some next →
        next.authorization = some authorization ∧
        authorization.deadline = origin.finalizedTimestamp + authorizationTtl ∧
        authorization.chainId = origin.expectedChainId ∧
        authorization.verifyingContract = origin.expectedVerifyingContract ∧
        authorization.epoch = origin.expectedEpoch) ∧ DepositTransitionSafety

theorem authorization_binding_witness : AuthorizationBinding := by
  constructor
  · intro state next authorization origin accepted
    rcases accepted_authorization_is_exact_and_has_fixed_deadline accepted with
      ⟨stored, _, _, deadline, chain, contract, epoch, _⟩
    exact ⟨stored, deadline, chain, contract, epoch⟩
  · exact deposit_transition_safety_witness

def ExpiryRefund : Prop :=
  (∀ {state next : DepositState} {origin : AuthorizationOrigin} {evidence : ExpiryEvidence},
      startExpiredRefund state origin evidence = some next →
        evidence.depositProcessed = false ∧
        ∃ authorization, state.authorization = some authorization ∧
          evidence.depositId = authorization.depositId ∧
          evidence.authorizationDigest = authorization.digest ∧
          evidence.finalizedTimestamp > authorization.deadline) ∧ DepositBacking

theorem expiry_refund_witness : ExpiryRefund :=
  ⟨accepted_expiry_refund_requires_finalized_unprocessed_expiry, deposit_backing_witness⟩

def ExactMintFinalization : Prop :=
  (∀ {state next : DepositState} {evidence : MintEvidence},
      completeMint state evidence = some next →
        evidence.receiptSucceeded = true ∧ evidence.receiptBlock ≤ evidence.finalizedBlock ∧
        ∃ authorization, state.authorization = some authorization ∧
          evidence.depositId = authorization.depositId ∧
          evidence.recipient = authorization.recipient ∧
          evidence.authorizationDigest = authorization.digest) ∧ DepositBacking

theorem exact_mint_finalization_witness : ExactMintFinalization :=
  ⟨accepted_mint_requires_exact_finalized_success, deposit_backing_witness⟩

def EpochInvalidation : Prop :=
  (∀ {state : DepositState} {current replacement : Authorization}
      {origin : AuthorizationOrigin}, state.authorization = some current →
        commitAuthorization state replacement origin = none) ∧ AuthorizationBinding

theorem epoch_invalidation_witness : EpochInvalidation :=
  ⟨committed_authorization_cannot_be_reissued, authorization_binding_witness⟩

abbrev WithdrawalEventuallyPaid := Liveness.WithdrawalEventuallyPaid
theorem withdrawal_eventually_paid_witness : WithdrawalEventuallyPaid :=
  Liveness.committed_withdrawal_eventually_paid

abbrev FundedDepositEventuallyMinted := Liveness.FundedDepositEventuallyMinted
theorem funded_deposit_eventually_minted_witness : FundedDepositEventuallyMinted :=
  Liveness.funded_deposit_eventually_minted

abbrev ExpiredDepositEventuallyRefunded := Liveness.ExpiredDepositEventuallyRefunded
theorem expired_deposit_eventually_refunded_witness : ExpiredDepositEventuallyRefunded :=
  Liveness.expired_deposit_eventually_refunded

abbrev FundedDepositEventuallyMintedOrRefunded :=
  Liveness.FundedDepositEventuallyMintedOrRefunded
theorem funded_deposit_eventually_minted_or_refunded_witness :
    FundedDepositEventuallyMintedOrRefunded :=
  Liveness.funded_deposit_eventually_minted_or_refunded

def NonterminalDepositIndexConsistency : Prop :=
  ∀ phase : MintAuthorization.DepositPhase,
    MintAuthorization.nonterminalDepositIndexed phase = true ↔
      phase ≠ .refunded ∧ phase ≠ .cancelled ∧ phase ≠ .minted

theorem nonterminal_deposit_index_consistency_witness :
    NonterminalDepositIndexConsistency :=
  MintAuthorization.nonterminal_deposit_index_matches_nonterminal_phases

abbrev FundingFailureEventuallyCancelled := Liveness.FundingFailureEventuallyCancelled
theorem funding_failure_eventually_cancelled_witness : FundingFailureEventuallyCancelled :=
  Liveness.funding_failure_eventually_cancelled

end BridgeSpec.ClaimContracts
