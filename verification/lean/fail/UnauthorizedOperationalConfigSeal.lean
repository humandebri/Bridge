import BridgeSpec.ClaimContracts

open BridgeSpec.ClaimContracts

example : operationalConfigSealCallerAuthorized false true = true := by
  decide
