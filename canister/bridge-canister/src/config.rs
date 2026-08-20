use candid::{CandidType, Deserialize, Principal};
use serde::Serialize;
use std::collections::BTreeSet;

#[cfg(feature = "test-deployment")]
use sha2::{Digest, Sha256};

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct FeeRecipientConfig {
    pub owner: Principal,
    pub subaccount: Vec<u8>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ActivationAttestation {
    pub chain_id: u64,
    pub finalized_block_number: u64,
    pub finalized_block_hash: Vec<u8>,
    pub observed_at_ns: u64,
    pub bridge_signer: Vec<u8>,
    pub bridge_runtime_sha256: Vec<u8>,
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
    pub bridge_timelock: Vec<u8>,
    pub runtime_administrator: Vec<u8>,
    pub timelock_admin: Vec<u8>,
    pub timelock_proposer: Vec<u8>,
    pub timelock_canceller: Vec<u8>,
    pub timelock_executor: Vec<u8>,
    pub timelock_runtime_code_hash: Vec<u8>,
    pub bridge_approved_timelock_runtime_code_hash: Vec<u8>,
    pub timelock_minimum_delay_seconds: u64,
    pub bsns_address: Vec<u8>,
    pub bsns_runtime_sha256: Vec<u8>,
    pub bsns_name: String,
    pub bsns_symbol: String,
    pub bsns_decimals: u8,
    pub bsns_bridge: Vec<u8>,
    pub base_service_fee: u128,
}

pub const KINIC_LEDGER_CANISTER_ID: &str = "73mez-iiaaa-aaaaq-aaasq-cai";
pub const KINIC_INDEX_CANISTER_ID: &str = "7vojr-tyaaa-aaaaq-aaatq-cai";
pub const BASE_MAINNET_CHAIN_ID: u64 = 8453;
pub const OFFICIAL_EVM_RPC_CANISTER_ID: &str = "7hfb6-caaaa-aaaar-qadga-cai";

#[cfg(feature = "test-deployment")]
pub const BASE_SEPOLIA_CHAIN_ID: u64 = 84_532;
#[cfg(feature = "test-deployment")]
pub const STAGING_OLD_RPC_URLS: [&str; 3] = [
    "https://base-sepolia-rpc.publicnode.com",
    "https://sepolia.base.org",
    "https://base-sepolia.api.onfinality.io/public",
];
#[cfg(feature = "test-deployment")]
pub const STAGING_NEW_RPC_URLS: [&str; 3] = [
    "https://base-sepolia-rpc.publicnode.com",
    "https://sepolia.base.org",
    "https://base-sepolia.drpc.org",
];
#[cfg(feature = "test-deployment")]
pub const STAGING_OLD_RPC_URLS_SHA256: [u8; 32] = [
    0x3a, 0xb5, 0x3c, 0x05, 0x32, 0xb8, 0x0b, 0x3f, 0x39, 0xed, 0x07, 0x6f, 0x96, 0x61, 0x79, 0x4c,
    0x0a, 0x84, 0x7b, 0x0d, 0x2e, 0xba, 0x18, 0x45, 0xb5, 0xc7, 0xe0, 0xed, 0x16, 0x63, 0xed, 0x48,
];
#[cfg(feature = "test-deployment")]
pub const STAGING_NEW_RPC_URLS_SHA256: [u8; 32] = [
    0xdf, 0x7e, 0x86, 0x7a, 0xaf, 0x6a, 0xbe, 0xaf, 0x00, 0xb0, 0xf6, 0x1e, 0x86, 0x62, 0xfa, 0x87,
    0xc6, 0xf8, 0x67, 0x5e, 0xb0, 0xae, 0xbd, 0xf7, 0xb0, 0x9f, 0x8c, 0x99, 0xa4, 0x99, 0xd0, 0x64,
];

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
    pub minimum_withdrawal_id: Vec<u8>,
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
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    pub governance_principal: Principal,
    pub pause_principal: Principal,
    pub confirmation_relayer_principal: Principal,
    pub fee_recipient: FeeRecipientConfig,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationalConfigArgs {
    pub governance_evm_fee: EvmFeePolicy,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
}

#[cfg(feature = "test-deployment")]
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StagingUpgradeArgs {
    pub status_counts_guard_version: u8,
    pub rpc_provider_update: Option<StagingRpcProviderUpdate>,
    pub minimum_withdrawal_id: Option<Vec<u8>>,
    pub confirmation_relayer_principal: Option<Principal>,
}

#[cfg(feature = "test-deployment")]
impl Default for StagingUpgradeArgs {
    fn default() -> Self {
        Self {
            status_counts_guard_version: 1,
            rpc_provider_update: None,
            minimum_withdrawal_id: None,
            confirmation_relayer_principal: None,
        }
    }
}

#[cfg(feature = "test-deployment")]
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StagingRpcProviderUpdate {
    pub custom_evm_rpc_urls: Vec<String>,
    pub expected_status_counts: StagingExpectedStatusCounts,
}

#[cfg(feature = "test-deployment")]
#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StagingExpectedStatusCounts {
    pub retained_audit_events: u64,
    pub reconciliation_holds: u64,
    pub retained_deposit_index_entries: u64,
    pub pending_ledger_operations: u64,
    pub withdrawals: u64,
    pub deposits: u64,
    pub reserved_deposit_mint_operations: u64,
    pub reserved_deposit_mint_amount: u128,
    pub pruned_audit_events: u64,
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
    pub minimum_withdrawal_id: Vec<u8>,
    pub ecdsa_key_name: String,
    pub ecdsa_derivation_path: Vec<Vec<u8>>,
    pub governance_ecdsa_derivation_path: Vec<Vec<u8>>,
    pub deposit_rate_limit_window_seconds: u64,
    pub deposit_rate_limit_global: u16,
    pub deposit_rate_limit_per_principal: u16,
    pub notification_rate_limit_window_seconds: u64,
    pub notification_rate_limit_global: u16,
    #[serde(default = "default_notification_ingestion_rate_limit_global")]
    pub notification_ingestion_rate_limit_global: u16,
    pub settlement_rate_limit_window_seconds: u64,
    pub settlement_rate_limit_global: u16,
    pub settlement_rate_limit_per_principal: u16,
    pub settlement_rate_limit_per_record: u16,
    pub settlement_retry_interval_seconds: u64,
    pub governance_evm_fee: EvmFeePolicy,
    pub governance_replacement: GovernanceReplacementPolicy,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    #[serde(default = "anonymous_principal")]
    pub confirmation_relayer_principal: Principal,
    #[serde(default)]
    pub activation_attestation: Option<ActivationAttestation>,
}

const fn default_notification_ingestion_rate_limit_global() -> u16 {
    30
}

fn anonymous_principal() -> Principal {
    Principal::anonymous()
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
            minimum_withdrawal_id: value.minimum_withdrawal_id.clone(),
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
            cycles_floor: value.cycles_floor,
            settlement_cycle_ceiling: value.settlement_cycle_ceiling,
            confirmation_relayer_principal: value.confirmation_relayer_principal,
            activation_attestation: None,
        }
    }

    pub(crate) fn with_activation_attestation(mut self, value: ActivationAttestation) -> Self {
        self.activation_attestation = Some(value);
        self
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
            minimum_withdrawal_id: self.minimum_withdrawal_id,
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
            cycles_floor: self.cycles_floor,
            settlement_cycle_ceiling: self.settlement_cycle_ceiling,
            governance_principal,
            pause_principal,
            confirmation_relayer_principal: self.confirmation_relayer_principal,
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
    pub fn with_operational_config(&self, value: OperationalConfigArgs) -> Self {
        let mut next = self.clone();
        next.governance_evm_fee = value.governance_evm_fee;
        next.cycles_floor = value.cycles_floor;
        next.settlement_cycle_ceiling = value.settlement_cycle_ceiling;
        next
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.bridge_contract.len() != 20
            || self.expected_bridge_runtime_sha256.len() != 32
            || self.timelock_contract.len() != 20
            || self.deployment_instance_id.len() != 32
            || self.minimum_withdrawal_id.len() != 32
        {
            return Err("bridge and Timelock contracts must be 20 bytes; deployment instance ID and minimum withdrawal ID must be 32 bytes");
        }
        if self.bridge_contract.iter().all(|byte| *byte == 0)
            || self.timelock_contract.iter().all(|byte| *byte == 0)
            || self.deployment_instance_id.iter().all(|byte| *byte == 0)
            || self.minimum_withdrawal_id.iter().all(|byte| *byte == 0)
            || self
                .expected_bridge_runtime_sha256
                .iter()
                .all(|byte| *byte == 0)
            || self.bridge_contract == self.timelock_contract
        {
            return Err("bridge, Timelock, deployment instance ID, minimum withdrawal ID, and runtime hash must be nonzero, with distinct contracts");
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
        if self.cycles_floor == 0 || self.settlement_cycle_ceiling == 0 {
            return Err("cycles limits must be non-zero");
        }
        if self.governance_principal == Principal::anonymous()
            || self.pause_principal == Principal::anonymous()
            || self.confirmation_relayer_principal == Principal::anonymous()
            || self.pause_principal == self.governance_principal
            || (!cfg!(feature = "test-deployment")
                && self.confirmation_relayer_principal == self.governance_principal)
            || self.confirmation_relayer_principal == self.pause_principal
            || self.fee_recipient.owner == Principal::anonymous()
            || self.fee_recipient.owner == self.pause_principal
            || self.fee_recipient.owner == self.governance_principal
            || self.fee_recipient.owner == self.confirmation_relayer_principal
            || !matches!(self.fee_recipient.subaccount.len(), 0 | 32)
        {
            return Err("administrator principals and fee recipient must be valid");
        }
        Ok(())
    }

    pub const fn reserve_policy(&self) -> bridge_core::ReservePolicy {
        bridge_core::ReservePolicy {
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

    #[cfg(feature = "test-deployment")]
    pub(crate) fn staging_rpc_replacement(
        &self,
        requested_urls: &[String],
    ) -> Result<Option<Self>, &'static str> {
        if self.base_chain_id != BASE_SEPOLIA_CHAIN_ID {
            return Err("staging RPC replacement requires Base Sepolia");
        }
        if self.evm_rpc_canister_id != official_evm_rpc_canister_id() {
            return Err("staging RPC replacement requires the official EVM RPC Canister");
        }
        if !rpc_urls_match(requested_urls, &STAGING_NEW_RPC_URLS)
            || rpc_urls_sha256(requested_urls) != STAGING_NEW_RPC_URLS_SHA256
        {
            return Err("staging RPC replacement only accepts the reviewed dRPC provider set");
        }
        if rpc_urls_match(&self.custom_evm_rpc_urls, &STAGING_NEW_RPC_URLS) {
            if rpc_urls_sha256(&self.custom_evm_rpc_urls) != STAGING_NEW_RPC_URLS_SHA256 {
                return Err("staging RPC replacement found an invalid current provider digest");
            }
            return Ok(None);
        }
        if !rpc_urls_match(&self.custom_evm_rpc_urls, &STAGING_OLD_RPC_URLS)
            || rpc_urls_sha256(&self.custom_evm_rpc_urls) != STAGING_OLD_RPC_URLS_SHA256
        {
            return Err("staging RPC replacement requires the reviewed OnFinality provider set");
        }

        let mut next = self.clone();
        next.custom_evm_rpc_urls = requested_urls.to_vec();
        next.validate()?;
        Ok(Some(next))
    }
}

#[cfg(feature = "test-deployment")]
fn rpc_urls_match(actual: &[String], expected: &[&str; 3]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual == expected)
}

#[cfg(feature = "test-deployment")]
fn rpc_urls_sha256(urls: &[String]) -> [u8; 32] {
    let normalized = urls.iter().map(|url| url.trim()).collect::<Vec<_>>();
    Sha256::digest(
        serde_json::to_vec(&normalized).expect("staging RPC URL serialization must succeed"),
    )
    .into()
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
    const PUBLIC_PATH_SEGMENTS: [&str; 8] = [
        "rpc",
        "v1",
        "v2",
        "public",
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
    fn administrator_roles_are_separated_except_for_the_staging_relayer() {
        let mut args = valid_args();
        args.fee_recipient.owner = args.governance_principal;
        assert!(args.validate().is_err());
        let mut args = valid_args();
        args.confirmation_relayer_principal = Principal::anonymous();
        assert!(args.validate().is_err());
        for principal in [
            valid_args().pause_principal,
            valid_args().fee_recipient.owner,
        ] {
            let mut args = valid_args();
            args.confirmation_relayer_principal = principal;
            assert!(args.validate().is_err());
        }
        let mut args = valid_args();
        args.confirmation_relayer_principal = args.governance_principal;
        if cfg!(feature = "test-deployment") {
            assert!(args.validate().is_ok());
        } else {
            assert!(args.validate().is_err());
        }
    }

    #[test]
    fn cycle_limits_must_be_nonzero() {
        let mut args = valid_args();
        args.cycles_floor = 0;
        assert_eq!(args.validate(), Err("cycles limits must be non-zero"));

        let mut args = valid_args();
        args.settlement_cycle_ceiling = 0;
        assert_eq!(args.validate(), Err("cycles limits must be non-zero"));
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
            minimum_withdrawal_id: [vec![0; 31], vec![1]].concat(),
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
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: principal,
            pause_principal: Principal::from_slice(&[2]),
            confirmation_relayer_principal: Principal::from_slice(&[5]),
            fee_recipient: FeeRecipientConfig {
                owner: Principal::from_slice(&[3]),
                subaccount: vec![],
            },
        }
    }

    #[test]
    fn withdrawal_admission_boundary_must_be_a_nonzero_uint256() {
        let mut args = valid_args();
        args.minimum_withdrawal_id = vec![0; 32];
        assert!(args.validate().is_err());

        args.minimum_withdrawal_id = vec![1; 31];
        assert!(args.validate().is_err());
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

    #[cfg(feature = "test-deployment")]
    fn reviewed_staging_args() -> BridgeInitArgs {
        let mut args = valid_args();
        args.base_chain_id = BASE_SEPOLIA_CHAIN_ID;
        args.custom_evm_rpc_urls = STAGING_OLD_RPC_URLS.map(str::to_owned).to_vec();
        args
    }

    #[cfg(feature = "test-deployment")]
    #[test]
    fn staging_rpc_replacement_is_fixed_and_idempotent() {
        let current = reviewed_staging_args();
        let requested = STAGING_NEW_RPC_URLS.map(str::to_owned).to_vec();
        let next = current
            .staging_rpc_replacement(&requested)
            .expect("reviewed replacement")
            .expect("configuration changes");
        assert_eq!(next.custom_evm_rpc_urls, requested);
        assert_eq!(next.staging_rpc_replacement(&requested), Ok(None));
        assert_eq!(
            rpc_urls_sha256(&current.custom_evm_rpc_urls),
            STAGING_OLD_RPC_URLS_SHA256
        );
        assert_eq!(
            rpc_urls_sha256(&next.custom_evm_rpc_urls),
            STAGING_NEW_RPC_URLS_SHA256
        );
    }

    #[cfg(feature = "test-deployment")]
    #[test]
    fn staging_rpc_replacement_rejects_unreviewed_bindings() {
        let requested = STAGING_NEW_RPC_URLS.map(str::to_owned).to_vec();

        let mut wrong_order = requested.clone();
        wrong_order.swap(0, 1);
        assert!(reviewed_staging_args()
            .staging_rpc_replacement(&wrong_order)
            .is_err());

        let mut wrong_provider = requested.clone();
        wrong_provider[2] = "https://three.example".into();
        assert!(reviewed_staging_args()
            .staging_rpc_replacement(&wrong_provider)
            .is_err());

        let mut unknown_current = reviewed_staging_args();
        unknown_current.custom_evm_rpc_urls[2] = "https://three.example".into();
        assert!(unknown_current.staging_rpc_replacement(&requested).is_err());

        let mut wrong_chain = reviewed_staging_args();
        wrong_chain.base_chain_id = BASE_MAINNET_CHAIN_ID;
        assert!(wrong_chain.staging_rpc_replacement(&requested).is_err());

        let mut wrong_canister = reviewed_staging_args();
        wrong_canister.evm_rpc_canister_id = Principal::management_canister();
        assert!(wrong_canister.staging_rpc_replacement(&requested).is_err());
    }
}
