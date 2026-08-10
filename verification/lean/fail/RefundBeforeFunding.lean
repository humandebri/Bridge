import BridgeSpec.LedgerBlockProvenance

open BridgeSpec.LedgerBlockProvenance

example :
    step { funding := none, refund := none, release := none }
      (.refundSucceeded 7) =
        some { funding := none, refund := some 7, release := none } := by
  decide
