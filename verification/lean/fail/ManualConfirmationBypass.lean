import BridgeSpec.Protocol

open BridgeSpec

example : manualClaimAllowed true false false true true true = true := by
  decide
