import BridgeSpec.DepositAuthorization

open BridgeSpec

example :
    decideRefundRequestIdentity true none =
      RefundRequestIdentityDecision.allow := by
  decide
