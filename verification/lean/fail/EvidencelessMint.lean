import BridgeSpec.DepositAuthorization

open BridgeSpec.MintAuthorization

def state : DepositState := {
  phase := .refundAvailable, authorization := none, escrow := 11,
  baseSupply := 0, feeReserve := 0, pendingDepositLiability := 11,
  reservedMint := 10, feeCounted := false
}

def evidence : MintEvidence := {
  depositId := 1, recipient := 2, authorizationDigest := 3, chainId := 8453,
  verifyingContract := 4, grossAmount := 11, chargedServiceFee := 1,
  mintedAmount := 10, receiptSucceeded := true, receiptBlock := 1,
  finalizedBlock := 1
}

example : completeMint state evidence = some { state with phase := .minted } := by
  decide
