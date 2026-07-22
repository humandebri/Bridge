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

def pay (w : Withdrawal) (ledgerFee : Nat) : Option Withdrawal :=
  if ledgerFee ≤ w.chargedServiceFee then some { w with paid := true } else none

theorem committed_quote_is_fixed
    {amount serviceFee : Nat} {destination : Account} {w : Withdrawal}
    (h : commit amount serviceFee destination = some w) :
    QuoteValid w := by
  unfold commit at h
  split at h
  next feeLt =>
    simp only [Option.some.injEq] at h
    subst w
    simp only [QuoteValid]
    omega
  next => simp at h

theorem payment_preserves_destination_and_amount
    {w paid : Withdrawal} {ledgerFee : Nat}
    (h : pay w ledgerFee = some paid) :
    paid.destination = w.destination ∧ paid.amountOut = w.amountOut := by
  unfold pay at h
  split at h
  next =>
    simp only [Option.some.injEq] at h
    subst paid
    simp
  next => simp at h

theorem excessive_ledger_fee_stops
    {w : Withdrawal} {ledgerFee : Nat}
    (h : w.chargedServiceFee < ledgerFee) :
    pay w ledgerFee = none := by
  simp [pay, Nat.not_le.mpr h]

structure EconomicState where
  escrow : Nat
  baseSupply : Nat
  feeReserve : Nat
  unpaidLiability : Nat

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

theorem observe_burn_preserves_backing
    {s : EconomicState} {amount : Nat}
    (backed : Backed s) (available : amount ≤ s.baseSupply) :
    Backed (observeBurn s amount) := by
  unfold Backed at backed ⊢
  simp only [observeBurn]
  omega

theorem paid_debt_preserves_backing
    {s : EconomicState} {amountOut serviceFee ledgerFee : Nat}
    (backed : Backed s)
    (feeBound : ledgerFee ≤ serviceFee)
    (liability : amountOut + serviceFee ≤ s.unpaidLiability)
    (escrow : amountOut + ledgerFee ≤ s.escrow) :
    Backed (settleDebt s amountOut serviceFee ledgerFee) := by
  unfold Backed at backed ⊢
  simp only [settleDebt]
  omega

/-
The frontend model begins after the wallet has returned a transaction hash.
Browser storage, RPC correctness, and wallet behavior remain external assumptions;
the model proves only the decision made from an observed receipt and finalized head.
-/
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

structure WithdrawalBroadcast where
  transactionHash : List UInt8
  pendingSaved : Bool
deriving DecidableEq

def recordBroadcast (transactionHash : List UInt8) (pendingSaved : Bool) : WithdrawalBroadcast :=
  { transactionHash, pendingSaved }

theorem broadcast_hash_is_retained_when_storage_fails (transactionHash : List UInt8) :
    (recordBroadcast transactionHash false).transactionHash = transactionHash := by
  rfl

theorem withdrawal_notify_requires_finalized_success
    {receiptSucceeded : Bool} {receiptBlock finalizedBlock : Nat}
    (h : decideWithdrawalFinalization receiptSucceeded receiptBlock (some finalizedBlock) =
      .notify) :
    receiptSucceeded = true ∧ receiptBlock ≤ finalizedBlock := by
  simp only [decideWithdrawalFinalization] at h
  split at h
  next notFinalized => contradiction
  next finalized =>
    split at h
    next succeeded => exact ⟨succeeded, Nat.le_of_not_gt finalized⟩
    next => contradiction

theorem finalized_revert_is_never_notified
    {receiptBlock finalizedBlock : Nat} (finalized : receiptBlock ≤ finalizedBlock) :
    decideWithdrawalFinalization false receiptBlock (some finalizedBlock) =
      .discardReverted := by
  simp [decideWithdrawalFinalization, Nat.not_lt.mpr finalized]

theorem unfinalized_receipt_remains_retryable
    {receiptSucceeded : Bool} {receiptBlock finalizedBlock : Nat}
    (unfinalized : finalizedBlock < receiptBlock) :
    decideWithdrawalFinalization receiptSucceeded receiptBlock (some finalizedBlock) = .retry := by
  simp [decideWithdrawalFinalization, unfinalized]

/-
Web Locks and localStorage durability are external assumptions. This model covers the pure queue
decision executed inside the serialized critical section and retained in session memory first.
-/
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

theorem serialized_upsert_preserves_different_entry
    {queue : PendingQueue} {incoming : PendingQueueEntry} {key : Nat}
    (different : Not (key = incoming.key)) :
    upsertPendingQueue queue incoming key = queue key := by
  simp [upsertPendingQueue, different]

theorem restore_preserves_blocked_retry
    {queue : PendingQueue} {existing incoming : PendingQueueEntry}
    (blocked : existing.blocked = true)
    (current : queue incoming.key = some existing) :
    (restorePendingQueue queue incoming incoming.key).map (fun entry => entry.blocked) = some true := by
  simp [restorePendingQueue, current, blocked, upsertPendingQueue]

structure PendingQueueWrite where
  session : PendingQueue
  durable : Option PendingQueue

def recordPendingQueueWrite (queue : PendingQueue) (durableSucceeded : Bool) :
    PendingQueueWrite :=
  { session := queue, durable := if durableSucceeded then some queue else none }

theorem storage_failure_retains_session_queue (queue : PendingQueue) :
    (recordPendingQueueWrite queue false).session = queue := by
  rfl
