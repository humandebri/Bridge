import BridgeSpec.Model

namespace BridgeSpec.LedgerBlockProvenance

structure State where
  funding : Option Nat
  refund : Option Nat
  release : Option Nat
  deriving DecidableEq

inductive Event where
  | preserve
  | fundingSucceeded (block : Nat)
  | refundSucceeded (block : Nat)
  | releaseSucceeded (block : Nat)
  deriving DecidableEq

abbrev installOnce := BridgeSpec.ledgerBlockProvenance

def step (state : State) : Event → Option State
  | .preserve => some state
  | .fundingSucceeded block => do
      let funding ← installOnce state.funding block
      pure { state with funding := funding }
  | .refundSucceeded block => do
      if state.funding.isNone then none else
        let refund ← installOnce state.refund block
        pure { state with refund := refund }
  | .releaseSucceeded block => do
      let release ← installOnce state.release block
      pure { state with release := release }

inductive Runs : State → List Event → State → Prop where
  | nil (state) : Runs state [] state
  | cons {state next final event events} :
      step state event = some next → Runs next events final →
        Runs state (event :: events) final

inductive IndexRuns : Option Nat → List Nat → Option Nat → Prop where
  | nil (index) : IndexRuns index [] index
  | cons {index next final block blocks} :
      installOnce index block = some next → IndexRuns next blocks final →
        IndexRuns index (block :: blocks) final

theorem install_once_preserves_existing
    {current next : Option Nat} {block prior : Nat}
    (present : current = some prior)
    (accepted : installOnce current block = some next) : next = some prior := by
  subst current
  simp [BridgeSpec.ledgerBlockProvenance] at accepted
  rcases accepted with ⟨rfl, rfl⟩
  rfl

theorem first_success_installs_exact_block (block : Nat) :
    installOnce none block = some (some block) := by
  rfl

theorem conflicting_replay_is_rejected (prior block : Nat) (different : prior ≠ block) :
    installOnce (some prior) block = none := by
  simp [BridgeSpec.ledgerBlockProvenance, different]

theorem index_runs_preserve_existing
    {initial final : Option Nat} {blocks : List Nat} {block : Nat}
    (present : initial = some block)
    (runs : IndexRuns initial blocks final) : final = some block := by
  induction runs with
  | nil => exact present
  | cons accepted rest ih =>
      exact ih (install_once_preserves_existing present accepted)

theorem preserve_keeps_every_index (state : State) :
    step state .preserve = some state := by
  rfl

theorem first_funding_success_installs_exact_block (state : State) (block : Nat)
    (empty : state.funding = none) :
    step state (.fundingSucceeded block) =
      some { state with funding := some block } := by
  simp [step, empty, BridgeSpec.ledgerBlockProvenance]

theorem refund_success_requires_funding
    {state next : State} {block : Nat}
    (accepted : step state (.refundSucceeded block) = some next) :
    ∃ funding, state.funding = some funding := by
  cases present : state.funding with
  | none => simp [step, present] at accepted
  | some funding => exact ⟨funding, rfl⟩

theorem first_refund_success_installs_exact_block
    (state : State) (funding block : Nat)
    (funded : state.funding = some funding)
    (empty : state.refund = none) :
    step state (.refundSucceeded block) =
      some { state with refund := some block } := by
  simp [step, funded, empty, BridgeSpec.ledgerBlockProvenance]

theorem first_release_success_installs_exact_block (state : State) (block : Nat)
    (empty : state.release = none) :
    step state (.releaseSucceeded block) =
      some { state with release := some block } := by
  simp [step, empty, BridgeSpec.ledgerBlockProvenance]

theorem conflicting_funding_step_is_rejected
    (state : State) (prior block : Nat) (different : prior ≠ block)
    (present : state.funding = some prior) :
    step state (.fundingSucceeded block) = none := by
  simp [step, present, BridgeSpec.ledgerBlockProvenance, different]

theorem conflicting_refund_step_is_rejected
    (state : State) (funding prior block : Nat) (different : prior ≠ block)
    (funded : state.funding = some funding)
    (present : state.refund = some prior) :
    step state (.refundSucceeded block) = none := by
  simp [step, funded, present, BridgeSpec.ledgerBlockProvenance, different]

theorem conflicting_release_step_is_rejected
    (state : State) (prior block : Nat) (different : prior ≠ block)
    (present : state.release = some prior) :
    step state (.releaseSucceeded block) = none := by
  simp [step, present, BridgeSpec.ledgerBlockProvenance, different]

theorem step_preserves_installed_funding
    {state next : State} {event : Event} {block : Nat}
    (present : state.funding = some block)
    (accepted : step state event = some next) :
    next.funding = some block := by
  cases event with
  | preserve =>
      simp [step] at accepted
      subst next
      exact present
  | fundingSucceeded candidate =>
      cases installed : installOnce state.funding candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          exact install_once_preserves_existing present installed
  | refundSucceeded candidate =>
      cases funded : state.funding with
      | none => simp [step, funded] at accepted
      | some funding =>
          cases installed : installOnce state.refund candidate with
          | none => simp [step, funded, installed] at accepted
          | some updated =>
              simp [step, funded, installed] at accepted
              subst next
              simpa [funded] using present
  | releaseSucceeded candidate =>
      cases installed : installOnce state.release candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          exact present

theorem step_preserves_installed_refund
    {state next : State} {event : Event} {block : Nat}
    (present : state.refund = some block)
    (accepted : step state event = some next) :
    next.refund = some block := by
  cases event with
  | preserve =>
      simp [step] at accepted
      subst next
      exact present
  | fundingSucceeded candidate =>
      cases installed : installOnce state.funding candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          exact present
  | refundSucceeded candidate =>
      cases funded : state.funding with
      | none => simp [step, funded] at accepted
      | some funding =>
          cases installed : installOnce state.refund candidate with
          | none => simp [step, funded, installed] at accepted
          | some updated =>
              simp [step, funded, installed] at accepted
              subst next
              exact install_once_preserves_existing present installed
  | releaseSucceeded candidate =>
      cases installed : installOnce state.release candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          exact present

theorem step_preserves_installed_release
    {state next : State} {event : Event} {block : Nat}
    (present : state.release = some block)
    (accepted : step state event = some next) :
    next.release = some block := by
  cases event with
  | preserve =>
      simp [step] at accepted
      subst next
      exact present
  | fundingSucceeded candidate =>
      cases installed : installOnce state.funding candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          exact present
  | refundSucceeded candidate =>
      cases funded : state.funding with
      | none => simp [step, funded] at accepted
      | some funding =>
          cases installed : installOnce state.refund candidate with
          | none => simp [step, funded, installed] at accepted
          | some updated =>
              simp [step, funded, installed] at accepted
              subst next
              exact present
  | releaseSucceeded candidate =>
      cases installed : installOnce state.release candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          exact install_once_preserves_existing present installed

theorem runs_preserve_installed_funding
    {initial final : State} {events : List Event} {block : Nat}
    (present : initial.funding = some block)
    (runs : Runs initial events final) :
    final.funding = some block := by
  induction runs with
  | nil => exact present
  | cons accepted rest ih =>
      exact ih (step_preserves_installed_funding present accepted)

theorem runs_preserve_installed_refund
    {initial final : State} {events : List Event} {block : Nat}
    (present : initial.refund = some block)
    (runs : Runs initial events final) :
    final.refund = some block := by
  induction runs with
  | nil => exact present
  | cons accepted rest ih =>
      exact ih (step_preserves_installed_refund present accepted)

theorem runs_preserve_installed_release
    {initial final : State} {events : List Event} {block : Nat}
    (present : initial.release = some block)
    (runs : Runs initial events final) :
    final.release = some block := by
  induction runs with
  | nil => exact present
  | cons accepted rest ih =>
      exact ih (step_preserves_installed_release present accepted)

theorem accepted_step_changes_have_success_origin
    {state next : State} {event : Event}
    (accepted : step state event = some next) :
    (next.funding ≠ state.funding → ∃ block, event = .fundingSucceeded block) ∧
    (next.refund ≠ state.refund → ∃ block, event = .refundSucceeded block) ∧
    (next.release ≠ state.release → ∃ block, event = .releaseSucceeded block) := by
  cases event with
  | preserve =>
      simp [step] at accepted
      subst next
      simp
  | fundingSucceeded candidate =>
      cases installed : installOnce state.funding candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          simp
  | refundSucceeded candidate =>
      cases funded : state.funding with
      | none => simp [step, funded] at accepted
      | some funding =>
          cases installed : installOnce state.refund candidate with
          | none => simp [step, funded, installed] at accepted
          | some updated =>
              simp [step, funded, installed] at accepted
              subst next
              simp
  | releaseSucceeded candidate =>
      cases installed : installOnce state.release candidate with
      | none => simp [step, installed] at accepted
      | some updated =>
          simp [step, installed] at accepted
          subst next
          simp

def ClaimContract : Prop :=
  (∀ state : State, step state .preserve = some state) ∧
  (∀ (state : State) (block : Nat), state.funding = none →
      step state (.fundingSucceeded block) = some { state with funding := some block }) ∧
  (∀ {state next : State} {block : Nat},
      step state (.refundSucceeded block) = some next →
        ∃ funding, state.funding = some funding) ∧
  (∀ (state : State) (funding block : Nat),
      state.funding = some funding → state.refund = none →
      step state (.refundSucceeded block) = some { state with refund := some block }) ∧
  (∀ (state : State) (block : Nat), state.release = none →
      step state (.releaseSucceeded block) = some { state with release := some block }) ∧
  (∀ (state : State) (prior block : Nat), prior ≠ block →
      state.funding = some prior → step state (.fundingSucceeded block) = none) ∧
  (∀ (state : State) (funding prior block : Nat), prior ≠ block →
      state.funding = some funding → state.refund = some prior →
      step state (.refundSucceeded block) = none) ∧
  (∀ (state : State) (prior block : Nat), prior ≠ block →
      state.release = some prior → step state (.releaseSucceeded block) = none) ∧
  (∀ {state next : State} {event : Event}, step state event = some next →
      (next.funding ≠ state.funding → ∃ block, event = .fundingSucceeded block) ∧
      (next.refund ≠ state.refund → ∃ block, event = .refundSucceeded block) ∧
      (next.release ≠ state.release → ∃ block, event = .releaseSucceeded block)) ∧
  (∀ {initial final : State} {events : List Event} {block : Nat},
      initial.funding = some block → Runs initial events final →
        final.funding = some block) ∧
  (∀ {initial final : State} {events : List Event} {block : Nat},
      initial.refund = some block → Runs initial events final →
        final.refund = some block) ∧
  (∀ {initial final : State} {events : List Event} {block : Nat},
      initial.release = some block → Runs initial events final →
        final.release = some block)

theorem claim_contract_witness : ClaimContract :=
  ⟨preserve_keeps_every_index, first_funding_success_installs_exact_block,
    refund_success_requires_funding, first_refund_success_installs_exact_block,
    first_release_success_installs_exact_block, conflicting_funding_step_is_rejected,
    conflicting_refund_step_is_rejected, conflicting_release_step_is_rejected,
    accepted_step_changes_have_success_origin, runs_preserve_installed_funding,
    runs_preserve_installed_refund,
    runs_preserve_installed_release⟩

theorem ledger_block_provenance_claim : ClaimContract := claim_contract_witness

end BridgeSpec.LedgerBlockProvenance
