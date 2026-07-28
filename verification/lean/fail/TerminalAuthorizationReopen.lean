import BridgeSpec.DepositAuthorization

open BridgeSpec.MintAuthorization

def state : DepositState := {
  phase := .minted, authorization := none, escrow := 0, baseSupply := 0,
  feeReserve := 0, pendingDepositLiability := 0, reservedMint := 0,
  feeCounted := true
}

example : installSignature state ≠ none := by
  decide
