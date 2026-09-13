import BridgeSpec.Protocol

open BridgeSpec.Protocol

example : step initialState (.fundingSucceeded 0) = some {
    initialState with
    deposit := { initialState.deposit with phase := .escrowedUnquoted }
    committedAmountOut := 1 } := by decide
