namespace BridgeSpec

structure Account where
  owner : List UInt8
  subaccount : List UInt8
deriving DecidableEq

structure Withdrawal where
  amount : Nat
  chargedServiceFee : Nat
  amountOut : Nat
  destination : Account
  paid : Bool
deriving DecidableEq

structure LedgerTransfer where
  amount : Nat
  ledgerFee : Nat
  destination : Account
deriving DecidableEq

def QuoteValid (w : Withdrawal) : Prop :=
  w.amountOut + w.chargedServiceFee = w.amount ∧ w.amountOut > 0

def commit (amount serviceFee : Nat) (destination : Account) : Option Withdrawal :=
  if serviceFee < amount then
    some {
      amount := amount
      chargedServiceFee := serviceFee
      amountOut := amount - serviceFee
      destination := destination
      paid := false
    }
  else none

def pay (w : Withdrawal) (transfer : LedgerTransfer) : Option Withdrawal :=
  if !w.paid ∧ transfer.ledgerFee ≤ w.chargedServiceFee ∧
      transfer.amount = w.amountOut ∧ transfer.destination = w.destination then
    some { w with paid := true }
  else none

structure EconomicState where
  escrow : Nat
  baseSupply : Nat
  feeReserve : Nat
  unpaidLiability : Nat
deriving DecidableEq

def Backed (s : EconomicState) : Prop :=
  s.escrow = s.baseSupply + s.feeReserve + s.unpaidLiability

def observeBurn (s : EconomicState) (amount : Nat) : EconomicState :=
  { s with baseSupply := s.baseSupply - amount
           unpaidLiability := s.unpaidLiability + amount }

def settleDebt (s : EconomicState) (amountOut serviceFee ledgerFee : Nat) : EconomicState :=
  { escrow := s.escrow - amountOut - ledgerFee
    baseSupply := s.baseSupply
    feeReserve := s.feeReserve + serviceFee - ledgerFee
    unpaidLiability := s.unpaidLiability - (amountOut + serviceFee) }

def outboundSettlement (amountOut ledgerFee serviceFee : Nat) : Option (Nat × Nat × Nat) :=
  if ledgerFee ≤ serviceFee then
    some (amountOut + ledgerFee, serviceFee - ledgerFee, amountOut + serviceFee)
  else none

def checkedSettlement
    (s : EconomicState) (amountOut serviceFee ledgerFee : Nat) : Option EconomicState :=
  if s.escrow = s.baseSupply + s.feeReserve + s.unpaidLiability ∧
      ledgerFee ≤ serviceFee ∧
      amountOut + serviceFee ≤ s.unpaidLiability ∧
      amountOut + ledgerFee ≤ s.escrow then
    some (settleDebt s amountOut serviceFee ledgerFee)
  else none

inductive WithdrawalFinalizationDecision where
  | retry
  | notify
  | discardReverted
deriving DecidableEq

def decideWithdrawalFinalization
    (receiptSucceeded : Bool) (receiptBlock : Nat) (finalizedBlock : Option Nat) :
    WithdrawalFinalizationDecision :=
  match finalizedBlock with
  | none => .retry
  | some finalized =>
      if finalized < receiptBlock then .retry
      else if receiptSucceeded then .notify else .discardReverted

structure PendingQueueEntry where
  key : Nat
  owner : Nat
  blocked : Bool
deriving DecidableEq

abbrev PendingQueue := Nat → Option PendingQueueEntry

def upsertPendingQueue (queue : PendingQueue) (incoming : PendingQueueEntry) : PendingQueue :=
  fun key => if key = incoming.key then some incoming else queue key

def restorePendingQueue (queue : PendingQueue) (incoming : PendingQueueEntry) : PendingQueue :=
  let blocked := (queue incoming.key).map (fun entry => entry.blocked) |>.getD incoming.blocked
  upsertPendingQueue queue { incoming with blocked }

structure PendingQueueWrite where
  session : PendingQueue
  durable : Option PendingQueue

def recordPendingQueueWrite (queue : PendingQueue) (durableSucceeded : Bool) : PendingQueueWrite :=
  { session := queue, durable := if durableSucceeded then some queue else none }

def canonicalProbeMatches (receiptBlock snapshotBlock : Nat) : Bool :=
  receiptBlock = snapshotBlock

structure DepositAdmission where
  serviceFee : Nat
  maximumServiceFee : Nat
  grossAmount : Nat
  perDepositLimit : Nat
  mintedInWindow : Nat
  mintWindowLimit : Nat
deriving DecidableEq

def DepositAdmissible (a : DepositAdmission) : Prop :=
  a.serviceFee ≤ a.maximumServiceFee ∧
    a.serviceFee < a.grossAmount ∧
    a.grossAmount - a.serviceFee ≤ a.perDepositLimit ∧
    a.mintedInWindow + (a.grossAmount - a.serviceFee) ≤ a.mintWindowLimit

def admitDeposit (a : DepositAdmission) : Option Nat :=
  if a.serviceFee ≤ a.maximumServiceFee ∧
      a.serviceFee < a.grossAmount ∧
      a.grossAmount - a.serviceFee ≤ a.perDepositLimit ∧
      a.mintedInWindow + (a.grossAmount - a.serviceFee) ≤ a.mintWindowLimit then
    some (a.grossAmount - a.serviceFee)
  else none

inductive DepositIdentityDecision where
  | allow
  | conflict
deriving DecidableEq

def decideDepositIdentity (processed : Bool) : DepositIdentityDecision :=
  if processed then .conflict else .allow

def commitMintReservation (reserved candidate : Nat) : Nat × Nat :=
  (reserved + candidate, 0)

structure FeeState where
  reserve : Nat
  confirmedDepositFees : Nat
  confirmedWithdrawalFees : Nat
  pendingPayout : Nat
  recipient : Nat
deriving DecidableEq

def rotateFeeRecipient (state : FeeState) (recipient : Nat) : Option FeeState :=
  if state.pendingPayout = 0 then some { state with recipient } else none

def serviceFeeChangeAllowed (serviceFee maximumServiceFee : Nat) : Bool :=
  serviceFee ≤ maximumServiceFee

def feePayoutAllowed (reserve pending amount fee : Nat) : Bool :=
  pending ≤ reserve && amount + fee ≤ reserve - pending

def payoutDebit (confirmedFirstTime : Bool) (amount fee : Nat) : Nat :=
  if confirmedFirstTime then amount + fee else 0

def holdRetryAllowed (exactSuccessEvidence completeAbsenceEvidence : Bool) : Bool :=
  exactSuccessEvidence || completeAbsenceEvidence

def leaseOutcomeCurrent (active : Bool) (currentGeneration outcomeGeneration : Nat) : Bool :=
  active && currentGeneration = outcomeGeneration

def manualClaimAllowed
    (scheduled active stopped overdue expired : Bool) : Bool :=
  (!active || expired) && (!scheduled || stopped || overdue || expired)

inductive RefundRequestIdentityDecision where
  | allow
  | ownerLookupRequired
  | anonymousCaller
  | ownerMismatch
deriving DecidableEq

def decideRefundRequestIdentity
    (authenticated : Bool) (ownerMatch : Option Bool) :
    RefundRequestIdentityDecision :=
  if !authenticated then .anonymousCaller
  else
    match ownerMatch with
    | none => .ownerLookupRequired
    | some true => .allow
    | some false => .ownerMismatch

structure NotificationIsolationState where
  persistentVerificationCount : Nat
  persistentIngestionCount : Nat
  callerWindowCount : Nat
  settlementAdmission : Nat
  settlementJobs : Nat
deriving DecidableEq

def processNotificationVerification (state : NotificationIsolationState) : NotificationIsolationState :=
  { state with
      persistentVerificationCount := state.persistentVerificationCount + 1
      callerWindowCount := state.callerWindowCount + 1 }

def processNotificationIngestion (state : NotificationIsolationState) : NotificationIsolationState :=
  { state with persistentIngestionCount := state.persistentIngestionCount + 1 }

def notificationAdmissionAllowed
    (globalCount callerCount globalLimit callerLimit : Nat) : Bool :=
  globalCount < globalLimit && callerCount < callerLimit

def notificationIngestionAllowed (ingestionCount ingestionLimit : Nat) : Bool :=
  ingestionCount < ingestionLimit

inductive LeaseLaneClaimDecision where
  | allow
  | automaticProgressPending
  | busy
deriving DecidableEq

def decideLeaseLaneClaim
    (targetActive targetAutomatic : Bool) (activeInLane capacity : Nat) :
    LeaseLaneClaimDecision :=
  if targetActive then
    if targetAutomatic then .automaticProgressPending else .busy
  else if activeInLane ≥ capacity then .busy
  else .allow

inductive FundingOutcomeKind where
  | success
  | duplicate
  | ambiguous
  | definitiveFailure
  | retryableFailure
deriving DecidableEq

inductive FundingAttemptDecision where
  | promoteSuccess
  | promoteAmbiguous
  | release
  | retain
deriving DecidableEq

def decideFundingAttempt : FundingOutcomeKind → FundingAttemptDecision
  | .success | .duplicate => .promoteSuccess
  | .ambiguous => .promoteAmbiguous
  | .definitiveFailure => .release
  | .retryableFailure => .retain

inductive FundingReconciliationDecision where
  | wait
  | restartFresh
  | release
deriving DecidableEq

def decideFundingReconciliation
    (completeAbsence finalScan dedupExpired : Bool) : FundingReconciliationDecision :=
  if completeAbsence = false then .wait
  else if finalScan = false then .restartFresh
  else if dedupExpired then .release
  else .wait

end BridgeSpec
