import BridgeSpec.Model

namespace BridgeSpec.MintAuthorization

def authorizationTtl : Nat := 7200
def maxU64 : Nat := 2 ^ 64 - 1

def deadlineFromFinalized (finalizedTimestamp : Nat) : Option Nat :=
  if finalizedTimestamp + authorizationTtl ≤ maxU64 then
    some (finalizedTimestamp + authorizationTtl)
  else none

structure Authorization where
  depositId : Nat
  recipient : Nat
  grossAmount : Nat
  maxServiceFee : Nat
  chargedServiceFee : Nat
  netAmount : Nat
  deadline : Nat
  epoch : Nat
  chainId : Nat
  verifyingContract : Nat
  digest : Nat
deriving DecidableEq

structure AuthorizationOrigin where
  finalizedBlock : Nat
  finalizedHash : Nat
  finalizedTimestamp : Nat
  expectedChainId : Nat
  expectedVerifyingContract : Nat
  expectedEpoch : Nat
deriving DecidableEq

def Authorization.valid (authorization : Authorization) (origin : AuthorizationOrigin) : Prop :=
  authorization.recipient ≠ 0 ∧
    authorization.chainId = origin.expectedChainId ∧
    authorization.verifyingContract = origin.expectedVerifyingContract ∧
    authorization.digest ≠ 0 ∧
    authorization.epoch = origin.expectedEpoch ∧
    origin.finalizedHash ≠ 0 ∧
    authorization.netAmount + authorization.chargedServiceFee =
      authorization.grossAmount ∧
    authorization.deadline = origin.finalizedTimestamp + authorizationTtl ∧
    deadlineFromFinalized origin.finalizedTimestamp = some authorization.deadline

instance (authorization : Authorization) (origin : AuthorizationOrigin) :
    Decidable (authorization.valid origin) := by
  unfold Authorization.valid
  infer_instance

inductive DepositPhase where
  | fundingPending
  | escrowedUnquoted
  | authorizationPending
  | authorizationAvailable
  | expiryReconciliation
  | fundingReconciliationHold
  | refundPending
  | refundReconciliationHold
  | refunded
  | cancelled
  | minted
deriving DecidableEq

structure DepositState where
  phase : DepositPhase
  authorization : Option Authorization
  escrow : Nat
  baseSupply : Nat
  feeReserve : Nat
  pendingDepositLiability : Nat
  reservedMint : Nat
  feeCounted : Bool
  jobNextRun : Nat := 0
  leaseGeneration : Nat := 0
deriving DecidableEq

def Backed (state : DepositState) : Prop :=
  state.escrow =
    state.baseSupply + state.feeReserve + state.pendingDepositLiability

def fund (state : DepositState) (grossAmount : Nat) : DepositState :=
  { state with
    phase := .escrowedUnquoted
    escrow := state.escrow + grossAmount
    pendingDepositLiability := state.pendingDepositLiability + grossAmount }

def commitAuthorization
    (state : DepositState) (authorization : Authorization)
    (origin : AuthorizationOrigin) : Option DepositState :=
  if state.phase = .escrowedUnquoted ∧ state.authorization = none ∧
      authorization.valid origin ∧
      authorization.grossAmount ≤ state.pendingDepositLiability then
    some { state with
      phase := .authorizationPending
      authorization := some authorization
      reservedMint := authorization.netAmount }
  else none

def installSignature (state : DepositState) : Option DepositState :=
  if state.phase = .authorizationPending then
    some { state with phase := .authorizationAvailable }
  else none

def beginExpiryReconciliation (state : DepositState) : Option DepositState :=
  if state.phase = .authorizationPending ∨ state.phase = .authorizationAvailable then
    some { state with phase := .expiryReconciliation }
  else none

structure ExpiryEvidence where
  depositId : Nat
  authorizationDigest : Nat
  chainId : Nat
  verifyingContract : Nat
  depositProcessed : Bool
  finalizedBlock : Nat
  finalizedHash : Nat
  finalizedTimestamp : Nat
  runtimeSha256 : Nat := 0
  rpcRequestDigest : Nat := 0
  rpcResponseDigest : Nat := 0
deriving DecidableEq

def ExpiryEvidence.valid
    (evidence : ExpiryEvidence) (authorization : Authorization)
    (origin : AuthorizationOrigin) : Prop :=
  evidence.depositId = authorization.depositId ∧
    evidence.authorizationDigest = authorization.digest ∧
    evidence.chainId = authorization.chainId ∧
    evidence.verifyingContract = authorization.verifyingContract ∧
    evidence.depositProcessed = false ∧
    evidence.finalizedBlock ≥ origin.finalizedBlock ∧
    evidence.finalizedHash ≠ 0 ∧
    evidence.runtimeSha256 ≠ 0 ∧
    evidence.rpcRequestDigest ≠ 0 ∧
    evidence.rpcResponseDigest ≠ 0 ∧
    evidence.finalizedTimestamp > authorization.deadline

instance (evidence : ExpiryEvidence) (authorization : Authorization)
    (origin : AuthorizationOrigin) : Decidable (evidence.valid authorization origin) := by
  unfold ExpiryEvidence.valid
  infer_instance

def startExpiredRefund
    (state : DepositState) (origin : AuthorizationOrigin)
    (evidence : ExpiryEvidence) : Option DepositState :=
  match state.authorization with
  | none => none
  | some authorization =>
      if state.phase = .expiryReconciliation ∧ evidence.valid authorization origin then
        some { state with phase := .refundPending, reservedMint := 0 }
      else none

structure MintEvidence where
  depositId : Nat
  recipient : Nat
  authorizationDigest : Nat
  chainId : Nat
  verifyingContract : Nat
  grossAmount : Nat
  chargedServiceFee : Nat
  mintedAmount : Nat
  transactionHash : Nat := 0
  logIndex : Nat := 0
  receiptSucceeded : Bool
  receiptBlock : Nat
  receiptBlockHash : Nat := 0
  finalizedBlock : Nat
  finalizedBlockHash : Nat := 0
  rpcRequestDigest : Nat := 0
  rpcResponseDigest : Nat := 0
  exactEventCount : Nat := 0
deriving DecidableEq

def MintEvidence.valid (evidence : MintEvidence) (authorization : Authorization) : Prop :=
  evidence.depositId = authorization.depositId ∧
    evidence.recipient = authorization.recipient ∧
    evidence.authorizationDigest = authorization.digest ∧
    evidence.chainId = authorization.chainId ∧
    evidence.verifyingContract = authorization.verifyingContract ∧
    evidence.grossAmount = authorization.grossAmount ∧
    evidence.chargedServiceFee = authorization.chargedServiceFee ∧
    evidence.mintedAmount = authorization.netAmount ∧
    evidence.transactionHash ≠ 0 ∧
    evidence.receiptSucceeded = true ∧
    evidence.receiptBlock ≤ evidence.finalizedBlock ∧
    evidence.receiptBlockHash ≠ 0 ∧
    evidence.finalizedBlockHash ≠ 0 ∧
    evidence.rpcRequestDigest ≠ 0 ∧
    evidence.rpcResponseDigest ≠ 0 ∧
    evidence.exactEventCount = 1 ∧
    authorization.netAmount + authorization.chargedServiceFee =
      authorization.grossAmount

instance (evidence : MintEvidence) (authorization : Authorization) :
    Decidable (evidence.valid authorization) := by
  unfold MintEvidence.valid
  infer_instance

def completeMint (state : DepositState) (evidence : MintEvidence) : Option DepositState :=
  match state.authorization with
  | none => none
  | some authorization =>
      if state.phase = .expiryReconciliation ∧ evidence.valid authorization ∧
          authorization.grossAmount ≤ state.pendingDepositLiability ∧
          state.feeCounted = false then
        some { state with
          phase := .minted
          baseSupply := state.baseSupply + authorization.netAmount
          feeReserve := state.feeReserve + authorization.chargedServiceFee
          pendingDepositLiability :=
            state.pendingDepositLiability - authorization.grossAmount
          reservedMint := 0
          feeCounted := true }
      else none

def completeRefund (state : DepositState) : Option DepositState :=
  match state.authorization with
  | none => none
  | some authorization =>
      if state.phase = .refundPending ∧
          authorization.grossAmount ≤ state.pendingDepositLiability ∧
          authorization.grossAmount ≤ state.escrow then
        some { state with
          phase := .refunded
          escrow := state.escrow - authorization.grossAmount
          pendingDepositLiability :=
            state.pendingDepositLiability - authorization.grossAmount
          reservedMint := 0 }
      else none

def terminal : DepositPhase → Bool
  | .minted | .refunded | .cancelled => true
  | _ => false

theorem funding_preserves_backing
    {state : DepositState} {grossAmount : Nat} (backed : Backed state) :
    Backed (fund state grossAmount) := by
  simp only [Backed, fund] at backed ⊢
  omega

theorem accepted_authorization_is_exact_and_has_fixed_deadline
    {state next : DepositState} {authorization : Authorization}
    {origin : AuthorizationOrigin}
    (accepted : commitAuthorization state authorization origin = some next) :
    next.authorization = some authorization ∧
      next.phase = .authorizationPending ∧
      next.reservedMint = authorization.netAmount ∧
      authorization.deadline = origin.finalizedTimestamp + authorizationTtl ∧
      authorization.chainId = origin.expectedChainId ∧
      authorization.verifyingContract = origin.expectedVerifyingContract ∧
      authorization.epoch = origin.expectedEpoch ∧
      deadlineFromFinalized origin.finalizedTimestamp = some authorization.deadline := by
  unfold commitAuthorization at accepted
  split at accepted
  next valid =>
    rcases valid with ⟨_, _, authValid, _⟩
    rcases authValid with ⟨_, chain, contract, _, epoch, _, _, deadline, checked⟩
    simp only [Option.some.injEq] at accepted
    subst next
    exact ⟨rfl, rfl, rfl, deadline, chain, contract, epoch, checked⟩
  next => simp at accepted

theorem committed_authorization_cannot_be_reissued
    {state : DepositState} {current replacement : Authorization}
    {origin : AuthorizationOrigin}
    (committed : state.authorization = some current) :
    commitAuthorization state replacement origin = none := by
  unfold commitAuthorization
  split
  next allowed => simp [committed] at allowed
  next => rfl

theorem deadline_overflow_is_rejected :
    deadlineFromFinalized maxU64 = none := by
  simp [deadlineFromFinalized, maxU64, authorizationTtl]

theorem accepted_expiry_refund_requires_finalized_unprocessed_expiry
    {state next : DepositState} {origin : AuthorizationOrigin}
    {evidence : ExpiryEvidence}
    (accepted : startExpiredRefund state origin evidence = some next) :
    evidence.depositProcessed = false ∧
      ∃ authorization, state.authorization = some authorization ∧
        evidence.depositId = authorization.depositId ∧
        evidence.authorizationDigest = authorization.digest ∧
        evidence.finalizedTimestamp > authorization.deadline := by
  unfold startExpiredRefund at accepted
  cases auth : state.authorization with
  | none => simp [auth] at accepted
  | some authorization =>
      simp only [auth] at accepted
      split at accepted
      next valid =>
        rcases valid with ⟨_, exactEvidence⟩
        rcases exactEvidence with
          ⟨depositId, digest, _, _, unprocessed, _, _, _, _, _, expired⟩
        exact ⟨unprocessed, authorization, rfl, depositId, digest, expired⟩
      next => simp at accepted

theorem accepted_mint_requires_exact_finalized_success
    {state next : DepositState} {evidence : MintEvidence}
    (accepted : completeMint state evidence = some next) :
    evidence.receiptSucceeded = true ∧
      evidence.receiptBlock ≤ evidence.finalizedBlock ∧
      ∃ authorization, state.authorization = some authorization ∧
        evidence.depositId = authorization.depositId ∧
        evidence.recipient = authorization.recipient ∧
        evidence.authorizationDigest = authorization.digest := by
  unfold completeMint at accepted
  cases auth : state.authorization with
  | none => simp [auth] at accepted
  | some authorization =>
      simp only [auth] at accepted
      split at accepted
      next valid =>
        rcases valid with ⟨_, exactEvidence, _, _⟩
        rcases exactEvidence with
          ⟨depositId, recipient, digest, _, _, _, _, _, _, succeeded, finalized,
            _, _, _, _, _, _⟩
        exact ⟨succeeded, finalized, authorization, rfl, depositId, recipient, digest⟩
      next => simp at accepted

theorem mint_preserves_backing
    {state next : DepositState} {evidence : MintEvidence}
    (backed : Backed state)
    (accepted : completeMint state evidence = some next) :
    Backed next ∧ next.phase = .minted ∧ next.reservedMint = 0 ∧
      next.feeCounted = true := by
  unfold completeMint at accepted
  cases auth : state.authorization with
  | none => simp [auth] at accepted
  | some authorization =>
      simp only [auth] at accepted
      split at accepted
      next valid =>
        rcases valid with ⟨_, exactEvidence, liabilityBound, _⟩
        simp only [Option.some.injEq] at accepted
        subst next
        have amount :
            authorization.netAmount + authorization.chargedServiceFee =
              authorization.grossAmount := by
          exact exactEvidence.2.2.2.2.2.2.2.2.2.2.2.2.2.2.2.2
        simp only [Backed] at backed ⊢
        constructor
        · omega
        · simp
      next => simp at accepted

theorem accepted_mint_counts_exact_service_fee
    {state next : DepositState} {evidence : MintEvidence}
    (accepted : completeMint state evidence = some next) :
    ∃ authorization, state.authorization = some authorization ∧
      next.feeReserve = state.feeReserve + authorization.chargedServiceFee ∧
      next.feeCounted = true := by
  unfold completeMint at accepted
  cases auth : state.authorization with
  | none => simp [auth] at accepted
  | some authorization =>
      simp only [auth] at accepted
      split at accepted
      next =>
        simp only [Option.some.injEq] at accepted
        subst next
        exact ⟨authorization, rfl, rfl, rfl⟩
      next => simp at accepted

theorem refund_preserves_backing_and_never_counts_fee
    {state next : DepositState}
    (backed : Backed state) (feeNotCounted : state.feeCounted = false)
    (accepted : completeRefund state = some next) :
    Backed next ∧ next.phase = .refunded ∧ next.reservedMint = 0 ∧
      next.feeCounted = false := by
  unfold completeRefund at accepted
  cases auth : state.authorization with
  | none => simp [auth] at accepted
  | some authorization =>
      simp only [auth] at accepted
      split at accepted
      next valid =>
        simp only [Option.some.injEq] at accepted
        subst next
        simp only [Backed] at backed ⊢
        constructor
        · omega
        · simp [feeNotCounted]
      next => simp at accepted

theorem minted_and_refunded_are_disjoint :
    DepositPhase.minted ≠ DepositPhase.refunded := by decide

theorem terminal_phases_are_absorbing_for_authorization_progress
    {state : DepositState} (terminalState : terminal state.phase = true) :
    installSignature state = none ∧ beginExpiryReconciliation state = none := by
  cases phaseEq : state.phase <;>
    simp [terminal, phaseEq, installSignature, beginExpiryReconciliation] at terminalState ⊢

def manualClaim
    (state : DepositState) (now nextLeaseGeneration : Nat) : Option DepositState :=
  if terminal state.phase = false ∧
      nextLeaseGeneration = state.leaseGeneration + 1 then
    some { state with
      jobNextRun := now
      leaseGeneration := nextLeaseGeneration }
  else none

theorem manual_claim_changes_only_scheduler
    {state next : DepositState} {now nextLeaseGeneration : Nat}
    (accepted : manualClaim state now nextLeaseGeneration = some next) :
    next.phase = state.phase ∧
      next.authorization = state.authorization ∧
      next.escrow = state.escrow ∧
      next.baseSupply = state.baseSupply ∧
      next.feeReserve = state.feeReserve ∧
      next.pendingDepositLiability = state.pendingDepositLiability ∧
      next.reservedMint = state.reservedMint ∧
      next.feeCounted = state.feeCounted ∧
      next.leaseGeneration = state.leaseGeneration + 1 := by
  unfold manualClaim at accepted
  split at accepted
  next allowed =>
    simp only [Option.some.injEq] at accepted
    subst next
    simp [allowed.2]
  next => simp at accepted

theorem manual_claim_cannot_bypass_evidence
    {state next : DepositState} {now nextLeaseGeneration : Nat}
    (accepted : manualClaim state now nextLeaseGeneration = some next) :
    next.phase = state.phase ∧ next.authorization = state.authorization :=
  let preserved := manual_claim_changes_only_scheduler accepted
  ⟨preserved.1, preserved.2.1⟩

end BridgeSpec.MintAuthorization
