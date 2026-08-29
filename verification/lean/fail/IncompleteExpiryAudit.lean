import BridgeSpec.Protocol

open BridgeSpec.MintAuthorization

def authorization : Authorization := {
  depositId := 1, recipient := 2, grossAmount := 11, maxServiceFee := 1,
  chargedServiceFee := 1, netAmount := 10, deadline := 600, epoch := 1,
  chainId := 8453, verifyingContract := 3, digest := 4
}

def origin : AuthorizationOrigin := {
  finalizedBlock := 1, finalizedHash := 2, finalizedTimestamp := 0, issuedAtTimestamp := 0,
  expectedChainId := 8453, expectedVerifyingContract := 3, expectedEpoch := 1
}

example : ExpiryEvidence.valid {
    depositId := 1, authorizationDigest := 4, chainId := 8453,
    verifyingContract := 3, depositProcessed := false, finalizedBlock := 2,
    finalizedHash := 5, finalizedTimestamp := 601, runtimeSha256 := 6,
    rpcRequestDigest := 7, rpcResponseDigest := 0
  } authorization origin := by
  decide
