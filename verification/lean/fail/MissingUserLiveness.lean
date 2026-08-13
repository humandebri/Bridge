import BridgeSpec.Liveness

open BridgeSpec.Liveness

def noUserAction : RuntimeSignals := {
  schedulerContinues := true
  timeAdvances := true
  cyclesAvailable := true
  storageCommitAvailable := true
  externalResolutionAvailable := true
  userActionAvailable := false
  unpaused := true }

example : RuntimeReady true noUserAction := by
  unfold RuntimeReady
  decide
