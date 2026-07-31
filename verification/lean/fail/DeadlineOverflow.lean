import BridgeSpec.Protocol

open BridgeSpec.MintAuthorization

example : deadlineFromFinalized maxU64 ≠ none := by
  decide
