import BridgeSpec.DepositAuthorization

open BridgeSpec

example :
    decideRefundRequestIdentity false (some true) =
      RefundRequestIdentityDecision.allow := by
  decide
