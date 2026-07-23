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

def mintDeposit (s : EconomicState) (grossAmount serviceFee : Nat) : Option EconomicState :=
  if serviceFee ≤ grossAmount then
    some {
      escrow := s.escrow + grossAmount
      baseSupply := s.baseSupply + (grossAmount - serviceFee)
      feeReserve := s.feeReserve + serviceFee
      unpaidLiability := s.unpaidLiability
    }
  else none

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

end BridgeSpec
