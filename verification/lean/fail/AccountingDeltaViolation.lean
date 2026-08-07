import BridgeSpec.GlobalHistory

open BridgeSpec.GlobalHistory

def underbackedSignature : Record := {
  id := 1
  kind := .deposit
  phase := .funded
  economic := {
    escrow := 0, baseSupply := 0, feeReserve := 0,
    unmintedLiability := 0, unreleasedLiability := 0, reservedMint := 0 }
  netAmount := 10
  chargedServiceFee := 1
  paymentDestination := 0
  feeApplied := false
  mintApplied := false
  payoutApplied := false
  releaseApplied := false
  jobDue := true
  leaseGeneration := none }

example : applyRecord underbackedSignature (.installSignature 1) ≠ none := by
  decide
