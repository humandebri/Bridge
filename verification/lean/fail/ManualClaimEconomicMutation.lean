import BridgeSpec.Protocol

open BridgeSpec.MintAuthorization

def state : DepositState := {
  phase := .authorizationAvailable, authorization := none, escrow := 11,
  baseSupply := 0, feeReserve := 0, pendingDepositLiability := 11,
  reservedMint := 10, feeCounted := false, jobNextRun := 0,
  leaseGeneration := 4
}

example : manualClaim state 10 5 = some { state with escrow := 0 } := by
  decide
