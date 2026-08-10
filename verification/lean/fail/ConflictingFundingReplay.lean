import BridgeSpec.LedgerBlockProvenance

open BridgeSpec.LedgerBlockProvenance

example :
    step { funding := some 7, refund := none, release := none }
      (.fundingSucceeded 8) =
        some { funding := some 8, refund := none, release := none } := by
  decide
