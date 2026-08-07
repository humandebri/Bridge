import BridgeSpec.GlobalHistory

namespace BridgeSpec.Liveness

open BridgeSpec.GlobalHistory

structure RuntimeSignals where
  schedulerContinues : Bool
  timeAdvances : Bool
  cyclesAvailable : Bool
  storageCommitAvailable : Bool
  externalResolutionAvailable : Bool
  userActionAvailable : Bool
  unpaused : Bool
deriving DecidableEq

structure Execution where
  state : Nat → GlobalState
  action : Nat → Option Event
  signals : Nat → RuntimeSignals
  valid : ∀ time,
    match action time with
    | none => state (time + 1) = state time
    | some event => step (state time) event = some (state (time + 1))

def Eventually (predicate : GlobalState → Prop) (execution : Execution) (start : Nat) : Prop :=
  ∃ later, start ≤ later ∧ predicate (execution.state later)

def ReachesPhase (id : Nat) (target : Phase) (state : GlobalState) : Prop :=
  ∃ record, findRecord? state.records id = some record ∧ record.phase = target

def OccurredBefore (execution : Execution) (event : Event) (start time : Nat) : Prop :=
  ∃ happened, start ≤ happened ∧ happened < time ∧ execution.action happened = some event

def RuntimeReady (needsUser : Bool) (signals : RuntimeSignals) : Prop :=
  signals.schedulerContinues = true ∧ signals.timeAdvances = true ∧
    signals.cyclesAvailable = true ∧ signals.storageCommitAvailable = true ∧
    signals.externalResolutionAvailable = true ∧ signals.unpaused = true ∧
    (needsUser = true → signals.userActionAvailable = true)

def AdmissibleUntilOccurs
    (execution : Execution) (event : Event) (start : Nat) : Prop :=
  ∀ time, start ≤ time → ¬ OccurredBefore execution event start time →
    ∃ next, step (execution.state time) event = some next

def EnabledUntilOccurs
    (execution : Execution) (event : Event) (needsUser : Bool) (start : Nat) : Prop :=
  ∀ time, start ≤ time → ¬ OccurredBefore execution event start time →
    RuntimeReady needsUser (execution.signals time) ∧
      ∃ next, step (execution.state time) event = some next

def WeakFair (execution : Execution) : Prop :=
  ∀ event needsUser start, EnabledUntilOccurs execution event needsUser start →
    ∃ time, start ≤ time ∧ execution.action time = some event

structure CommonOperationalAssumptions (execution : Execution) (start : Nat) where
  readyAt : Nat
  readyAfterStart : start ≤ readyAt
  schedulerWeakFair : WeakFair execution
  schedulerContinues : ∀ time, readyAt ≤ time →
    (execution.signals time).schedulerContinues = true
  timeAdvances : ∀ time, readyAt ≤ time →
    (execution.signals time).timeAdvances = true
  cyclesAvailable : ∀ time, readyAt ≤ time →
    (execution.signals time).cyclesAvailable = true
  storageEventuallyCommits : ∀ time, readyAt ≤ time →
    (execution.signals time).storageCommitAvailable = true
  externalSystemEventuallyResolves : ∀ time, readyAt ≤ time →
    (execution.signals time).externalResolutionAvailable = true
  notPermanentlyPaused : ∀ time, readyAt ≤ time →
    (execution.signals time).unpaused = true

structure UserActionAssumption
    (execution : Execution) (readyAt : Nat) where
  eventuallyActs : ∀ time, readyAt ≤ time →
    (execution.signals time).userActionAvailable = true

theorem common_enables_without_user
    {execution : Execution} {start : Nat}
    (common : CommonOperationalAssumptions execution start)
    {event : Event}
    (admissible : AdmissibleUntilOccurs execution event common.readyAt) :
    EnabledUntilOccurs execution event false common.readyAt := by
  intro time after noOccurrence
  exact ⟨⟨common.schedulerContinues time after, common.timeAdvances time after,
    common.cyclesAvailable time after, common.storageEventuallyCommits time after,
    common.externalSystemEventuallyResolves time after,
    common.notPermanentlyPaused time after, by simp⟩,
    admissible time after noOccurrence⟩

theorem common_enables_with_user
    {execution : Execution} {start : Nat}
    (common : CommonOperationalAssumptions execution start)
    (user : UserActionAssumption execution common.readyAt)
    {event : Event}
    (admissible : AdmissibleUntilOccurs execution event common.readyAt) :
    EnabledUntilOccurs execution event true common.readyAt := by
  intro time after noOccurrence
  exact ⟨⟨common.schedulerContinues time after, common.timeAdvances time after,
    common.cyclesAvailable time after, common.storageEventuallyCommits time after,
    common.externalSystemEventuallyResolves time after,
    common.notPermanentlyPaused time after, by
      intro
      exact user.eventuallyActs time after⟩,
    admissible time after noOccurrence⟩

theorem occurrence_produces_valid_step
    {execution : Execution} {event : Event} {time : Nat}
    (occurs : execution.action time = some event) :
    step (execution.state time) event = some (execution.state (time + 1)) := by
  have valid := execution.valid time
  rw [occurs] at valid
  exact valid

def WithdrawalEventuallyPaid : Prop :=
  ∀ (execution : Execution) (id ledgerFee transferAmount destination start : Nat),
    (common : CommonOperationalAssumptions execution start) →
    AdmissibleUntilOccurs execution
      (.payout id ledgerFee transferAmount destination) common.readyAt →
    Eventually (ReachesPhase id .paid) execution start

theorem committed_withdrawal_eventually_paid : WithdrawalEventuallyPaid := by
  intro execution id ledgerFee transferAmount destination start common admissible
  obtain ⟨time, after, occurs⟩ := common.schedulerWeakFair
    (.payout id ledgerFee transferAmount destination) false common.readyAt
    (common_enables_without_user common admissible)
  have accepted := occurrence_produces_valid_step occurs
  have paid := (step_terminal_event_reaches_phase accepted).2.2.2
    ledgerFee transferAmount destination rfl
  have startBefore : start ≤ time := Nat.le_trans common.readyAfterStart after
  exact ⟨time + 1, by omega, paid⟩

def FundedDepositEventuallyMinted : Prop :=
  ∀ (execution : Execution) (id start : Nat),
    (common : CommonOperationalAssumptions execution start) →
    UserActionAssumption execution common.readyAt →
    AdmissibleUntilOccurs execution (.mint id) common.readyAt →
    Eventually (ReachesPhase id .minted) execution start

theorem funded_deposit_eventually_minted : FundedDepositEventuallyMinted := by
  intro execution id start common user admissible
  obtain ⟨time, after, occurs⟩ := common.schedulerWeakFair (.mint id) true common.readyAt
    (common_enables_with_user common user admissible)
  have accepted := occurrence_produces_valid_step occurs
  have minted := (step_terminal_event_reaches_phase accepted).1 rfl
  have startBefore : start ≤ time := Nat.le_trans common.readyAfterStart after
  exact ⟨time + 1, by omega, minted⟩

def ExpiredDepositEventuallyRefunded : Prop :=
  ∀ (execution : Execution) (id amount start : Nat),
    (common : CommonOperationalAssumptions execution start) →
    UserActionAssumption execution common.readyAt →
    AdmissibleUntilOccurs execution (.refund id amount) common.readyAt →
    Eventually (ReachesPhase id .refunded) execution start

theorem expired_deposit_eventually_refunded : ExpiredDepositEventuallyRefunded := by
  intro execution id amount start common user admissible
  obtain ⟨time, after, occurs⟩ := common.schedulerWeakFair (.refund id amount) true common.readyAt
    (common_enables_with_user common user admissible)
  have accepted := occurrence_produces_valid_step occurs
  have refunded := (step_terminal_event_reaches_phase accepted).2.1 amount rfl
  have startBefore : start ≤ time := Nat.le_trans common.readyAfterStart after
  exact ⟨time + 1, by omega, refunded⟩

def FundingFailureEventuallyCancelled : Prop :=
  ∀ (execution : Execution) (id start : Nat),
    (common : CommonOperationalAssumptions execution start) →
    AdmissibleUntilOccurs execution (.cancel id) common.readyAt →
    Eventually (ReachesPhase id .cancelled) execution start

theorem funding_failure_eventually_cancelled : FundingFailureEventuallyCancelled := by
  intro execution id start common admissible
  obtain ⟨time, after, occurs⟩ := common.schedulerWeakFair (.cancel id) false common.readyAt
    (common_enables_without_user common admissible)
  have accepted := occurrence_produces_valid_step occurs
  have cancelled := (step_terminal_event_reaches_phase accepted).2.2.1 rfl
  have startBefore : start ≤ time := Nat.le_trans common.readyAfterStart after
  exact ⟨time + 1, by omega, cancelled⟩

end BridgeSpec.Liveness
