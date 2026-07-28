import BridgeSpec.DepositAuthorization

open BridgeSpec.MintAuthorization

def authorization : Authorization := {
  depositId := 1, recipient := 2, grossAmount := 11, maxServiceFee := 1,
  chargedServiceFee := 1, netAmount := 10, deadline := 7200, epoch := 1,
  chainId := 8453, verifyingContract := 3, digest := 4
}

def evidence : MintEvidence := {
  depositId := 1, recipient := 2, authorizationDigest := 4, chainId := 8453,
  verifyingContract := 3, grossAmount := 11, chargedServiceFee := 1,
  mintedAmount := 10, transactionHash := 5, receiptSucceeded := true,
  receiptBlock := 7, receiptBlockHash := 8, finalizedBlock := 7,
  finalizedBlockHash := 9, rpcRequestDigest := 10, rpcResponseDigest := 11,
  exactEventCount := 1
}

def state : DepositState := {
  phase := .expiryReconciliation, authorization := some authorization,
  escrow := 11, baseSupply := 0, feeReserve := 0,
  pendingDepositLiability := 11, reservedMint := 10, feeCounted := true
}

example : (completeMint state evidence).any (fun next => next.feeReserve = state.feeReserve) := by
  decide
