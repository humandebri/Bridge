use bridge_core::{
    canonical_probe_matches, committed_quote_matches, fee_recipient_rotation_allowed,
    funding_attempt_decision, hold_retry_allowed, lease_lane_claim_decision,
    lease_outcome_is_current, manual_claim_allowed, notification_admission_allowed,
    outbound_settlement, payout_allowed, payout_debit, release_transfer_matches,
    reserve_admission_preserves_requirement, restored_pending_blocked, service_fee_change_allowed,
    withdrawal_finalization_decision, Amount, BaseMintSnapshot, FundingAttemptDecision,
    LeaseLaneClaimDecision, WithdrawalFinalizationDecision,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolVectors {
    schema_version: u8,
    quote_cases: Vec<QuoteCase>,
    quote_count: usize,
    settlement_cases: Vec<SettlementCase>,
    settlement_count: usize,
    payment_cases: Vec<PaymentCase>,
    payment_count: usize,
    deposit_admission_cases: Vec<DepositAdmissionCase>,
    deposit_admission_count: usize,
    reservation_cases: Vec<ReservationCase>,
    reservation_count: usize,
    service_fee_cases: Vec<ServiceFeeCase>,
    service_fee_count: usize,
    fee_rotation_cases: Vec<FeeRotationCase>,
    fee_rotation_count: usize,
    fee_payout_cases: Vec<FeePayoutCase>,
    fee_payout_count: usize,
    hold_cases: Vec<HoldCase>,
    hold_count: usize,
    lease_cases: Vec<LeaseCase>,
    lease_count: usize,
    manual_claim_cases: Vec<ManualClaimCase>,
    manual_claim_count: usize,
    notification_admission_cases: Vec<NotificationAdmissionCase>,
    notification_admission_count: usize,
    lease_lane_cases: Vec<LeaseLaneCase>,
    lease_lane_count: usize,
    funding_attempt_cases: Vec<FundingAttemptCase>,
    funding_attempt_count: usize,
    finalization_cases: Vec<FinalizationCase>,
    finalization_count: usize,
    queue_cases: Vec<QueueCase>,
    queue_count: usize,
    canonical_probe_cases: Vec<CanonicalProbeCase>,
    canonical_probe_count: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuoteCase {
    amount: String,
    service_fee: String,
    accepted: bool,
    amount_out: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettlementCase {
    amount_out: String,
    ledger_fee: String,
    service_fee: String,
    before_escrow: String,
    before_base_supply: String,
    before_fee_reserve: String,
    before_unpaid_liability: String,
    before_backed: bool,
    accepted: bool,
    escrow_debit: String,
    reserve_credit: String,
    liability_debit: String,
    after_escrow: String,
    after_base_supply: String,
    after_fee_reserve: String,
    after_unpaid_liability: String,
    after_backed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaymentCase {
    already_paid: bool,
    amount_out: String,
    charged_fee: String,
    transfer_amount: String,
    transfer_fee: String,
    destination_matches: bool,
    accepted: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DepositAdmissionCase {
    service_fee: String,
    maximum_service_fee: String,
    gross: String,
    per_deposit_limit: String,
    minted_in_window: String,
    mint_window_limit: String,
    accepted: bool,
    net: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationCase {
    before_reserved: String,
    before_candidate: String,
    after_reserved: String,
    after_candidate: String,
    accepted: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceFeeCase {
    service_fee: String,
    maximum: String,
    accepted: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeeRotationCase {
    before_reserve: String,
    before_deposit_fees: String,
    before_withdrawal_fees: String,
    pending: String,
    before_recipient: String,
    next_recipient: String,
    accepted: bool,
    after_reserve: String,
    after_deposit_fees: String,
    after_withdrawal_fees: String,
    after_recipient: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeePayoutCase {
    reserve: String,
    pending: String,
    amount: String,
    fee: String,
    allowed: bool,
    first_debit: String,
    replay_debit: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HoldCase {
    success: bool,
    absence: bool,
    allowed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseCase {
    active: bool,
    current: String,
    outcome: String,
    accepted: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManualClaimCase {
    scheduled: bool,
    active: bool,
    stopped: bool,
    overdue: bool,
    expired: bool,
    allowed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NotificationAdmissionCase {
    caller_count: String,
    hash_count: String,
    caller_limit: String,
    hash_limit: String,
    allowed: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseLaneCase {
    target_active: bool,
    target_automatic: bool,
    active_in_lane: String,
    capacity: String,
    decision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FundingAttemptCase {
    outcome_kind: String,
    decision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinalizationCase {
    receipt_succeeded: bool,
    receipt_block: String,
    finalized_block: Option<String>,
    decision: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueueCase {
    existing_blocked: Option<bool>,
    incoming_blocked: bool,
    other_blocked: bool,
    expected_blocked: bool,
    expected_other_blocked: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CanonicalProbeCase {
    receipt_block: String,
    snapshot_block: String,
    accepted: bool,
}

fn vectors() -> ProtocolVectors {
    let vectors: ProtocolVectors = serde_json::from_str(include_str!(
        "../../../verification/generated/protocol-vectors.json"
    ))
    .expect("Lean protocol vectors must match schema v2");
    assert_eq!(vectors.schema_version, 2);
    assert_eq!(vectors.quote_count, vectors.quote_cases.len());
    assert_eq!(vectors.settlement_count, vectors.settlement_cases.len());
    assert_eq!(vectors.payment_count, vectors.payment_cases.len());
    assert_eq!(
        vectors.deposit_admission_count,
        vectors.deposit_admission_cases.len()
    );
    assert_eq!(vectors.reservation_count, vectors.reservation_cases.len());
    assert_eq!(vectors.service_fee_count, vectors.service_fee_cases.len());
    assert_eq!(vectors.fee_rotation_count, vectors.fee_rotation_cases.len());
    assert_eq!(vectors.fee_payout_count, vectors.fee_payout_cases.len());
    assert_eq!(vectors.hold_count, vectors.hold_cases.len());
    assert_eq!(vectors.lease_count, vectors.lease_cases.len());
    assert_eq!(vectors.manual_claim_count, vectors.manual_claim_cases.len());
    assert_eq!(
        vectors.notification_admission_count,
        vectors.notification_admission_cases.len()
    );
    assert_eq!(vectors.lease_lane_count, vectors.lease_lane_cases.len());
    assert_eq!(
        vectors.funding_attempt_count,
        vectors.funding_attempt_cases.len()
    );
    assert_eq!(vectors.finalization_count, vectors.finalization_cases.len());
    assert_eq!(vectors.queue_count, vectors.queue_cases.len());
    assert_eq!(
        vectors.canonical_probe_count,
        vectors.canonical_probe_cases.len()
    );
    assert!(vectors.quote_count > 0);
    assert!(vectors.settlement_count > 0);
    assert!(vectors.payment_count > 0);
    assert!(vectors.deposit_admission_count > 0);
    assert!(vectors.reservation_count > 0);
    assert!(vectors.service_fee_count > 0);
    assert!(vectors.fee_rotation_count > 0);
    assert!(vectors.fee_payout_count > 0);
    assert!(vectors.hold_count > 0);
    assert!(vectors.lease_count > 0);
    assert!(vectors.manual_claim_count > 0);
    assert!(vectors.notification_admission_count > 0);
    assert!(vectors.lease_lane_count > 0);
    assert!(vectors.funding_attempt_count > 0);
    assert!(vectors.finalization_count > 0);
    assert!(vectors.queue_count > 0);
    assert!(vectors.canonical_probe_count > 0);
    for case in &vectors.finalization_cases {
        assert!(!case.receipt_block.is_empty());
        assert!(case
            .finalized_block
            .as_deref()
            .is_none_or(|value| !value.is_empty()));
        assert!(matches!(
            case.decision.as_str(),
            "retry" | "notify" | "discard-reverted"
        ));
        let _ = case.receipt_succeeded;
    }
    for case in &vectors.queue_cases {
        let _ = (
            case.existing_blocked,
            case.incoming_blocked,
            case.other_blocked,
            case.expected_blocked,
            case.expected_other_blocked,
        );
    }
    vectors
}

fn amount(value: &str) -> u128 {
    value.parse().expect("vector amount must be canonical u128")
}

fn block(value: &str) -> u64 {
    value.parse().expect("vector block must be canonical u64")
}

fn backed(state: (u128, u128, u128, u128)) -> bool {
    state
        .1
        .checked_add(state.2)
        .and_then(|value| value.checked_add(state.3))
        == Some(state.0)
}

#[test]
fn protocol_quote_cases_matches_production() {
    for case in vectors().quote_cases {
        let amount_value = amount(&case.amount);
        let service_fee = amount(&case.service_fee);
        let actual = case
            .amount_out
            .as_deref()
            .map(amount)
            .is_some_and(|amount_out| {
                committed_quote_matches(amount_value, amount_out, service_fee)
            });
        assert_eq!(
            actual, case.accepted,
            "quote amount={amount_value} fee={service_fee}"
        );
    }
}

#[test]
fn protocol_settlement_cases_matches_production() {
    for case in vectors().settlement_cases {
        let amount_out = amount(&case.amount_out);
        let ledger_fee = amount(&case.ledger_fee);
        let service_fee = amount(&case.service_fee);
        let arithmetic = outbound_settlement(amount_out, ledger_fee, service_fee);
        let expected_arithmetic = arithmetic.unwrap_or((0, 0, 0));
        assert_eq!(
            expected_arithmetic,
            (
                amount(&case.escrow_debit),
                amount(&case.reserve_credit),
                amount(&case.liability_debit),
            ),
        );
        let before = (
            amount(&case.before_escrow),
            amount(&case.before_base_supply),
            amount(&case.before_fee_reserve),
            amount(&case.before_unpaid_liability),
        );
        assert_eq!(backed(before), case.before_backed);
        let checked_after = arithmetic.and_then(|_| {
            Some((
                before.0.checked_sub(amount_out.checked_add(ledger_fee)?)?,
                before.1,
                before
                    .2
                    .checked_add(service_fee)
                    .and_then(|value| value.checked_sub(ledger_fee))?,
                before.3.checked_sub(amount_out.checked_add(service_fee)?)?,
            ))
        });
        let accepted = case.before_backed && checked_after.is_some();
        assert_eq!(accepted, case.accepted);
        let after = checked_after
            .filter(|_| case.before_backed)
            .unwrap_or(before);
        assert_eq!(
            after,
            (
                amount(&case.after_escrow),
                amount(&case.after_base_supply),
                amount(&case.after_fee_reserve),
                amount(&case.after_unpaid_liability),
            )
        );
        assert_eq!(backed(after), case.after_backed);
        if case.accepted {
            assert!(case.before_backed);
            assert!(case.after_backed);
        } else {
            assert_eq!(after, before);
        }
    }
}

#[test]
fn protocol_payment_cases_matches_production() {
    for case in vectors().payment_cases {
        let accepted = !case.already_paid
            && case.destination_matches
            && amount(&case.transfer_fee) <= amount(&case.charged_fee)
            && release_transfer_matches(
                amount(&case.transfer_amount),
                amount(&case.transfer_fee),
                amount(&case.amount_out),
                amount(&case.transfer_fee),
            );
        assert_eq!(accepted, case.accepted);
    }
}

#[test]
fn protocol_deposit_admission_cases_matches_production() {
    for case in vectors().deposit_admission_cases {
        let maximum = amount(&case.maximum_service_fee);
        let snapshot = BaseMintSnapshot {
            finalized_head_block_number: 1,
            confirmed_block_timestamp: 0,
            service_fee: Amount::new(amount(&case.service_fee)),
            max_service_fee: Amount::new(maximum),
            per_deposit_limit: Amount::new(amount(&case.per_deposit_limit)),
            mint_window_limit: Amount::new(amount(&case.mint_window_limit)),
            mint_window_started_at: 0,
            mint_window_duration: u64::MAX,
            minted_in_window: Amount::new(amount(&case.minted_in_window)),
        };
        let actual = snapshot
            .quote(Amount::new(amount(&case.gross)), Amount::new(maximum))
            .ok()
            .map(|value| value.get());
        assert_eq!(actual.is_some(), case.accepted);
        assert_eq!(actual, case.net.as_deref().map(amount));
    }
}

#[test]
fn protocol_reservation_cases_matches_production() {
    for case in vectors().reservation_cases {
        let before_reserved = amount(&case.before_reserved);
        let before_candidate = amount(&case.before_candidate);
        let after_reserved = amount(&case.after_reserved);
        let after_candidate = amount(&case.after_candidate);
        let exact_commit = before_reserved
            .checked_add(before_candidate)
            .is_some_and(|committed| committed == after_reserved && after_candidate == 0);
        let actual = exact_commit
            && reserve_admission_preserves_requirement(
                before_reserved,
                before_candidate,
                after_reserved,
                after_candidate,
            );
        assert_eq!(actual, case.accepted);
    }
}

#[test]
fn protocol_service_fee_cases_matches_production() {
    for case in vectors().service_fee_cases {
        assert_eq!(
            service_fee_change_allowed(amount(&case.service_fee), amount(&case.maximum)),
            case.accepted
        );
    }
}

#[test]
fn protocol_fee_rotation_cases_matches_production() {
    for case in vectors().fee_rotation_cases {
        let accepted = fee_recipient_rotation_allowed(amount(&case.pending));
        assert_eq!(accepted, case.accepted);
        if accepted {
            assert_eq!(case.before_reserve, case.after_reserve);
            assert_eq!(case.before_deposit_fees, case.after_deposit_fees);
            assert_eq!(case.before_withdrawal_fees, case.after_withdrawal_fees);
            assert_eq!(case.next_recipient, case.after_recipient);
        } else {
            assert_eq!(case.before_recipient, case.after_recipient);
        }
    }
}

#[test]
fn protocol_fee_payout_cases_matches_production() {
    for case in vectors().fee_payout_cases {
        let reserve = amount(&case.reserve);
        let pending = amount(&case.pending);
        let payout_amount = amount(&case.amount);
        let fee = amount(&case.fee);
        assert_eq!(
            payout_allowed(reserve, pending, payout_amount, fee),
            case.allowed
        );
        assert_eq!(
            payout_debit(true, payout_amount, fee),
            Some(amount(&case.first_debit))
        );
        assert_eq!(
            payout_debit(false, payout_amount, fee),
            Some(amount(&case.replay_debit))
        );
    }
}

#[test]
fn protocol_hold_cases_matches_production() {
    for case in vectors().hold_cases {
        assert_eq!(hold_retry_allowed(case.success, case.absence), case.allowed);
    }
}

#[test]
fn protocol_lease_cases_matches_production() {
    for case in vectors().lease_cases {
        assert_eq!(
            lease_outcome_is_current(
                amount(&case.current)
                    .try_into()
                    .expect("generation fits u64"),
                amount(&case.outcome)
                    .try_into()
                    .expect("generation fits u64"),
                case.active,
            ),
            case.accepted
        );
    }
}

#[test]
fn protocol_manual_claim_cases_matches_production() {
    for case in vectors().manual_claim_cases {
        assert_eq!(
            manual_claim_allowed(
                case.scheduled,
                case.active,
                case.stopped,
                case.overdue,
                case.expired,
            ),
            case.allowed
        );
    }
}

#[test]
fn protocol_notification_admission_cases_matches_production() {
    for case in vectors().notification_admission_cases {
        assert_eq!(
            notification_admission_allowed(
                block(&case.caller_count)
                    .try_into()
                    .expect("caller count fits u8"),
                block(&case.hash_count)
                    .try_into()
                    .expect("hash count fits u8"),
                block(&case.caller_limit)
                    .try_into()
                    .expect("caller limit fits u8"),
                block(&case.hash_limit)
                    .try_into()
                    .expect("hash limit fits u8"),
            ),
            case.allowed
        );
    }
}

#[test]
fn protocol_lease_lane_cases_matches_production() {
    for case in vectors().lease_lane_cases {
        let actual = lease_lane_claim_decision(
            case.target_active,
            case.target_automatic,
            block(&case.active_in_lane),
            block(&case.capacity),
        );
        let expected = match case.decision.as_str() {
            "allow" => LeaseLaneClaimDecision::Allow,
            "automatic-progress-pending" => LeaseLaneClaimDecision::AutomaticProgressPending,
            "busy" => LeaseLaneClaimDecision::Busy,
            value => panic!("unknown lease lane decision: {value}"),
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn protocol_funding_attempt_cases_matches_production() {
    for case in vectors().funding_attempt_cases {
        let actual = funding_attempt_decision(
            block(&case.outcome_kind)
                .try_into()
                .expect("funding outcome kind fits u8"),
        );
        let expected = match case.decision.as_str() {
            "promote-success" => FundingAttemptDecision::PromoteSuccess,
            "promote-ambiguous" => FundingAttemptDecision::PromoteAmbiguous,
            "release" => FundingAttemptDecision::Release,
            "retain" => FundingAttemptDecision::Retain,
            value => panic!("unknown funding attempt decision: {value}"),
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn protocol_finalization_cases_matches_rust_decision() {
    for case in vectors().finalization_cases {
        let decision = withdrawal_finalization_decision(
            case.receipt_succeeded,
            block(&case.receipt_block),
            case.finalized_block.as_deref().map(block),
        );
        let expected = match case.decision.as_str() {
            "retry" => WithdrawalFinalizationDecision::Retry,
            "notify" => WithdrawalFinalizationDecision::Notify,
            "discard-reverted" => WithdrawalFinalizationDecision::DiscardReverted,
            value => panic!("unknown finalization decision: {value}"),
        };
        assert_eq!(decision, expected);
    }
}

#[test]
fn protocol_queue_cases_matches_rust_decision() {
    for case in vectors().queue_cases {
        assert_eq!(
            restored_pending_blocked(case.existing_blocked, case.incoming_blocked),
            case.expected_blocked
        );
        assert_eq!(case.other_blocked, case.expected_other_blocked);
    }
}

#[test]
fn protocol_canonical_probe_cases_matches_production() {
    for case in vectors().canonical_probe_cases {
        assert_eq!(
            canonical_probe_matches(block(&case.receipt_block), block(&case.snapshot_block)),
            case.accepted
        );
    }
}
