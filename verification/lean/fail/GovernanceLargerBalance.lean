import BridgeSpec.ClaimContracts

example : (BridgeSpec.ClaimContracts.governanceAffordabilityDecision 11 9 10).2 = true := by
  decide
