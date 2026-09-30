use bridge_core::asset_governance_policy::*;
#[test]
fn shared_gas_is_bounded_and_legacy_is_preserved() {
    assert_eq!(
        asset_governance_gas_limit(AssetGasClass::Legacy, 114_000),
        114_000
    );
    assert!(asset_governance_gas_limit(AssetGasClass::TokenRegistration, 114_000) > 872_400);
    for class in [
        AssetGasClass::RegistrationSchedule,
        AssetGasClass::TokenRegistration,
        AssetGasClass::Activation,
        AssetGasClass::Runtime,
    ] {
        assert!(asset_governance_gas_limit(class, u128::MAX) <= 2_000_000);
    }
}
#[test]
fn sns_asset_binding_rejects_wrong_domain_state_nonce_and_deadline() {
    assert!(sns_asset_proposal_allowed(true, true, true, 10, 11, true));
    for flags in [
        (false, true, true, true),
        (true, false, true, true),
        (true, true, false, true),
        (true, true, true, false),
    ] {
        assert!(!sns_asset_proposal_allowed(
            flags.0, flags.1, flags.2, 10, 11, flags.3
        ));
    }
    for expiry in [0, 9, 10, u64::MAX] {
        assert!(!sns_asset_proposal_allowed(
            true, true, true, 10, expiry, true
        ));
    }
}
