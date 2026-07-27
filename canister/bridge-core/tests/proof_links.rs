use bridge_core::{
    canonical_probe_matches, committed_quote_matches, deposit_admission_decision,
    fee_recipient_rotation_decision, funding_attempt_decision, hold_resolution_decision,
    lease_lane_claim_decision, lease_outcome_decision, manual_claim_decision,
    notification_admission_allowed, payout_decision, release_transfer_matches,
    reservation_decision, service_fee_change_allowed, settlement_decision,
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
        fn(bool, bool, bool, bool, bool, bool) -> bridge_core::ManualClaimDecision
    );
    production_link!(
        "notification_quota_isolation",
        "canister/bridge-core/src/kernel.rs#notification_admission_allowed",
        notification_admission_allowed,
        fn(u8, u8, u8, u8) -> bool
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
        "canonical_probe",
        "canister/bridge-core/src/kernel.rs#canonical_probe_matches",
        canonical_probe_matches,
        fn(u64, u64) -> bool
    );
}
