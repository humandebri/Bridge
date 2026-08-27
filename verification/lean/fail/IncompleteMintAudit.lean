import BridgeSpec.Protocol

open BridgeSpec.MintAuthorization

def authorization : Authorization := {
  depositId := 1, recipient := 2, grossAmount := 11, maxServiceFee := 1,
  chargedServiceFee := 1, netAmount := 10, deadline := 600, epoch := 1,
  chainId := 8453, verifyingContract := 3, digest := 4
}

def evidence : MintEvidence := {
  depositId := 1, recipient := 2, authorizationDigest := 4, chainId := 8453,
  verifyingContract := 3, grossAmount := 11, chargedServiceFee := 1,
  mintedAmount := 10, transactionHash := 5, receiptSucceeded := true,
  receiptBlock := 2, receiptBlockHash := 6, finalizedBlock := 2,
  finalizedBlockHash := 7, rpcRequestDigest := 8, rpcResponseDigest := 0,
  exactEventCount := 1
}

example : evidence.valid authorization := by
  decide
