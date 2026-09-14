import BridgeSpec.ModelBoundaries

example : (BridgeSpec.MintAuthorization.installSignature
    BridgeSpec.ModelBoundaries.awaitingSignature 701).isSome = true := by decide
