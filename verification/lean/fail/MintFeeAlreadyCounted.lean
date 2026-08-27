import BridgeSpec.DepositAuthorization

open BridgeSpec.MintAuthorization

def authorization : Authorization := {
  depositId := 1, recipient := 2, grossAmount := 11, maxServiceFee := 1,
  chargedServiceFee := 1, netAmount := 10, deadline := 600, epoch := 1,
  chainId := 8453, verifyingContract := 3, digest := 4
}

def state : DepositState := {
  phase := .authorizationPending, authorization := some authorization,
  escrow := 11, baseSupply := 0, feeReserve := 1,
  pendingDepositLiability := 10, reservedMint := 10, feeCounted := true
}

example : (installSignature state).any
    (fun next => next.feeReserve = state.feeReserve + authorization.chargedServiceFee) := by
  decide
