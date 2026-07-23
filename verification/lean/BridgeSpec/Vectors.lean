import BridgeSpec.Model
import BridgeSpec.Theorems

namespace BridgeSpec.Vectors

open BridgeSpec

def boolJson : Bool → String
  | true => "true"
  | false => "false"

def quoted (value : String) : String := String.singleton '"' ++ value ++ String.singleton '"'

def quoteCase (amount serviceFee : Nat) : String :=
  let result := commit amount serviceFee { owner := [], subaccount := [] }
  let accepted := result.isSome
  let amountOut := match result with | some value => toString value.amountOut | none => "null"
  "{" ++ quoted "amount" ++ ":" ++ quoted (toString amount) ++ "," ++
    quoted "service_fee" ++ ":" ++ quoted (toString serviceFee) ++ "," ++
    quoted "accepted" ++ ":" ++ boolJson accepted ++ "," ++ quoted "amount_out" ++ ":" ++
    (if accepted then quoted amountOut else amountOut) ++ "}"

def settlementCase (amountOut ledgerFee serviceFee : Nat) : String :=
  match outboundSettlement amountOut ledgerFee serviceFee with
  | some (escrowDebit, reserveCredit, liabilityDebit) =>
      "{" ++ quoted "amount_out" ++ ":" ++ quoted (toString amountOut) ++ "," ++
        quoted "ledger_fee" ++ ":" ++ quoted (toString ledgerFee) ++ "," ++
        quoted "service_fee" ++ ":" ++ quoted (toString serviceFee) ++ "," ++
        quoted "accepted" ++ ":true," ++ quoted "escrow_debit" ++ ":" ++
        quoted (toString escrowDebit) ++ "," ++ quoted "reserve_credit" ++ ":" ++
        quoted (toString reserveCredit) ++ "," ++ quoted "liability_debit" ++ ":" ++
        quoted (toString liabilityDebit) ++ "}"
  | none =>
      "{" ++ quoted "amount_out" ++ ":" ++ quoted (toString amountOut) ++ "," ++
        quoted "ledger_fee" ++ ":" ++ quoted (toString ledgerFee) ++ "," ++
        quoted "service_fee" ++ ":" ++ quoted (toString serviceFee) ++ "," ++
        quoted "accepted" ++ ":false," ++ quoted "escrow_debit" ++ ":" ++ quoted "0" ++ "," ++
        quoted "reserve_credit" ++ ":" ++ quoted "0" ++ "," ++ quoted "liability_debit" ++
        ":" ++ quoted "0" ++ "}"

def decisionName : WithdrawalFinalizationDecision → String
  | .retry => "retry"
  | .notify => "notify"
  | .discardReverted => "discard-reverted"

def finalizationCase (succeeded : Bool) (receiptBlock : Nat) (finalized : Option Nat) : String :=
  let finalizedJson := match finalized with | some value => quoted (toString value) | none => "null"
  let decision := decisionName (decideWithdrawalFinalization succeeded receiptBlock finalized)
  "{" ++ quoted "receipt_succeeded" ++ ":" ++ boolJson succeeded ++ "," ++
    quoted "receipt_block" ++ ":" ++ quoted (toString receiptBlock) ++ "," ++
    quoted "finalized_block" ++ ":" ++ finalizedJson ++ "," ++
    quoted "decision" ++ ":" ++ quoted decision ++ "}"

def queueCase (existingBlocked : Option Bool) (incomingBlocked otherBlocked : Bool) : String :=
  let queue : PendingQueue := fun key =>
    if key = 1 then existingBlocked.map (fun blocked => { key := 1, owner := 7, blocked })
    else if key = 2 then some { key := 2, owner := 8, blocked := otherBlocked }
    else none
  let incoming : PendingQueueEntry := { key := 1, owner := 9, blocked := incomingBlocked }
  let restored := restorePendingQueue queue incoming
  let expectedBlocked := (restored 1).map (fun entry => entry.blocked) |>.getD false
  let preservedOther := (restored 2).map (fun entry => entry.blocked) |>.getD false
  let existingJson := match existingBlocked with | some value => boolJson value | none => "null"
  "{" ++ quoted "existing_blocked" ++ ":" ++ existingJson ++ "," ++
    quoted "incoming_blocked" ++ ":" ++ boolJson incomingBlocked ++ "," ++
    quoted "other_blocked" ++ ":" ++ boolJson otherBlocked ++ "," ++
    quoted "expected_blocked" ++ ":" ++ boolJson expectedBlocked ++ "," ++
    quoted "expected_other_blocked" ++ ":" ++ boolJson preservedOther ++ "}"

def confirmationKindName : ConfirmationKind → String
  | .deposit => "deposit"
  | .withdrawal => "withdrawal"

def notificationFailureName : NotificationFailure → String
  | .ledgerFeeExceedsServiceFee => "ledger-fee-exceeds-service-fee"
  | .other => "other"

def feeGuardPendingCase
    (kind : ConfirmationKind) (failure : NotificationFailure) (durableSucceeded : Bool) : String :=
  let queue : PendingQueue := fun key =>
    if key = 1 then some { key := 1, owner := 7, blocked := false }
    else if key = 2 then some { key := 2, owner := 8, blocked := true }
    else none
  let result := handleNotificationFailure queue 1 kind failure durableSucceeded
  let retainPending := match result with
    | some outcome => (outcome.write.session 1).isSome
    | none => false
  let targetPresent := match result with
    | some outcome => (outcome.write.session 1).isSome
    | none => (queue 1).isSome
  let otherPresent := match result with
    | some outcome => (outcome.write.session 2).isSome
    | none => (queue 2).isSome
  let historyRefresh := match result with
    | some outcome => outcome.historyRefresh
    | none => false
  let complete := match result with
    | some outcome => outcome.complete
    | none => false
  "{" ++ quoted "kind" ++ ":" ++ quoted (confirmationKindName kind) ++ "," ++
    quoted "failure" ++ ":" ++ quoted (notificationFailureName failure) ++ "," ++
    quoted "durable_succeeded" ++ ":" ++ boolJson durableSucceeded ++ "," ++
    quoted "retain_pending" ++ ":" ++ boolJson retainPending ++ "," ++
    quoted "target_present" ++ ":" ++ boolJson targetPresent ++ "," ++
    quoted "other_present" ++ ":" ++ boolJson otherPresent ++ "," ++
    quoted "history_refresh" ++ ":" ++ boolJson historyRefresh ++ "," ++
    quoted "complete" ++ ":" ++ boolJson complete ++ "}"

def canonicalProbeCase (receiptBlock snapshotBlock : Nat) : String :=
  "{" ++ quoted "receipt_block" ++ ":" ++ quoted (toString receiptBlock) ++ "," ++
    quoted "snapshot_block" ++ ":" ++ quoted (toString snapshotBlock) ++ "," ++
    quoted "accepted" ++ ":" ++ boolJson (canonicalProbeMatches receiptBlock snapshotBlock) ++ "}"

def join (values : List String) : String := String.intercalate "," values

def document : String :=
  let quotes := [quoteCase 100 10, quoteCase 10 10, quoteCase 9 10, quoteCase 1 0]
  let settlements := [settlementCase 90 1 10, settlementCase 90 10 10,
    settlementCase 90 11 10, settlementCase 0 0 0]
  let finalizations := [finalizationCase true 10 none, finalizationCase false 10 none,
    finalizationCase true 10 (some 9), finalizationCase false 10 (some 9),
    finalizationCase true 10 (some 10), finalizationCase false 10 (some 10)]
  let queues := [queueCase none false true, queueCase (some false) true false,
    queueCase (some true) false true]
  let feeGuardPending := [
    feeGuardPendingCase .withdrawal .ledgerFeeExceedsServiceFee true,
    feeGuardPendingCase .withdrawal .ledgerFeeExceedsServiceFee false,
    feeGuardPendingCase .withdrawal .other true,
    feeGuardPendingCase .deposit .ledgerFeeExceedsServiceFee true]
  let canonicalProbes := [
    canonicalProbeCase 0 0, canonicalProbeCase 42 42, canonicalProbeCase 42 43,
    canonicalProbeCase 18446744073709551615 18446744073709551615]
  "{" ++ quoted "schema_version" ++ ":2," ++ quoted "quote_cases" ++ ":[" ++
    join quotes ++ "]," ++ quoted "quote_count" ++ ":" ++ toString quotes.length ++ "," ++
    quoted "settlement_cases" ++ ":[" ++ join settlements ++ "]," ++
    quoted "settlement_count" ++ ":" ++ toString settlements.length ++ "," ++
    quoted "finalization_cases" ++ ":[" ++ join finalizations ++ "]," ++
    quoted "finalization_count" ++ ":" ++ toString finalizations.length ++ "," ++
    quoted "queue_cases" ++ ":[" ++ join queues ++ "]," ++ quoted "queue_count" ++ ":" ++
    toString queues.length ++ "," ++ quoted "fee_guard_pending_cases" ++ ":[" ++
    join feeGuardPending ++ "]," ++ quoted "fee_guard_pending_count" ++ ":" ++
    toString feeGuardPending.length ++ "," ++ quoted "canonical_probe_cases" ++ ":[" ++
    join canonicalProbes ++ "]," ++ quoted "canonical_probe_count" ++ ":" ++
    toString canonicalProbes.length ++ "}\n"

end BridgeSpec.Vectors
