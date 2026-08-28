import BridgeSpec.DepositAuthorization

open BridgeSpec.MintAuthorization

def current : Authorization := {
  depositId := 1, recipient := 2, grossAmount := 11, maxServiceFee := 1,
  chargedServiceFee := 1, netAmount := 10, deadline := 1600, epoch := 1,
  chainId := 8453, verifyingContract := 3, digest := 4
}

def replacement : Authorization := { current with deadline := 7300, digest := 5 }

def state : DepositState := {
  phase := .escrowedUnquoted, authorization := some current, escrow := 11,
  baseSupply := 0, feeReserve := 0, pendingDepositLiability := 11,
  reservedMint := 0, feeCounted := false
}

def origin : AuthorizationOrigin := {
  finalizedBlock := 1, finalizedHash := 2, finalizedTimestamp := 100, issuedAtTimestamp := 1_000,
  expectedChainId := 8453, expectedVerifyingContract := 3, expectedEpoch := 1
}

example : commitAuthorization state replacement origin ≠ none := by
  decide
