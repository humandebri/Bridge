import BridgeSpec.Protocol

open BridgeSpec.Protocol

example : holdEvidenceValid 1 2 3 (.completeAbsence {
    requestIdentity := 1
    holdIdentity := 2
    transferIdentity := 3
    startIndex := 0
    entries := []
    next := 0
    tip := 0
    watermark := 0
  }) = true := by
  decide
