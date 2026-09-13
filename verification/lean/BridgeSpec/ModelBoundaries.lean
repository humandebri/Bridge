import BridgeSpec.GlobalHistory

namespace BridgeSpec.ModelBoundaries

open BridgeSpec.GlobalHistory

-- Accounting preservation does not imply that a terminal callback paid a Ledger transfer.
def callbackReady : Record := {
  refundableDeposit with
  economic := { refundableDeposit.economic with reservedMint := 0 }
  leaseGeneration := some 1 }

theorem accounting_invariant_does_not_certify_payment :
    AccountingInvariant { records := [callbackReady], accounting := callbackReady.economic } ∧
    (applyRecord callbackReady (.callback 1 1 .paid)).map
      (fun record => (record.phase, record.payoutApplied, record.economic.escrow)) =
        some (.paid, false, 30001) := by
  constructor
  · simp [AccountingInvariant, UniqueIds, summarize, callbackReady, refundableDeposit,
      Economic.add, Economic.zero, GlobalHistory.Backed, ReservationConsistent]
  · decide

open BridgeSpec.MintAuthorization

def awaitingSignature : DepositState := {
  phase := .authorizationPending
  authorization := some {
    depositId := 1, recipient := 2, grossAmount := 11, maxServiceFee := 1,
    chargedServiceFee := 1, netAmount := 10, deadline := 1000, epoch := 1,
    chainId := 8453, verifyingContract := 3, digest := 4 }
  escrow := 11, baseSupply := 0, feeReserve := 0,
  pendingDepositLiability := 11, reservedMint := 10, feeCounted := false }

theorem signature_time_boundaries :
    (MintAuthorization.installSignature awaitingSignature 700).isSome = true ∧
    MintAuthorization.installSignature awaitingSignature 701 = none ∧
    MintAuthorization.installSignature awaitingSignature 1001 = none ∧
    MintAuthorization.installSignature awaitingSignature MintAuthorization.maxU64 = none ∧
    MintAuthorization.installSignature awaitingSignature (MintAuthorization.maxU64 + 1) = none := by
  decide

end BridgeSpec.ModelBoundaries
