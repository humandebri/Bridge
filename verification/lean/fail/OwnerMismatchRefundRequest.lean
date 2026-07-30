import BridgeSpec.DepositAuthorization

open BridgeSpec

example :
    decideRefundRequestIdentity true (some false) =
      RefundRequestIdentityDecision.allow := by
  decide
