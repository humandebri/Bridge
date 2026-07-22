use candid::{CandidType, Deserialize, Principal};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FeeRecipientConfig {
    pub owner: Principal,
    pub subaccount: Vec<u8>,
}

pub const KINIC_LEDGER_CANISTER_ID: &str = "73mez-iiaaa-aaaaq-aaasq-cai";
pub const KINIC_INDEX_CANISTER_ID: &str = "7vojr-tyaaa-aaaaq-aaatq-cai";
pub const KINIC_DECIMALS: u8 = 8;
pub const BASE_MAINNET_CHAIN_ID: u64 = 8453;
pub const OFFICIAL_EVM_RPC_CANISTER_ID: &str = "7hfb6-caaaa-aaaar-qadga-cai";

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct BridgeInitArgs {
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub evm_rpc_canister_id: Principal,
    pub custom_evm_rpc_urls: Vec<String>,
    pub base_chain_id: u64,
    pub bridge_contract: Vec<u8>,
    pub timelock_contract: Vec<u8>,
    pub ecdsa_key_name: String,
    pub ecdsa_derivation_path: Vec<Vec<u8>>,
    pub governance_ecdsa_derivation_path: Vec<Vec<u8>>,
    pub deposit_rate_limit_window_seconds: u64,
    pub deposit_rate_limit_global: u16,
    pub deposit_rate_limit_per_principal: u16,
    pub settlement_rate_limit_window_seconds: u64,
    pub settlement_rate_limit_global: u16,
    pub settlement_rate_limit_per_principal: u16,
    pub settlement_rate_limit_per_record: u16,
    pub transaction_gas_limit: u128,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub evm_liveness: EvmLivenessPolicy,
    pub eth_floor_wei: u128,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    pub governance_principal: Principal,
    pub pause_principal: Principal,
    pub fee_recipient: FeeRecipientConfig,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvmLivenessPolicy {
    pub check_interval_seconds: u64,
    pub rebroadcast_after_seconds: u64,
    pub replacement_after_seconds: u64,
    pub max_replacements: u8,
    pub fee_bump_bps: u16,
    pub fee_ceiling_multiplier_bps: u32,
}

impl Default for EvmLivenessPolicy {
    fn default() -> Self {
        Self {
            check_interval_seconds: 60,
            rebroadcast_after_seconds: 300,
            replacement_after_seconds: 1_800,
            max_replacements: 3,
            fee_bump_bps: 1_250,
            fee_ceiling_multiplier_bps: 40_000,
        }
    }
}

impl BridgeInitArgs {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.bridge_contract.len() != 20 || self.timelock_contract.len() != 20 {
            return Err("bridge and Timelock contracts must be 20 bytes");
        }
        if self.bridge_contract.iter().all(|byte| *byte == 0)
            || self.timelock_contract.iter().all(|byte| *byte == 0)
            || self.bridge_contract == self.timelock_contract
        {
            return Err("bridge and Timelock contracts must be nonzero and distinct");
        }
        if self.base_chain_id == 0
            || self.ecdsa_key_name.is_empty()
            || self.governance_ecdsa_derivation_path.is_empty()
            || self.ecdsa_derivation_path == self.governance_ecdsa_derivation_path
        {
            return Err("chain id and ECDSA key name must be configured");
        }
        #[cfg(not(feature = "test-deployment"))]
        if self.base_chain_id != BASE_MAINNET_CHAIN_ID
            || self.ledger_canister_id != kinic_ledger_canister_id()
            || self.index_canister_id != kinic_index_canister_id()
            || self.evm_rpc_canister_id != official_evm_rpc_canister_id()
        {
            return Err(
                "production builds require Base mainnet and the fixed KINIC and EVM RPC canisters",
            );
        }
        #[cfg(not(feature = "test-deployment"))]
        if self.custom_evm_rpc_urls.len() != 3 {
            return Err("production custom EVM RPC must configure exactly three providers");
        }
        #[cfg(feature = "test-deployment")]
        if !self.custom_evm_rpc_urls.is_empty() && self.custom_evm_rpc_urls.len() != 3 {
            return Err("custom EVM RPC must configure exactly three providers");
        }
        let rpc_hosts = self
            .custom_evm_rpc_urls
            .iter()
            .filter_map(|url| rpc_host(url))
            .collect::<BTreeSet<_>>();
        if rpc_hosts.len() != self.custom_evm_rpc_urls.len()
            || self
                .custom_evm_rpc_urls
                .iter()
                .any(|url| !credential_free_https(url))
        {
            return Err("custom EVM RPC providers must use distinct credential-free HTTPS hosts");
        }
        if !(60..=300).contains(&self.deposit_rate_limit_window_seconds)
            || self.deposit_rate_limit_per_principal == 0
            || self.deposit_rate_limit_per_principal > self.deposit_rate_limit_global
            || self.deposit_rate_limit_global > 100
        {
            return Err("deposit rate limit must satisfy 60 <= window <= 300 and 1 <= per-principal <= global <= 100");
        }
        if !(60..=3_600).contains(&self.settlement_rate_limit_window_seconds)
            || self.settlement_rate_limit_per_record == 0
            || self.settlement_rate_limit_per_record > self.settlement_rate_limit_per_principal
            || self.settlement_rate_limit_per_principal > self.settlement_rate_limit_global
            || self.settlement_rate_limit_global > 100
        {
            return Err("settlement rate limit must satisfy 60 <= window <= 3600 and 1 <= per-record <= per-principal <= global <= 100");
        }
        if self.transaction_gas_limit == 0 || self.max_fee_per_gas == 0 {
            return Err("transaction gas limits must be nonzero");
        }
        if self.max_priority_fee_per_gas > self.max_fee_per_gas {
            return Err("max priority fee per gas must not exceed max fee per gas");
        }
        let policy = self.evm_liveness;
        let replacement_checks = policy
            .replacement_after_seconds
            .div_ceil(policy.check_interval_seconds.max(1));
        let mut replacement_max_fee = self.max_fee_per_gas;
        let mut replacement_priority_fee = self.max_priority_fee_per_gas;
        let replacement_fees_increase = (0..policy.max_replacements).all(|_| {
            let Some((next_max_fee, next_priority_fee)) = next_replacement_fees(
                replacement_max_fee,
                replacement_priority_fee,
                self.max_fee_per_gas,
                self.max_priority_fee_per_gas,
                policy,
            ) else {
                return false;
            };
            replacement_max_fee = next_max_fee;
            replacement_priority_fee = next_priority_fee;
            true
        });
        if !(30..=300).contains(&policy.check_interval_seconds)
            || policy.rebroadcast_after_seconds < policy.check_interval_seconds
            || policy.replacement_after_seconds < policy.rebroadcast_after_seconds
            || !(1..=8).contains(&policy.max_replacements)
            || !(1_000..=5_000).contains(&policy.fee_bump_bps)
            || !(10_000..=100_000).contains(&policy.fee_ceiling_multiplier_bps)
            || replacement_checks.saturating_mul(u64::from(policy.max_replacements) + 1) > 255
            || !replacement_fees_increase
        {
            return Err("EVM liveness policy is outside the supported safety bounds");
        }
        if self.governance_principal == Principal::anonymous()
            || self.pause_principal == Principal::anonymous()
            || self.pause_principal == self.governance_principal
            || self.fee_recipient.owner == Principal::anonymous()
            || self.fee_recipient.owner == self.pause_principal
            || self.fee_recipient.owner == self.governance_principal
            || !matches!(self.fee_recipient.subaccount.len(), 0 | 32)
        {
            return Err("administrator principals and fee recipient must be valid");
        }
        Ok(())
    }

    pub const fn reserve_policy(&self) -> bridge_core::ReservePolicy {
        let replacement_fee_ceiling = replacement_fee_ceiling(
            self.max_fee_per_gas,
            self.evm_liveness.fee_ceiling_multiplier_bps,
        );
        bridge_core::ReservePolicy {
            eth_floor_wei: self.eth_floor_wei,
            cycles_floor: self.cycles_floor,
            settlement_cycle_ceiling: self.settlement_cycle_ceiling,
            transaction_gas_limit: self.transaction_gas_limit,
            max_fee_per_gas: replacement_fee_ceiling,
        }
    }

    pub fn contract_array(&self) -> [u8; 20] {
        self.bridge_contract
            .as_slice()
            .try_into()
            .expect("validated bridge contract")
    }

    pub fn timelock_array(&self) -> [u8; 20] {
        self.timelock_contract
            .as_slice()
            .try_into()
            .expect("validated Timelock contract")
    }
}

pub(crate) const fn replacement_fee_ceiling(initial: u128, multiplier_bps: u32) -> u128 {
    initial.saturating_mul(multiplier_bps as u128) / 10_000
}

pub(crate) fn next_replacement_fees(
    current_max_fee: u128,
    current_priority_fee: u128,
    initial_max_fee: u128,
    initial_priority_fee: u128,
    policy: EvmLivenessPolicy,
) -> Option<(u128, u128)> {
    let max_fee_ceiling =
        replacement_fee_ceiling(initial_max_fee, policy.fee_ceiling_multiplier_bps);
    let priority_fee_ceiling =
        replacement_fee_ceiling(initial_priority_fee, policy.fee_ceiling_multiplier_bps)
            .min(max_fee_ceiling);
    let next_max_fee = bump_fee(current_max_fee, max_fee_ceiling, policy.fee_bump_bps);
    if next_max_fee <= current_max_fee {
        return None;
    }
    let next_priority_fee = bump_fee(
        current_priority_fee,
        priority_fee_ceiling,
        policy.fee_bump_bps,
    )
    .min(next_max_fee);
    Some((next_max_fee, next_priority_fee))
}

fn bump_fee(current: u128, ceiling: u128, bump_bps: u16) -> u128 {
    current
        .saturating_mul(10_000u128.saturating_add(u128::from(bump_bps)))
        .saturating_add(9_999)
        .checked_div(10_000)
        .unwrap_or(ceiling)
        .max(current.saturating_add(1))
        .min(ceiling)
}

pub fn kinic_ledger_canister_id() -> Principal {
    Principal::from_text(KINIC_LEDGER_CANISTER_ID)
        .expect("the fixed KINIC ledger canister ID must be valid")
}

pub fn kinic_index_canister_id() -> Principal {
    Principal::from_text(KINIC_INDEX_CANISTER_ID)
        .expect("the fixed KINIC index canister ID must be valid")
}

pub fn official_evm_rpc_canister_id() -> Principal {
    Principal::from_text(OFFICIAL_EVM_RPC_CANISTER_ID)
        .expect("the official EVM RPC canister ID must be valid")
}

fn credential_free_https(url: &str) -> bool {
    if !url.is_ascii()
        || url
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())
        || !url.starts_with("https://")
        || url.contains(['@', '?', '#', '\\'])
    {
        return false;
    }
    let rest = &url["https://".len()..];
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() || authority.starts_with('.') || authority.ends_with('.') {
        return false;
    }
    let (host, port) = authority.rsplit_once(':').unwrap_or((authority, ""));
    if host.is_empty()
        || !host.contains('.')
        || !host.bytes().any(|byte| byte.is_ascii_alphabetic())
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        || host
            .split('.')
            .any(|label| label.is_empty() || label.starts_with('-') || label.ends_with('-'))
        || (authority.contains(':')
            && (port.is_empty() || port.parse::<u16>().ok().is_none_or(|value| value == 0)))
    {
        return false;
    }
    const PUBLIC_PATH_SEGMENTS: [&str; 7] = [
        "rpc",
        "v1",
        "v2",
        "ethereum",
        "base",
        "base-mainnet",
        "base-sepolia",
    ];
    let path = path.trim_end_matches('/');
    path.is_empty()
        || path
            .split('/')
            .all(|segment| PUBLIC_PATH_SEGMENTS.contains(&segment.to_ascii_lowercase().as_str()))
}

fn rpc_host(url: &str) -> Option<String> {
    credential_free_https(url).then(|| {
        let authority = url["https://".len()..]
            .split('/')
            .next()
            .expect("validated URL has an authority");
        authority
            .rsplit_once(':')
            .map(|(host, _)| host)
            .unwrap_or(authority)
            .to_ascii_lowercase()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_kinic_canister_ids_are_valid_and_distinct() {
        assert_eq!(
            kinic_ledger_canister_id().to_text(),
            KINIC_LEDGER_CANISTER_ID
        );
        assert_eq!(kinic_index_canister_id().to_text(), KINIC_INDEX_CANISTER_ID);
        assert_ne!(kinic_ledger_canister_id(), kinic_index_canister_id());
        assert_eq!(
            official_evm_rpc_canister_id().to_text(),
            OFFICIAL_EVM_RPC_CANISTER_ID
        );
    }

    #[test]
    fn rejects_priority_fee_above_max_fee() {
        let mut args = valid_args();
        args.max_priority_fee_per_gas = args.max_fee_per_gas + 1;
        assert_eq!(
            args.validate(),
            Err("max priority fee per gas must not exceed max fee per gas")
        );
    }

    #[test]
    fn replacement_policy_requires_a_distinct_fee_for_every_generation() {
        let mut args = valid_args();
        assert_eq!(args.validate(), Ok(()));

        args.evm_liveness.fee_ceiling_multiplier_bps = 10_000;
        assert!(args.validate().is_err());

        args = valid_args();
        args.max_fee_per_gas = 1;
        args.max_priority_fee_per_gas = 0;
        args.evm_liveness.fee_ceiling_multiplier_bps = 10_001;
        assert!(args.validate().is_err());

        args = valid_args();
        args.max_fee_per_gas = 1;
        args.max_priority_fee_per_gas = 0;
        args.evm_liveness.max_replacements = 2;
        args.evm_liveness.fee_bump_bps = 5_000;
        args.evm_liveness.fee_ceiling_multiplier_bps = 20_000;
        assert!(args.validate().is_err());

        args = valid_args();
        args.max_fee_per_gas = u128::MAX;
        args.max_priority_fee_per_gas = 0;
        assert!(args.validate().is_err());
    }

    #[test]
    fn replacement_fee_helper_returns_none_at_the_ceiling() {
        let policy = EvmLivenessPolicy {
            max_replacements: 2,
            fee_bump_bps: 5_000,
            fee_ceiling_multiplier_bps: 20_000,
            ..EvmLivenessPolicy::default()
        };
        assert_eq!(next_replacement_fees(1, 0, 1, 0, policy), Some((2, 0)));
        assert_eq!(next_replacement_fees(2, 0, 1, 0, policy), None);
    }

    #[test]
    fn validates_settlement_rate_limit_window_and_ordering() {
        let mut args = valid_args();
        args.settlement_rate_limit_window_seconds = 59;
        assert!(args.validate().is_err());
        args = valid_args();
        args.settlement_rate_limit_window_seconds = 3_601;
        assert!(args.validate().is_err());
        args = valid_args();
        args.settlement_rate_limit_per_record = 7;
        assert!(args.validate().is_err());
        args = valid_args();
        args.settlement_rate_limit_per_principal = 61;
        assert!(args.validate().is_err());
        args = valid_args();
        args.settlement_rate_limit_global = 101;
        assert!(args.validate().is_err());
    }

    #[test]
    fn contracts_must_be_nonzero_and_distinct() {
        let mut args = valid_args();
        args.bridge_contract = vec![0; 20];
        assert!(args.validate().is_err());
        args = valid_args();
        args.timelock_contract = vec![0; 20];
        assert!(args.validate().is_err());
        args = valid_args();
        args.timelock_contract = args.bridge_contract.clone();
        assert!(args.validate().is_err());
    }

    #[test]
    fn administrator_roles_are_pairwise_distinct() {
        let mut args = valid_args();
        args.fee_recipient.owner = args.governance_principal;
        assert!(args.validate().is_err());
    }
    fn valid_args() -> BridgeInitArgs {
        let principal = Principal::from_text("aaaaa-aa").expect("management principal");
        BridgeInitArgs {
            ledger_canister_id: kinic_ledger_canister_id(),
            index_canister_id: kinic_index_canister_id(),
            evm_rpc_canister_id: official_evm_rpc_canister_id(),
            custom_evm_rpc_urls: vec![
                "https://one.example".into(),
                "https://two.example".into(),
                "https://three.example".into(),
            ],
            base_chain_id: BASE_MAINNET_CHAIN_ID,
            bridge_contract: vec![1; 20],
            timelock_contract: vec![2; 20],
            ecdsa_key_name: "key_1".into(),
            ecdsa_derivation_path: vec![],
            governance_ecdsa_derivation_path: vec![b"governance-operator".to_vec()],
            deposit_rate_limit_window_seconds: 60,
            deposit_rate_limit_global: 10,
            deposit_rate_limit_per_principal: 2,
            settlement_rate_limit_window_seconds: 600,
            settlement_rate_limit_global: 60,
            settlement_rate_limit_per_principal: 6,
            settlement_rate_limit_per_record: 3,
            transaction_gas_limit: 500_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 1,
            evm_liveness: EvmLivenessPolicy::default(),
            eth_floor_wei: 1,
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: principal,
            pause_principal: Principal::from_slice(&[2]),
            fee_recipient: FeeRecipientConfig {
                owner: Principal::from_slice(&[3]),
                subaccount: vec![],
            },
        }
    }

    #[cfg(not(feature = "test-deployment"))]
    #[test]
    fn production_build_rejects_non_kinic_or_non_base_configuration() {
        let mut args = valid_args();
        assert_eq!(args.validate(), Ok(()));

        args.base_chain_id = 31_337;
        assert!(args.validate().is_err());
        args = valid_args();
        args.ledger_canister_id = Principal::anonymous();
        assert!(args.validate().is_err());
        args = valid_args();
        args.index_canister_id = Principal::anonymous();
        assert!(args.validate().is_err());
        args = valid_args();
        args.evm_rpc_canister_id = Principal::management_canister();
        assert!(args.validate().is_err());
        args = valid_args();
        args.custom_evm_rpc_urls.clear();
        assert!(args.validate().is_err());
    }

    #[test]
    fn custom_rpc_urls_must_be_distinct_credential_free_https() {
        let valid = vec![
            "https://one.example".into(),
            "https://two.example/rpc".into(),
            "https://three.example".into(),
        ];
        let mut args = valid_args();
        args.custom_evm_rpc_urls = valid.clone();
        assert_eq!(args.validate(), Ok(()));
        args.custom_evm_rpc_urls = vec![
            "https://one.example:443/rpc".into(),
            "https://two.example:8443/v1".into(),
            "https://three.example:9443".into(),
        ];
        assert_eq!(args.validate(), Ok(()));

        for invalid in [
            vec![valid[0].clone(), valid[0].to_uppercase(), valid[2].clone()],
            vec![
                "https://one.example/rpc".into(),
                "https://one.example/v1".into(),
                valid[2].clone(),
            ],
            vec![
                "https://one.example:443/rpc".into(),
                "https://one.example:8443/v1".into(),
                valid[2].clone(),
            ],
            vec![
                "http://one.example".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://user@example.invalid".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://one.example?token=secret".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://one.example/opaque-secret-1234567890".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://one.example\\rpc".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://one.example:0/rpc".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://-one.example/rpc".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://one.example/rpc\n".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
            vec![
                "https://例.example/rpc".into(),
                valid[1].clone(),
                valid[2].clone(),
            ],
        ] {
            args = valid_args();
            args.custom_evm_rpc_urls = invalid;
            assert!(args.validate().is_err());
        }
    }

    #[cfg(feature = "test-deployment")]
    #[test]
    fn test_deployment_build_accepts_local_bindings() {
        let mut args = valid_args();
        args.base_chain_id = 31_337;
        args.ledger_canister_id = Principal::anonymous();
        args.index_canister_id = Principal::management_canister();
        args.evm_rpc_canister_id = Principal::management_canister();
        assert_eq!(args.validate(), Ok(()));
    }
}
