import BridgeSpec.ClaimContracts

open BridgeSpec.ClaimContracts

example :
    confirmationCallerAuthorized 1 1 2 3 = true ∧
      confirmationCallerAuthorized 1 4 2 3 = true := by
  decide
