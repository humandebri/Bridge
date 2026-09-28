import BridgeSpec.ClaimContracts

open BridgeSpec.ClaimContracts

example : multiAssetAuthorizationAccepted 1 2 3 3 4 4 = true := by
  decide
