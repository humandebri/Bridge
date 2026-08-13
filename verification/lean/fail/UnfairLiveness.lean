import BridgeSpec.Liveness

open BridgeSpec.Liveness

def unavailable : RuntimeSignals := {
  schedulerContinues := false
  timeAdvances := true
  cyclesAvailable := true
  storageCommitAvailable := true
  externalResolutionAvailable := true
  userActionAvailable := true
  unpaused := true }

example : RuntimeReady false unavailable := by
  unfold RuntimeReady
  decide
