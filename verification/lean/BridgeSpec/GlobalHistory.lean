import BridgeSpec.Protocol

namespace BridgeSpec.GlobalHistory

inductive RecordKind where
  | deposit
  | withdrawal
deriving DecidableEq

inductive Phase where
  | pending
  | funded
  | committed
  | paid
  | minted
  | refunded
  | cancelled
deriving DecidableEq

def Phase.terminal : Phase → Bool
  | .paid | .minted | .refunded | .cancelled => true
  | _ => false

structure Economic where
  escrow : Nat
  baseSupply : Nat
  feeReserve : Nat
  unmintedLiability : Nat
  unreleasedLiability : Nat
  reservedMint : Nat
deriving DecidableEq

def Economic.add (left right : Economic) : Economic := {
  escrow := left.escrow + right.escrow
  baseSupply := left.baseSupply + right.baseSupply
  feeReserve := left.feeReserve + right.feeReserve
  unmintedLiability := left.unmintedLiability + right.unmintedLiability
  unreleasedLiability := left.unreleasedLiability + right.unreleasedLiability
  reservedMint := left.reservedMint + right.reservedMint }

def Economic.zero : Economic := {
  escrow := 0
  baseSupply := 0
  feeReserve := 0
  unmintedLiability := 0
  unreleasedLiability := 0
  reservedMint := 0 }

def Backed (economic : Economic) : Prop :=
  economic.escrow = economic.baseSupply + economic.feeReserve +
    economic.unmintedLiability + economic.unreleasedLiability

structure Record where
  id : Nat
  kind : RecordKind
  phase : Phase
  economic : Economic
  netAmount : Nat
  chargedServiceFee : Nat
  paymentDestination : Nat
  feeApplied : Bool
  mintApplied : Bool
  payoutApplied : Bool
  releaseApplied : Bool
  jobDue : Bool
  leaseGeneration : Option Nat
deriving DecidableEq

abbrev Accounting := Economic

def summarize : List Record → Accounting
  | [] => Economic.zero
  | record :: rest => record.economic.add (summarize rest)

structure GlobalState where
  records : List Record
  accounting : Accounting
deriving DecidableEq

def UniqueIds (records : List Record) : Prop :=
  (records.map Record.id).Nodup

def ReservationConsistent (records : List Record) : Prop :=
  ∀ record ∈ records, record.economic.reservedMint > 0 →
    record.kind = .deposit

def Safe (state : GlobalState) : Prop :=
  UniqueIds state.records ∧
    state.accounting = summarize state.records ∧
    Backed state.accounting ∧
    ReservationConsistent state.records

inductive Event where
  | installSignature (id : Nat)
  | mint (id : Nat)
  | refund (id amount : Nat)
  | cancel (id : Nat)
  | payout (id ledgerFee transferAmount destination : Nat)
  | releaseReservation (id : Nat)
  | callback (id generation : Nat) (nextPhase : Phase)
deriving DecidableEq

def Event.id : Event → Nat
  | .installSignature id | .mint id | .refund id _ | .cancel id | .payout id _ _ _
  | .releaseReservation id | .callback id _ _ => id

inductive Delta where
  | signature (fee : Nat)
  | mint (amount : Nat)
  | refund (amount : Nat)
  | payout (escrowDebit reserveCredit liabilityDebit : Nat)
  | releaseReservation (amount : Nat)
  | none
deriving DecidableEq

def Delta.WellFormed : Delta → Prop
  | .payout escrowDebit reserveCredit liabilityDebit =>
      escrowDebit + reserveCredit = liabilityDebit
  | _ => True

def applyDelta (economic : Economic) : Delta → Option Economic
  | .signature fee =>
      if fee ≤ economic.unmintedLiability then
        some { economic with
          feeReserve := economic.feeReserve + fee
          unmintedLiability := economic.unmintedLiability - fee }
      else none
  | .mint amount =>
      if amount ≤ economic.unmintedLiability then
        some { economic with
          baseSupply := economic.baseSupply + amount
          unmintedLiability := economic.unmintedLiability - amount }
      else none
  | .refund amount =>
      if amount ≤ economic.escrow ∧ amount ≤ economic.unmintedLiability then
        some { economic with
          escrow := economic.escrow - amount
          unmintedLiability := economic.unmintedLiability - amount }
      else none
  | .payout escrowDebit reserveCredit liabilityDebit =>
      if escrowDebit ≤ economic.escrow ∧ liabilityDebit ≤ economic.unreleasedLiability then
        some { economic with
          escrow := economic.escrow - escrowDebit
          feeReserve := economic.feeReserve + reserveCredit
          unreleasedLiability := economic.unreleasedLiability - liabilityDebit }
      else none
  | .releaseReservation amount =>
      if amount ≤ economic.reservedMint then
        some { economic with reservedMint := economic.reservedMint - amount }
      else none
  | .none => some economic

def eventDelta (record : Record) (event : Event) : Option Delta :=
  if event.id ≠ record.id ∨ record.phase.terminal then none
  else match event with
  | .installSignature _ =>
      if record.kind = .deposit ∧ record.phase = .funded ∧ !record.feeApplied then
        some (.signature record.chargedServiceFee)
      else none
  | .mint _ =>
      if record.kind = .deposit ∧ record.phase = .funded ∧ record.economic.reservedMint = 0 ∧
          !record.mintApplied then
        some (.mint record.netAmount)
      else none
  | .refund _ amount =>
      if record.kind = .deposit ∧ record.phase = .funded ∧ record.economic.reservedMint = 0 ∧
          !record.releaseApplied then
        some (.refund amount)
      else none
  | .cancel _ =>
      if record.kind = .deposit ∧ record.phase = .pending ∧
          record.economic.reservedMint = 0 then some .none
      else none
  | .payout _ ledgerFee transferAmount destination =>
      if record.kind = .withdrawal ∧ record.phase = .committed ∧ !record.payoutApplied ∧
          ledgerFee ≤ record.chargedServiceFee ∧ transferAmount = record.netAmount ∧
          destination = record.paymentDestination then
        some (.payout
          (record.netAmount + ledgerFee)
          (record.chargedServiceFee - ledgerFee)
          (record.netAmount + record.chargedServiceFee))
      else none
  | .releaseReservation _ =>
      if record.kind = .deposit ∧ !record.releaseApplied then
        some (.releaseReservation record.economic.reservedMint)
      else none
  | .callback _ generation nextPhase =>
      if record.leaseGeneration = some generation ∧
          (nextPhase.terminal = false ∨ record.economic.reservedMint = 0) then
        some .none
      else none

def transitionRecord (record : Record) (event : Event) : Option (Record × Delta) := do
  let delta ← eventDelta record event
  let economic ← applyDelta record.economic delta
  let next := match event with
    | .installSignature _ => { record with economic, feeApplied := true }
    | .mint _ => { record with economic, phase := .minted, mintApplied := true, jobDue := false }
    | .refund _ _ =>
        { record with economic, phase := .refunded, releaseApplied := true, jobDue := false }
    | .cancel _ => { record with economic, phase := .cancelled, jobDue := false }
    | .payout _ _ _ _ =>
        { record with economic, phase := .paid, payoutApplied := true, jobDue := false }
    | .releaseReservation _ => { record with economic, releaseApplied := true }
    | .callback _ _ nextPhase =>
        { record with economic, phase := nextPhase, jobDue := false, leaseGeneration := none }
  some (next, delta)

def applyRecord (record : Record) (event : Event) : Option Record := do
  let (next, _) ← transitionRecord record event
  some next

def findRecord? : List Record → Nat → Option Record
  | [], _ => none
  | record :: rest, id => if record.id = id then some record else findRecord? rest id

theorem find_record_has_requested_id {records : List Record} {id : Nat} {record : Record}
    (found : findRecord? records id = some record) : record.id = id := by
  induction records with
  | nil => simp [findRecord?] at found
  | cons head rest ih =>
      simp only [findRecord?] at found
      split at found
      next same => simp only [Option.some.injEq] at found; subst record; exact same
      next => exact ih found

def updateRecord : List Record → Event → Option (List Record × Delta)
  | [], _ => none
  | record :: rest, event =>
      if record.id = event.id then do
        let (next, delta) ← transitionRecord record event
        some (next :: rest, delta)
      else do
        let (updated, delta) ← updateRecord rest event
        some (record :: updated, delta)

def step (state : GlobalState) (event : Event) : Option GlobalState := do
  let (records, delta) ← updateRecord state.records event
  let accounting ← applyDelta state.accounting delta
  some { records, accounting }

def Runs : GlobalState → List Event → GlobalState → Prop
  | state, [], final => final = state
  | state, event :: rest, final =>
      ∃ next, step state event = some next ∧ Runs next rest final

theorem apply_delta_add_right {left next right : Economic} {delta : Delta}
    (accepted : applyDelta left delta = some next) :
    applyDelta (left.add right) delta = some (next.add right) := by
  cases delta with
  | signature fee =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | mint amount =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | refund amount =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | payout debit credit liability =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | releaseReservation amount =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | none => simp [applyDelta] at accepted ⊢; subst next; rfl

theorem apply_delta_add_left {left right next : Economic} {delta : Delta}
    (accepted : applyDelta right delta = some next) :
    applyDelta (left.add right) delta = some (left.add next) := by
  cases delta with
  | signature fee =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | mint amount =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | refund amount =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | payout debit credit liability =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | releaseReservation amount =>
      simp only [applyDelta] at accepted ⊢
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp [Economic.add]
        omega
      · simp at accepted
  | none => simp [applyDelta] at accepted ⊢; subst next; rfl

theorem transition_record_preserves_id {record next : Record} {event : Event} {delta : Delta}
    (accepted : transitionRecord record event = some (next, delta)) : next.id = record.id := by
  unfold transitionRecord at accepted
  obtain ⟨selected, selectedEq, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨economic, economicEq, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq, Prod.mk.injEq] at result
  rcases result with ⟨rfl, rfl⟩
  cases event <;> rfl

theorem transition_record_economic {record next : Record} {event : Event} {delta : Delta}
    (accepted : transitionRecord record event = some (next, delta)) :
    applyDelta record.economic delta = some next.economic := by
  unfold transitionRecord at accepted
  obtain ⟨selected, _, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨economic, economicEq, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq, Prod.mk.injEq] at result
  rcases result with ⟨rfl, rfl⟩
  cases event <;> exact economicEq

theorem transition_record_preserves_kind {record next : Record} {event : Event} {delta : Delta}
    (accepted : transitionRecord record event = some (next, delta)) : next.kind = record.kind := by
  unfold transitionRecord at accepted
  obtain ⟨selected, _, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨economic, _, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq, Prod.mk.injEq] at result
  rcases result with ⟨rfl, rfl⟩
  cases event <;> rfl

theorem update_record_preserves_ids
    {records updated : List Record} {event : Event} {delta : Delta}
    (accepted : updateRecord records event = some (updated, delta)) :
    updated.map Record.id = records.map Record.id := by
  induction records generalizing updated delta with
  | nil => simp [updateRecord] at accepted
  | cons record rest ih =>
      simp only [updateRecord] at accepted
      split at accepted
      next =>
        obtain ⟨pair, transition, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨next, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        simp [transition_record_preserves_id transition]
      next =>
        obtain ⟨pair, tailAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨tail, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        simp [ih tailAccepted]

theorem update_record_updates_summary
    {records updated : List Record} {event : Event} {delta : Delta}
    (accepted : updateRecord records event = some (updated, delta)) :
    applyDelta (summarize records) delta = some (summarize updated) := by
  induction records generalizing updated delta with
  | nil => simp [updateRecord] at accepted
  | cons record rest ih =>
      simp only [updateRecord] at accepted
      split at accepted
      next =>
        obtain ⟨pair, transition, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨next, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        exact apply_delta_add_right (transition_record_economic transition)
      next =>
        obtain ⟨pair, tailAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨tail, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        exact apply_delta_add_left (ih tailAccepted)

theorem update_record_frames_other_id
    {records updated : List Record} {event : Event} {delta : Delta} {other : Nat}
    (different : other ≠ event.id)
    (accepted : updateRecord records event = some (updated, delta)) :
    findRecord? updated other = findRecord? records other := by
  induction records generalizing updated delta with
  | nil => simp [updateRecord] at accepted
  | cons record rest ih =>
      simp only [updateRecord] at accepted
      split at accepted
      next same =>
        obtain ⟨pair, transition, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨next, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        have nextId := transition_record_preserves_id transition
        have notOther : record.id ≠ other := by omega
        simp [findRecord?, nextId, notOther]
      next =>
        obtain ⟨pair, tailAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨tail, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        simp only [findRecord?]
        split
        next => rfl
        next => exact ih tailAccepted

theorem update_record_finds_updated_target
    {records updated : List Record} {event : Event} {delta : Delta}
    (accepted : updateRecord records event = some (updated, delta)) :
    ∃ before after,
      findRecord? records event.id = some before ∧
      transitionRecord before event = some (after, delta) ∧
      findRecord? updated event.id = some after := by
  induction records generalizing updated delta with
  | nil => simp [updateRecord] at accepted
  | cons record rest ih =>
      simp only [updateRecord] at accepted
      split at accepted
      next same =>
        obtain ⟨pair, transition, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨after, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        exact ⟨record, after, by simp [findRecord?, same], transition,
          by simp [findRecord?, transition_record_preserves_id transition, same]⟩
      next different =>
        obtain ⟨pair, tailAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨tail, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        obtain ⟨before, after, foundBefore, transition, foundAfter⟩ := ih tailAccepted
        exact ⟨before, after, by simp [findRecord?, different, foundBefore], transition,
          by simp [findRecord?, different, foundAfter]⟩

theorem transition_terminal_event_reaches_phase
    {record next : Record} {event : Event} {delta : Delta}
    (accepted : transitionRecord record event = some (next, delta)) :
    (event = .mint record.id → next.phase = .minted) ∧
    (∀ amount, event = .refund record.id amount → next.phase = .refunded) ∧
    (event = .cancel record.id → next.phase = .cancelled) ∧
    (∀ ledgerFee transferAmount destination,
      event = .payout record.id ledgerFee transferAmount destination → next.phase = .paid) := by
  unfold transitionRecord at accepted
  obtain ⟨selected, selectedEq, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨economic, economicEq, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq, Prod.mk.injEq] at result
  rcases result with ⟨rfl, rfl⟩
  cases event <;> simp

theorem step_terminal_event_reaches_phase
    {state next : GlobalState} {event : Event}
    (accepted : step state event = some next) :
    (event = .mint event.id → ∃ record, findRecord? next.records event.id = some record ∧
      record.phase = .minted) ∧
    (∀ amount, event = .refund event.id amount →
      ∃ record, findRecord? next.records event.id = some record ∧ record.phase = .refunded) ∧
    (event = .cancel event.id →
      ∃ record, findRecord? next.records event.id = some record ∧ record.phase = .cancelled) ∧
    (∀ ledgerFee transferAmount destination,
      event = .payout event.id ledgerFee transferAmount destination →
        ∃ record, findRecord? next.records event.id = some record ∧ record.phase = .paid) := by
  unfold step at accepted
  obtain ⟨pair, updateAccepted, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  rcases pair with ⟨updated, delta⟩
  obtain ⟨accounting, accountingAccepted, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq] at result
  subst next
  obtain ⟨before, after, foundBefore, transition, foundAfter⟩ :=
    update_record_finds_updated_target updateAccepted
  have phases := transition_terminal_event_reaches_phase transition
  have idEq := find_record_has_requested_id foundBefore
  constructor
  · intro eventEq
    exact ⟨after, foundAfter, phases.1 (by simpa [idEq] using eventEq)⟩
  constructor
  · intro amount eventEq
    exact ⟨after, foundAfter, phases.2.1 amount (by simpa [idEq] using eventEq)⟩
  constructor
  · intro eventEq
    exact ⟨after, foundAfter, phases.2.2.1 (by simpa [idEq] using eventEq)⟩
  · intro ledgerFee transferAmount destination eventEq
    exact ⟨after, foundAfter, phases.2.2.2 ledgerFee transferAmount destination
      (by simpa [idEq] using eventEq)⟩

theorem apply_delta_preserves_backing {before after : Economic} {delta : Delta}
    (backed : Backed before) (accepted : applyDelta before delta = some after)
    (wellFormed : delta.WellFormed) : Backed after := by
  unfold Backed at backed ⊢
  cases delta with
  | signature fee =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp
        omega
      · simp at accepted
  | mint amount =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp
        omega
      · simp at accepted
  | refund amount =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp
        omega
      · simp at accepted
  | payout debit credit liability =>
      simp only [Delta.WellFormed] at wellFormed
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp
        omega
      · simp at accepted
  | releaseReservation amount =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨_, rfl⟩
        exact backed
      · simp at accepted
  | none => simp [applyDelta] at accepted; simpa [accepted] using backed

theorem event_delta_well_formed {record : Record} {event : Event} {delta : Delta}
    (selected : eventDelta record event = some delta) : delta.WellFormed := by
  cases event <;> simp [eventDelta] at selected
  all_goals rcases selected with ⟨_, _, rfl⟩
  all_goals simp [Delta.WellFormed] <;> omega

theorem apply_delta_reserved_positive_before {before after : Economic} {delta : Delta}
    (accepted : applyDelta before delta = some after)
    (positive : after.reservedMint > 0) : before.reservedMint > 0 := by
  cases delta with
  | signature fee | mint fee =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨_, rfl⟩
        exact positive
      · simp at accepted
  | refund amount =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨_, rfl⟩
        exact positive
      · simp at accepted
  | payout debit credit liability =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨_, rfl⟩
        exact positive
      · simp at accepted
  | releaseReservation amount =>
      simp only [applyDelta] at accepted
      split at accepted
      · rcases accepted with ⟨bound, rfl⟩
        simp at positive
        omega
      · simp at accepted
  | none => simp [applyDelta] at accepted; subst after; exact positive

theorem transition_record_preserves_reservation_consistency
    {record next : Record} {event : Event} {delta : Delta}
    (consistent : record.economic.reservedMint > 0 → record.kind = .deposit)
    (accepted : transitionRecord record event = some (next, delta)) :
    next.economic.reservedMint > 0 → next.kind = .deposit := by
  intro positive
  rw [transition_record_preserves_kind accepted]
  exact consistent (apply_delta_reserved_positive_before
    (transition_record_economic accepted) positive)

theorem transition_record_delta_well_formed
    {record next : Record} {event : Event} {delta : Delta}
    (accepted : transitionRecord record event = some (next, delta)) : delta.WellFormed := by
  unfold transitionRecord at accepted
  obtain ⟨selected, selectedEq, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨economic, economicEq, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq, Prod.mk.injEq] at result
  rcases result with ⟨_, rfl⟩
  exact event_delta_well_formed selectedEq

theorem apply_record_has_transition {record next : Record} {event : Event}
    (accepted : applyRecord record event = some next) :
    ∃ delta, transitionRecord record event = some (next, delta) := by
  unfold applyRecord at accepted
  obtain ⟨pair, transition, result⟩ := Option.bind_eq_some_iff.mp accepted
  rcases pair with ⟨candidate, delta⟩
  simp only [Option.some.injEq] at result
  subst candidate
  exact ⟨delta, transition⟩

theorem transition_record_selected {record next : Record} {event : Event} {delta : Delta}
    (accepted : transitionRecord record event = some (next, delta)) :
    eventDelta record event = some delta := by
  unfold transitionRecord at accepted
  obtain ⟨selected, selectedEq, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  obtain ⟨economic, economicEq, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq, Prod.mk.injEq] at result
  exact result.2 ▸ selectedEq

theorem update_record_delta_well_formed
    {records updated : List Record} {event : Event} {delta : Delta}
    (accepted : updateRecord records event = some (updated, delta)) : delta.WellFormed := by
  induction records generalizing updated delta with
  | nil => simp [updateRecord] at accepted
  | cons record rest ih =>
      simp only [updateRecord] at accepted
      split at accepted
      next =>
        obtain ⟨pair, transition, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨next, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨_, rfl⟩
        exact transition_record_delta_well_formed transition
      next =>
        obtain ⟨pair, tailAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨tail, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        exact result.2 ▸ ih tailAccepted

theorem update_record_preserves_reservation_consistency
    {records updated : List Record} {event : Event} {delta : Delta}
    (consistent : ReservationConsistent records)
    (accepted : updateRecord records event = some (updated, delta)) :
    ReservationConsistent updated := by
  induction records generalizing updated delta with
  | nil => simp [updateRecord] at accepted
  | cons record rest ih =>
      simp only [updateRecord] at accepted
      split at accepted
      next =>
        obtain ⟨pair, transition, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨next, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        intro candidate member positive
        simp only [List.mem_cons] at member
        rcases member with rfl | member
        · exact transition_record_preserves_reservation_consistency
            (consistent record (by simp)) transition positive
        · exact consistent candidate (by simp [member]) positive
      next =>
        obtain ⟨pair, tailAccepted, result⟩ := Option.bind_eq_some_iff.mp accepted
        rcases pair with ⟨tail, selected⟩
        simp only [Option.some.injEq, Prod.mk.injEq] at result
        rcases result with ⟨rfl, rfl⟩
        intro candidate member positive
        simp only [List.mem_cons] at member
        rcases member with rfl | member
        · exact consistent candidate (by simp) positive
        · exact ih (fun item inRest => consistent item (by simp [inRest])) tailAccepted
            candidate member positive

theorem step_preserves_ids {state next : GlobalState} {event : Event}
    (accepted : step state event = some next) :
    next.records.map Record.id = state.records.map Record.id := by
  unfold step at accepted
  obtain ⟨pair, updateAccepted, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  rcases pair with ⟨updated, delta⟩
  obtain ⟨accounting, accountingAccepted, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq] at result
  subst next
  exact update_record_preserves_ids updateAccepted

theorem step_preserves_safe {state next : GlobalState} {event : Event}
    (safe : Safe state) (accepted : step state event = some next) : Safe next := by
  rcases safe with ⟨unique, total, backed, reservation⟩
  rw [total] at backed
  unfold step at accepted
  obtain ⟨pair, updateAccepted, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  rcases pair with ⟨updated, delta⟩
  obtain ⟨accounting, accountingAccepted, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq] at result
  subst next
  have summaryAccepted : applyDelta (summarize state.records) delta = some (summarize updated) :=
    update_record_updates_summary updateAccepted
  rw [total] at accountingAccepted
  rw [summaryAccepted] at accountingAccepted
  simp only [Option.some.injEq] at accountingAccepted
  subst accounting
  refine ⟨?_, rfl, ?_, update_record_preserves_reservation_consistency reservation updateAccepted⟩
  · unfold UniqueIds at unique ⊢
    rw [update_record_preserves_ids updateAccepted]
    exact unique
  · exact apply_delta_preserves_backing backed summaryAccepted
      (update_record_delta_well_formed updateAccepted)

theorem runs_preserve_safe {state final : GlobalState} {events : List Event}
    (safe : Safe state) (runs : Runs state events final) : Safe final := by
  induction events generalizing state with
  | nil => simp only [Runs] at runs; subst final; exact safe
  | cons event rest ih =>
      simp only [Runs] at runs
      obtain ⟨next, accepted, tail⟩ := runs
      exact ih (step_preserves_safe safe accepted) tail

theorem step_frames_other_record
    {state next : GlobalState} {event : Event} {other : Nat}
    (different : other ≠ event.id) (accepted : step state event = some next) :
    findRecord? next.records other = findRecord? state.records other := by
  unfold step at accepted
  obtain ⟨pair, updateAccepted, remainder⟩ := Option.bind_eq_some_iff.mp accepted
  rcases pair with ⟨updated, delta⟩
  obtain ⟨accounting, accountingAccepted, result⟩ := Option.bind_eq_some_iff.mp remainder
  simp only [Option.some.injEq] at result
  subst next
  exact update_record_frames_other_id different updateAccepted

theorem terminal_record_is_absorbing {record : Record} {event : Event}
    (terminal : record.phase.terminal = true) : applyRecord record event = none := by
  simp [applyRecord, transitionRecord, eventDelta, terminal]

theorem stale_callback_is_rejected {record : Record} {id generation current : Nat}
    (recordId : record.id = id) (active : record.leaseGeneration = some current)
    (stale : generation ≠ current) (nonterminal : record.phase.terminal = false) :
    applyRecord record (.callback id generation .paid) = none := by
  simp [applyRecord, transitionRecord, eventDelta, recordId, active, Ne.symm stale, nonterminal]

theorem duplicate_fee_is_rejected {record : Record}
    (counted : record.feeApplied = true) (nonterminal : record.phase.terminal = false) :
    applyRecord record (.installSignature record.id) = none := by
  simp [applyRecord, transitionRecord, eventDelta, counted, nonterminal]

theorem duplicate_mint_is_rejected {record : Record}
    (minted : record.mintApplied = true) (nonterminal : record.phase.terminal = false) :
    applyRecord record (.mint record.id) = none := by
  simp [applyRecord, transitionRecord, eventDelta, minted, nonterminal]

theorem duplicate_payout_is_rejected {record : Record}
    (paid : record.payoutApplied = true) (nonterminal : record.phase.terminal = false)
    (ledgerFee transferAmount destination : Nat) :
    applyRecord record (.payout record.id ledgerFee transferAmount destination) = none := by
  simp [applyRecord, transitionRecord, eventDelta, paid, nonterminal]

theorem duplicate_release_is_rejected {record : Record}
    (released : record.releaseApplied = true) (nonterminal : record.phase.terminal = false) :
    applyRecord record (.releaseReservation record.id) = none := by
  simp [applyRecord, transitionRecord, eventDelta, released, nonterminal]

theorem payout_requires_exact_identity {record next : Record}
    {ledgerFee transferAmount destination : Nat}
    (accepted : applyRecord record
      (.payout record.id ledgerFee transferAmount destination) = some next) :
    transferAmount = record.netAmount ∧ destination = record.paymentDestination ∧
      ledgerFee ≤ record.chargedServiceFee := by
  obtain ⟨delta, transition⟩ := apply_record_has_transition accepted
  have selected := transition_record_selected transition
  simp [eventDelta] at selected
  rcases selected with ⟨_, guard, _⟩
  exact ⟨guard.2.2.2.2.1, guard.2.2.2.2.2, guard.2.2.2.1⟩

theorem payout_applies_exact_delta {record next : Record}
    {ledgerFee transferAmount destination : Nat}
    (accepted : applyRecord record
      (.payout record.id ledgerFee transferAmount destination) = some next) :
    next.economic.escrow = record.economic.escrow - (record.netAmount + ledgerFee) ∧
      next.economic.feeReserve = record.economic.feeReserve +
        (record.chargedServiceFee - ledgerFee) ∧
      next.economic.unreleasedLiability = record.economic.unreleasedLiability -
        (record.netAmount + record.chargedServiceFee) := by
  obtain ⟨delta, transition⟩ := apply_record_has_transition accepted
  have selected := transition_record_selected transition
  simp [eventDelta] at selected
  rcases selected with ⟨_, _, rfl⟩
  have economic := transition_record_economic transition
  simp only [applyDelta] at economic
  split at economic
  · have equality := Option.some.inj economic
    exact ⟨(congrArg Economic.escrow equality).symm,
      (congrArg Economic.feeReserve equality).symm,
      (congrArg Economic.unreleasedLiability equality).symm⟩
  · simp at economic

theorem signature_applies_exact_fee {record next : Record}
    (accepted : applyRecord record (.installSignature record.id) = some next) :
    next.economic.feeReserve = record.economic.feeReserve + record.chargedServiceFee ∧
      next.economic.unmintedLiability =
        record.economic.unmintedLiability - record.chargedServiceFee := by
  obtain ⟨delta, transition⟩ := apply_record_has_transition accepted
  have selected := transition_record_selected transition
  simp [eventDelta] at selected
  rcases selected with ⟨_, _, rfl⟩
  have economic := transition_record_economic transition
  simp only [applyDelta] at economic
  split at economic
  · have equality := Option.some.inj economic
    exact ⟨(congrArg Economic.feeReserve equality).symm,
      (congrArg Economic.unmintedLiability equality).symm⟩
  · simp at economic

theorem mint_applies_exact_amount {record next : Record}
    (accepted : applyRecord record (.mint record.id) = some next) :
    next.economic.baseSupply = record.economic.baseSupply + record.netAmount ∧
      next.economic.unmintedLiability = record.economic.unmintedLiability - record.netAmount := by
  obtain ⟨delta, transition⟩ := apply_record_has_transition accepted
  have selected := transition_record_selected transition
  simp [eventDelta] at selected
  rcases selected with ⟨_, _, rfl⟩
  have economic := transition_record_economic transition
  simp only [applyDelta] at economic
  split at economic
  · have equality := Option.some.inj economic
    exact ⟨(congrArg Economic.baseSupply equality).symm,
      (congrArg Economic.unmintedLiability equality).symm⟩
  · simp at economic

theorem refund_applies_exact_amount {record next : Record} {amount : Nat}
    (accepted : applyRecord record (.refund record.id amount) = some next) :
    next.economic.escrow = record.economic.escrow - amount ∧
      next.economic.unmintedLiability = record.economic.unmintedLiability - amount := by
  obtain ⟨delta, transition⟩ := apply_record_has_transition accepted
  have selected := transition_record_selected transition
  simp [eventDelta] at selected
  rcases selected with ⟨_, _, rfl⟩
  have economic := transition_record_economic transition
  simp only [applyDelta] at economic
  split at economic
  · have equality := Option.some.inj economic
    exact ⟨(congrArg Economic.escrow equality).symm,
      (congrArg Economic.unmintedLiability equality).symm⟩
  · simp at economic

theorem release_clears_exact_reservation {record next : Record}
    (accepted : applyRecord record (.releaseReservation record.id) = some next) :
    next.economic.reservedMint = 0 := by
  obtain ⟨delta, transition⟩ := apply_record_has_transition accepted
  have selected := transition_record_selected transition
  simp [eventDelta] at selected
  rcases selected with ⟨_, _, rfl⟩
  have economic := transition_record_economic transition
  simp only [applyDelta] at economic
  split at economic
  · have equality := Option.some.inj economic
    have reserved := (congrArg Economic.reservedMint equality).symm
    simpa using reserved
  · simp at economic

end BridgeSpec.GlobalHistory
