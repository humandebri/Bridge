use bridge_core::{canonical_probe_matches, committed_quote_matches, outbound_settlement};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolVectors {
    schema_version: u8,
    quote_cases: Vec<QuoteCase>,
    quote_count: usize,
    settlement_cases: Vec<SettlementCase>,
    settlement_count: usize,
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
    accepted: bool,
    escrow_debit: String,
    reserve_credit: String,
    liability_debit: String,
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
    assert_eq!(vectors.finalization_count, vectors.finalization_cases.len());
    assert_eq!(vectors.queue_count, vectors.queue_cases.len());
    assert_eq!(
        vectors.canonical_probe_count,
        vectors.canonical_probe_cases.len()
    );
    assert!(vectors.quote_count > 0);
    assert!(vectors.settlement_count > 0);
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
        let actual = outbound_settlement(
            amount(&case.amount_out),
            amount(&case.ledger_fee),
            amount(&case.service_fee),
        );
        let expected = case.accepted.then(|| {
            (
                amount(&case.escrow_debit),
                amount(&case.reserve_credit),
                amount(&case.liability_debit),
            )
        });
        assert_eq!(actual, expected);
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
