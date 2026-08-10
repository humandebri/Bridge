import BridgeSpec.DepositAuthorization

open BridgeSpec.MintAuthorization

example : nonterminalDepositIndexed DepositPhase.refunded = true := by
  decide
