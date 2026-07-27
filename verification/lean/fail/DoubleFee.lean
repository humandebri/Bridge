import BridgeSpec.Protocol

open BridgeSpec

example : payoutDebit true 5 1 + payoutDebit true 5 1 = 6 := by
  decide
