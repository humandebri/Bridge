use serde_json::Value;

const SECTIONS: &[&str] = &[
    "quote_cases",
    "settlement_cases",
    "payment_cases",
    "deposit_admission_cases",
    "deposit_identity_cases",
    "reservation_cases",
    "service_fee_cases",
    "fee_rotation_cases",
    "fee_payout_cases",
    "hold_cases",
    "lease_cases",
    "manual_claim_cases",
    "refund_request_identity_cases",
    "deposit_nonterminal_index_cases",
    "notification_admission_cases",
    "lease_lane_cases",
    "funding_attempt_cases",
    "funding_reconciliation_cases",
    "finalization_cases",
    "queue_cases",
    "canonical_probe_cases",
    "ledger_block_provenance_cases",
];

fn vectors() -> Value {
    serde_json::from_str(include_str!(
        "../../../verification/generated/protocol-vectors.json"
    ))
    .expect("Lean protocol vectors must be valid JSON")
}

#[test]
fn protocol_vector_schema_is_current_complete_and_nonempty() {
    let document = vectors();
    let object = document
        .as_object()
        .expect("protocol vectors must be a JSON object");
    assert_eq!(document["schema_version"].as_u64(), Some(3));
    assert_eq!(object.len(), 1 + SECTIONS.len() * 2);
    for section in SECTIONS {
        let cases = document[*section]
            .as_array()
            .unwrap_or_else(|| panic!("{section} must be an array"));
        assert!(!cases.is_empty(), "{section} must be nonempty");
        let count = section
            .strip_suffix("_cases")
            .map(|prefix| format!("{prefix}_count"))
            .expect("registered section must end in _cases");
        assert_eq!(document[&count].as_u64(), Some(cases.len() as u64));
    }
}
