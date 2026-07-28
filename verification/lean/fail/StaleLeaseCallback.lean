import BridgeSpec.Protocol

open BridgeSpec.Protocol

example : finishLease {
    activeGeneration := some 7
    nextGeneration := 7
  } 6 ≠ none := by
  decide
