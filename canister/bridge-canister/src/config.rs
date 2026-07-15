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
    pub ecdsa_key_name: String,
    pub ecdsa_derivation_path: Vec<Vec<u8>>,
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
    pub eth_floor_wei: u128,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    pub governance_principal: Principal,
    pub pause_principals: Vec<Principal>,
    pub finance_administrator: Principal,
    pub fee_recipient: FeeRecipientConfig,
}

impl BridgeInitArgs {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.bridge_contract.len() != 20 {
            return Err("bridge contract must be 20 bytes");
        }
        if self.base_chain_id == 0 || self.ecdsa_key_name.is_empty() {
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
        let rpc_urls = self
            .custom_evm_rpc_urls
            .iter()
            .map(|url| url.trim().to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if rpc_urls.len() != self.custom_evm_rpc_urls.len()
            || self
                .custom_evm_rpc_urls
                .iter()
                .any(|url| !credential_free_https(url))
        {
            return Err("custom EVM RPC providers must be distinct credential-free HTTPS URLs");
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
        {
            return Err("settlement rate limit must satisfy 60 <= window <= 3600 and 1 <= per-record <= per-principal <= global");
        }
        if self.transaction_gas_limit == 0 || self.max_fee_per_gas == 0 {
            return Err("transaction gas limits must be nonzero");
        }
        if self.max_priority_fee_per_gas > self.max_fee_per_gas {
            return Err("max priority fee per gas must not exceed max fee per gas");
        }
        if self.governance_principal == Principal::anonymous()
            || self.finance_administrator == Principal::anonymous()
            || self.pause_principals.is_empty()
            || self.pause_principals.len() > 10
            || self
                .pause_principals
                .iter()
                .any(|principal| *principal == Principal::anonymous())
            || self.fee_recipient.owner == Principal::anonymous()
            || !matches!(self.fee_recipient.subaccount.len(), 0 | 32)
        {
            return Err("administrator principals and fee recipient must be valid");
        }
        let mut principals = self.pause_principals.clone();
        principals.sort();
        principals.dedup();
        if principals.len() != self.pause_principals.len() {
            return Err("pause principals must be distinct");
        }
        Ok(())
    }

    pub const fn reserve_policy(&self) -> bridge_core::ReservePolicy {
        bridge_core::ReservePolicy {
            eth_floor_wei: self.eth_floor_wei,
            cycles_floor: self.cycles_floor,
            settlement_cycle_ceiling: self.settlement_cycle_ceiling,
            transaction_gas_limit: self.transaction_gas_limit,
            max_fee_per_gas: self.max_fee_per_gas,
        }
    }

    pub fn contract_array(&self) -> [u8; 20] {
        self.bridge_contract
            .as_slice()
            .try_into()
            .expect("validated bridge contract")
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
            ecdsa_key_name: "key_1".into(),
            ecdsa_derivation_path: vec![],
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
            eth_floor_wei: 1,
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: principal,
            pause_principals: vec![principal],
            finance_administrator: principal,
            fee_recipient: FeeRecipientConfig {
                owner: principal,
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

        for invalid in [
            vec![valid[0].clone(), valid[0].to_uppercase(), valid[2].clone()],
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
