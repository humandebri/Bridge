import BridgeSpec.DepositAuthorization

open BridgeSpec.MintAuthorization

-- Deliberately false: an old Base Finalized timestamp is not the authorization clock.
example : deadlineFromIssuedAt 1800 = some (0 + authorizationTtl) := by
  decide
