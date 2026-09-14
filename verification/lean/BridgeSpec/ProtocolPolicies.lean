import BridgeSpec.DepositHistory

namespace BridgeSpec.Protocol

open BridgeSpec
open BridgeSpec.FiniteWidthModel


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
  | refundAvailable
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
    let verified := processNotificationVerification state
    let ingested := processNotificationIngestion verified
    verified.persistentVerificationCount = state.persistentVerificationCount + 1 ∧
      verified.persistentIngestionCount = state.persistentIngestionCount ∧
      ingested.persistentIngestionCount = state.persistentIngestionCount + 1 ∧
      ingested.settlementAdmission = state.settlementAdmission ∧
      ingested.settlementJobs = state.settlementJobs := by
  simp [processNotificationVerification, processNotificationIngestion]

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

theorem funding_reconciliation_requires_fresh_expired_absence :
    decideFundingReconciliation false false false = .wait ∧
      decideFundingReconciliation false false true = .wait ∧
      decideFundingReconciliation false true false = .wait ∧
      decideFundingReconciliation false true true = .wait ∧
      decideFundingReconciliation true false false = .restartFresh ∧
      decideFundingReconciliation true false true = .restartFresh ∧
      decideFundingReconciliation true true false = .wait ∧
      decideFundingReconciliation true true true = .release := by
  decide


end BridgeSpec.Protocol
