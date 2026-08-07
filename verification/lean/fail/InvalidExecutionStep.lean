import BridgeSpec.Liveness

open BridgeSpec.GlobalHistory

def emptyState : GlobalState := {
  records := []
  accounting := Economic.zero }

example : step emptyState (.cancel 1) = some emptyState := by
  decide
