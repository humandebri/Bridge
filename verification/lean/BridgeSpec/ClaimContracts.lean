import BridgeSpec.Protocol
import BridgeSpec.ControlPlane
import BridgeSpec.GlobalHistory
import BridgeSpec.Liveness

namespace BridgeSpec.ClaimContracts

open BridgeSpec
open BridgeSpec.MintAuthorization
open BridgeSpec.Protocol.Deposit

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
      {authenticated : Bool} {ownerMatch : Option Bool}
      {origin : AuthorizationOrigin} {evidence : ExpiryEvidence},
    depositRun initial historyPrefix = some state →
      requestExpiredRefund authenticated ownerMatch state origin evidence = some next →
        authenticated = true ∧ ownerMatch = some true ∧
          evidence.depositProcessed = false

theorem refund_request_authorization_witness : RefundRequestAuthorization := by
  exact refund_request_after_accepted_prefix_requires_owner_and_absence

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

abbrev WithdrawalEventuallyPaid := Liveness.WithdrawalEventuallyPaid
theorem withdrawal_eventually_paid_witness : WithdrawalEventuallyPaid :=
  Liveness.committed_withdrawal_eventually_paid

abbrev FundedDepositEventuallyMinted := Liveness.FundedDepositEventuallyMinted
theorem funded_deposit_eventually_minted_witness : FundedDepositEventuallyMinted :=
  Liveness.funded_deposit_eventually_minted

abbrev ExpiredDepositEventuallyRefunded := Liveness.ExpiredDepositEventuallyRefunded
theorem expired_deposit_eventually_refunded_witness : ExpiredDepositEventuallyRefunded :=
  Liveness.expired_deposit_eventually_refunded

abbrev FundingFailureEventuallyCancelled := Liveness.FundingFailureEventuallyCancelled
theorem funding_failure_eventually_cancelled_witness : FundingFailureEventuallyCancelled :=
  Liveness.funding_failure_eventually_cancelled

end BridgeSpec.ClaimContracts
