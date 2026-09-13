import BridgeSpec.ProtocolPolicies

namespace BridgeSpec.Protocol

open BridgeSpec
open BridgeSpec.FiniteWidthModel

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

def initialState : ProtocolState := {
  economic := { escrow := 0, baseSupply := 0, feeReserve := 0, unpaidLiability := 0 }
  withdrawal := {
    amount := 0
    chargedServiceFee := 0
    amountOut := 0
    destination := { owner := [], subaccount := [] }
    paid := false }
  committedDestination := { owner := [], subaccount := [] }
  committedAmountOut := 0
  committedPayloadIdentity := 0
  committedTransferIdentity := 0
  canonicalObserved := false
  withdrawalFeeCounted := false
  fee := {
    reserve := 0
    confirmedDepositFees := 0
    confirmedWithdrawalFees := 0
    pendingPayout := 0
    recipient := 0 }
  deposit := {
    phase := .fundingPending
    transferIdentity := 0
    requestIdentity := 0
    reserved := 0
    candidate := 0
    requirement := 0
    feeCounted := false }
  window := { windowId := 0, consumed := 0, reserved := 0 }
  holdOpen := false
  holdRequestIdentity := 0
  holdIdentity := 0
  holdTransferIdentity := 0
  job := { kind := .deposit, status := .scheduled, overdue := false, expired := false }
  lease := { activeGeneration := none, nextGeneration := 0 }
}

theorem initial_state_is_safe : Safe initialState := by
  simp [Safe, initialState, Backed, leaseBounded]

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
  | releaseExpiredReservation
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
        some { state with deposit := {
          state.deposit with phase := .authorizationAvailable, feeCounted := true } }
      else none
  | .releaseExpiredReservation =>
      if state.deposit.phase = .authorizationPending then
        some { state with deposit := {
          state.deposit with
            phase := .refundAvailable
            reserved := 0
            candidate := 0
            requirement := 0 } }
      else if state.deposit.phase = .authorizationAvailable then
        some { state with deposit := {
          state.deposit with
            phase := .refundAvailable
            reserved := 0
            candidate := 0
            requirement := 0 } }
      else none
  | .mintReconciled =>
      if (state.deposit.phase = .authorizationAvailable ∨ state.deposit.phase = .refundAvailable) ∧ state.deposit.feeCounted = true then
        some { state with deposit := {
          state.deposit with
            phase := .minted
            reserved := 0
            candidate := 0
            requirement := 0 } }
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
  | releaseExpiredReservation =>
      simp only [rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨backed, feeEconomic, payoutBound, windowBound, by simp,
          destination, amountOut, counted, leaseSafe⟩
      next =>
        split at accepted
        next =>
          simp only [Option.some.injEq] at accepted
          subst next
          exact ⟨backed, feeEconomic, payoutBound, windowBound, by simp,
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

def Reachable (state : ProtocolState) : Prop :=
  ∃ events, Runs initialState events state

theorem reachable_is_safe {state : ProtocolState} (reachable : Reachable state) :
    Safe state := by
  obtain ⟨events, runs⟩ := reachable
  exact runs_preserve_safe initial_state_is_safe runs

noncomputable def filterSafeStoredState (stored : ProtocolState) : Option ProtocolState := by
  classical
  exact if Safe stored then some stored else none

theorem filtered_stored_state_is_safe {stored reopened : ProtocolState}
    (accepted : filterSafeStoredState stored = some reopened) : Safe reopened := by
  unfold filterSafeStoredState at accepted
  classical
  split at accepted
  next safe =>
    simp only [Option.some.injEq] at accepted
    subst reopened
    exact safe
  next => simp at accepted

theorem runs_preserve_backing
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    Backed final.economic :=
  (runs_preserve_safe safe runs).1

theorem step_preserves_committed_quote
    {state next : ProtocolState} {event : ProtocolEvent}
    (accepted : step state event = some next) :
    next.committedDestination = state.committedDestination ∧
      next.committedAmountOut = state.committedAmountOut := by
  cases event <;> simp [step, rawStep, Option.bind_eq_some_iff] at accepted <;> grind

theorem runs_preserve_committed_quote
    {state final : ProtocolState} {events : List ProtocolEvent}
    (runs : Runs state events final) :
    final.committedDestination = state.committedDestination ∧
      final.committedAmountOut = state.committedAmountOut := by
  induction events generalizing state with
  | nil => simp [Runs] at runs; subst final; exact ⟨rfl, rfl⟩
  | cons event rest ih =>
      obtain ⟨next, accepted, tail⟩ := runs
      obtain ⟨destination, amount⟩ := step_preserves_committed_quote accepted
      obtain ⟨finalDestination, finalAmount⟩ := ih tail
      exact ⟨finalDestination.trans destination, finalAmount.trans amount⟩

theorem committed_quote_is_immutable_across_trace
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    final.withdrawal.destination = state.committedDestination ∧
      final.withdrawal.amountOut = state.committedAmountOut := by
  have finalSafe := runs_preserve_safe safe runs
  obtain ⟨destination, amount⟩ := runs_preserve_committed_quote runs
  exact ⟨finalSafe.2.2.2.2.2.1.trans destination,
    finalSafe.2.2.2.2.2.2.1.trans amount⟩

theorem reservation_requirement_is_preserved_across_trace
    {state final : ProtocolState} {events : List ProtocolEvent}
    (safe : Safe state) (runs : Runs state events final) :
    final.deposit.reserved + final.deposit.candidate = final.deposit.requirement :=
  (runs_preserve_safe safe runs).2.2.2.2.1

theorem pending_payout_is_bounded_by_reserve_across_trace
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
    decideWithdrawalFinalization true receiptBlock (some finalizedBlock) true = .notify := by
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
  | releaseExpiredReservation =>
      simp only [step, rawStep] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        rfl
      next =>
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
