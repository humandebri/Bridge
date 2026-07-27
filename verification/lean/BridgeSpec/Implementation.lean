import BridgeSpec.Model

namespace BridgeSpec.Implementation

open BridgeSpec

def maxU128 : Nat := 2 ^ 128 - 1
def maxU64 : Nat := 2 ^ 64 - 1

structure U128 where
  val : Nat
  bounded : val ≤ maxU128
deriving DecidableEq

structure U64 where
  val : Nat
  bounded : val ≤ maxU64
deriving DecidableEq

def checkedAdd128 (left right : Nat) : Option Nat :=
  if left + right ≤ maxU128 then some (left + right) else none

def checkedSub128 (left right : Nat) : Option Nat :=
  if right ≤ left ∧ left ≤ maxU128 then some (left - right) else none

def checkedMul128 (left right : Nat) : Option Nat :=
  if left * right ≤ maxU128 then some (left * right) else none

def checkedNext64 (current : Nat) : Option Nat :=
  if current < maxU64 then some (current + 1) else none

def checkedWindowId64 (now windowSize : Nat) : Option Nat :=
  if windowSize = 0 then none
  else
    let windowId := now / windowSize
    if windowId ≤ maxU64 then some windowId else none

def checkedCounterDelta64 (current : Nat) (wasActive isActive : Bool) : Option Nat :=
  if wasActive = isActive then
    if current ≤ maxU64 then some current else none
  else if isActive then checkedNext64 current
  else if current ≤ maxU64 then checkedSub128 current 1 else none

def commitImpl (amount serviceFee : U128) (destination : Account) : Option Withdrawal :=
  commit amount.val serviceFee.val destination

def settlementImpl
    (state : EconomicState) (amountOut serviceFee ledgerFee : U128) : Option EconomicState :=
  if state.escrow ≤ maxU128 ∧ state.baseSupply ≤ maxU128 ∧
      state.feeReserve ≤ maxU128 ∧ state.unpaidLiability ≤ maxU128 ∧
      amountOut.val + serviceFee.val ≤ maxU128 ∧
      amountOut.val + ledgerFee.val ≤ maxU128 then
    checkedSettlement state amountOut.val serviceFee.val ledgerFee.val
  else none

def paymentImpl (withdrawal : Withdrawal) (transfer : LedgerTransfer) : Option Withdrawal :=
  if withdrawal.amount ≤ maxU128 ∧ withdrawal.amountOut ≤ maxU128 ∧
      withdrawal.chargedServiceFee ≤ maxU128 ∧ transfer.amount ≤ maxU128 ∧
      transfer.ledgerFee ≤ maxU128 then
    pay withdrawal transfer
  else none

def depositAdmissionImpl (admission : DepositAdmission) : Option Nat :=
  if admission.serviceFee ≤ maxU128 ∧ admission.maximumServiceFee ≤ maxU128 ∧
      admission.grossAmount ≤ maxU128 ∧ admission.perDepositLimit ≤ maxU128 ∧
      admission.mintedInWindow ≤ maxU128 ∧ admission.mintWindowLimit ≤ maxU128 ∧
      admission.mintedInWindow +
        (admission.grossAmount - admission.serviceFee) ≤ maxU128 then
    admitDeposit admission
  else none

def reservationImpl (reserved candidate : U128) : Option (Nat × Nat) :=
  (checkedAdd128 reserved.val candidate.val).map (fun total => (total, 0))

def serviceFeeImpl (serviceFee maximumServiceFee : U128) : Bool :=
  serviceFeeChangeAllowed serviceFee.val maximumServiceFee.val

def feeRotationImpl (state : FeeState) (recipient : U64) : Option FeeState :=
  rotateFeeRecipient state recipient.val

def feePayoutImpl (reserve pending amount fee : U128) : Bool :=
  match checkedAdd128 amount.val fee.val with
  | none => false
  | some _ => feePayoutAllowed reserve.val pending.val amount.val fee.val

def holdImpl (exactSuccess completeAbsence : Bool) : Bool :=
  holdRetryAllowed exactSuccess completeAbsence

def leaseImpl (active : Bool) (current outcome : U64) : Bool :=
  leaseOutcomeCurrent active current.val outcome.val

def manualClaimImpl
    (confirmation scheduled active stopped overdue expired : Bool) : Bool :=
  manualClaimAllowed confirmation scheduled active stopped overdue expired

def notificationAdmissionImpl
    (callerCount hashCount callerLimit hashLimit : U64) : Bool :=
  notificationAdmissionAllowed callerCount.val hashCount.val callerLimit.val hashLimit.val

def leaseLaneClaimImpl
    (targetActive targetAutomatic : Bool) (activeInLane capacity : U64) :
    LeaseLaneClaimDecision :=
  decideLeaseLaneClaim targetActive targetAutomatic activeInLane.val capacity.val

def fundingAttemptImpl (outcome : FundingOutcomeKind) : FundingAttemptDecision :=
  decideFundingAttempt outcome

def finalizationImpl
    (receiptSucceeded : Bool) (receiptBlock : U64) (finalizedBlock : Option U64) :
    WithdrawalFinalizationDecision :=
  decideWithdrawalFinalization receiptSucceeded receiptBlock.val
    (finalizedBlock.map U64.val)

def pendingQueueImpl
    (queue : PendingQueue) (incoming : PendingQueueEntry) : PendingQueue :=
  restorePendingQueue queue incoming

def canonicalProbeImpl (receiptBlock snapshotBlock : U64) : Bool :=
  canonicalProbeMatches receiptBlock.val snapshotBlock.val

end BridgeSpec.Implementation
