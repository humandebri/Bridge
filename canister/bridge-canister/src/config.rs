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
    pub expected_bridge_runtime_sha256: Vec<u8>,
    pub timelock_contract: Vec<u8>,
    pub deployment_instance_id: Vec<u8>,
    pub ecdsa_key_name: String,
    pub ecdsa_derivation_path: Vec<Vec<u8>>,
    pub governance_ecdsa_derivation_path: Vec<Vec<u8>>,
    pub deposit_rate_limit_window_seconds: u64,
    pub deposit_rate_limit_global: u16,
    pub deposit_rate_limit_per_principal: u16,
    pub notification_rate_limit_window_seconds: u64,
    pub notification_rate_limit_global: u16,
    pub notification_ingestion_rate_limit_global: u16,
    pub settlement_rate_limit_window_seconds: u64,
    pub settlement_rate_limit_global: u16,
    pub settlement_rate_limit_per_principal: u16,
    pub settlement_rate_limit_per_record: u16,
    pub settlement_retry_interval_seconds: u64,
    pub governance_evm_fee: EvmFeePolicy,
    pub governance_replacement: GovernanceReplacementPolicy,
    pub governance_eth_floor_wei: u128,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    pub governance_principal: Principal,
    pub pause_principal: Principal,
    pub fee_recipient: FeeRecipientConfig,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GovernanceReplacementPolicy {
    pub max_replacements: u8,
    pub fee_bump_bps: u16,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvmFeePolicy {
    pub gas_limit_ceiling: u128,
    pub max_fee_per_gas_ceiling: u128,
    pub max_priority_fee_per_gas_ceiling: u128,
    pub l1_fee_per_transaction_ceiling_wei: u128,
    pub quote_validity_seconds: u64,
    pub gas_limit_multiplier_bps: u32,
    pub base_fee_multiplier_bps: u32,
    pub l1_fee_multiplier_bps: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImmutableBridgeConfig {
    pub ledger_canister_id: Principal,
    pub index_canister_id: Principal,
    pub evm_rpc_canister_id: Principal,
    pub custom_evm_rpc_urls: Vec<String>,
    pub base_chain_id: u64,
    pub bridge_contract: Vec<u8>,
    pub expected_bridge_runtime_sha256: Vec<u8>,
    pub timelock_contract: Vec<u8>,
    pub deployment_instance_id: Vec<u8>,
    pub ecdsa_key_name: String,
    pub ecdsa_derivation_path: Vec<Vec<u8>>,
    pub governance_ecdsa_derivation_path: Vec<Vec<u8>>,
    pub deposit_rate_limit_window_seconds: u64,
    pub deposit_rate_limit_global: u16,
    pub deposit_rate_limit_per_principal: u16,
    pub notification_rate_limit_window_seconds: u64,
    pub notification_rate_limit_global: u16,
    pub notification_ingestion_rate_limit_global: u16,
    pub settlement_rate_limit_window_seconds: u64,
    pub settlement_rate_limit_global: u16,
    pub settlement_rate_limit_per_principal: u16,
    pub settlement_rate_limit_per_record: u16,
    pub settlement_retry_interval_seconds: u64,
    pub governance_evm_fee: EvmFeePolicy,
    pub governance_replacement: GovernanceReplacementPolicy,
    pub governance_eth_floor_wei: u128,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
}

impl ImmutableBridgeConfig {
    pub(crate) fn from_init(value: &BridgeInitArgs) -> Self {
        Self {
            ledger_canister_id: value.ledger_canister_id,
            index_canister_id: value.index_canister_id,
            evm_rpc_canister_id: value.evm_rpc_canister_id,
            custom_evm_rpc_urls: value.custom_evm_rpc_urls.clone(),
            base_chain_id: value.base_chain_id,
            bridge_contract: value.bridge_contract.clone(),
            expected_bridge_runtime_sha256: value.expected_bridge_runtime_sha256.clone(),
            timelock_contract: value.timelock_contract.clone(),
            deployment_instance_id: value.deployment_instance_id.clone(),
            ecdsa_key_name: value.ecdsa_key_name.clone(),
            ecdsa_derivation_path: value.ecdsa_derivation_path.clone(),
            governance_ecdsa_derivation_path: value.governance_ecdsa_derivation_path.clone(),
            deposit_rate_limit_window_seconds: value.deposit_rate_limit_window_seconds,
            deposit_rate_limit_global: value.deposit_rate_limit_global,
            deposit_rate_limit_per_principal: value.deposit_rate_limit_per_principal,
            notification_rate_limit_window_seconds: value.notification_rate_limit_window_seconds,
            notification_rate_limit_global: value.notification_rate_limit_global,
            notification_ingestion_rate_limit_global: value
                .notification_ingestion_rate_limit_global,
            settlement_rate_limit_window_seconds: value.settlement_rate_limit_window_seconds,
            settlement_rate_limit_global: value.settlement_rate_limit_global,
            settlement_rate_limit_per_principal: value.settlement_rate_limit_per_principal,
            settlement_rate_limit_per_record: value.settlement_rate_limit_per_record,
            settlement_retry_interval_seconds: value.settlement_retry_interval_seconds,
            governance_evm_fee: value.governance_evm_fee,
            governance_replacement: value.governance_replacement,
            governance_eth_floor_wei: value.governance_eth_floor_wei,
            cycles_floor: value.cycles_floor,
            settlement_cycle_ceiling: value.settlement_cycle_ceiling,
        }
    }

    pub(crate) fn with_admin(
        self,
        governance_principal: Principal,
        pause_principal: Principal,
        fee_recipient: FeeRecipientConfig,
    ) -> BridgeInitArgs {
        BridgeInitArgs {
            ledger_canister_id: self.ledger_canister_id,
            index_canister_id: self.index_canister_id,
            evm_rpc_canister_id: self.evm_rpc_canister_id,
            custom_evm_rpc_urls: self.custom_evm_rpc_urls,
            base_chain_id: self.base_chain_id,
            bridge_contract: self.bridge_contract,
            expected_bridge_runtime_sha256: self.expected_bridge_runtime_sha256,
            timelock_contract: self.timelock_contract,
            deployment_instance_id: self.deployment_instance_id,
            ecdsa_key_name: self.ecdsa_key_name,
            ecdsa_derivation_path: self.ecdsa_derivation_path,
            governance_ecdsa_derivation_path: self.governance_ecdsa_derivation_path,
            deposit_rate_limit_window_seconds: self.deposit_rate_limit_window_seconds,
            deposit_rate_limit_global: self.deposit_rate_limit_global,
            deposit_rate_limit_per_principal: self.deposit_rate_limit_per_principal,
            notification_rate_limit_window_seconds: self.notification_rate_limit_window_seconds,
            notification_rate_limit_global: self.notification_rate_limit_global,
            notification_ingestion_rate_limit_global: self.notification_ingestion_rate_limit_global,
            settlement_rate_limit_window_seconds: self.settlement_rate_limit_window_seconds,
            settlement_rate_limit_global: self.settlement_rate_limit_global,
            settlement_rate_limit_per_principal: self.settlement_rate_limit_per_principal,
            settlement_rate_limit_per_record: self.settlement_rate_limit_per_record,
            settlement_retry_interval_seconds: self.settlement_retry_interval_seconds,
            governance_evm_fee: self.governance_evm_fee,
            governance_replacement: self.governance_replacement,
            governance_eth_floor_wei: self.governance_eth_floor_wei,
            cycles_floor: self.cycles_floor,
            settlement_cycle_ceiling: self.settlement_cycle_ceiling,
            governance_principal,
            pause_principal,
            fee_recipient,
        }
    }
}

impl Default for GovernanceReplacementPolicy {
    fn default() -> Self {
        Self {
            max_replacements: 3,
            fee_bump_bps: 1_250,
        }
    }
}

impl BridgeInitArgs {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.bridge_contract.len() != 20
            || self.expected_bridge_runtime_sha256.len() != 32
            || self.timelock_contract.len() != 20
            || self.deployment_instance_id.len() != 32
        {
            return Err("bridge and Timelock contracts must be 20 bytes and deployment instance ID must be 32 bytes");
        }
        if self.bridge_contract.iter().all(|byte| *byte == 0)
            || self.timelock_contract.iter().all(|byte| *byte == 0)
            || self.deployment_instance_id.iter().all(|byte| *byte == 0)
            || self
                .expected_bridge_runtime_sha256
                .iter()
                .all(|byte| *byte == 0)
            || self.bridge_contract == self.timelock_contract
        {
            return Err("bridge, Timelock, and deployment instance ID must be nonzero, with distinct contracts");
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
        if !self.custom_evm_rpc_urls.is_empty() {
            return Err("production builds use the built-in Base mainnet EVM RPC providers");
        }
        #[cfg(feature = "test-deployment")]
        if (self.custom_evm_rpc_urls.is_empty() && self.base_chain_id != BASE_MAINNET_CHAIN_ID)
            || (!self.custom_evm_rpc_urls.is_empty() && self.custom_evm_rpc_urls.len() != 3)
        {
            return Err(
                "test deployments require built-in Base mainnet or exactly three custom providers",
            );
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
        if !(60..=3_600).contains(&self.notification_rate_limit_window_seconds)
            || !(1..=100).contains(&self.notification_rate_limit_global)
            || !(1..=100).contains(&self.notification_ingestion_rate_limit_global)
        {
            return Err(
                "notification rate limit must satisfy 60 <= window <= 3600 and 1 <= global <= 100",
            );
        }
        if !(60..=3_600).contains(&self.settlement_rate_limit_window_seconds)
            || self.settlement_rate_limit_per_record == 0
            || self.settlement_rate_limit_per_record > self.settlement_rate_limit_per_principal
            || self.settlement_rate_limit_per_principal > self.settlement_rate_limit_global
            || self.settlement_rate_limit_global > 100
            || !(1..=900).contains(&self.settlement_retry_interval_seconds)
        {
            return Err("settlement policy must satisfy 60 <= rate window <= 3600, 1 <= per-record <= per-principal <= global <= 100, and 1 <= retry interval <= 900");
        }
        let fee = self.governance_evm_fee;
        if fee.gas_limit_ceiling == 0
            || fee.max_fee_per_gas_ceiling == 0
            || fee.max_priority_fee_per_gas_ceiling > fee.max_fee_per_gas_ceiling
            || fee.l1_fee_per_transaction_ceiling_wei == 0
            || !(30..=300).contains(&fee.quote_validity_seconds)
            || !(10_000..=20_000).contains(&fee.gas_limit_multiplier_bps)
            || !(10_000..=100_000).contains(&fee.base_fee_multiplier_bps)
            || !(10_000..=30_000).contains(&fee.l1_fee_multiplier_bps)
        {
            return Err("EVM fee policy is outside the supported safety bounds");
        }
        let policy = self.governance_replacement;
        if !(1..=8).contains(&policy.max_replacements)
            || !(1_000..=5_000).contains(&policy.fee_bump_bps)
        {
            return Err("governance replacement policy is outside the supported safety bounds");
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
        bridge_core::ReservePolicy {
            governance_eth_floor_wei: self.governance_eth_floor_wei,
            cycles_floor: self.cycles_floor,
            settlement_cycle_ceiling: self.settlement_cycle_ceiling,
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
        args.governance_evm_fee.max_priority_fee_per_gas_ceiling =
            args.governance_evm_fee.max_fee_per_gas_ceiling + 1;
        assert!(args.validate().is_err());
    }

    #[test]
    fn fee_policy_rejects_invalid_safety_bounds() {
        let mut args = valid_args();
        assert_eq!(args.validate(), Ok(()));
        args.governance_evm_fee.quote_validity_seconds = 0;
        assert!(args.validate().is_err());
        args = valid_args();
        args.governance_evm_fee.gas_limit_multiplier_bps = 9_999;
        assert!(args.validate().is_err());
        args = valid_args();
        args.governance_evm_fee.l1_fee_per_transaction_ceiling_wei = 0;
        assert!(args.validate().is_err());
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
        args = valid_args();
        args.settlement_retry_interval_seconds = 0;
        assert!(args.validate().is_err());
        args = valid_args();
        args.settlement_retry_interval_seconds = 901;
        assert!(args.validate().is_err());
    }

    #[test]
    fn validates_notification_rate_limit_bounds() {
        let mut args = valid_args();
        args.notification_rate_limit_window_seconds = 59;
        assert!(args.validate().is_err());
        args = valid_args();
        args.notification_rate_limit_window_seconds = 3_601;
        assert!(args.validate().is_err());
        args = valid_args();
        args.notification_rate_limit_global = 0;
        assert!(args.validate().is_err());
        args = valid_args();
        args.notification_rate_limit_global = 101;
        assert!(args.validate().is_err());
        args = valid_args();
        args.notification_ingestion_rate_limit_global = 0;
        assert!(args.validate().is_err());
        args = valid_args();
        args.notification_ingestion_rate_limit_global = 101;
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
        args = valid_args();
        args.expected_bridge_runtime_sha256 = vec![0; 32];
        assert!(args.validate().is_err());
        args = valid_args();
        args.expected_bridge_runtime_sha256 = vec![4; 31];
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
            custom_evm_rpc_urls: vec![],
            base_chain_id: BASE_MAINNET_CHAIN_ID,
            bridge_contract: vec![1; 20],
            expected_bridge_runtime_sha256: vec![4; 32],
            timelock_contract: vec![2; 20],
            deployment_instance_id: vec![3; 32],
            ecdsa_key_name: "key_1".into(),
            ecdsa_derivation_path: vec![],
            governance_ecdsa_derivation_path: vec![b"governance-operator".to_vec()],
            deposit_rate_limit_window_seconds: 60,
            deposit_rate_limit_global: 10,
            deposit_rate_limit_per_principal: 2,
            notification_rate_limit_window_seconds: 600,
            notification_rate_limit_global: 60,
            notification_ingestion_rate_limit_global: 30,
            settlement_rate_limit_window_seconds: 600,
            settlement_rate_limit_global: 60,
            settlement_rate_limit_per_principal: 6,
            settlement_rate_limit_per_record: 3,
            settlement_retry_interval_seconds: 60,
            governance_evm_fee: EvmFeePolicy {
                gas_limit_ceiling: 500_000,
                max_fee_per_gas_ceiling: 200_000_000_000,
                max_priority_fee_per_gas_ceiling: 10_000_000_000,
                l1_fee_per_transaction_ceiling_wei: 10_000_000_000_000_000,
                quote_validity_seconds: 90,
                gas_limit_multiplier_bps: 13_000,
                base_fee_multiplier_bps: 60_000,
                l1_fee_multiplier_bps: 15_000,
            },
            governance_replacement: GovernanceReplacementPolicy::default(),
            governance_eth_floor_wei: 1,
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
        args.custom_evm_rpc_urls = vec![
            "https://one.example".into(),
            "https://two.example".into(),
            "https://three.example".into(),
        ];
        assert!(args.validate().is_err());
    }

    #[cfg(feature = "test-deployment")]
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
        assert!(args.validate().is_err());
        args.custom_evm_rpc_urls = vec![
            "https://one.example".into(),
            "https://two.example".into(),
            "https://three.example".into(),
        ];
        assert_eq!(args.validate(), Ok(()));
    }
}
