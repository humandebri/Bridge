namespace BridgeModel

/- Refinement boundary. These are observations supplied by deployment/runtime evidence, not
   axioms asserted by the algebraic model. A cross-system theorem must receive a trusted world
   explicitly; the local transition theorems remain independent of it. -/
structure WorldAssumptions where
  honestBridgeSigner : Bool
  canonicalSafeChain : Bool
  authenticLedgerResults : Bool
  atomicIcSqliteCommit : Bool
  deriving DecidableEq, Repr

def TrustedWorld (world : WorldAssumptions) : Prop :=
  world.honestBridgeSigner = true ∧ world.canonicalSafeChain = true ∧
  world.authenticLedgerResults = true ∧ world.atomicIcSqliteCommit = true

inductive WithdrawalPhase where
  | idle
  | pending
  | releasing
  | released
  | refunded
  deriving DecidableEq, Repr

structure State where
  icpEscrow : Int
  baseSupply : Int
  feeReserve : Int
  depositLiability : Int
  targetWithdrawalLiability : Int
  otherWithdrawalLiability : Int
  withdrawalPhase : WithdrawalPhase
  receivedIcpRelease : Bool
  receivedBaseRefund : Bool
  deriving Repr

def OneToOne (s : State) : Prop :=
  s.icpEscrow =
    s.baseSupply + s.feeReserve + s.depositLiability +
      s.targetWithdrawalLiability + s.otherWithdrawalLiability

def NonnegativeBalances (s : State) : Prop :=
  0 ≤ s.icpEscrow ∧ 0 ≤ s.baseSupply ∧ 0 ≤ s.feeReserve ∧
  0 ≤ s.depositLiability ∧ 0 ≤ s.targetWithdrawalLiability ∧
  0 ≤ s.otherWithdrawalLiability

def EconomicTerminal (s : State) : Prop :=
  s.withdrawalPhase = .released ∨ s.withdrawalPhase = .refunded

def TerminalLiabilityCleared (s : State) : Prop :=
  EconomicTerminal s → s.targetWithdrawalLiability = 0

def ValidInitial (s : State) : Prop :=
  OneToOne s ∧ NonnegativeBalances s ∧ s.withdrawalPhase = .idle ∧
  s.targetWithdrawalLiability = 0 ∧ s.receivedIcpRelease = false ∧
  s.receivedBaseRefund = false

def ExclusiveSettlement (s : State) : Prop :=
  ¬(s.receivedIcpRelease = true ∧ s.receivedBaseRefund = true)

def ReceivedHistoryMonotone (before after : State) : Prop :=
  (before.receivedIcpRelease = true → after.receivedIcpRelease = true) ∧
  (before.receivedBaseRefund = true → after.receivedBaseRefund = true)

inductive Step : State → State → Prop where
  | acceptDeposit (s : State) (gross : Int) (nonnegative : 0 ≤ gross) :
      Step s { s with
        icpEscrow := s.icpEscrow + gross
        depositLiability := s.depositLiability + gross }
  | confirmMint (s : State) (gross net fee : Int)
      (nonnegative : 0 ≤ gross ∧ 0 ≤ net ∧ 0 ≤ fee)
      (partition : gross = net + fee)
      (covered : gross ≤ s.depositLiability) :
      Step s { s with
        baseSupply := s.baseSupply + net
        feeReserve := s.feeReserve + fee
        depositLiability := s.depositLiability - gross }
  | requestWithdrawal (s : State) (gross : Int)
      (phase : s.withdrawalPhase = .idle)
      (unencumbered : s.targetWithdrawalLiability = 0)
      (nonnegative : 0 ≤ gross)
      (covered : gross ≤ s.baseSupply)
      (unsettled : s.receivedIcpRelease = false ∧ s.receivedBaseRefund = false) :
      Step s { s with
        baseSupply := s.baseSupply - gross
        targetWithdrawalLiability := gross
        withdrawalPhase := .releasing }
  | cancelRelease (s : State) (phase : s.withdrawalPhase = .releasing) :
      Step s { s with withdrawalPhase := .pending }
  | releaseIcp (s : State) (amountOut serviceFee ledgerFee : Int)
      (phase : s.withdrawalPhase = .releasing)
      (nonnegative : 0 ≤ amountOut ∧ 0 ≤ serviceFee ∧ 0 ≤ ledgerFee)
      (escrowCovered : amountOut + ledgerFee ≤ s.icpEscrow)
      (liabilityPartition :
        amountOut + serviceFee + ledgerFee = s.targetWithdrawalLiability)
      (notRefunded : s.receivedBaseRefund = false) :
      Step s { s with
        icpEscrow := s.icpEscrow - amountOut - ledgerFee
        feeReserve := s.feeReserve + serviceFee
        targetWithdrawalLiability := 0
        withdrawalPhase := .released
        receivedIcpRelease := true }
  | refundBase (s : State) (gross : Int)
      (phase : s.withdrawalPhase = .pending)
      (nonnegative : 0 ≤ gross)
      (liabilityPartition : gross = s.targetWithdrawalLiability)
      (notReleased : s.receivedIcpRelease = false) :
      Step s { s with
        baseSupply := s.baseSupply + gross
        targetWithdrawalLiability := 0
        withdrawalPhase := .refunded
        receivedBaseRefund := true }

theorem step_preserves_one_to_one {before after : State}
    (backed : OneToOne before) (step : Step before after) : OneToOne after := by
  cases step <;> simp_all [OneToOne] <;> omega

theorem step_preserves_nonnegative_balances {before after : State}
    (nonnegative : NonnegativeBalances before) (step : Step before after) :
    NonnegativeBalances after := by
  cases step <;> simp_all [NonnegativeBalances] <;> omega

theorem step_preserves_exclusive_settlement {before after : State}
    (exclusive : ExclusiveSettlement before) (step : Step before after) :
    ExclusiveSettlement after := by
  cases step <;> simp_all [ExclusiveSettlement]

theorem step_preserves_received_history {before after : State} (step : Step before after) :
    ReceivedHistoryMonotone before after := by
  cases step <;> simp_all [ReceivedHistoryMonotone]

theorem step_preserves_terminal_liability_cleared {before after : State}
    (cleared : TerminalLiabilityCleared before) (step : Step before after) :
    TerminalLiabilityCleared after := by
  cases step <;> simp_all [TerminalLiabilityCleared, EconomicTerminal]

theorem step_preserves_other_withdrawal_liability {before after : State}
    (step : Step before after) :
    after.otherWithdrawalLiability = before.otherWithdrawalLiability := by
  cases step <;> rfl

inductive Reachable : State → State → Prop where
  | refl (s : State) : Reachable s s
  | tail {initial before after : State} :
      Reachable initial before → Step before after → Reachable initial after

def RefinedExecution (world : WorldAssumptions) (initial final : State) : Prop :=
  TrustedWorld world ∧ Reachable initial final

theorem valid_initial_has_cleared_terminal_liability {initial : State}
    (valid : ValidInitial initial) : TerminalLiabilityCleared initial := by
  intro terminal
  exact valid.2.2.2.1

theorem reachable_preserves_one_to_one {initial final : State}
    (backed : OneToOne initial) (reachable : Reachable initial final) : OneToOne final := by
  induction reachable with
  | refl => exact backed
  | tail reachable step ih => exact step_preserves_one_to_one ih step

theorem reachable_preserves_nonnegative_balances {initial final : State}
    (nonnegative : NonnegativeBalances initial) (reachable : Reachable initial final) :
    NonnegativeBalances final := by
  induction reachable with
  | refl => exact nonnegative
  | tail reachable step ih => exact step_preserves_nonnegative_balances ih step

theorem withdrawal_never_receives_release_and_refund {initial final : State}
    (exclusive : ExclusiveSettlement initial) (reachable : Reachable initial final) :
    ExclusiveSettlement final := by
  induction reachable with
  | refl => exact exclusive
  | tail reachable step ih => exact step_preserves_exclusive_settlement ih step

theorem reachable_preserves_received_history {initial final : State}
    (reachable : Reachable initial final) : ReceivedHistoryMonotone initial final := by
  induction reachable with
  | refl => simp [ReceivedHistoryMonotone]
  | tail reachable step ih =>
      have current := step_preserves_received_history step
      exact ⟨fun initialTrue => current.1 (ih.1 initialTrue),
        fun initialTrue => current.2 (ih.2 initialTrue)⟩

theorem reachable_preserves_terminal_liability_cleared {initial final : State}
    (cleared : TerminalLiabilityCleared initial) (reachable : Reachable initial final) :
    TerminalLiabilityCleared final := by
  induction reachable with
  | refl => exact cleared
  | tail reachable step ih => exact step_preserves_terminal_liability_cleared ih step

theorem economic_terminal_has_zero_target_liability {initial final : State}
    (cleared : TerminalLiabilityCleared initial) (reachable : Reachable initial final)
    (terminal : EconomicTerminal final) : final.targetWithdrawalLiability = 0 := by
  exact reachable_preserves_terminal_liability_cleared cleared reachable terminal

theorem valid_execution_terminal_has_zero_target_liability {initial final : State}
    (valid : ValidInitial initial) (reachable : Reachable initial final)
    (terminal : EconomicTerminal final) : final.targetWithdrawalLiability = 0 := by
  exact economic_terminal_has_zero_target_liability
    (valid_initial_has_cleared_terminal_liability valid) reachable terminal

theorem refined_execution_terminal_safety {world : WorldAssumptions} {initial final : State}
    (valid : ValidInitial initial) (execution : RefinedExecution world initial final)
    (terminal : EconomicTerminal final) :
    final.targetWithdrawalLiability = 0 ∧ OneToOne final ∧ ExclusiveSettlement final := by
  have reachable := execution.2
  exact ⟨valid_execution_terminal_has_zero_target_liability valid reachable terminal,
    reachable_preserves_one_to_one valid.1 reachable,
    withdrawal_never_receives_release_and_refund (by simp [ExclusiveSettlement, valid.2.2.2.2])
      reachable⟩

theorem reachable_preserves_other_withdrawal_liability {initial final : State}
    (reachable : Reachable initial final) :
    final.otherWithdrawalLiability = initial.otherWithdrawalLiability := by
  induction reachable with
  | refl => rfl
  | tail reachable step ih =>
      rw [step_preserves_other_withdrawal_liability step, ih]

end BridgeModel
