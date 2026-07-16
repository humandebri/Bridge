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
