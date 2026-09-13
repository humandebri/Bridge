import BridgeSpec.Protocol

open BridgeSpec.Protocol.Deposit

example :
    traceFeeCreditCount [.installSignature 0, .installSignature 0] ≤ 1 := by
  decide
