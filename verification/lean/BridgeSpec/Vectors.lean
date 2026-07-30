import BridgeSpec.Refinement

namespace BridgeSpec.Vectors

open BridgeSpec

def boolJson : Bool → String
  | true => "true"
  | false => "false"

def quoted (value : String) : String := String.singleton '"' ++ value ++ String.singleton '"'

def field (name value : String) : String := quoted name ++ ":" ++ value

def stringField (name value : String) : String := field name (quoted value)

def natField (name : String) (value : Nat) : String := stringField name (toString value)

def backedBool (state : EconomicState) : Bool :=
  state.escrow == state.baseSupply + state.feeReserve + state.unpaidLiability

def quoteCase (amount serviceFee : Nat) : String :=
  let result := commit amount serviceFee { owner := [], subaccount := [] }
  let accepted := result.isSome
  let amountOut := match result with | some value => quoted (toString value.amountOut) | none => "null"
  "{" ++ natField "amount" amount ++ "," ++ natField "service_fee" serviceFee ++ "," ++
    field "accepted" (boolJson accepted) ++ "," ++ field "amount_out" amountOut ++ "}"

def settlementCase
    (amountOut ledgerFee serviceFee escrow baseSupply feeReserve unpaidLiability : Nat) : String :=
  let before : EconomicState := { escrow, baseSupply, feeReserve, unpaidLiability }
  let result := checkedSettlement before amountOut serviceFee ledgerFee
  let accepted := result.isSome
  let after := result.getD before
  let arithmetic := outboundSettlement amountOut ledgerFee serviceFee
  let escrowDebit := arithmetic.map (fun value => value.1) |>.getD 0
  let reserveCredit := arithmetic.map (fun value => value.2.1) |>.getD 0
  let liabilityDebit := arithmetic.map (fun value => value.2.2) |>.getD 0
  "{" ++ natField "amount_out" amountOut ++ "," ++ natField "ledger_fee" ledgerFee ++ "," ++
    natField "service_fee" serviceFee ++ "," ++ natField "before_escrow" escrow ++ "," ++
    natField "before_base_supply" baseSupply ++ "," ++
    natField "before_fee_reserve" feeReserve ++ "," ++
    natField "before_unpaid_liability" unpaidLiability ++ "," ++
    field "before_backed" (boolJson (backedBool before)) ++ "," ++
    field "accepted" (boolJson accepted) ++ "," ++ natField "escrow_debit" escrowDebit ++ "," ++
    natField "reserve_credit" reserveCredit ++ "," ++ natField "liability_debit" liabilityDebit ++
    "," ++ natField "after_escrow" after.escrow ++ "," ++
    natField "after_base_supply" after.baseSupply ++ "," ++
    natField "after_fee_reserve" after.feeReserve ++ "," ++
    natField "after_unpaid_liability" after.unpaidLiability ++ "," ++
    field "after_backed" (boolJson (backedBool after)) ++ "}"

def paymentCase
    (alreadyPaid : Bool) (amountOut chargedFee transferAmount transferFee : Nat)
    (destinationMatches : Bool) : String :=
  let destination : Account := { owner := [1], subaccount := [2] }
  let other : Account := { owner := [3], subaccount := [4] }
  let withdrawal : Withdrawal :=
    { amount := amountOut + chargedFee, chargedServiceFee := chargedFee,
      amountOut, destination, paid := alreadyPaid }
  let transfer : LedgerTransfer :=
    { amount := transferAmount, ledgerFee := transferFee,
      destination := if destinationMatches then destination else other }
  let result := pay withdrawal transfer
  "{" ++ field "already_paid" (boolJson alreadyPaid) ++ "," ++ natField "amount_out" amountOut ++
    "," ++ natField "charged_fee" chargedFee ++ "," ++
    natField "transfer_amount" transferAmount ++ "," ++ natField "transfer_fee" transferFee ++ "," ++
    field "destination_matches" (boolJson destinationMatches) ++ "," ++
    field "accepted" (boolJson result.isSome) ++ "}"

def depositAdmissionCase
    (serviceFee maximumServiceFee gross perLimit minted windowLimit : Nat) : String :=
  let admission : DepositAdmission :=
    { serviceFee, maximumServiceFee, grossAmount := gross, perDepositLimit := perLimit,
      mintedInWindow := minted, mintWindowLimit := windowLimit }
  let result := admitDeposit admission
  let net := match result with | some value => quoted (toString value) | none => "null"
  "{" ++ natField "service_fee" serviceFee ++ "," ++
    natField "maximum_service_fee" maximumServiceFee ++ "," ++ natField "gross" gross ++ "," ++
    natField "per_deposit_limit" perLimit ++ "," ++ natField "minted_in_window" minted ++ "," ++
    natField "mint_window_limit" windowLimit ++ "," ++
    field "accepted" (boolJson result.isSome) ++ "," ++ field "net" net ++ "}"

def reservationCase
    (beforeReserved beforeCandidate afterReserved afterCandidate : Nat) : String :=
  let committed := commitMintReservation beforeReserved beforeCandidate
  let accepted :=
    committed = (afterReserved, afterCandidate) ∧
      beforeReserved + beforeCandidate = afterReserved + afterCandidate
  "{" ++ natField "before_reserved" beforeReserved ++ "," ++
    natField "before_candidate" beforeCandidate ++ "," ++ natField "after_reserved" afterReserved ++
    "," ++ natField "after_candidate" afterCandidate ++ "," ++
    field "accepted" (boolJson (decide accepted)) ++ "}"

def serviceFeeCase (serviceFee maximum : Nat) : String :=
  "{" ++ natField "service_fee" serviceFee ++ "," ++ natField "maximum" maximum ++ "," ++
    field "accepted" (boolJson (serviceFeeChangeAllowed serviceFee maximum)) ++ "}"

def feeRotationCase (reserve depositFees withdrawalFees pending recipient nextRecipient : Nat) :
    String :=
  let before : FeeState :=
    { reserve, confirmedDepositFees := depositFees, confirmedWithdrawalFees := withdrawalFees,
      pendingPayout := pending, recipient }
  let result := rotateFeeRecipient before nextRecipient
  let after := result.getD before
  "{" ++ natField "before_reserve" reserve ++ "," ++ natField "before_deposit_fees" depositFees ++
    "," ++ natField "before_withdrawal_fees" withdrawalFees ++ "," ++
    natField "pending" pending ++ "," ++ natField "before_recipient" recipient ++ "," ++
    natField "next_recipient" nextRecipient ++ "," ++
    field "accepted" (boolJson result.isSome) ++ "," ++
    natField "after_reserve" after.reserve ++ "," ++
    natField "after_deposit_fees" after.confirmedDepositFees ++ "," ++
    natField "after_withdrawal_fees" after.confirmedWithdrawalFees ++ "," ++
    natField "after_recipient" after.recipient ++ "}"

def feePayoutCase (reserve pending amount fee : Nat) : String :=
  "{" ++ natField "reserve" reserve ++ "," ++ natField "pending" pending ++ "," ++
    natField "amount" amount ++ "," ++ natField "fee" fee ++ "," ++
    field "allowed" (boolJson (feePayoutAllowed reserve pending amount fee)) ++ "," ++
    natField "first_debit" (payoutDebit true amount fee) ++ "," ++
    natField "replay_debit" (payoutDebit false amount fee) ++ "}"

def holdCase (success absence : Bool) : String :=
  "{" ++ field "success" (boolJson success) ++ "," ++
    field "absence" (boolJson absence) ++ "," ++
    field "allowed" (boolJson (holdRetryAllowed success absence)) ++ "}"

def leaseCase (active : Bool) (current outcome : Nat) : String :=
  "{" ++ field "active" (boolJson active) ++ "," ++ natField "current" current ++ "," ++
    natField "outcome" outcome ++ "," ++
    field "accepted" (boolJson (leaseOutcomeCurrent active current outcome)) ++ "}"

def manualClaimCase
    (scheduled active stopped overdue expired : Bool) : String :=
  "{" ++ field "scheduled" (boolJson scheduled) ++ "," ++ field "active" (boolJson active) ++ "," ++
    field "stopped" (boolJson stopped) ++ "," ++ field "overdue" (boolJson overdue) ++ "," ++
    field "expired" (boolJson expired) ++ "," ++
    field "allowed"
      (boolJson (manualClaimAllowed scheduled active stopped overdue expired)) ++ "}"

def refundRequestIdentityDecisionName : RefundRequestIdentityDecision → String
  | .allow => "allow"
  | .ownerLookupRequired => "owner-lookup-required"
  | .anonymousCaller => "anonymous-caller"
  | .ownerMismatch => "owner-mismatch"

def refundRequestIdentityCase
    (authenticated : Bool) (ownerMatch : Option Bool) : String :=
  let ownerMatchJson := match ownerMatch with
    | none => "null"
    | some value => boolJson value
  "{" ++ field "authenticated" (boolJson authenticated) ++ "," ++
    field "owner_match" ownerMatchJson ++ "," ++
    stringField "decision"
      (refundRequestIdentityDecisionName
        (decideRefundRequestIdentity authenticated ownerMatch)) ++ "}"

def notificationAdmissionCase
    (globalCount callerCount globalLimit callerLimit : Nat) : String :=
  "{" ++ natField "global_count" globalCount ++ "," ++
    natField "caller_count" callerCount ++ "," ++ natField "global_limit" globalLimit ++ "," ++
    natField "caller_limit" callerLimit ++ "," ++
    field "allowed"
      (boolJson (notificationAdmissionAllowed globalCount callerCount globalLimit callerLimit)) ++ "}"

def leaseLaneDecisionName : LeaseLaneClaimDecision → String
  | .allow => "allow"
  | .automaticProgressPending => "automatic-progress-pending"
  | .busy => "busy"

def leaseLaneCase
    (targetActive targetAutomatic : Bool) (activeInLane capacity : Nat) : String :=
  let decision :=
    decideLeaseLaneClaim targetActive targetAutomatic activeInLane capacity
  "{" ++ field "target_active" (boolJson targetActive) ++ "," ++
    field "target_automatic" (boolJson targetAutomatic) ++ "," ++
    natField "active_in_lane" activeInLane ++ "," ++ natField "capacity" capacity ++ "," ++
    stringField "decision" (leaseLaneDecisionName decision) ++ "}"

def fundingDecisionName : FundingAttemptDecision → String
  | .promoteSuccess => "promote-success"
  | .promoteAmbiguous => "promote-ambiguous"
  | .release => "release"
  | .retain => "retain"

def fundingAttemptCase (outcomeKind : Nat) (outcome : FundingOutcomeKind) : String :=
  "{" ++ natField "outcome_kind" outcomeKind ++ "," ++
    stringField "decision" (fundingDecisionName (decideFundingAttempt outcome)) ++ "}"

def fundingReconciliationDecisionName : FundingReconciliationDecision → String
  | .wait => "wait"
  | .restartFresh => "restart-fresh"
  | .release => "release"

def fundingReconciliationCase
    (completeAbsence finalScan dedupExpired : Bool) : String :=
  "{" ++ field "complete_absence" (boolJson completeAbsence) ++ "," ++
    field "final_scan" (boolJson finalScan) ++ "," ++
    field "dedup_expired" (boolJson dedupExpired) ++ "," ++
    stringField "decision"
      (fundingReconciliationDecisionName
        (decideFundingReconciliation completeAbsence finalScan dedupExpired)) ++ "}"

def decisionName : WithdrawalFinalizationDecision → String
  | .retry => "retry"
  | .notify => "notify"
  | .discardReverted => "discard-reverted"

def finalizationCase (succeeded : Bool) (receiptBlock : Nat) (finalized : Option Nat) : String :=
  let finalizedJson := match finalized with | some value => quoted (toString value) | none => "null"
  let decision := decisionName (decideWithdrawalFinalization succeeded receiptBlock finalized)
  "{" ++ field "receipt_succeeded" (boolJson succeeded) ++ "," ++
    natField "receipt_block" receiptBlock ++ "," ++ field "finalized_block" finalizedJson ++ "," ++
    stringField "decision" decision ++ "}"

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
  "{" ++ field "existing_blocked" existingJson ++ "," ++
    field "incoming_blocked" (boolJson incomingBlocked) ++ "," ++
    field "other_blocked" (boolJson otherBlocked) ++ "," ++
    field "expected_blocked" (boolJson expectedBlocked) ++ "," ++
    field "expected_other_blocked" (boolJson preservedOther) ++ "}"

def canonicalProbeCase (receiptBlock snapshotBlock : Nat) : String :=
  "{" ++ natField "receipt_block" receiptBlock ++ "," ++ natField "snapshot_block" snapshotBlock ++
    "," ++ field "accepted" (boolJson (canonicalProbeMatches receiptBlock snapshotBlock)) ++ "}"

def join (values : List String) : String := String.intercalate "," values

def jsonSection (name : String) (values : List String) : String :=
  field (name ++ "_cases") ("[" ++ join values ++ "]") ++ "," ++
    field (name ++ "_count") (toString values.length)

def document : String :=
  let max := 340282366920938463463374607431768211455
  let quotes := [quoteCase 100 10, quoteCase 10 10, quoteCase 9 10, quoteCase 1 0]
  let settlements := [
    settlementCase 90 1 10 105 0 5 100,
    settlementCase 90 10 10 105 0 5 100,
    settlementCase 90 11 10 105 0 5 100,
    settlementCase 90 1 10 104 0 5 99,
    settlementCase 0 0 0 0 0 0 0,
    settlementCase max 0 0 max 0 0 max]
  let payments := [
    paymentCase false 90 10 90 1 true,
    paymentCase false 90 10 89 1 true,
    paymentCase false 90 10 90 11 true,
    paymentCase false 90 10 90 1 false,
    paymentCase true 90 10 90 1 true]
  let deposits := [
    depositAdmissionCase 10 10 100 90 10 100,
    depositAdmissionCase 11 10 100 90 0 100,
    depositAdmissionCase 10 10 100 89 0 100,
    depositAdmissionCase 10 10 100 90 11 100,
    depositAdmissionCase 0 0 max max 0 max]
  let reservations := [
    reservationCase 7 1 8 0,
    reservationCase 7 1 7 0,
    reservationCase max 0 max 0]
  let serviceFees := [serviceFeeCase 10 10, serviceFeeCase 11 10, serviceFeeCase 0 0]
  let feeRotations := [
    feeRotationCase 100 40 60 0 1 2,
    feeRotationCase 100 40 60 1 1 2]
  let feePayouts := [
    feePayoutCase 101 0 100 1,
    feePayoutCase 100 0 100 1,
    feePayoutCase 101 1 99 1,
    feePayoutCase max 0 max 0]
  let holds := [holdCase false false, holdCase true false, holdCase false true, holdCase true true]
  let leases := [leaseCase true 7 7, leaseCase true 7 6, leaseCase false 7 7]
  let manualClaims := [
    manualClaimCase true true false false false,
    manualClaimCase true true false true false,
    manualClaimCase true false false true false,
    manualClaimCase false true false false true,
    manualClaimCase false false true false false,
    manualClaimCase false false false false false]
  let refundRequestIdentities := [
    refundRequestIdentityCase false none,
    refundRequestIdentityCase false (some true),
    refundRequestIdentityCase true none,
    refundRequestIdentityCase true (some false),
    refundRequestIdentityCase true (some true)]
  let notificationAdmissions := [
    notificationAdmissionCase 0 0 48 6,
    notificationAdmissionCase 47 5 48 6,
    notificationAdmissionCase 48 0 48 6,
    notificationAdmissionCase 0 6 48 6]
  let leaseLanes := [
    leaseLaneCase false false 0 4,
    leaseLaneCase false false 4 4,
    leaseLaneCase true true 0 4,
    leaseLaneCase true false 0 4]
  let fundingAttempts := [
    fundingAttemptCase 0 .success,
    fundingAttemptCase 1 .duplicate,
    fundingAttemptCase 2 .ambiguous,
    fundingAttemptCase 3 .definitiveFailure,
    fundingAttemptCase 4 .retryableFailure]
  let fundingReconciliations := [
    fundingReconciliationCase false false false,
    fundingReconciliationCase false false true,
    fundingReconciliationCase false true false,
    fundingReconciliationCase false true true,
    fundingReconciliationCase true false false,
    fundingReconciliationCase true false true,
    fundingReconciliationCase true true false,
    fundingReconciliationCase true true true]
  let finalizations := [finalizationCase true 10 none, finalizationCase false 10 none,
    finalizationCase true 10 (some 9), finalizationCase false 10 (some 9),
    finalizationCase true 10 (some 10), finalizationCase false 10 (some 10)]
  let queues := [queueCase none false true, queueCase (some false) true false,
    queueCase (some true) false true]
  let probes := [canonicalProbeCase 0 0, canonicalProbeCase 42 42,
    canonicalProbeCase 42 43, canonicalProbeCase 18446744073709551615 18446744073709551615]
  "{" ++ field "schema_version" "3" ++ "," ++
    jsonSection "quote" quotes ++ "," ++ jsonSection "settlement" settlements ++ "," ++
    jsonSection "payment" payments ++ "," ++ jsonSection "deposit_admission" deposits ++ "," ++
    jsonSection "reservation" reservations ++ "," ++
    jsonSection "service_fee" serviceFees ++ "," ++
    jsonSection "fee_rotation" feeRotations ++ "," ++
    jsonSection "fee_payout" feePayouts ++ "," ++
    jsonSection "hold" holds ++ "," ++ jsonSection "lease" leases ++ "," ++
    jsonSection "manual_claim" manualClaims ++ "," ++
    jsonSection "refund_request_identity" refundRequestIdentities ++ "," ++
    jsonSection "notification_admission" notificationAdmissions ++ "," ++
    jsonSection "lease_lane" leaseLanes ++ "," ++
    jsonSection "funding_attempt" fundingAttempts ++ "," ++
    jsonSection "funding_reconciliation" fundingReconciliations ++ "," ++
    jsonSection "finalization" finalizations ++ "," ++ jsonSection "queue" queues ++ "," ++
    jsonSection "canonical_probe" probes ++ "}\n"

end BridgeSpec.Vectors
