import BridgeSpec.Protocol

namespace BridgeSpec.ControlPlane

structure InstallDomain where
  chainId : Nat
  runtimeHash : Nat
  instanceId : Nat
deriving DecidableEq

structure Attestation where
  domain : InstallDomain
  finalizedBlock : Nat
deriving DecidableEq

structure State where
  configuredChainId : Nat
  domain : InstallDomain
  cachedAttestation : Option Attestation
  lastReusedDomain : Option InstallDomain
  lastGovernanceChain : Option Nat
  pendingNonce : Option Nat
  paused : Bool
  activationCount : Nat
  lastActivationValidated : Bool
deriving DecidableEq

def Initial (domain : InstallDomain) : State := {
  configuredChainId := domain.chainId
  domain
  cachedAttestation := none
  lastReusedDomain := none
  lastGovernanceChain := none
  pendingNonce := none
  paused := false
  activationCount := 0
  lastActivationValidated := false
}

def Safe (state : State) : Prop :=
  (∀ attestation, state.cachedAttestation = some attestation →
      attestation.domain = state.domain) ∧
    (∀ domain, state.lastReusedDomain = some domain → domain = state.domain) ∧
    (∀ chainId, state.lastGovernanceChain = some chainId →
      chainId = state.configuredChainId) ∧
    (state.paused = false →
      state.activationCount = 0 ∨ state.lastActivationValidated = true)

instance (state : State) : Decidable (Safe state) := by
  unfold Safe
  infer_instance

inductive Event where
  | coldAttest (attestation : Attestation)
  | reuseAttestation (requestedDomain : InstallDomain)
  | prepareGovernance (requestedChainId nonce : Nat)
  | confirmActivation (candidate : InstallDomain)
      (authorized precondition postcondition emergencyClear : Bool)
  | pause
  | upgrade (nextDomain : InstallDomain)
  | reinstall (nextDomain : InstallDomain)
deriving DecidableEq

def step (state : State) : Event → Option State
  | .coldAttest attestation =>
      if attestation.domain = state.domain then
        some { state with cachedAttestation := some attestation }
      else none
  | .reuseAttestation requestedDomain =>
      match state.cachedAttestation with
      | some attestation =>
          if attestation.domain = state.domain ∧ requestedDomain = state.domain then
            some { state with lastReusedDomain := some requestedDomain }
          else none
      | none => none
  | .prepareGovernance requestedChainId nonce =>
      if state.paused = true ∧ requestedChainId = state.configuredChainId then
        some { state with
          lastGovernanceChain := some requestedChainId
          pendingNonce := some nonce }
      else none
  | .confirmActivation candidate authorized precondition postcondition emergencyClear =>
      if state.paused = true ∧ authorized = true ∧ precondition = true ∧
          postcondition = true ∧ emergencyClear = true ∧
          candidate.chainId = state.configuredChainId then
        some { state with
          domain := candidate
          cachedAttestation := none
          lastReusedDomain := none
          pendingNonce := none
          paused := false
          activationCount := state.activationCount + 1
          lastActivationValidated := true }
      else none
  | .pause => some { state with paused := true }
  | .upgrade nextDomain =>
      if nextDomain.chainId = state.configuredChainId then
        some { state with
          domain := nextDomain
          cachedAttestation := none
          lastReusedDomain := none
          pendingNonce := none
          paused := true
          lastActivationValidated := false }
      else none
  | .reinstall nextDomain =>
      if nextDomain.chainId = state.configuredChainId then
        some { state with
          domain := nextDomain
          cachedAttestation := none
          lastReusedDomain := none
          lastGovernanceChain := none
          pendingNonce := none
          paused := true
          activationCount := 0
          lastActivationValidated := false }
      else none

def Runs : State → List Event → State → Prop
  | state, [], final => final = state
  | state, event :: events, final =>
      ∃ next, step state event = some next ∧ Runs next events final

theorem initial_safe (domain : InstallDomain) : Safe (Initial domain) := by
  simp [Safe, Initial]

theorem step_preserves_safe
    {state next : State} {event : Event}
    (safe : Safe state) (accepted : step state event = some next) :
    Safe next := by
  rcases safe with ⟨cache, reuse, governance, activation⟩
  cases event with
  | coldAttest attestation =>
      simp only [step] at accepted
      split at accepted
      next domainMatches =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨by simp [domainMatches], reuse, governance, activation⟩
      next => simp at accepted
  | reuseAttestation requestedDomain =>
      simp only [step] at accepted
      cases cached : state.cachedAttestation with
      | none => simp [cached] at accepted
      | some attestation =>
          simp only [cached] at accepted
          split at accepted
          next domainMatches =>
            simp only [Option.some.injEq] at accepted
            subst next
            have cachedDomain := cache attestation cached
            exact ⟨by
                intro current currentEq
                simp only [Option.some.injEq] at currentEq
                subst current
                exact cachedDomain,
              by simp [domainMatches.2], governance, activation⟩
          next => simp at accepted
  | prepareGovernance requestedChainId nonce =>
      simp only [step] at accepted
      split at accepted
      next allowed =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨cache, reuse, by simp [allowed.2], activation⟩
      next => simp at accepted
  | confirmActivation candidate authorized precondition postcondition emergencyClear =>
      simp only [step] at accepted
      split at accepted
      next allowed =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨by simp, by simp, governance, by simp⟩
      next => simp at accepted
  | pause =>
      simp only [step, Option.some.injEq] at accepted
      subst next
      exact ⟨cache, reuse, governance, by simp⟩
  | upgrade nextDomain =>
      simp only [step] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨by simp, by simp, governance, by simp⟩
      next => simp at accepted
  | reinstall nextDomain =>
      simp only [step] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        simp [Safe]
      next => simp at accepted

theorem runs_preserve_safe
    {state final : State} {events : List Event}
    (safe : Safe state) (runs : Runs state events final) : Safe final := by
  induction events generalizing state with
  | nil =>
      simp only [Runs] at runs
      subst final
      exact safe
  | cons event events ih =>
      simp only [Runs] at runs
      obtain ⟨next, accepted, tail⟩ := runs
      exact ih (step_preserves_safe safe accepted) tail

def Reachable (domain : InstallDomain) (state : State) : Prop :=
  ∃ events, Runs (Initial domain) events state

theorem reachable_is_safe
    {domain : InstallDomain} {state : State} (reachable : Reachable domain state) :
    Safe state := by
  obtain ⟨events, runs⟩ := reachable
  exact runs_preserve_safe (initial_safe domain) runs

def reopen (stored : State) : Option State :=
  if Safe stored then some stored else none

theorem reopened_state_is_safe
    {stored reopened : State} (accepted : reopen stored = some reopened) :
    Safe reopened := by
  unfold reopen at accepted
  split at accepted
  next safe =>
    simp only [Option.some.injEq] at accepted
    subst reopened
    exact safe
  next => simp at accepted

theorem reused_attestation_binds_current_install_domain
    {state : State} (safe : Safe state) :
    ∀ domain, state.lastReusedDomain = some domain → domain = state.domain :=
  safe.2.1

theorem governance_nonce_binds_configured_chain
    {state : State} (safe : Safe state) :
    ∀ chainId, state.lastGovernanceChain = some chainId →
      chainId = state.configuredChainId :=
  safe.2.2.1

inductive GovernanceNonceLane where
  | governance
  | runtimeAdministrator
  | independentCanceller
  deriving DecidableEq

def laneAvailable (pending : GovernanceNonceLane → Bool) (lane : GovernanceNonceLane) : Bool :=
  !(pending lane)

theorem governance_pending_does_not_block_emergency_lanes
    (pending : GovernanceNonceLane → Bool)
    (_governancePending : pending .governance = true)
    (runtimeAvailable : pending .runtimeAdministrator = false)
    (cancellerAvailable : pending .independentCanceller = false) :
    laneAvailable pending .runtimeAdministrator = true ∧
      laneAvailable pending .independentCanceller = true := by
  simp [laneAvailable, runtimeAvailable, cancellerAvailable]

def confirmedControlPlaneRotation (receiptSucceeded observedRolesMatch : Bool) (next : α) : Option α :=
  if receiptSucceeded && observedRolesMatch then some next else none

theorem control_plane_rotation_commits_only_after_finalized_match
    (receiptSucceeded observedRolesMatch : Bool) (next : α)
    (accepted : confirmedControlPlaneRotation receiptSucceeded observedRolesMatch next = some next) :
    receiptSucceeded = true ∧ observedRolesMatch = true := by
  simp [confirmedControlPlaneRotation] at accepted
  exact accepted

theorem unpaused_activation_is_validated
    {state : State} (safe : Safe state)
    (unpaused : state.paused = false) (activated : state.activationCount > 0) :
    state.lastActivationValidated = true := by
  rcases safe.2.2.2 unpaused with initial | validated
  · omega
  · exact validated

end BridgeSpec.ControlPlane

namespace BridgeSpec.IdentityHistory

abbrev State := Nat → Bool

def initial : State := fun _ => false

def preflight (state : State) (candidate : Nat) : Option State :=
  if state candidate then none
  else some (fun id => if id = candidate then true else state id)

def Runs : State → List Nat → State → Prop
  | state, [], final => final = state
  | state, candidate :: rest, final =>
      ∃ next, preflight state candidate = some next ∧ Runs next rest final

theorem accepted_preflight_is_fresh {state next : State} {candidate : Nat}
    (accepted : preflight state candidate = some next) : state candidate = false := by
  unfold preflight at accepted
  split at accepted
  next processed => simp at accepted
  next fresh => simpa using fresh

theorem accepted_preflight_marks_only_candidate
    {state next : State} {candidate other : Nat}
    (accepted : preflight state candidate = some next) (different : other ≠ candidate) :
    next candidate = true ∧ next other = state other := by
  unfold preflight at accepted
  split at accepted
  next => simp at accepted
  next =>
    simp only [Option.some.injEq] at accepted
    subst next
    simp [different]

def Reachable (state : State) : Prop := ∃ candidates, Runs initial candidates state

theorem reachable_preflight_is_fresh
    {state next : State} {candidate : Nat}
    (reachable : Reachable state) (accepted : preflight state candidate = some next) :
    state candidate = false :=
  accepted_preflight_is_fresh accepted

end BridgeSpec.IdentityHistory
