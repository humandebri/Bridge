use bridge_core::{committed_quote_matches, outbound_settlement};
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

fn vectors() -> ProtocolVectors {
    let vectors: ProtocolVectors = serde_json::from_str(include_str!(
        "../../../verification/generated/protocol-vectors.json"
    ))
    .expect("Lean protocol vectors must match schema v1");
    assert_eq!(vectors.schema_version, 1);
    assert_eq!(vectors.quote_count, vectors.quote_cases.len());
    assert_eq!(vectors.settlement_count, vectors.settlement_cases.len());
    assert_eq!(vectors.finalization_count, vectors.finalization_cases.len());
    assert_eq!(vectors.queue_count, vectors.queue_cases.len());
    assert!(vectors.quote_count > 0);
    assert!(vectors.settlement_count > 0);
    assert!(vectors.finalization_count > 0);
    assert!(vectors.queue_count > 0);
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

#[test]
fn production_quote_kernel_matches_lean_vectors() {
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
fn production_settlement_kernel_matches_lean_vectors() {
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
