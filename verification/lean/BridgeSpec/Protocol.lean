import BridgeSpec.Refinement
import BridgeSpec.DepositAuthorization

namespace BridgeSpec.Protocol

open BridgeSpec
open BridgeSpec.Implementation

namespace Deposit

open BridgeSpec.MintAuthorization

abbrev State := DepositState

inductive Event where
  | fund (grossAmount : Nat)
  | commitAuthorization (authorization : Authorization) (origin : AuthorizationOrigin)
  | installSignature
  | beginExpiryReconciliation
  | startExpiredRefund (origin : AuthorizationOrigin) (evidence : ExpiryEvidence)
  | completeMint (evidence : MintEvidence)
  | completeRefund
  | manualClaim (now nextLeaseGeneration : Nat)
deriving DecidableEq

def depositStep (state : State) : Event → Option State
  | .fund grossAmount =>
      if state.phase = .fundingPending ∧ state.authorization = none then
        some (fund state grossAmount)
      else none
  | .commitAuthorization authorization origin =>
      commitAuthorization state authorization origin
  | .installSignature => installSignature state
  | .beginExpiryReconciliation => beginExpiryReconciliation state
  | .startExpiredRefund origin evidence => startExpiredRefund state origin evidence
  | .completeMint evidence => completeMint state evidence
  | .completeRefund => completeRefund state
  | .manualClaim now nextLeaseGeneration => manualClaim state now nextLeaseGeneration

def depositRun : State → List Event → Option State
  | state, [] => some state
  | state, event :: events =>
      match depositStep state event with
      | none => none
      | some next => depositRun next events

def initial : State := {
  phase := .fundingPending
  authorization := none
  escrow := 0
  baseSupply := 0
  feeReserve := 0
  pendingDepositLiability := 0
  reservedMint := 0
  feeCounted := false
}

def ReservationConsistent (state : State) : Prop :=
  match state.phase with
  | .authorizationPending | .authorizationAvailable | .expiryReconciliation =>
      ∃ authorization,
        state.authorization = some authorization ∧
        state.reservedMint = authorization.netAmount ∧
        authorization.grossAmount ≤ state.pendingDepositLiability
  | _ => state.reservedMint = 0

def FeeConsistent (state : State) : Prop :=
  state.feeCounted = true → state.phase = .minted

def WellFormed (state : State) : Prop :=
  BridgeSpec.MintAuthorization.Backed state ∧
    ReservationConsistent state ∧ FeeConsistent state

theorem initial_well_formed : WellFormed initial := by
  simp [WellFormed, initial, BridgeSpec.MintAuthorization.Backed,
    ReservationConsistent, FeeConsistent]

theorem fee_false_of_consistent_of_not_minted
    {state : State} (consistent : FeeConsistent state)
    (notMinted : state.phase ≠ .minted) :
    state.feeCounted = false := by
  cases counted : state.feeCounted with
  | false => rfl
  | true => exact (notMinted (consistent counted)).elim

theorem deposit_step_preserves_well_formed
    {state next : State} {event : Event}
    (wellFormed : WellFormed state)
    (accepted : depositStep state event = some next) :
    WellFormed next := by
  rcases wellFormed with ⟨backed, reservation, fee⟩
  cases event with
  | fund grossAmount =>
      simp only [depositStep] at accepted
      split at accepted
      next allowed =>
        simp only [Option.some.injEq] at accepted
        subst next
        rcases allowed with ⟨phase, authorization⟩
        have reserved : state.reservedMint = 0 := by
          simpa [ReservationConsistent, phase] using reservation
        have feeFalse : state.feeCounted = false :=
          fee_false_of_consistent_of_not_minted fee (by simp [phase])
        constructor
        · exact funding_preserves_backing backed
        · simp [ReservationConsistent, FeeConsistent, fund, reserved, feeFalse]
      next => simp at accepted
  | commitAuthorization authorization origin =>
      simp only [depositStep] at accepted
      unfold commitAuthorization at accepted
      split at accepted
      next allowed =>
        simp only [Option.some.injEq] at accepted
        subst next
        rcases allowed with ⟨phase, _, _, liability⟩
        have feeFalse : state.feeCounted = false :=
          fee_false_of_consistent_of_not_minted fee (by simp [phase])
        exact ⟨backed, by
          simp [ReservationConsistent, liability], by
          simp [FeeConsistent, feeFalse]⟩
      next => simp at accepted
  | installSignature =>
      simp only [depositStep] at accepted
      unfold installSignature at accepted
      split at accepted
      next phase =>
        simp [phase, ReservationConsistent] at reservation
        have feeFalse : state.feeCounted = false :=
          fee_false_of_consistent_of_not_minted fee (by simp [phase])
        simp only [Option.some.injEq] at accepted
        subst next
        rcases reservation with ⟨authorization, auth, reserved, liability⟩
        exact ⟨backed, by
          simp [ReservationConsistent, auth, reserved, liability], by
          simp [FeeConsistent, feeFalse]⟩
      next => simp at accepted
  | beginExpiryReconciliation =>
      simp only [depositStep] at accepted
      unfold beginExpiryReconciliation at accepted
      split at accepted
      next phase =>
        simp only [Option.some.injEq] at accepted
        subst next
        cases phase with
        | inl pending =>
            simp [pending, ReservationConsistent] at reservation
            have feeFalse : state.feeCounted = false :=
              fee_false_of_consistent_of_not_minted fee (by simp [pending])
            exact ⟨backed, by
              simpa [ReservationConsistent] using reservation, by
              simp [FeeConsistent, feeFalse]⟩
        | inr available =>
            simp [available, ReservationConsistent] at reservation
            have feeFalse : state.feeCounted = false :=
              fee_false_of_consistent_of_not_minted fee (by simp [available])
            exact ⟨backed, by
              simpa [ReservationConsistent] using reservation, by
              simp [FeeConsistent, feeFalse]⟩
      next => simp at accepted
  | startExpiredRefund origin evidence =>
      simp only [depositStep] at accepted
      unfold startExpiredRefund at accepted
      cases auth : state.authorization with
      | none => simp [auth] at accepted
      | some authorization =>
          simp only [auth] at accepted
          split at accepted
          next allowed =>
            have feeFalse : state.feeCounted = false :=
              fee_false_of_consistent_of_not_minted fee (by
                exact fun minted => by simp [minted] at allowed)
            simp only [Option.some.injEq] at accepted
            subst next
            exact ⟨backed, by simp [ReservationConsistent], by
              simp [FeeConsistent, feeFalse]⟩
          next => simp at accepted
  | completeMint evidence =>
      simp only [depositStep] at accepted
      unfold completeMint at accepted
      cases auth : state.authorization with
      | none => simp [auth] at accepted
      | some authorization =>
          simp only [auth] at accepted
          split at accepted
          next allowed =>
            rcases allowed with ⟨_, exactEvidence, liability, _⟩
            simp only [Option.some.injEq] at accepted
            subst next
            have amount :
                authorization.netAmount + authorization.chargedServiceFee =
                  authorization.grossAmount := by
              exact exactEvidence.2.2.2.2.2.2.2.2.2.2.2.2.2.2.2.2
            constructor
            · simp only [BridgeSpec.MintAuthorization.Backed] at backed ⊢
              omega
            · simp [ReservationConsistent, FeeConsistent]
          next => simp at accepted
  | completeRefund =>
      simp only [depositStep] at accepted
      unfold completeRefund at accepted
      cases auth : state.authorization with
      | none => simp [auth] at accepted
      | some authorization =>
          simp only [auth] at accepted
          split at accepted
          next allowed =>
            rcases allowed with ⟨phase, liability, escrowBound⟩
            have feeFalse : state.feeCounted = false :=
              fee_false_of_consistent_of_not_minted fee (by simp [phase])
            simp only [Option.some.injEq] at accepted
            subst next
            constructor
            · simp only [BridgeSpec.MintAuthorization.Backed] at backed ⊢
              omega
            · constructor
              · simp [ReservationConsistent]
              · simp [FeeConsistent, feeFalse]
          next => simp at accepted
  | manualClaim now nextLeaseGeneration =>
      simp only [depositStep] at accepted
      unfold manualClaim at accepted
      split at accepted
      next allowed =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, by
          simpa [ReservationConsistent] using reservation, by
          simpa [FeeConsistent] using fee⟩
      next => simp at accepted

theorem deposit_run_preserves_well_formed
    {state next : State} {events : List Event}
    (wellFormed : WellFormed state)
    (accepted : depositRun state events = some next) :
    WellFormed next := by
  induction events generalizing state with
  | nil =>
      simp [depositRun] at accepted
      simpa [accepted] using wellFormed
  | cons event events ih =>
      simp only [depositRun] at accepted
      cases step : depositStep state event with
      | none => simp [step] at accepted
      | some intermediate =>
          simp only [step] at accepted
          exact ih (deposit_step_preserves_well_formed wellFormed step) accepted

theorem deposit_run_preserves_backing
    {state next : State} {events : List Event}
    (wellFormed : WellFormed state)
    (accepted : depositRun state events = some next) :
    BridgeSpec.MintAuthorization.Backed next :=
  (deposit_run_preserves_well_formed wellFormed accepted).1

theorem deposit_run_preserves_reservation_consistency
    {state next : State} {events : List Event}
    (wellFormed : WellFormed state)
    (accepted : depositRun state events = some next) :
    ReservationConsistent next :=
  (deposit_run_preserves_well_formed wellFormed accepted).2.1

def feeCreditCount (state : State) : Nat :=
  if state.feeCounted then 1 else 0

theorem deposit_run_fee_credit_count_at_most_once
    {state next : State} {events : List Event}
    (wellFormed : WellFormed state)
    (accepted : depositRun state events = some next) :
    feeCreditCount next ≤ 1 := by
  have _ := deposit_run_preserves_well_formed wellFormed accepted
  cases counted : next.feeCounted <;> simp [feeCreditCount, counted]

theorem deposit_step_preserves_existing_authorization
    {state next : State} {event : Event} {authorization : Authorization}
    (committed : state.authorization = some authorization)
    (accepted : depositStep state event = some next) :
    next.authorization = some authorization := by
  cases event <;>
    simp only [depositStep] at accepted
  · split at accepted
    · simp_all
    · simp at accepted
  · unfold commitAuthorization at accepted
    simp [committed] at accepted
  · unfold installSignature at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simpa using committed
    · simp at accepted
  · unfold beginExpiryReconciliation at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simpa using committed
    · simp at accepted
  · unfold startExpiredRefund at accepted
    simp only [committed] at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simpa using committed
    · simp at accepted
  · unfold completeMint at accepted
    simp only [committed] at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simpa using committed
    · simp at accepted
  · unfold completeRefund at accepted
    simp only [committed] at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simpa using committed
    · simp at accepted
  · unfold manualClaim at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simpa using committed
    · simp at accepted

theorem deposit_run_preserves_existing_authorization
    {state next : State} {events : List Event} {authorization : Authorization}
    (committed : state.authorization = some authorization)
    (accepted : depositRun state events = some next) :
    next.authorization = some authorization := by
  induction events generalizing state with
  | nil =>
      simp [depositRun] at accepted
      subst next
      exact committed
  | cons event events ih =>
      simp only [depositRun] at accepted
      cases step : depositStep state event with
      | none => simp [step] at accepted
      | some intermediate =>
          simp only [step] at accepted
          exact ih (deposit_step_preserves_existing_authorization committed step) accepted

theorem terminal_step_is_rejected
    {state : State} (terminalState : terminal state.phase = true) (event : Event) :
    depositStep state event = none := by
  cases event <;>
    cases phaseEq : state.phase <;>
    simp [depositStep, terminal, phaseEq, commitAuthorization, installSignature,
      beginExpiryReconciliation, startExpiredRefund, completeMint, completeRefund,
      manualClaim] at terminalState ⊢
  all_goals cases state.authorization <;> simp

theorem terminal_trace_is_absorbing
    {state : State} (terminalState : terminal state.phase = true)
    {event : Event} {events : List Event} :
    depositRun state (event :: events) = none := by
  simp [depositRun, terminal_step_is_rejected terminalState event]

end Deposit

abbrev RequestIdentity := Nat
abbrev HoldIdentity := Nat
abbrev TransferIdentity := Nat
abbrev BlockHash := Nat
abbrev PayloadIdentity := Nat

structure HistoryEntry where
  index : Nat
  transferIdentity : TransferIdentity
deriving DecidableEq

def contiguousFrom : Nat → List HistoryEntry → Bool
  | _, [] => true
  | expected, entry :: rest =>
      decide (entry.index = expected) && contiguousFrom (expected + 1) rest

def noTransferMatch (identity : TransferIdentity) (entries : List HistoryEntry) : Bool :=
  entries.all (fun entry => decide (entry.transferIdentity ≠ identity))

structure AbsenceCertificate where
  requestIdentity : RequestIdentity
  holdIdentity : HoldIdentity
  transferIdentity : TransferIdentity
  startIndex : Nat
  entries : List HistoryEntry
  next : Nat
  tip : Nat
  watermark : Nat
deriving DecidableEq

def AbsenceCertificate.complete (certificate : AbsenceCertificate) : Prop :=
  contiguousFrom certificate.startIndex certificate.entries = true ∧
    certificate.next = certificate.startIndex + certificate.entries.length ∧
    certificate.tip < certificate.next ∧
    certificate.tip ≤ certificate.watermark ∧
    noTransferMatch certificate.transferIdentity certificate.entries = true

structure SuccessCertificate where
  requestIdentity : RequestIdentity
  holdIdentity : HoldIdentity
  transferIdentity : TransferIdentity
deriving DecidableEq

structure CanonicalCertificate where
  receiptBlock : Nat
  receiptBlockHash : BlockHash
  snapshotBlock : Nat
  snapshotBlockHash : BlockHash
  finalizedWatermark : Nat
  payloadIdentity : PayloadIdentity
  committedPayloadIdentity : PayloadIdentity
deriving DecidableEq

def CanonicalCertificate.valid (certificate : CanonicalCertificate) : Prop :=
  certificate.receiptBlock = certificate.snapshotBlock ∧
    certificate.receiptBlockHash = certificate.snapshotBlockHash ∧
    certificate.receiptBlock ≤ certificate.finalizedWatermark ∧
    certificate.payloadIdentity = certificate.committedPayloadIdentity

def canonicalCertificateAccepted (certificate : CanonicalCertificate) : Bool :=
  decide (certificate.receiptBlock = certificate.snapshotBlock) &&
    decide (certificate.receiptBlockHash = certificate.snapshotBlockHash) &&
    decide (certificate.receiptBlock ≤ certificate.finalizedWatermark) &&
    decide (certificate.payloadIdentity = certificate.committedPayloadIdentity)

theorem accepted_canonical_certificate_binds_finalized_payload
    {certificate : CanonicalCertificate}
    (accepted : canonicalCertificateAccepted certificate = true) :
    certificate.valid := by
  simp [canonicalCertificateAccepted, Bool.and_eq_true] at accepted
  exact ⟨accepted.1.1.1, accepted.1.1.2, accepted.1.2, accepted.2⟩

inductive HoldEvidence where
  | exactSuccess (certificate : SuccessCertificate)
  | completeAbsence (certificate : AbsenceCertificate)
deriving DecidableEq

def holdEvidenceValid
    (requestIdentity : RequestIdentity)
    (holdIdentity : HoldIdentity)
    (transferIdentity : TransferIdentity) : HoldEvidence → Bool
  | .exactSuccess certificate =>
      decide (certificate.requestIdentity = requestIdentity) &&
        decide (certificate.holdIdentity = holdIdentity) &&
        decide (certificate.transferIdentity = transferIdentity)
  | .completeAbsence certificate =>
      decide (certificate.requestIdentity = requestIdentity) &&
        decide (certificate.holdIdentity = holdIdentity) &&
        decide (certificate.transferIdentity = transferIdentity) &&
        contiguousFrom certificate.startIndex certificate.entries &&
        decide (certificate.next = certificate.startIndex + certificate.entries.length) &&
        decide (certificate.tip < certificate.next) &&
        decide (certificate.tip ≤ certificate.watermark) &&
        noTransferMatch certificate.transferIdentity certificate.entries

theorem accepted_absence_is_complete
    {requestIdentity : RequestIdentity}
    {holdIdentity : HoldIdentity}
    {transferIdentity : TransferIdentity}
    {certificate : AbsenceCertificate}
    (accepted : holdEvidenceValid requestIdentity holdIdentity transferIdentity
      (.completeAbsence certificate) = true) :
    certificate.complete ∧
      certificate.requestIdentity = requestIdentity ∧
      certificate.holdIdentity = holdIdentity ∧
      certificate.transferIdentity = transferIdentity := by
  simp [holdEvidenceValid, Bool.and_eq_true] at accepted
  rcases accepted with ⟨⟨⟨⟨⟨⟨⟨request, hold⟩, transfer⟩, contiguous⟩,
    nextIndex⟩, tipNext⟩, watermark⟩, noMatch⟩
  exact ⟨⟨contiguous, nextIndex, tipNext, watermark, noMatch⟩,
    request, hold, transfer⟩

structure FeeRotationRequest where
  governance : Bool
  anonymous : Bool
  roleCollision : Bool
  subaccountLength : Nat
  pendingPayout : Nat
  reconciliationHolds : Nat
  oldHashLength : Nat
  newHashLength : Nat
  recipient : Nat
deriving DecidableEq

def feeRotationRequestAllowed (request : FeeRotationRequest) : Bool :=
  decide (request.governance = true ∧ request.anonymous = false ∧
    request.roleCollision = false ∧
    (request.subaccountLength = 0 ∨ request.subaccountLength = 32) ∧
    request.pendingPayout = 0 ∧ request.reconciliationHolds = 0 ∧
    request.oldHashLength = 32 ∧ request.newHashLength = 32)

def rotateFeeRecipientChecked
    (state : FeeState) (request : FeeRotationRequest) : Option FeeState :=
  if feeRotationRequestAllowed request && state.pendingPayout = request.pendingPayout then
    rotateFeeRecipient state request.recipient
  else none

theorem accepted_rotation_checks_authority_input_and_preserves_accounting
    {state next : FeeState} {request : FeeRotationRequest}
    (accepted : rotateFeeRecipientChecked state request = some next) :
    request.governance = true ∧ request.anonymous = false ∧
      request.roleCollision = false ∧
      (request.subaccountLength = 0 ∨ request.subaccountLength = 32) ∧
      request.pendingPayout = 0 ∧ request.reconciliationHolds = 0 ∧
      request.oldHashLength = 32 ∧ request.newHashLength = 32 ∧
      next.reserve = state.reserve ∧
      next.confirmedDepositFees = state.confirmedDepositFees ∧
      next.confirmedWithdrawalFees = state.confirmedWithdrawalFees ∧
      next.pendingPayout = 0 := by
  unfold rotateFeeRecipientChecked at accepted
  split at accepted
  next allowed =>
    have both :
        feeRotationRequestAllowed request = true ∧
          decide (state.pendingPayout = request.pendingPayout) = true := by
      simpa only [Bool.and_eq_true] using allowed
    have guards :
        request.governance = true ∧ request.anonymous = false ∧
        request.roleCollision = false ∧
        (request.subaccountLength = 0 ∨ request.subaccountLength = 32) ∧
        request.pendingPayout = 0 ∧ request.reconciliationHolds = 0 ∧
        request.oldHashLength = 32 ∧ request.newHashLength = 32 := by
      simpa [feeRotationRequestAllowed] using both.1
    have rotated := Claims.fee_rotation_claim accepted
    exact ⟨guards.1, guards.2.1, guards.2.2.1,
      guards.2.2.2.1, guards.2.2.2.2.1,
      guards.2.2.2.2.2.1, guards.2.2.2.2.2.2.1,
      guards.2.2.2.2.2.2.2, rotated.2.1, rotated.2.2.1,
      rotated.2.2.2.1, rotated.2.2.2.2.1⟩
  next => simp at accepted

structure WindowState where
  windowId : Nat
  consumed : Nat
  reserved : Nat
deriving DecidableEq

structure WindowRequest where
  now : Nat
  windowSize : Nat
  grossAmount : Nat
  serviceFee : Nat
  maximumServiceFee : Nat
  perDepositLimit : Nat
  windowLimit : Nat
deriving DecidableEq

def windowIdFor (request : WindowRequest) : Nat :=
  request.now / request.windowSize

def windowConsumed (state : WindowState) (request : WindowRequest) : Nat :=
  if state.windowId = windowIdFor request then state.consumed else 0

def windowNet (request : WindowRequest) : Nat :=
  request.grossAmount - request.serviceFee

def admitWindowDeposit
    (state : WindowState) (request : WindowRequest) : Option (WindowState × Nat) :=
  if request.windowSize = 0 then none
  else if request.serviceFee ≤ request.maximumServiceFee ∧
      request.serviceFee < request.grossAmount ∧
      windowNet request ≤ request.perDepositLimit ∧
      windowConsumed state request + state.reserved + windowNet request ≤
        request.windowLimit ∧
      windowConsumed state request + state.reserved + windowNet request ≤ maxU128 then
    some ({
      windowId := windowIdFor request
      consumed := windowConsumed state request
      reserved := state.reserved + windowNet request
    }, windowNet request)
  else none

theorem accepted_window_deposit_is_positive_bounded_and_reserved
    {state next : WindowState} {request : WindowRequest} {net : Nat}
    (accepted : admitWindowDeposit state request = some (next, net)) :
    request.windowSize > 0 ∧ request.serviceFee ≤ request.maximumServiceFee ∧
      request.serviceFee < request.grossAmount ∧
      net = request.grossAmount - request.serviceFee ∧ net > 0 ∧
      net ≤ request.perDepositLimit ∧
      next.reserved = state.reserved + net ∧
      next.consumed + next.reserved ≤ request.windowLimit ∧
      next.consumed + next.reserved ≤ maxU128 := by
  unfold admitWindowDeposit at accepted
  split at accepted
  next zero => simp at accepted
  next nonzero =>
    split at accepted
    next admissible =>
      simp only [Option.some.injEq, Prod.mk.injEq] at accepted
      obtain ⟨nextEq, netEq⟩ := accepted
      subst next
      subst net
      unfold windowNet at admissible
      refine ⟨Nat.pos_of_ne_zero nonzero, admissible.1, admissible.2.1, rfl, ?_,
        admissible.2.2.1, rfl, ?_, ?_⟩
      · exact Nat.sub_pos_of_lt admissible.2.1
      · simpa only [Nat.add_assoc] using admissible.2.2.2.1
      · simpa only [Nat.add_assoc] using admissible.2.2.2.2
    next => simp at accepted

structure LeaseState where
  activeGeneration : Option Nat
  nextGeneration : Nat
deriving DecidableEq

def claimLease (state : LeaseState) : Option LeaseState :=
  if state.activeGeneration = none ∧ state.nextGeneration < maxU64 then
    some {
      activeGeneration := some (state.nextGeneration + 1)
      nextGeneration := state.nextGeneration + 1
    }
  else none

def finishLease (state : LeaseState) (outcomeGeneration : Nat) : Option LeaseState :=
  if state.activeGeneration = some outcomeGeneration then
    some { state with activeGeneration := none }
  else none

theorem claimed_lease_has_one_strictly_new_generation
    {state next : LeaseState} (accepted : claimLease state = some next) :
    state.activeGeneration = none ∧ state.nextGeneration < next.nextGeneration ∧
      next.activeGeneration = some next.nextGeneration := by
  unfold claimLease at accepted
  split at accepted
  next allowed =>
    simp only [Option.some.injEq] at accepted
    subst next
    exact ⟨allowed.1, Nat.lt_add_one _, rfl⟩
  next => simp at accepted

theorem stale_lease_cannot_finish
    {state : LeaseState} {current stale : Nat}
    (active : state.activeGeneration = some current) (different : stale ≠ current) :
    finishLease state stale = none := by
  simp [finishLease, active, Ne.symm different]

inductive DepositTracePhase where
  | fundingPending
  | escrowedUnquoted
  | authorizationPending
  | authorizationAvailable
  | expiryReconciliation
  | minted
  | fundingReconciliationHold
  | refundPending
  | refundReconciliationHold
  | refunded
  | cancelled
deriving DecidableEq

structure DepositTrace where
  phase : DepositTracePhase
  transferIdentity : TransferIdentity
  requestIdentity : RequestIdentity
  reserved : Nat
  candidate : Nat
  requirement : Nat
  feeCounted : Bool
deriving DecidableEq

inductive JobKind where
  | deposit
  | withdrawal
deriving DecidableEq

inductive JobStatus where
  | scheduled
  | leased
  | stopped
deriving DecidableEq

structure JobTrace where
  kind : JobKind
  status : JobStatus
  overdue : Bool
  expired : Bool
deriving DecidableEq

def JobTrace.scheduled (job : JobTrace) : Bool :=
  job.status == .scheduled

def JobTrace.active (job : JobTrace) : Bool :=
  job.status == .leased

def JobTrace.stopped (job : JobTrace) : Bool :=
  job.status == .stopped

def manualClaimAllowedFor (job : JobTrace) : Bool :=
  manualClaimAllowed job.scheduled job.active job.stopped job.overdue job.expired

theorem manual_claim_cannot_select_active_or_fresh_scheduled_job
    (job : JobTrace)
    (blocked : (job.active = true ∧ job.expired = false) ∨
      (job.scheduled = true ∧ job.stopped = false ∧
        job.overdue = false ∧ job.expired = false)) :
    manualClaimAllowedFor job = false := by
  rcases blocked with active | scheduled
  · simp [manualClaimAllowedFor, manualClaimAllowed, active.1, active.2]
  · simp [manualClaimAllowedFor, manualClaimAllowed, scheduled.1, scheduled.2.1,
      scheduled.2.2.1, scheduled.2.2.2]

theorem notification_quota_isolation (state : NotificationIsolationState) :
    let next := processNotification state
    next.settlementAdmission = state.settlementAdmission ∧
      next.settlementJobs = state.settlementJobs := by
  simp [processNotification]

structure LeaseLaneSnapshot where
  targetActive : Bool
  targetAutomatic : Bool
  activeInRequestedLane : Nat
deriving DecidableEq

def observeUnrelatedLease (snapshot : LeaseLaneSnapshot) : LeaseLaneSnapshot := snapshot

theorem lease_lane_isolation (snapshot : LeaseLaneSnapshot) (capacity : Nat) :
    decideLeaseLaneClaim
        (observeUnrelatedLease snapshot).targetActive
        (observeUnrelatedLease snapshot).targetAutomatic
        (observeUnrelatedLease snapshot).activeInRequestedLane
        capacity =
      decideLeaseLaneClaim snapshot.targetActive snapshot.targetAutomatic
        snapshot.activeInRequestedLane capacity := by
  rfl

structure FundingLifecycle where
  attemptActive : Bool
  formalArtifacts : Nat
  promotions : Nat
deriving DecidableEq

def applyFundingDecision
    (state : FundingLifecycle) (decision : FundingAttemptDecision) : FundingLifecycle :=
  match decision with
  | .release => { state with attemptActive := false }
  | .retain => state
  | .promoteSuccess | .promoteAmbiguous =>
      if state.attemptActive then
        { attemptActive := false, formalArtifacts := 1, promotions := state.promotions + 1 }
      else state

theorem funding_attempt_lifecycle :
    let initial : FundingLifecycle :=
      { attemptActive := true, formalArtifacts := 0, promotions := 0 }
    let released := applyFundingDecision initial (decideFundingAttempt .definitiveFailure)
    let succeeded := applyFundingDecision initial (decideFundingAttempt .success)
    let duplicated := applyFundingDecision initial (decideFundingAttempt .duplicate)
    let ambiguous := applyFundingDecision initial (decideFundingAttempt .ambiguous)
    released.formalArtifacts = 0 ∧ released.promotions = 0 ∧
      succeeded.formalArtifacts = 1 ∧ succeeded.promotions = 1 ∧
      duplicated.formalArtifacts = 1 ∧ duplicated.promotions = 1 ∧
      ambiguous.formalArtifacts = 1 ∧ ambiguous.promotions = 1 ∧
      (applyFundingDecision succeeded (decideFundingAttempt .duplicate)) = succeeded := by
  decide

structure ExecutorWitness where
  cyclesSufficient : Bool
  fair : Bool
deriving DecidableEq

def ExecutorWitness.valid (witness : ExecutorWitness) : Prop :=
  witness.cyclesSufficient = true ∧ witness.fair = true

structure ProtocolState where
  economic : EconomicState
  withdrawal : Withdrawal
  committedDestination : Account
  committedAmountOut : Nat
  committedPayloadIdentity : PayloadIdentity
  committedTransferIdentity : TransferIdentity
  canonicalObserved : Bool
  withdrawalFeeCounted : Bool
  fee : FeeState
  deposit : DepositTrace
  window : WindowState
  holdOpen : Bool
  holdRequestIdentity : RequestIdentity
  holdIdentity : HoldIdentity
  holdTransferIdentity : TransferIdentity
  job : JobTrace
  lease : LeaseState
deriving DecidableEq

def leaseBounded (lease : LeaseState) : Prop :=
  match lease.activeGeneration with
  | none => True
  | some generation => generation ≤ lease.nextGeneration

def Safe (state : ProtocolState) : Prop :=
  Backed state.economic ∧
    state.fee.reserve = state.economic.feeReserve ∧
    state.fee.pendingPayout ≤ state.fee.reserve ∧
    state.window.consumed + state.window.reserved ≤ maxU128 ∧
    state.deposit.reserved + state.deposit.candidate = state.deposit.requirement ∧
    state.withdrawal.destination = state.committedDestination ∧
    state.withdrawal.amountOut = state.committedAmountOut ∧
    state.withdrawalFeeCounted = state.withdrawal.paid ∧
    leaseBounded state.lease

structure LedgerSuccessCertificate where
  transferIdentity : TransferIdentity
  transfer : LedgerTransfer
  succeeded : Bool
deriving DecidableEq

def LedgerSuccessCertificate.valid
    (certificate : LedgerSuccessCertificate) (state : ProtocolState) : Prop :=
  certificate.succeeded = true ∧
    certificate.transferIdentity = state.committedTransferIdentity ∧
    certificate.transfer.amount = state.withdrawal.amountOut ∧
    certificate.transfer.ledgerFee ≤ state.withdrawal.chargedServiceFee ∧
    certificate.transfer.destination = state.withdrawal.destination

inductive ProtocolEvent where
  | observeCanonical (certificate : CanonicalCertificate)
  | executorClaim (witness : ExecutorWitness)
  | settle (certificate : LedgerSuccessCertificate)
  | rotateFeeRecipient (request : FeeRotationRequest)
  | reserveDeposit (request : WindowRequest)
  | fundingSucceeded (transferIdentity : TransferIdentity)
  | fundingAmbiguous (requestIdentity : RequestIdentity)
      (holdIdentity : HoldIdentity) (transferIdentity : TransferIdentity)
  | fundingFailed (transferIdentity : TransferIdentity)
  | authorizationSigned
  | beginExpiryReconciliation
  | mintReconciled
  | feePayoutSucceeded (amount fee : Nat)
  | resolveHold (evidence : HoldEvidence)
  | finishLease (generation : Nat)
  | manualClaim
deriving DecidableEq

def payoutEconomic (state : EconomicState) (debit : Nat) : Option EconomicState :=
  if debit ≤ state.escrow ∧ debit ≤ state.feeReserve then
    some { state with
      escrow := state.escrow - debit
      feeReserve := state.feeReserve - debit }
  else none

def rawStep (state : ProtocolState) : ProtocolEvent → Option ProtocolState
  | .observeCanonical certificate =>
      if canonicalCertificateAccepted certificate &&
          decide (certificate.committedPayloadIdentity = state.committedPayloadIdentity) then
        some { state with canonicalObserved := true }
      else none
  | .executorClaim witness =>
      if witness.cyclesSufficient && witness.fair then do
        let lease ← claimLease state.lease
        some { state with lease, job := { state.job with status := .leased } }
      else none
  | .settle certificate =>
      if state.canonicalObserved && certificate.succeeded &&
          certificate.transferIdentity == state.committedTransferIdentity then do
        let paid ← pay state.withdrawal certificate.transfer
        let economic ← checkedSettlement state.economic state.withdrawal.amountOut
          state.withdrawal.chargedServiceFee certificate.transfer.ledgerFee
        some { state with
          withdrawal := paid
          withdrawalFeeCounted := true
          fee := { state.fee with reserve := economic.feeReserve }
          economic }
      else none
  | .rotateFeeRecipient request => do
      let fee ← rotateFeeRecipientChecked state.fee request
      some { state with fee }
  | .reserveDeposit request => do
      let (window, net) ← admitWindowDeposit state.window request
      if state.deposit.phase = .escrowedUnquoted ∧ state.deposit.candidate = 0 then
        some { state with
          window
          deposit := { state.deposit with
            phase := .authorizationPending
            reserved := state.deposit.reserved + net
            candidate := 0
            requirement := state.deposit.requirement + net } }
      else none
  | .fundingSucceeded identity =>
      if state.deposit.phase = .fundingPending ∧
          identity = state.deposit.transferIdentity then
        some { state with deposit := { state.deposit with phase := .escrowedUnquoted } }
      else none
  | .fundingAmbiguous request hold identity =>
      if state.deposit.phase = .fundingPending ∧
          request = state.deposit.requestIdentity ∧
          identity = state.deposit.transferIdentity then
        some { state with
          deposit := { state.deposit with phase := .fundingReconciliationHold }
          holdOpen := true
          holdRequestIdentity := request
          holdIdentity := hold
          holdTransferIdentity := identity }
      else none
  | .fundingFailed identity =>
      if state.deposit.phase = .fundingPending ∧
          identity = state.deposit.transferIdentity then
        some { state with deposit := { state.deposit with phase := .cancelled } }
      else none
  | .authorizationSigned =>
      if state.deposit.phase = .authorizationPending then
        some { state with deposit := { state.deposit with phase := .authorizationAvailable } }
      else none
  | .beginExpiryReconciliation =>
      if state.deposit.phase = .authorizationPending ∨
          state.deposit.phase = .authorizationAvailable then
        some { state with deposit := { state.deposit with phase := .expiryReconciliation } }
      else none
  | .mintReconciled =>
      if state.deposit.phase = .expiryReconciliation then
        some { state with deposit := {
          state.deposit with
            phase := .minted
            reserved := 0
            candidate := 0
            requirement := 0
            feeCounted := true } }
      else none
  | .feePayoutSucceeded amount fee =>
      let debit := amount + fee
      if debit ≤ state.fee.pendingPayout ∧ debit ≤ state.fee.reserve then do
        let economic ← payoutEconomic state.economic debit
        some { state with
          economic
          fee := { state.fee with
            reserve := state.fee.reserve - debit
            pendingPayout := state.fee.pendingPayout - debit } }
      else none
  | .resolveHold evidence =>
      if state.holdOpen &&
          holdEvidenceValid state.holdRequestIdentity state.holdIdentity
            state.holdTransferIdentity evidence then
        some { state with holdOpen := false }
      else none
  | .finishLease generation => do
      let lease ← finishLease state.lease generation
      some { state with lease, job := { state.job with status := .scheduled } }
  | .manualClaim =>
      if manualClaimAllowedFor state.job then do
        let lease ← claimLease state.lease
        some { state with lease, job := { state.job with status := .leased } }
      else none

def step (state : ProtocolState) (event : ProtocolEvent) : Option ProtocolState :=
  rawStep state event

private theorem payoutEconomic_preserves_backing
    {state next : EconomicState} {debit : Nat}
    (backed : Backed state)
    (accepted : payoutEconomic state debit = some next) :
    Backed next ∧ next.feeReserve = state.feeReserve - debit := by
  unfold payoutEconomic at accepted
  split at accepted
  next bounds =>
    simp only [Option.some.injEq] at accepted
    subst next
    constructor
    · simp only [Backed] at backed ⊢
      omega
    · rfl
  next => simp at accepted

theorem raw_step_preserves_safe
    {state next : ProtocolState} {event : ProtocolEvent}
    (safe : Safe state) (accepted : rawStep state event = some next) :
    Safe next := by
  rcases safe with ⟨backed, feeEconomic, payoutBound, windowBound,
    reservation, destination, amountOut, counted, leaseSafe⟩
  cases event with
  | observeCanonical certificate =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | executorClaim witness =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        obtain ⟨lease, leaseAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        simp only [Option.some.injEq] at result
        subst next
        have claimed := claimed_lease_has_one_strictly_new_generation leaseAccepted
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, by simp [leaseBounded, claimed.2.2]⟩
      next => simp at accepted
  | settle certificate =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        obtain ⟨paid, payAccepted, rest⟩ := Option.bind_eq_some_iff.mp accepted
        obtain ⟨economic, settlementAccepted, result⟩ := Option.bind_eq_some_iff.mp rest
        simp only [Option.some.injEq] at result
        subst next
        have settlement := Claims.settlement_backing_claim settlementAccepted
        have payment := Claims.payment_claim.1 payAccepted
        have reserveMonotone : state.economic.feeReserve ≤ economic.feeReserve := by
          unfold checkedSettlement at settlementAccepted
          split at settlementAccepted
          next guards =>
            simp only [Option.some.injEq] at settlementAccepted
            subst economic
            simp only [settleDebt]
            omega
          next => simp at settlementAccepted
        have nextPayoutBound : state.fee.pendingPayout ≤ economic.feeReserve := by
          calc
            state.fee.pendingPayout ≤ state.fee.reserve := payoutBound
            _ = state.economic.feeReserve := feeEconomic
            _ ≤ economic.feeReserve := reserveMonotone
        exact ⟨settlement.2.2.2.2, rfl, nextPayoutBound,
          windowBound, reservation, payment.2.2.1.trans destination,
          payment.2.2.2.trans amountOut, by
            have paidTrue : paid.paid = true := by
              unfold pay at payAccepted
              split at payAccepted
              next =>
                simp only [Option.some.injEq] at payAccepted
                subst paid
                rfl
              next => simp at payAccepted
            simp [paidTrue], leaseSafe⟩
      next => simp at accepted
  | rotateFeeRecipient request =>
      obtain ⟨fee, feeAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
      simp only [Option.some.injEq] at result
      subst next
      have rotated :=
        accepted_rotation_checks_authority_input_and_preserves_accounting feeAccepted
      rcases rotated with
        ⟨_, _, _, _, _, _, _, _, reserveEq, _, _, pendingZero⟩
      exact ⟨backed, reserveEq.trans feeEconomic,
        by simp [pendingZero],
        windowBound, reservation, destination, amountOut, counted, leaseSafe⟩
  | reserveDeposit request =>
      cases admissionEq : admitWindowDeposit state.window request with
      | none => simp [rawStep, admissionEq] at accepted
      | some admission =>
          rcases admission with ⟨window, net⟩
          simp only [rawStep, admissionEq] at accepted
          split at accepted
          next empty =>
            injection accepted with accepted
            subst next
            have admitted :=
              accepted_window_deposit_is_positive_bounded_and_reserved admissionEq
            exact ⟨backed, feeEconomic, payoutBound, admitted.2.2.2.2.2.2.2.2,
              by
                change state.deposit.reserved + net + 0 =
                  state.deposit.requirement + net
                omega,
              destination, amountOut, counted, leaseSafe⟩
          next => simp at accepted
  | fundingSucceeded identity =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | fundingAmbiguous request hold identity =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | fundingFailed identity =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | authorizationSigned =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | beginExpiryReconciliation =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | mintReconciled =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, by simp,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | feePayoutSucceeded amount fee =>
      simp only [rawStep] at accepted
      split at accepted
      next debitAllowed =>
        obtain ⟨economic, economicAccepted, result⟩ :=
          Option.bind_eq_some_iff.mp accepted
        simp only [Option.some.injEq] at result
        subst next
        have economicSafe := payoutEconomic_preserves_backing backed economicAccepted
        have nextFeeEconomic :
            state.fee.reserve - (amount + fee) = economic.feeReserve := by
          rw [feeEconomic, economicSafe.2]
        have nextPayoutBound :
            state.fee.pendingPayout - (amount + fee) ≤
              state.fee.reserve - (amount + fee) :=
          Nat.sub_le_sub_right payoutBound (amount + fee)
        exact ⟨economicSafe.1, nextFeeEconomic, nextPayoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | resolveHold evidence =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, leaseSafe⟩
      next => simp at accepted
  | finishLease generation =>
      obtain ⟨lease, leaseAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
      simp only [Option.some.injEq] at result
      subst next
      unfold finishLease at leaseAccepted
      split at leaseAccepted
      next =>
        simp only [Option.some.injEq] at leaseAccepted
        subst lease
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, by simp [leaseBounded]⟩
      next => simp at leaseAccepted
  | manualClaim =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        obtain ⟨lease, leaseAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        simp only [Option.some.injEq] at result
        subst next
        have claimed := claimed_lease_has_one_strictly_new_generation leaseAccepted
        exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
          destination, amountOut, counted, by simp [leaseBounded, claimed.2.2]⟩
      next => simp at accepted

theorem step_preserves_safe
    {state next : ProtocolState} {event : ProtocolEvent}
    (safe : Safe state) (accepted : step state event = some next) :
    Safe next :=
  raw_step_preserves_safe safe accepted

theorem step_preserves_backing
    {state next : ProtocolState} {event : ProtocolEvent}
    (safe : Safe state) (accepted : step state event = some next) :
    Backed next.economic :=
  (step_preserves_safe safe accepted).1

def Runs : ProtocolState → List ProtocolEvent → ProtocolState → Prop
  | state, [], final => final = state
  | state, event :: rest, final =>
      ∃ next, step state event = some next ∧ Runs next rest final

theorem runs_preserve_safe
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    Safe final := by
  induction events generalizing state with
  | nil =>
      simp only [Runs] at runs
      subst final
      exact safe
  | cons event rest ih =>
      simp only [Runs] at runs
      obtain ⟨next, accepted, tail⟩ := runs
      exact ih (step_preserves_safe safe accepted) tail

theorem runs_preserve_backing
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    Backed final.economic :=
  (runs_preserve_safe safe runs).1

theorem committed_quote_is_immutable_across_trace
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    final.withdrawal.destination = final.committedDestination ∧
      final.withdrawal.amountOut = final.committedAmountOut := by
  have finalSafe := runs_preserve_safe safe runs
  exact ⟨finalSafe.2.2.2.2.2.1, finalSafe.2.2.2.2.2.2.1⟩

theorem reservation_requirement_is_preserved_across_trace
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    final.deposit.reserved + final.deposit.candidate = final.deposit.requirement :=
  (runs_preserve_safe safe runs).2.2.2.2.1

theorem service_fee_bound_is_preserved_across_trace
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    final.fee.pendingPayout ≤ final.fee.reserve :=
  (runs_preserve_safe safe runs).2.2.1

theorem payout_preserves_backing_and_reserve_bound
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    Backed final.economic ∧ final.fee.reserve = final.economic.feeReserve ∧
      final.fee.pendingPayout ≤ final.fee.reserve := by
  have finalSafe := runs_preserve_safe safe runs
  exact ⟨finalSafe.1, finalSafe.2.1, finalSafe.2.2.1⟩

theorem finalized_success_notifies
    {receiptBlock finalizedBlock : Nat}
    (finalized : receiptBlock ≤ finalizedBlock) :
    decideWithdrawalFinalization true receiptBlock (some finalizedBlock) = .notify := by
  simp [decideWithdrawalFinalization, Nat.not_lt.mpr finalized]

theorem restored_queue_preserves_other_keys
    {queue : PendingQueue} {incoming : PendingQueueEntry} {key : Nat}
    (different : key ≠ incoming.key) :
    restorePendingQueue queue incoming key = queue key := by
  simp [restorePendingQueue, upsertPendingQueue, different]

theorem paid_is_terminal_across_step
    {state next : ProtocolState} {event : ProtocolEvent}
    (paid : state.withdrawal.paid = true)
    (accepted : step state event = some next) :
    next.withdrawal = state.withdrawal := by
  cases event with
  | observeCanonical certificate =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | executorClaim witness =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        obtain ⟨lease, _, result⟩ := Option.bind_eq_some_iff.mp accepted
        simp only [Option.some.injEq] at result
        subst next
        rfl
      next => simp at accepted
  | settle certificate =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        obtain ⟨paidWithdrawal, payAccepted, _⟩ :=
          Option.bind_eq_some_iff.mp accepted
        simp [pay, paid] at payAccepted
      next => simp at accepted
  | rotateFeeRecipient request =>
      obtain ⟨fee, _, result⟩ := Option.bind_eq_some_iff.mp accepted
      simp only [Option.some.injEq] at result
      subst next
      rfl
  | reserveDeposit request =>
      cases admissionEq : admitWindowDeposit state.window request with
      | none => simp [step, rawStep, admissionEq] at accepted
      | some admission =>
          rcases admission with ⟨window, net⟩
          simp only [step, rawStep, admissionEq] at accepted
          split at accepted
          next =>
            injection accepted with accepted
            subst next
            rfl
          next => simp at accepted
  | fundingSucceeded identity =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | fundingAmbiguous request hold identity =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | fundingFailed identity =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | authorizationSigned =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | beginExpiryReconciliation =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | mintReconciled =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | feePayoutSucceeded amount fee =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        obtain ⟨economic, _, result⟩ := Option.bind_eq_some_iff.mp accepted
        simp only [Option.some.injEq] at result
        subst next
        rfl
      next => simp at accepted
  | resolveHold evidence =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next => simp at accepted
  | finishLease generation =>
      obtain ⟨lease, _, result⟩ := Option.bind_eq_some_iff.mp accepted
      simp only [Option.some.injEq] at result
      subst next
      rfl
  | manualClaim =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        obtain ⟨lease, _, result⟩ := Option.bind_eq_some_iff.mp accepted
        simp only [Option.some.injEq] at result
        subst next
        rfl
      next => simp at accepted

theorem conditional_committed_withdrawal_reaches_paid
    {state : ProtocolState}
    {canonical : CanonicalCertificate}
    {ledger : LedgerSuccessCertificate}
    {executor : ExecutorWitness}
    (safe : Safe state)
    (unpaid : state.withdrawal.paid = false)
    (canonicalValid : canonical.valid)
    (payload : canonical.committedPayloadIdentity = state.committedPayloadIdentity)
    (ledgerValid : ledger.valid state)
    (executorValid : executor.valid)
    (leaseFree : state.lease.activeGeneration = none)
    (generationBound : state.lease.nextGeneration < maxU64)
    (liability : state.withdrawal.amountOut + state.withdrawal.chargedServiceFee ≤
      state.economic.unpaidLiability)
    (escrow : state.withdrawal.amountOut + ledger.transfer.ledgerFee ≤
      state.economic.escrow) :
    ∃ final,
      Runs state [.observeCanonical canonical, .executorClaim executor, .settle ledger] final ∧
      final.withdrawal.paid = true ∧
      final.withdrawal.destination = state.withdrawal.destination ∧
      final.withdrawal.amountOut = state.withdrawal.amountOut := by
  rcases safe with ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
    destination, amountOut, counted, leaseSafe⟩
  rcases ledgerValid with
    ⟨ledgerSucceeded, transferIdentity, transferAmount, feeBound, transferDestination⟩
  let observed : ProtocolState := { state with canonicalObserved := true }
  have observedSafe : Safe observed := by
    exact ⟨backed, feeEconomic, payoutBound, windowBound, reservation,
      destination, amountOut, counted, leaseSafe⟩
  have canonicalAccepted : canonicalCertificateAccepted canonical = true := by
    rcases canonicalValid with ⟨block, hash, finalized, payloadIdentity⟩
    have snapshotFinalized : canonical.snapshotBlock ≤ canonical.finalizedWatermark := by
      rw [← block]
      exact finalized
    simp [canonicalCertificateAccepted, block, hash, snapshotFinalized, payloadIdentity]
  have observeStep : step state (.observeCanonical canonical) = some observed := by
    simp [step, rawStep, canonicalAccepted, payload, observed]
  let claimedLease : LeaseState := {
    activeGeneration := some (state.lease.nextGeneration + 1)
    nextGeneration := state.lease.nextGeneration + 1 }
  let claimed : ProtocolState := {
    observed with
    lease := claimedLease
    job := { state.job with status := .leased } }
  have claimStep : step observed (.executorClaim executor) = some claimed := by
    rcases executorValid with ⟨cycles, fair⟩
    simp [step, rawStep, cycles, fair, claimLease, leaseFree, generationBound,
      observed, claimed, claimedLease]
  have payAccepted :
      pay claimed.withdrawal ledger.transfer =
        some { claimed.withdrawal with paid := true } := by
    simp [pay, unpaid, transferAmount, feeBound, transferDestination, claimed, observed]
  have settlementAccepted :
      ∃ economic, checkedSettlement claimed.economic claimed.withdrawal.amountOut
        claimed.withdrawal.chargedServiceFee ledger.transfer.ledgerFee = some economic := by
    let economic := settleDebt state.economic state.withdrawal.amountOut
      state.withdrawal.chargedServiceFee ledger.transfer.ledgerFee
    refine ⟨economic, ?_⟩
    have escrowBound :
        state.withdrawal.amountOut + ledger.transfer.ledgerFee ≤
          state.economic.baseSupply + state.economic.feeReserve +
            state.economic.unpaidLiability := by
      rw [← backed]
      exact escrow
    unfold Backed at backed
    simp [checkedSettlement, economic, claimed, observed, backed, feeBound, liability,
      escrowBound]
  obtain ⟨economic, settlementAccepted⟩ := settlementAccepted
  let final : ProtocolState := {
    claimed with
    economic
    fee := { claimed.fee with reserve := economic.feeReserve }
    withdrawal := { claimed.withdrawal with paid := true }
    withdrawalFeeCounted := true }
  have settleStep : step claimed (.settle ledger) = some final := by
    simp [step, rawStep, claimed, observed, ledgerSucceeded, transferIdentity,
      payAccepted, settlementAccepted, final]
  refine ⟨final, ?_, by simp [final], by simp [final, claimed, observed],
    by simp [final, claimed, observed]⟩
  simp [Runs, observeStep, claimStep, settleStep]

end BridgeSpec.Protocol
