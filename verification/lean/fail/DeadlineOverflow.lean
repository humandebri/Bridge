import BridgeSpec.Protocol

open BridgeSpec.MintAuthorization

example : deadlineFromIssuedAt maxU64 ≠ none := by
  decide
