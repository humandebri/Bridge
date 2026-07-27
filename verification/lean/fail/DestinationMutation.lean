import BridgeSpec.Protocol

open BridgeSpec

def committedAccount : Account := { owner := [], subaccount := [] }
def foreignAccount : Account := { owner := [1], subaccount := [] }
def pendingWithdrawal : Withdrawal := {
  amount := 11
  chargedServiceFee := 1
  amountOut := 10
  destination := committedAccount
  paid := false
}

example : pay pendingWithdrawal {
    amount := 10
    ledgerFee := 1
    destination := foreignAccount
  } = some { pendingWithdrawal with destination := foreignAccount, paid := true } := by
  decide
