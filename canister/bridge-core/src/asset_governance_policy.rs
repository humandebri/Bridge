//! Bounded policy for additional-asset governance; legacy sealed parameters remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetGasClass {
    Legacy,
    RegistrationSchedule,
    TokenRegistration,
    Activation,
    Runtime,
}

pub fn asset_governance_gas_limit(class: AssetGasClass, legacy_limit: u128) -> u128 {
    match class {
        AssetGasClass::Legacy => legacy_limit,
        AssetGasClass::RegistrationSchedule => 500_000,
        AssetGasClass::TokenRegistration => 2_000_000,
        AssetGasClass::Activation => 500_000,
        AssetGasClass::Runtime => 200_000,
    }
}

pub fn sns_asset_proposal_allowed(
    domain_matches: bool,
    activated: bool,
    unpaused: bool,
    now_ns: u64,
    expires_at_ns: u64,
    operation_matches: bool,
) -> bool {
    domain_matches
        && activated
        && unpaused
        && operation_matches
        && expires_at_ns > now_ns
        && expires_at_ns - now_ns <= 30 * 24 * 60 * 60 * 1_000_000_000
}
