import BridgeSpec.Protocol

open BridgeSpec

example : Backed {
    escrow := 0
    baseSupply := 1
    feeReserve := 0
    unpaidLiability := 0
  } := by
  unfold Backed
  decide
