import BridgeSpec.Protocol

open BridgeSpec

example : manualClaimAllowed false true false false false = true := by
  decide
