import BridgeSpec.GlobalHistory

open BridgeSpec.GlobalHistory

example : (refundedDepositTrace.bind fun record => applyRecord record (.refund 1 20001)) ≠ none := by
  decide
