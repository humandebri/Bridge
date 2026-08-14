use bridge_core::{
    canonical_probe_matches, committed_quote_matches, deposit_admission_decision,
    fee_recipient_rotation_decision, funding_attempt_decision, funding_reconciliation_decision,
    hold_resolution_decision, lease_lane_claim_decision, lease_outcome_decision,
    manual_claim_decision, notification_admission_allowed, payout_decision,
    release_transfer_matches, reservation_decision, service_fee_change_allowed,
    settlement_decision, withdrawal_finalized_checkpoint, withdrawal_id_is_admissible,
};

macro_rules! production_link {
    ($claim:literal, $link:literal, $value:expr, $function_type:ty) => {{
        let _: &str = $claim;
        let _: &str = $link;
        let _: $function_type = $value;
    }};
}

#[test]
fn phase5_production_links_typecheck() {
    production_link!(
        "withdrawal_admission_boundary",
        "canister/bridge-core/src/kernel.rs#withdrawal_id_is_admissible",
        withdrawal_id_is_admissible,
        fn(&[u8; 32], &[u8]) -> bool
    );
    production_link!(
        "committed_quote",
        "canister/bridge-core/src/kernel.rs#committed_quote_matches",
        committed_quote_matches,
        fn(u128, u128, u128) -> bool
    );
    production_link!(
        "settlement_backing",
        "canister/bridge-core/src/kernel.rs#settlement_decision",
        settlement_decision,
        fn(u128, u128, u128) -> Option<bridge_core::SettlementDecision>
    );
    production_link!(
        "payment_identity",
        "canister/bridge-core/src/kernel.rs#release_transfer_matches",
        release_transfer_matches,
        fn(u128, u128, u128, u128) -> bool
    );
    production_link!(
        "deposit_admission",
        "canister/bridge-core/src/kernel.rs#deposit_admission_decision",
        deposit_admission_decision,
        fn(
            u128,
            u128,
            u128,
            u128,
            u128,
            u128,
            u128,
        ) -> Option<bridge_core::DepositAdmissionDecision>
    );
    production_link!(
        "reservation_commit",
        "canister/bridge-core/src/kernel.rs#reservation_decision",
        reservation_decision,
        fn(u128, u128) -> Option<bridge_core::ReservationDecision>
    );
    production_link!(
        "service_fee_maximum",
        "canister/bridge-core/src/kernel.rs#service_fee_change_allowed",
        service_fee_change_allowed,
        fn(u128, u128) -> bool
    );
    production_link!(
        "fee_recipient_rotation",
        "canister/bridge-core/src/kernel.rs#fee_recipient_rotation_decision",
        fee_recipient_rotation_decision,
        fn(bool, bool, bool, usize, u128) -> bridge_core::FeeRecipientRotationDecision
    );
    production_link!(
        "fee_payout",
        "canister/bridge-core/src/kernel.rs#payout_decision",
        payout_decision,
        fn(u128, u128, u128, u128, bool) -> Option<bridge_core::PayoutDecision>
    );
    production_link!(
        "hold_resolution",
        "canister/bridge-core/src/kernel.rs#hold_resolution_decision",
        hold_resolution_decision,
        fn(bool, bool) -> bridge_core::HoldResolutionDecision
    );
    production_link!(
        "lease_outcome",
        "canister/bridge-core/src/kernel.rs#lease_outcome_decision",
        lease_outcome_decision,
        fn(u64, u64, bool) -> bridge_core::LeaseOutcomeDecision
    );
    production_link!(
        "manual_claim_exclusion",
        "canister/bridge-core/src/kernel.rs#manual_claim_decision",
        manual_claim_decision,
        fn(bool, bool, bool, bool, bool) -> bridge_core::ManualClaimDecision
    );
    production_link!(
        "notification_quota_isolation",
        "canister/bridge-core/src/kernel.rs#notification_admission_allowed",
        notification_admission_allowed,
        fn(u16, u16, u16, u16) -> bool
    );
    production_link!(
        "lease_lane_isolation",
        "canister/bridge-core/src/kernel.rs#lease_lane_claim_decision",
        lease_lane_claim_decision,
        fn(bool, bool, u64, u64) -> bridge_core::LeaseLaneClaimDecision
    );
    production_link!(
        "funding_attempt_lifecycle",
        "canister/bridge-core/src/kernel.rs#funding_attempt_decision",
        funding_attempt_decision,
        fn(u8) -> bridge_core::FundingAttemptDecision
    );
    production_link!(
        "funding_attempt_lifecycle",
        "canister/bridge-core/src/kernel.rs#funding_reconciliation_decision",
        funding_reconciliation_decision,
        fn(bool, bool, bool) -> bridge_core::FundingReconciliationDecision
    );
    production_link!(
        "canonical_probe",
        "canister/bridge-core/src/kernel.rs#canonical_probe_matches",
        canonical_probe_matches,
        fn(u64, u64) -> bool
    );
    production_link!(
        "withdrawal_finality_quorum",
        "canister/bridge-core/src/kernel.rs#withdrawal_finalized_checkpoint",
        withdrawal_finalized_checkpoint,
        fn(Option<u64>, Option<u64>, Option<u64>) -> Option<u64>
    );
}

#[test]
fn withdrawal_admission_boundary_uses_the_full_big_endian_uint256() {
    let mut minimum = [0u8; 32];
    minimum[15] = 1;
    let mut below = minimum;
    below[15] = 0;
    below[31] = u8::MAX;
    let mut above = minimum;
    above[31] = 1;

    assert!(!withdrawal_id_is_admissible(&below, &minimum));
    assert!(withdrawal_id_is_admissible(&minimum, &minimum));
    assert!(withdrawal_id_is_admissible(&above, &minimum));
    assert!(!withdrawal_id_is_admissible(&minimum, &[0; 32]));
    assert!(!withdrawal_id_is_admissible(&minimum, &[1; 31]));
}

#[test]
fn withdrawal_finality_quorum_requires_an_exact_two_provider_checkpoint() {
    assert_eq!(
        withdrawal_finalized_checkpoint(Some(100), Some(100), Some(102)),
        Some(100)
    );
    assert_eq!(
        withdrawal_finalized_checkpoint(Some(102), Some(100), Some(102)),
        Some(102)
    );
    assert_eq!(
        withdrawal_finalized_checkpoint(Some(100), None, Some(102)),
        None
    );
    assert_eq!(
        withdrawal_finalized_checkpoint(Some(100), Some(101), Some(102)),
        None
    );
    assert_eq!(withdrawal_finalized_checkpoint(Some(102), None, None), None);
}
