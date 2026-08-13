import BridgeSpec.DepositAuthorization

open BridgeSpec

example :
    decideRefundRequestIdentity false =
      RefundRequestIdentityDecision.allow := by
  decide
