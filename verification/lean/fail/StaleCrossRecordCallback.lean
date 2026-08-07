import BridgeSpec.GlobalHistory

open BridgeSpec.GlobalHistory

def first : Record := {
  id := 1
  kind := .withdrawal
  phase := .committed
  economic := {
    escrow := 1, baseSupply := 0, feeReserve := 0,
    unmintedLiability := 0, unreleasedLiability := 1, reservedMint := 0 }
  netAmount := 1
  chargedServiceFee := 0
  paymentDestination := 1
  feeApplied := false
  mintApplied := false
  payoutApplied := false
  releaseApplied := false
  jobDue := true
  leaseGeneration := some 7 }

def second : Record := { first with id := 2, leaseGeneration := some 9 }

example :
    applyRecord second (.callback 1 7 .paid) ≠ none := by
  decide
