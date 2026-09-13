import BridgeSpec.ModelRefinement
import BridgeSpec.DepositAuthorization

namespace BridgeSpec.Protocol

open BridgeSpec
open BridgeSpec.FiniteWidthModel

namespace Deposit

open BridgeSpec.MintAuthorization

abbrev State := DepositState

inductive Event where
  | fund (grossAmount : Nat)
  | commitAuthorization (authorization : Authorization) (origin : AuthorizationOrigin)
  | installSignature (observedTimestamp : Nat)
  | releaseExpiredReservation (finalizedTimestamp : Nat)
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
  | .installSignature observedTimestamp => installSignature state observedTimestamp
  | .releaseExpiredReservation finalizedTimestamp =>
      releaseExpiredReservation state finalizedTimestamp
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

theorem deposit_run_append {state final : State} {historyPrefix suffix : List Event}
    (accepted : depositRun state (historyPrefix ++ suffix) = some final) :
    ∃ middle, depositRun state historyPrefix = some middle ∧
      depositRun middle suffix = some final := by
  induction historyPrefix generalizing state with
  | nil => exact ⟨state, rfl, accepted⟩
  | cons event rest ih =>
      simp only [List.cons_append, depositRun] at accepted
      cases stepped : depositStep state event with
      | none => simp [stepped] at accepted
      | some next =>
          simp only [stepped] at accepted
          obtain ⟨middle, prefixAccepted, suffixAccepted⟩ := ih accepted
          exact ⟨middle, by simp [depositRun, stepped, prefixAccepted], suffixAccepted⟩

theorem refund_event_in_accepted_trace_requires_finalized_absence
    {final : State} {historyPrefix suffix : List Event}
    {origin : AuthorizationOrigin} {evidence : ExpiryEvidence}
    (accepted :
      depositRun initial
        (historyPrefix ++ .startExpiredRefund origin evidence :: suffix) = some final) :
    evidence.depositProcessed = false ∧
      ∃ authorization : Authorization,
        evidence.depositId = authorization.depositId ∧
        evidence.authorizationDigest = authorization.digest ∧
        evidence.finalizedTimestamp > authorization.deadline := by
  obtain ⟨middle, _, tailAccepted⟩ := deposit_run_append accepted
  simp only [depositRun] at tailAccepted
  cases started : depositStep middle (.startExpiredRefund origin evidence) with
  | none => simp [started] at tailAccepted
  | some next =>
      have localAccepted : startExpiredRefund middle origin evidence = some next := by
        simpa [depositStep] using started
      obtain ⟨unprocessed, authorization, _, depositId, digest, expired⟩ :=
        refund_request_cannot_bypass_finalized_evidence localAccepted
      exact ⟨unprocessed, authorization, depositId, digest, expired⟩

theorem refund_request_after_accepted_prefix_requires_authentication_and_absence
    {state next : State} {historyPrefix : List Event}
    {authenticated : Bool}
    {origin : AuthorizationOrigin} {evidence : ExpiryEvidence}
    (_prefixAccepted : depositRun initial historyPrefix = some state)
    (accepted :
      requestExpiredRefund authenticated state origin evidence = some next) :
    authenticated = true ∧
      evidence.depositProcessed = false := by
  have checked :=
    accepted_refund_request_requires_authentication_and_finalized_absence accepted
  exact ⟨checked.1, checked.2.1⟩

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
  | .authorizationPending =>
      ∃ authorization,
        state.authorization = some authorization ∧
        state.reservedMint = authorization.netAmount ∧
        authorization.grossAmount ≤ state.pendingDepositLiability ∧
        authorization.netAmount + authorization.chargedServiceFee =
          authorization.grossAmount
  | .authorizationAvailable =>
      ∃ authorization,
        state.authorization = some authorization ∧
        state.reservedMint = authorization.netAmount ∧
        authorization.netAmount ≤ state.pendingDepositLiability ∧
        authorization.netAmount + authorization.chargedServiceFee =
          authorization.grossAmount
  | _ => state.reservedMint = 0

def FeeConsistent (state : State) : Prop :=
  (state.phase = .authorizationAvailable →
      state.feeCounted = true) ∧
    (state.feeCounted = true →
      state.phase = .authorizationAvailable ∨
      state.phase = .refundAvailable ∨
      state.phase = .refundPending ∨
      state.phase = .refundReconciliationHold ∨
      state.phase = .refunded ∨ state.phase = .minted)

def WellFormed (state : State) : Prop :=
  BridgeSpec.MintAuthorization.Backed state ∧
    ReservationConsistent state ∧ FeeConsistent state

theorem initial_well_formed : WellFormed initial := by
  simp [WellFormed, initial, BridgeSpec.MintAuthorization.Backed,
    ReservationConsistent, FeeConsistent]

theorem fee_false_of_consistent_of_preauthorization
    {state : State} (consistent : FeeConsistent state)
    (preauthorization :
      state.phase = .fundingPending ∨
      state.phase = .escrowedUnquoted ∨
      state.phase = .authorizationPending ∨
      state.phase = .cancelled) :
    state.feeCounted = false := by
  cases counted : state.feeCounted with
  | false => rfl
  | true =>
      rcases consistent.2 counted with available | refundAvailable | refundPending | refundHold | refunded | minted <;>
        simp_all

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
          fee_false_of_consistent_of_preauthorization fee (Or.inl phase)
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
        rcases allowed with ⟨phase, _, authValid, liability⟩
        have amount :
            authorization.netAmount + authorization.chargedServiceFee =
              authorization.grossAmount :=
          authValid.2.2.2.2.2.2.1
        have feeFalse : state.feeCounted = false :=
          fee_false_of_consistent_of_preauthorization fee (Or.inr (Or.inl phase))
        exact ⟨backed, by
          simp [ReservationConsistent, liability, amount], by
          simp [FeeConsistent, feeFalse]⟩
      next => simp at accepted
  | installSignature observedTimestamp =>
      simp only [depositStep] at accepted
      unfold installSignature at accepted
      cases auth : state.authorization with
      | none => simp [auth] at accepted
      | some authorization =>
          simp only [auth] at accepted
          split at accepted
          next allowed =>
            rcases allowed with ⟨phase, feeFalse, feeBound, timeAllowed⟩
            simp [phase, ReservationConsistent, auth] at reservation
            rcases reservation with ⟨reserved, liability, amount⟩
            simp only [Option.some.injEq] at accepted
            subst next
            constructor
            · simp only [BridgeSpec.MintAuthorization.Backed] at backed ⊢
              omega
            · constructor
              · simp [ReservationConsistent, auth, reserved, amount]
                omega
              · simp [FeeConsistent]
          next => simp at accepted
  | releaseExpiredReservation finalizedTimestamp =>
      simp only [depositStep] at accepted
      unfold releaseExpiredReservation at accepted
      cases auth : state.authorization with
      | none => simp [auth] at accepted
      | some authorization =>
          simp only [auth] at accepted
          split at accepted
          next =>
            split at accepted
            next pending =>
              simp only [Option.some.injEq] at accepted
              subst next
              have feeFalse :=
                fee_false_of_consistent_of_preauthorization fee
                  (Or.inr (Or.inr (Or.inl pending)))
              exact ⟨backed, by simp [ReservationConsistent], by
                simp [FeeConsistent, feeFalse]⟩
            next =>
              split at accepted
              next available =>
                simp only [Option.some.injEq] at accepted
                subst next
                have feeCounted := fee.1 available
                exact ⟨backed, by simp [ReservationConsistent], by
                  simp [FeeConsistent, feeCounted]⟩
              next => simp at accepted
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
            have feeCounted : state.feeCounted = true := allowed.2.2
            simp only [Option.some.injEq] at accepted
            subst next
            exact ⟨backed, by simp [ReservationConsistent], by
              simp [FeeConsistent, feeCounted]⟩
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
            rcases allowed with ⟨_, exactEvidence, liability, feeCounted⟩
            simp only [Option.some.injEq] at accepted
            subst next
            have amount :
                authorization.netAmount + authorization.chargedServiceFee =
                  authorization.grossAmount := by
              exact exactEvidence.2.2.2.2.2.2.2.2.2.2.2.2.2.2.2.2
            constructor
            · simp only [BridgeSpec.MintAuthorization.Backed] at backed ⊢
              omega
            · simp [ReservationConsistent, FeeConsistent, feeCounted]
          next => simp at accepted
  | completeRefund =>
      have refundSafe :=
        refund_preserves_backing_and_keeps_charged_fee backed accepted
      rcases refundSafe with ⟨nextBacked, nextPhase, nextReserved, nextCounted, _⟩
      constructor
      · exact nextBacked
      · constructor
        · simp [ReservationConsistent, nextPhase, nextReserved]
        · cases counted : state.feeCounted with
          | false => simp [FeeConsistent, nextCounted, counted, nextPhase]
          | true => simp [FeeConsistent, nextCounted, counted, nextPhase]
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

def Event.feeCreditCount : Event → Nat
  | .installSignature _ => 1
  | _ => 0

def traceFeeCreditCount : List Event → Nat
  | [] => 0
  | event :: events => event.feeCreditCount + traceFeeCreditCount events

theorem deposit_step_fee_credit_count
    {state next : State} {event : Event}
    (accepted : depositStep state event = some next) :
    feeCreditCount next = feeCreditCount state + event.feeCreditCount := by
  cases event <;>
    simp only [depositStep] at accepted
  · split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simp [feeCreditCount, Event.feeCreditCount, fund]
    · simp at accepted
  · unfold commitAuthorization at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simp [feeCreditCount, Event.feeCreditCount]
    · simp at accepted
  · unfold installSignature at accepted
    cases auth : state.authorization with
    | none => simp [auth] at accepted
    | some authorization =>
        simp only [auth] at accepted
        split at accepted
        next allowed =>
          simp only [Option.some.injEq] at accepted
          subst next
          simp [feeCreditCount, Event.feeCreditCount, allowed.2.1]
        next => simp at accepted
  · unfold releaseExpiredReservation at accepted
    cases auth : state.authorization with
    | none => simp [auth] at accepted
    | some authorization =>
        simp only [auth] at accepted
        split at accepted
        · split at accepted
          · simp only [Option.some.injEq] at accepted
            subst next
            simp [feeCreditCount, Event.feeCreditCount]
          · split at accepted
            · simp only [Option.some.injEq] at accepted
              subst next
              simp [feeCreditCount, Event.feeCreditCount]
            · simp at accepted
        · simp at accepted
  · unfold startExpiredRefund at accepted
    cases auth : state.authorization with
    | none => simp [auth] at accepted
    | some authorization =>
        simp only [auth] at accepted
        split at accepted
        · simp only [Option.some.injEq] at accepted
          subst next
          simp [feeCreditCount, Event.feeCreditCount]
        · simp at accepted
  · unfold completeMint at accepted
    cases auth : state.authorization with
    | none => simp [auth] at accepted
    | some authorization =>
        simp only [auth] at accepted
        split at accepted
        · simp only [Option.some.injEq] at accepted
          subst next
          simp [feeCreditCount, Event.feeCreditCount]
        · simp at accepted
  · unfold completeRefund at accepted
    cases auth : state.authorization with
    | none => simp [auth] at accepted
    | some authorization =>
        simp only [auth] at accepted
        split at accepted <;> split at accepted
        all_goals try
          simp only [Option.some.injEq] at accepted
          subst next
          simp [feeCreditCount, Event.feeCreditCount]
        all_goals simp at accepted
  · unfold manualClaim at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simp [feeCreditCount, Event.feeCreditCount]
    · simp at accepted

theorem deposit_run_fee_credit_count_exact
    {state next : State} {events : List Event}
    (accepted : depositRun state events = some next) :
    feeCreditCount next = feeCreditCount state + traceFeeCreditCount events := by
  induction events generalizing state with
  | nil =>
      simp [depositRun] at accepted
      subst next
      simp [traceFeeCreditCount]
  | cons event events ih =>
      simp only [depositRun] at accepted
      cases step : depositStep state event with
      | none => simp [step] at accepted
      | some intermediate =>
          simp only [step] at accepted
          rw [ih accepted, deposit_step_fee_credit_count step]
          simp [traceFeeCreditCount, Nat.add_assoc, Nat.add_left_comm]

theorem deposit_run_fee_credit_count_at_most_once
    {next : State} {events : List Event}
    (accepted : depositRun initial events = some next) :
    traceFeeCreditCount events ≤ 1 := by
  have exact := deposit_run_fee_credit_count_exact accepted
  simp [feeCreditCount, initial] at exact
  rw [← exact]
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
    simp only [committed] at accepted
    split at accepted
    · simp only [Option.some.injEq] at accepted
      subst next
      simpa using committed
    · simp at accepted
  · unfold releaseExpiredReservation at accepted
    simp only [committed] at accepted
    split at accepted
    · split at accepted
      · simp only [Option.some.injEq] at accepted
        subst next
        simpa using committed
      · split at accepted
        · simp only [Option.some.injEq] at accepted
          subst next
          simpa using committed
        · simp at accepted
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
    · split at accepted
      · simp only [Option.some.injEq] at accepted
        subst next
        simpa using committed
      · simp at accepted
    · split at accepted
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
      releaseExpiredReservation, startExpiredRefund, completeMint, completeRefund,
      manualClaim] at terminalState ⊢
  all_goals cases state.authorization <;> simp

theorem terminal_trace_is_absorbing
    {state : State} (terminalState : terminal state.phase = true)
    {event : Event} {events : List Event} :
    depositRun state (event :: events) = none := by
  simp [depositRun, terminal_step_is_rejected terminalState event]

end Deposit

end BridgeSpec.Protocol
