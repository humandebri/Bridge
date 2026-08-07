import BridgeSpec.Protocol

open BridgeSpec.Protocol.Deposit

example :
    traceFeeCreditCount [.installSignature, .installSignature] ≤ 1 := by
  decide
