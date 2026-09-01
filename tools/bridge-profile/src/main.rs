#![recursion_limit = "256"]

use candid::{CandidType, Decode, Encode, Nat, Principal, Reserved};
use ic_agent::Agent;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    env, fs,
    fs::OpenOptions,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{self, Command},
    time::{SystemTime, UNIX_EPOCH},
};
use tiny_keccak::{Hasher, Keccak};

const KINIC_LEDGER: &str = "73mez-iiaaa-aaaaq-aaasq-cai";
const KINIC_INDEX: &str = "7vojr-tyaaa-aaaaq-aaatq-cai";
const KINIC_ROOT: &str = "7jkta-eyaaa-aaaaq-aaarq-cai";
const KINIC_GOVERNANCE: &str = "74ncn-fqaaa-aaaaq-aaasa-cai";
const OFFICIAL_EVM_RPC_CANISTER: &str = "7hfb6-caaaa-aaaar-qadga-cai";
const MAX_EVIDENCE_AGE_SECS: u64 = 90 * 24 * 60 * 60;
const MAX_ACTIVATION_ATTESTATION_AGE_SECS: u64 = 5 * 60;
const CURRENT_STABLE_SCHEMA_VERSION: u16 = 35;
const RELEASE_PROFILE_SCHEMA_VERSION: u8 = 5;
const PRODUCTION_CANISTER_INSTALL_RECEIPT_SCHEMA_VERSION: u8 = 3;
const GATE_A_ARTIFACTS: [&str; 6] = [
    "profile.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "bsns-creation.bin",
    "bsns-runtime.bin",
    "bsns-runtime-layout.json",
];
const GATE_B_ARTIFACTS: [&str; 13] = [
    "profile.json",
    "rpc-e2e.json",
    "monitor-drill.json",
    "initial-operational-parameters.json",
    "provider-independence.json",
    "ui-assets.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "bsns-creation.bin",
    "bsns-runtime.bin",
    "bsns-runtime-layout.json",
    "gate-a-receipt.json",
    "post-gate-a-policy-transition.json",
];

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema_version: u8,
    environment: String,
    test_assets_only: bool,
    chain_id: u64,
    evm_rpc_canister_id: String,
    ledger_canister_id: String,
    index_canister_id: String,
    root_canister_id: String,
    governance_principal: String,
    confirmation_relayer_principal: String,
    decimals: u8,
    bridge_canister_id: String,
    canister_schema_version: u16,
    ic_host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_rpc_url: Option<String>,
    bridge_contract: String,
    bsns_contract: String,
    deployment_instance_id: String,
    minimum_withdrawal_id: String,
    deployment_block: u64,
    expected_bridge_signer: String,
    bridge_canister_wasm_sha256: String,
    bridge_runtime_bytecode_sha256: String,
    bsns_runtime_bytecode_sha256: String,
    bsns_runtime_template_sha256: String,
    ecdsa_key_name: String,
    ecdsa_derivation_path: Vec<String>,
    governance_ecdsa_derivation_path: Vec<String>,
    governance_operator: String,
    runtime_administrator: String,
    independent_canceller: String,
    initial_base_deployment: InitialBaseDeployment,
    timelock: Timelock,
    pause_principal: String,
    fee_recipient: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    rpc_providers: Vec<RpcProvider>,
    monitoring: Monitoring,
    parameters: Parameters,
    rate_limits: RateLimits,
    governance_replacement: GovernanceReplacementPolicy,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct InitialBaseDeployment {
    deployer_address: String,
    starting_nonce: u64,
    #[serde(with = "u128_string")]
    gas_limit: u128,
    #[serde(with = "u128_string")]
    max_fee_per_gas: u128,
    #[serde(with = "u128_string")]
    max_priority_fee_per_gas: u128,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct RpcProvider {
    url: String,
    operator: String,
    dns_owner: String,
    failure_domain: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct Monitoring {
    routing_sha256: String,
    detection_minutes: u8,
    acknowledgement_minutes: u8,
    pause_both_sides_minutes: u8,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct Timelock {
    address: String,
    runtime_code_hash: String,
    minimum_delay_seconds: u64,
    proposer: String,
    canceller: String,
    executor: String,
    external_admins: u8,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct Parameters {
    #[serde(with = "u128_string")]
    ledger_fee: u128,
    #[serde(with = "u128_string")]
    per_deposit_limit: u128,
    #[serde(with = "u128_string")]
    mint_throughput_limit: u128,
    mint_window_duration_seconds: u64,
    #[serde(with = "u128_string")]
    max_service_fee: u128,
    #[serde(with = "u128_string")]
    service_fee: u128,
    #[serde(with = "u128_string")]
    gas_limit_ceiling: u128,
    #[serde(with = "u128_string")]
    max_fee_per_gas_ceiling: u128,
    #[serde(with = "u128_string")]
    max_priority_fee_per_gas_ceiling: u128,
    #[serde(with = "u128_string")]
    l1_fee_per_transaction_ceiling_wei: u128,
    quote_validity_seconds: u64,
    gas_limit_multiplier_bps: u32,
    base_fee_multiplier_bps: u32,
    l1_fee_multiplier_bps: u32,
    #[serde(with = "u128_string")]
    cycles_floor: u128,
    #[serde(with = "u128_string")]
    settlement_cycle_ceiling: u128,
}

impl Parameters {
    fn governance_evm_fee(&self) -> EvmFeePolicy {
        EvmFeePolicy {
            gas_limit_ceiling: self.gas_limit_ceiling,
            max_fee_per_gas_ceiling: self.max_fee_per_gas_ceiling,
            max_priority_fee_per_gas_ceiling: self.max_priority_fee_per_gas_ceiling,
            l1_fee_per_transaction_ceiling_wei: self.l1_fee_per_transaction_ceiling_wei,
            quote_validity_seconds: self.quote_validity_seconds,
            gas_limit_multiplier_bps: self.gas_limit_multiplier_bps,
            base_fee_multiplier_bps: self.base_fee_multiplier_bps,
            l1_fee_multiplier_bps: self.l1_fee_multiplier_bps,
        }
    }
}

mod u128_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty() || (value.len() > 1 && value.starts_with('0')) {
            return Err(serde::de::Error::custom(
                "u128 values must be canonical decimal strings",
            ));
        }
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct RateLimits {
    deposit_window_seconds: u64,
    deposit_global: u16,
    deposit_per_principal: u16,
    notification_window_seconds: u64,
    notification_global: u16,
    notification_ingestion_global: u16,
    settlement_window_seconds: u64,
    settlement_global: u16,
    settlement_per_principal: u16,
    settlement_per_record: u16,
    settlement_retry_interval_seconds: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct GovernanceReplacementPolicy {
    max_replacements: u8,
    fee_bump_bps: u16,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct EvmFeePolicy {
    #[serde(with = "u128_string")]
    gas_limit_ceiling: u128,
    #[serde(with = "u128_string")]
    max_fee_per_gas_ceiling: u128,
    #[serde(with = "u128_string")]
    max_priority_fee_per_gas_ceiling: u128,
    #[serde(with = "u128_string")]
    l1_fee_per_transaction_ceiling_wei: u128,
    quote_validity_seconds: u64,
    gas_limit_multiplier_bps: u32,
    base_fee_multiplier_bps: u32,
    l1_fee_multiplier_bps: u32,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MeasurementSample {
    #[serde(with = "u128_string")]
    value: u128,
    observed_at_unix: u64,
    source_ref: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FeeMeasurementSample {
    #[serde(with = "u128_string")]
    base_fee_per_gas: u128,
    #[serde(with = "u128_string")]
    priority_fee_per_gas: u128,
    #[serde(with = "u128_string")]
    l1_fee_upper_bound_wei: u128,
    observed_at_unix: u64,
    source_ref: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema_version: u8,
    environment: String,
    ledger_fee: u128,
    governance_gas_samples: Vec<MeasurementSample>,
    fee_samples: Vec<FeeMeasurementSample>,
    settlement_cycle_samples: Vec<MeasurementSample>,
    baseline_cycles_sample: MeasurementSample,
    expected_daily_settlements: u128,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InitialGasEstimate {
    action: String,
    sender: String,
    target: String,
    #[serde(with = "u128_string")]
    value_wei: u128,
    calldata_hex: String,
    #[serde(with = "u128_string")]
    gas: u128,
    block_number: u64,
    block_hash: String,
    observed_at_unix: u64,
    source_ref: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InitialFeeSample {
    #[serde(with = "u128_string")]
    base_fee_per_gas: u128,
    #[serde(with = "u128_string")]
    priority_fee_per_gas: u128,
    #[serde(with = "u128_string")]
    l1_fee_upper_bound_wei: u128,
    block_number: u64,
    block_hash: String,
    observed_at_unix: u64,
    source_ref: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct InitialOperationalParameters {
    schema_version: u8,
    environment: String,
    chain_id: u64,
    bridge_canister_id: String,
    bridge_contract: String,
    timelock_contract: String,
    governance_sender: String,
    deployment_instance_id: String,
    governance_operation_id: u64,
    operation_salt: String,
    timelock_delay_seconds: u64,
    profile_sha256: String,
    gas_estimates: Vec<InitialGasEstimate>,
    fee_samples: Vec<InitialFeeSample>,
    #[serde(with = "u128_string")]
    idle_cycles_burned_per_day: u128,
    idle_cycles_observed_at_unix: u64,
    idle_cycles_source_ref: String,
    expected_daily_settlements: u128,
    #[serde(with = "u128_string")]
    settlement_cycle_ceiling: u128,
    derived: InitialDerivedParameters,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct InitialDerivedParameters {
    #[serde(with = "u128_string")]
    gas_limit_ceiling: u128,
    #[serde(with = "u128_string")]
    max_fee_per_gas_ceiling: u128,
    #[serde(with = "u128_string")]
    max_priority_fee_per_gas_ceiling: u128,
    #[serde(with = "u128_string")]
    l1_fee_per_transaction_ceiling_wei: u128,
    quote_validity_seconds: u64,
    gas_limit_multiplier_bps: u32,
    base_fee_multiplier_bps: u32,
    l1_fee_multiplier_bps: u32,
    #[serde(with = "u128_string")]
    cycles_floor: u128,
    #[serde(with = "u128_string")]
    settlement_cycle_ceiling: u128,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PostGateAPolicyTransition {
    schema_version: u8,
    reason: String,
    observed_at_unix: u64,
    gate_a_manifest_sha256: String,
    gate_a_receipt_sha256: String,
    from_source_revision: String,
    from_source_tree_sha256: String,
    to_source_revision: String,
    to_source_tree_sha256: String,
    bridge_canister_id: String,
    bridge_contract: String,
    bsns_contract: String,
    timelock_contract: String,
    bridge_canister_wasm_sha256: String,
    bridge_runtime_bytecode_sha256: String,
    bsns_runtime_bytecode_sha256: String,
    bsns_runtime_template_sha256: String,
    bridge_deployment_transaction_hash: String,
    timelock_deployment_transaction_hash: String,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct DerivedParameters {
    ledger_fee: u128,
    max_service_fee: u128,
    service_fee: u128,
    gas_limit_ceiling: u128,
    max_fee_per_gas_ceiling: u128,
    max_priority_fee_per_gas_ceiling: u128,
    l1_fee_per_transaction_ceiling_wei: u128,
    cycles_floor: u128,
    settlement_cycle_ceiling: u128,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReleaseManifest {
    schema_version: u8,
    release_id: String,
    test_only: bool,
    source_revision: String,
    source_tree_sha256: String,
    created_at_unix: u64,
    expires_at_unix: u64,
    parent_gate_a_manifest_sha256: Option<String>,
    artifacts: Vec<ArtifactDigest>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactDigest {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UiAssetsReceipt {
    schema_version: u8,
    source_revision: String,
    source_tree_sha256: String,
    files: Vec<UiAssetDigest>,
    artifact_set_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct UiAssetDigest {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct GateAReceipt {
    schema_version: u8,
    gate_a_manifest_sha256: String,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    gate_a_profile_sha256: String,
    post_deploy_profile_sha256: String,
    bridge_canister_wasm_sha256: String,
    bridge_runtime_bytecode_sha256: String,
    bridge_deployment_transaction_hash: String,
    bridge_deployment_block_number: u64,
    bridge_deployment_block_hash: String,
    timelock_deployment_transaction_hash: String,
    timelock_deployment_block_number: u64,
    timelock_deployment_block_hash: String,
    canister_install: ProductionCanisterInstallReceipt,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionDeploymentBinding {
    deployer_address: String,
    starting_nonce: u64,
    timelock: ProductionContractDeploymentBinding,
    bridge: ProductionContractDeploymentBinding,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionContractDeploymentBinding {
    transaction_hash: String,
    address: String,
    block_number: u64,
    block_hash: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ProductionCanisterPlan {
    schema_version: u8,
    environment: String,
    source_revision: String,
    source_tree_sha256: String,
    bridge_canister_id: String,
    bridge_canister_wasm_sha256: String,
    init: ProductionCanisterInitInput,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ProductionCanisterInitInput {
    ledger_canister_id: String,
    index_canister_id: String,
    evm_rpc_canister_id: String,
    custom_evm_rpc_urls: Vec<String>,
    base_chain_id: u64,
    bridge_contract_hex: String,
    expected_bridge_runtime_sha256_hex: String,
    timelock_contract_hex: String,
    expected_timelock_minimum_delay_seconds: u64,
    expected_bsns_runtime_sha256_hex: String,
    expected_bsns_decimals: u8,
    #[serde(with = "u128_string")]
    expected_minimum_service_fee: u128,
    deployment_instance_id_hex: String,
    minimum_withdrawal_id_hex: String,
    ecdsa_key_name: String,
    ecdsa_derivation_path_utf8: Vec<String>,
    governance_ecdsa_derivation_path_utf8: Vec<String>,
    deposit_rate_limit_window_seconds: u64,
    deposit_rate_limit_global: u16,
    deposit_rate_limit_per_principal: u16,
    notification_rate_limit_window_seconds: u64,
    notification_rate_limit_global: u16,
    notification_ingestion_rate_limit_global: u16,
    settlement_rate_limit_window_seconds: u64,
    settlement_rate_limit_global: u16,
    settlement_rate_limit_per_principal: u16,
    settlement_rate_limit_per_record: u16,
    settlement_retry_interval_seconds: u64,
    governance_evm_fee: EvmFeePolicy,
    governance_replacement: GovernanceReplacementPolicy,
    #[serde(with = "u128_string")]
    cycles_floor: u128,
    #[serde(with = "u128_string")]
    settlement_cycle_ceiling: u128,
    governance_principal: String,
    pause_principal: String,
    confirmation_relayer_principal: String,
    fee_recipient: ProductionFeeRecipientInput,
}

fn production_bootstrap_evm_fee() -> EvmFeePolicy {
    EvmFeePolicy {
        gas_limit_ceiling: 1,
        max_fee_per_gas_ceiling: 1,
        max_priority_fee_per_gas_ceiling: 0,
        l1_fee_per_transaction_ceiling_wei: 1,
        quote_validity_seconds: 30,
        gas_limit_multiplier_bps: 10_000,
        base_fee_multiplier_bps: 10_000,
        l1_fee_multiplier_bps: 10_000,
    }
}

const PRODUCTION_BOOTSTRAP_CYCLES_FLOOR: u128 = 1;
const PRODUCTION_BOOTSTRAP_SETTLEMENT_CYCLE_CEILING: u128 = u128::MAX;

fn profile_uses_production_bootstrap_operational_config(profile: &Profile) -> bool {
    profile.parameters.governance_evm_fee() == production_bootstrap_evm_fee()
        && profile.parameters.cycles_floor == PRODUCTION_BOOTSTRAP_CYCLES_FLOOR
        && profile.parameters.settlement_cycle_ceiling
            == PRODUCTION_BOOTSTRAP_SETTLEMENT_CYCLE_CEILING
}

fn set_production_bootstrap_operational_config(profile: &mut Profile) {
    let fee = production_bootstrap_evm_fee();
    profile.parameters.gas_limit_ceiling = fee.gas_limit_ceiling;
    profile.parameters.max_fee_per_gas_ceiling = fee.max_fee_per_gas_ceiling;
    profile.parameters.max_priority_fee_per_gas_ceiling = fee.max_priority_fee_per_gas_ceiling;
    profile.parameters.l1_fee_per_transaction_ceiling_wei = fee.l1_fee_per_transaction_ceiling_wei;
    profile.parameters.quote_validity_seconds = fee.quote_validity_seconds;
    profile.parameters.gas_limit_multiplier_bps = fee.gas_limit_multiplier_bps;
    profile.parameters.base_fee_multiplier_bps = fee.base_fee_multiplier_bps;
    profile.parameters.l1_fee_multiplier_bps = fee.l1_fee_multiplier_bps;
    profile.parameters.cycles_floor = PRODUCTION_BOOTSTRAP_CYCLES_FLOOR;
    profile.parameters.settlement_cycle_ceiling = PRODUCTION_BOOTSTRAP_SETTLEMENT_CYCLE_CEILING;
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ProductionFeeRecipientInput {
    owner: String,
    subaccount_hex: String,
}

#[derive(CandidType)]
struct ProductionCanisterInitArgs {
    ledger_canister_id: Principal,
    index_canister_id: Principal,
    evm_rpc_canister_id: Principal,
    custom_evm_rpc_urls: Vec<String>,
    base_chain_id: u64,
    bridge_contract: Vec<u8>,
    expected_bridge_runtime_sha256: Vec<u8>,
    timelock_contract: Vec<u8>,
    expected_timelock_minimum_delay_seconds: u64,
    expected_bsns_runtime_sha256: Vec<u8>,
    expected_bsns_decimals: u8,
    expected_minimum_service_fee: u128,
    deployment_instance_id: Vec<u8>,
    minimum_withdrawal_id: Vec<u8>,
    ecdsa_key_name: String,
    ecdsa_derivation_path: Vec<Vec<u8>>,
    governance_ecdsa_derivation_path: Vec<Vec<u8>>,
    deposit_rate_limit_window_seconds: u64,
    deposit_rate_limit_global: u16,
    deposit_rate_limit_per_principal: u16,
    notification_rate_limit_window_seconds: u64,
    notification_rate_limit_global: u16,
    notification_ingestion_rate_limit_global: u16,
    settlement_rate_limit_window_seconds: u64,
    settlement_rate_limit_global: u16,
    settlement_rate_limit_per_principal: u16,
    settlement_rate_limit_per_record: u16,
    settlement_retry_interval_seconds: u64,
    governance_evm_fee: EvmFeePolicy,
    governance_replacement: GovernanceReplacementPolicy,
    cycles_floor: u128,
    settlement_cycle_ceiling: u128,
    governance_principal: Principal,
    pause_principal: Principal,
    confirmation_relayer_principal: Principal,
    fee_recipient: OperationalFeeRecipientView,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(CandidType, Deserialize)]
struct ProductionCanisterInitArgsCallView {
    ledger_canister_id: Principal,
    index_canister_id: Principal,
    evm_rpc_canister_id: Principal,
    custom_evm_rpc_urls: Vec<String>,
    base_chain_id: u64,
    bridge_contract: Vec<u8>,
    expected_bridge_runtime_sha256: Vec<u8>,
    timelock_contract: Vec<u8>,
    expected_timelock_minimum_delay_seconds: u64,
    expected_bsns_runtime_sha256: Vec<u8>,
    expected_bsns_decimals: u8,
    expected_minimum_service_fee: u128,
    deployment_instance_id: Vec<u8>,
    minimum_withdrawal_id: Vec<u8>,
    ecdsa_key_name: String,
    ecdsa_derivation_path: Vec<Vec<u8>>,
    governance_ecdsa_derivation_path: Vec<Vec<u8>>,
    deposit_rate_limit_window_seconds: u64,
    deposit_rate_limit_global: u16,
    deposit_rate_limit_per_principal: u16,
    notification_rate_limit_window_seconds: u64,
    notification_rate_limit_global: u16,
    notification_ingestion_rate_limit_global: u16,
    settlement_rate_limit_window_seconds: u64,
    settlement_rate_limit_global: u16,
    settlement_rate_limit_per_principal: u16,
    settlement_rate_limit_per_record: u16,
    settlement_retry_interval_seconds: u64,
    governance_evm_fee: EvmFeePolicyCallView,
    governance_replacement: GovernanceReplacementPolicy,
    cycles_floor: u128,
    settlement_cycle_ceiling: u128,
    governance_principal: Principal,
    pause_principal: Principal,
    confirmation_relayer_principal: Principal,
    fee_recipient: OperationalFeeRecipientCallView,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ProductionCanisterInstallReceipt {
    schema_version: u8,
    plan_sha256: String,
    plan: ProductionCanisterPlan,
    source_revision: String,
    source_tree_sha256: String,
    canister_id: String,
    installer_principal: String,
    module_sha256: String,
    init_candid_sha256: String,
    runtime_binding: LiveRuntimeBinding,
    governance_operator: String,
    runtime_administrator: String,
    independent_canceller: String,
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
    storage_validation_complete: bool,
    storage_checksum_complete: bool,
    deposits_paused: bool,
    state_is_empty: bool,
    cycles_reserve_sufficient: bool,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct LiveRuntimeBinding {
    base_chain_id: u64,
    bridge_contract: String,
    timelock_contract: String,
    deployment_instance_id: String,
    minimum_withdrawal_id: String,
    ledger_canister_id: String,
    index_canister_id: String,
    schema_version: u16,
    expected_bridge_signer: String,
    evm_rpc_canister_id: String,
    rpc_provider_urls_sha256: String,
    operational_config_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MonitorDrill {
    schema_version: u8,
    rehearsal_id: String,
    source_revision: String,
    source_tree_sha256: String,
    ic_network: String,
    base_chain_id: u64,
    bridge_canister_id: String,
    bridge_contract: String,
    timelock_contract: String,
    bridge_canister_wasm_sha256: String,
    bridge_runtime_bytecode_sha256: String,
    rpc_provider_urls_sha256: String,
    routing_sha256: String,
    fault_started_at_unix: u64,
    detected_at_unix: u64,
    acknowledged_at_unix: u64,
    base_paused_at_unix: u64,
    pending_timelock_operation_before: bool,
    base_actions: Vec<MonitorBaseAction>,
    ic_pause: MonitorIcPause,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MonitorBaseAction {
    kind: String,
    transaction_hash: String,
    block_number: u64,
    block_hash: String,
    receipt_status: u8,
    target: String,
    calldata_hex: String,
    canonical_finalized: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MonitorIcPause {
    paused_at_unix: u64,
    response_hex: String,
    response_sha256: String,
    pause_principal: String,
    request_id: String,
    certificate_hex: String,
    certificate_sha256: String,
    audit_sequence: u64,
    audit_sha256: String,
    audit_raw_hex: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KeeperDrill {
    schema_version: u8,
    source_revision: String,
    source_tree_sha256: String,
    bridge_canister_id: String,
    withdrawal_id: String,
    burn_transaction_hash: String,
    burned_at_unix: u64,
    paid_at_unix: u64,
    maximum_unprocessed_seconds: u64,
    keeper_ids: Vec<String>,
    keeper_failure_domains: Vec<String>,
    monitoring_receipt_sha256: String,
    manual_fallback_drilled: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MonitoringReceipt {
    schema_version: u8,
    source_revision: String,
    source_tree_sha256: String,
    bridge_canister_id: String,
    withdrawal_id: String,
    burn_transaction_hash: String,
    burn: MonitoringBurnReceipt,
    paid: MonitoringPaidObservation,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MonitoringBurnReceipt {
    base_chain_id: u64,
    bridge_contract: String,
    block_number: u64,
    block_hash: String,
    receipt_status: u8,
    withdrawal_committed_topic: String,
    withdrawal_id_topic: String,
    canonical_finalized: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MonitoringPaidObservation {
    observed_at_unix: u64,
    state: String,
    response_hex: String,
    response_sha256: String,
    authenticated_query: bool,
}

#[derive(CandidType, Deserialize, Serialize, Debug, Eq, PartialEq)]
enum WithdrawalPhaseView {
    Paid,
    ReleasePending,
    ReconciliationHold,
    Observed,
}

#[derive(CandidType, Deserialize, Serialize, Debug, Eq, PartialEq)]
struct WithdrawalView {
    charged_service_fee: Nat,
    withdrawal_id: Vec<u8>,
    max_service_fee: Nat,
    release_ledger_block_index: Option<Nat>,
    last_settlement_stop_reason: Option<String>,
    amount_out: Nat,
    state: WithdrawalPhaseView,
    ledger_fee: Nat,
    amount: Nat,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProviderIndependenceReceipt {
    schema_version: u8,
    observed_at_unix: u64,
    proposal_id: u64,
    provider_review_sha256: String,
    dns_monitoring_enabled: bool,
    endpoint_monitoring_enabled: bool,
    drift_action: String,
    governance_query_response_hex: String,
    governance_query_response_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ControllerHandover {
    schema_version: u8,
    stage: String,
    observed_at_unix: u64,
    bridge_canister_id: String,
    sns_root_canister_id: String,
    executing_principal: String,
    command_argv: Vec<String>,
    request_id: String,
    response_exit_code: i32,
    response_stdout_hex: String,
    response_stderr_hex: String,
    response_sha256: String,
    final_controllers: Vec<String>,
    cycles_balance: u128,
    freezing_threshold_seconds: u64,
    idle_cycles_burned_per_day: u128,
    required_freezing_cycles: u128,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SnsUpgrade {
    schema_version: u8,
    observed_at_unix: u64,
    executed_at_unix: u64,
    proposal_id: u64,
    governance_canister_id: String,
    root_canister_id: String,
    bridge_canister_id: String,
    wasm_sha256: String,
    status: String,
    before_module_sha256: String,
    after_module_sha256: String,
    before_public_state_sha256: String,
    after_public_state_sha256: String,
    proposal_action: String,
    install_mode: String,
    proposal_target_canister_id: String,
    proposal_wasm_sha256: String,
    governance_query_response_hex: String,
    governance_query_response_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ActivationSubmission {
    schema_version: u8,
    phase: String,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    gate_b_manifest_sha256: String,
    governance_canister_id: String,
    bridge_canister_id: String,
    function_id: u64,
    target_method_name: String,
    payload_hex: String,
    payload_sha256: String,
    proposer_principal: String,
    neuron_subaccount: String,
    proposal_id: u64,
    submitted_at_unix: u64,
    registry_response_sha256: String,
    proposal_response_hex: String,
    proposal_response_sha256: String,
    registry_command_argv: Vec<String>,
    proposal_command_argv: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ActivationReceipt {
    schema_version: u8,
    phase: String,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    gate_b_manifest_sha256: String,
    proposal_id: u64,
    function_id: u64,
    target_method_name: String,
    payload_sha256: String,
    executed_at_unix: u64,
    verified_at_unix: u64,
    governance_query_response_hex: String,
    governance_query_response_sha256: String,
    function_registry_response_hex: String,
    function_registry_response_sha256: String,
    activation_status_response_hex: String,
    activation_status_response_sha256: String,
    operation_id: String,
    operation_salt: String,
    prior_schedule_receipt_sha256: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ProposalId {
    id: u64,
}

#[derive(CandidType, Serialize)]
struct GetProposalRequest {
    proposal_id: Option<ProposalId>,
}

#[derive(CandidType, Deserialize)]
struct GetProposalResponse {
    result: Option<GetProposalResult>,
}

#[derive(CandidType, Deserialize)]
enum GetProposalResult {
    Error(GovernanceErrorView),
    Proposal(ProposalDataView),
}

#[derive(CandidType, Deserialize)]
struct GovernanceErrorView {
    error_message: String,
    error_type: i32,
}

#[derive(CandidType, Deserialize)]
struct ProposalDataView {
    id: Option<ProposalId>,
    failure_reason: Option<GovernanceErrorView>,
    failed_timestamp_seconds: u64,
    decided_timestamp_seconds: u64,
    proposal: Option<ProposalView>,
    executed_timestamp_seconds: u64,
}

#[derive(CandidType, Deserialize)]
struct ProposalView {
    action: Option<SnsProposalAction>,
    summary: String,
}

#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize)]
enum SnsProposalAction {
    ManageNervousSystemParameters(Reserved),
    AddGenericNervousSystemFunction(Reserved),
    SetTopicsForCustomProposals(Reserved),
    ManageDappCanisterSettings(Reserved),
    RemoveGenericNervousSystemFunction(Reserved),
    UpgradeSnsToNextVersion(Reserved),
    AdvanceSnsTargetVersion(Reserved),
    RegisterDappCanisters(Reserved),
    RegisterExtension(Reserved),
    UpgradeExtension(Reserved),
    ExecuteExtensionOperation(Reserved),
    TransferSnsTreasuryFunds(Reserved),
    UpgradeSnsControlledCanister(UpgradeSnsControlledCanisterView),
    DeregisterDappCanisters(Reserved),
    MintSnsTokens(Reserved),
    Unspecified(Reserved),
    ManageSnsMetadata(Reserved),
    ExecuteGenericNervousSystemFunction(ExecuteGenericFunctionView),
    ManageLedgerParameters(Reserved),
    Motion(Reserved),
}

#[derive(CandidType, Deserialize)]
struct UpgradeSnsControlledCanisterView {
    new_canister_wasm: Vec<u8>,
    canister_id: Option<Principal>,
}

#[derive(CandidType, Deserialize)]
struct ExecuteGenericFunctionView {
    function_id: u64,
    payload: Vec<u8>,
}

#[derive(CandidType, Deserialize)]
struct ListNervousSystemFunctionsResponseView {
    functions: Vec<NervousSystemFunctionView>,
}

#[derive(CandidType, Deserialize)]
struct NervousSystemFunctionView {
    id: u64,
    function_type: Option<FunctionTypeView>,
}

#[derive(CandidType, Deserialize)]
enum FunctionTypeView {
    NativeNervousSystemFunction(Reserved),
    GenericNervousSystemFunction(GenericNervousSystemFunctionView),
}

#[derive(CandidType, Deserialize)]
struct GenericNervousSystemFunctionView {
    target_canister_id: Option<Principal>,
    target_method_name: Option<String>,
}

#[derive(CandidType, Deserialize)]
struct ActivationOperationStatusView {
    operation_id: Vec<u8>,
    salt: Vec<u8>,
}

#[derive(CandidType, Deserialize)]
struct ActivationStatusView {
    deposits_paused: bool,
    pending_timelock_operation: Option<ActivationOperationStatusView>,
    last_confirmed_activation: Option<ActivationConfirmationStatusView>,
}

#[derive(CandidType, Deserialize)]
struct ActivationConfirmationStatusView {
    phase: String,
    governance_operation_id: u64,
    timelock_operation_id: Vec<u8>,
    transaction_hash: Vec<u8>,
    receipt_block_number: u64,
}

#[derive(CandidType, Deserialize)]
enum ActivationStatusResultView {
    Ok(ActivationStatusView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
struct ActivationAttestationView {
    chain_id: u64,
    finalized_block_number: u64,
    finalized_block_hash: Vec<u8>,
    observed_at_ns: u64,
    bridge_signer: Vec<u8>,
    bridge_runtime_sha256: Vec<u8>,
    deposits_paused: bool,
    withdrawals_paused: bool,
    bridge_timelock: Vec<u8>,
    runtime_administrator: Vec<u8>,
    timelock_admin: Vec<u8>,
    timelock_proposer: Vec<u8>,
    timelock_canceller: Vec<u8>,
    timelock_executor: Vec<u8>,
    timelock_runtime_code_hash: Vec<u8>,
    bridge_approved_timelock_runtime_code_hash: Vec<u8>,
    timelock_minimum_delay_seconds: u64,
    bsns_address: Vec<u8>,
    bsns_runtime_sha256: Vec<u8>,
    bsns_name: String,
    bsns_symbol: String,
    bsns_decimals: u8,
    bsns_bridge: Vec<u8>,
    base_service_fee: u128,
}

#[derive(CandidType, Deserialize)]
enum ActivationAttestationResultView {
    Ok(Box<ActivationAttestationView>),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
struct RuntimeBindingView {
    base_chain_id: u64,
    bridge_contract: Vec<u8>,
    expected_bridge_runtime_sha256: Vec<u8>,
    timelock_contract: Vec<u8>,
    deployment_instance_id: Vec<u8>,
    minimum_withdrawal_id: Vec<u8>,
    ledger_canister_id: Principal,
    index_canister_id: Principal,
    schema_version: u16,
    expected_bridge_signer: Vec<u8>,
    evm_rpc_canister_id: Principal,
    rpc_provider_urls_sha256: Vec<u8>,
    operational_config_sha256: Vec<u8>,
}

#[derive(CandidType)]
struct OperationalConfigBindingView {
    ledger_fee: u128,
    operational_config: OperationalConfigView,
}

#[derive(CandidType)]
struct OperationalConfigView {
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
    governance_operator: Vec<u8>,
    deposit_rate_limit_window_seconds: u64,
    deposit_rate_limit_global: u16,
    deposit_rate_limit_per_principal: u16,
    notification_rate_limit_window_seconds: u64,
    notification_rate_limit_global: u16,
    notification_ingestion_rate_limit_global: u16,
    settlement_rate_limit_window_seconds: u64,
    settlement_rate_limit_global: u16,
    settlement_rate_limit_per_principal: u16,
    settlement_rate_limit_per_record: u16,
    settlement_retry_interval_seconds: u64,
    governance_evm_fee: EvmFeePolicy,
    governance_replacement: GovernanceReplacementPolicy,
    cycles_floor: u128,
    settlement_cycle_ceiling: u128,
    governance_principal: Principal,
    pause_principal: Principal,
    confirmation_relayer_principal: Principal,
    fee_recipient: OperationalFeeRecipientView,
}

#[derive(CandidType)]
struct OperationalFeeRecipientView {
    owner: Principal,
    subaccount: Vec<u8>,
}

const OPERATIONAL_CONFIG_BINDING_DOMAIN: &[u8] = b"KINIC_OPERATIONAL_CONFIG_BINDING_V1\0";

#[derive(CandidType, Deserialize)]
struct ReserveStatusView {
    sufficient: bool,
}

#[derive(CandidType, Deserialize)]
struct BridgeStatusLiveView {
    reserve: ReserveStatusView,
    deposits_paused: bool,
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
    counts: ProductionStatusCountsView,
}

#[derive(CandidType, Deserialize)]
struct ProductionStatusCountsView {
    deposits: u64,
    withdrawals: u64,
    reconciliation_holds: u64,
    pending_ledger_operations: u64,
    reserved_deposit_mint_amount: u128,
    reserved_deposit_mint_operations: u64,
    retained_audit_events: u64,
    pruned_audit_events: u64,
    retained_deposit_index_entries: u64,
}

#[derive(CandidType, Deserialize)]
struct StorageValidationStatusView {
    complete: bool,
    phase: String,
    scanned_rows: u64,
}

#[derive(CandidType, Deserialize)]
enum StorageValidationResultView {
    Ok(StorageValidationStatusView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
struct StorageChecksumStatusView {
    complete: bool,
    checksum: u64,
    scanned_bytes: u64,
    db_size: u64,
}

#[derive(CandidType, Deserialize)]
enum StorageChecksumResultView {
    Ok(StorageChecksumStatusView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
enum PublicConfigInitializationResultView {
    Ok(()),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
enum OperationalConfigResultView {
    Ok(Box<OperationalConfigCallView>),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
enum ControlPlaneAddressesResultView {
    Ok(ControlPlaneAddressesCallView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize, Clone)]
struct ControlPlaneAddressesCallView {
    bridge_signer: Vec<u8>,
    governance_operator: Vec<u8>,
    runtime_administrator: Vec<u8>,
    independent_canceller: Vec<u8>,
}

#[derive(CandidType, Deserialize)]
struct OperationalConfigCallView {
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
    governance_operator: Vec<u8>,
    deposit_rate_limit_window_seconds: u64,
    deposit_rate_limit_global: u16,
    deposit_rate_limit_per_principal: u16,
    notification_rate_limit_window_seconds: u64,
    notification_rate_limit_global: u16,
    notification_ingestion_rate_limit_global: u16,
    settlement_rate_limit_window_seconds: u64,
    settlement_rate_limit_global: u16,
    settlement_rate_limit_per_principal: u16,
    settlement_rate_limit_per_record: u16,
    settlement_retry_interval_seconds: u64,
    governance_evm_fee: EvmFeePolicyCallView,
    governance_replacement: GovernanceReplacementPolicy,
    cycles_floor: u128,
    settlement_cycle_ceiling: u128,
    governance_principal: Principal,
    pause_principal: Principal,
    confirmation_relayer_principal: Principal,
    fee_recipient: OperationalFeeRecipientCallView,
}

#[derive(CandidType, Deserialize)]
struct EvmFeePolicyCallView {
    gas_limit_ceiling: u128,
    max_fee_per_gas_ceiling: u128,
    max_priority_fee_per_gas_ceiling: u128,
    l1_fee_per_transaction_ceiling_wei: u128,
    quote_validity_seconds: u64,
    gas_limit_multiplier_bps: u32,
    base_fee_multiplier_bps: u32,
    l1_fee_multiplier_bps: u32,
}

#[derive(CandidType, Deserialize)]
struct OperationalFeeRecipientCallView {
    owner: Principal,
    subaccount: Vec<u8>,
}

impl From<OperationalConfigCallView> for OperationalConfigView {
    fn from(value: OperationalConfigCallView) -> Self {
        Self {
            mint_authorization_ttl_seconds: value.mint_authorization_ttl_seconds,
            mint_authorization_epoch: value.mint_authorization_epoch,
            governance_operator: value.governance_operator,
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
            governance_evm_fee: EvmFeePolicy {
                gas_limit_ceiling: value.governance_evm_fee.gas_limit_ceiling,
                max_fee_per_gas_ceiling: value.governance_evm_fee.max_fee_per_gas_ceiling,
                max_priority_fee_per_gas_ceiling: value
                    .governance_evm_fee
                    .max_priority_fee_per_gas_ceiling,
                l1_fee_per_transaction_ceiling_wei: value
                    .governance_evm_fee
                    .l1_fee_per_transaction_ceiling_wei,
                quote_validity_seconds: value.governance_evm_fee.quote_validity_seconds,
                gas_limit_multiplier_bps: value.governance_evm_fee.gas_limit_multiplier_bps,
                base_fee_multiplier_bps: value.governance_evm_fee.base_fee_multiplier_bps,
                l1_fee_multiplier_bps: value.governance_evm_fee.l1_fee_multiplier_bps,
            },
            governance_replacement: value.governance_replacement,
            cycles_floor: value.cycles_floor,
            settlement_cycle_ceiling: value.settlement_cycle_ceiling,
            governance_principal: value.governance_principal,
            pause_principal: value.pause_principal,
            confirmation_relayer_principal: value.confirmation_relayer_principal,
            fee_recipient: OperationalFeeRecipientView {
                owner: value.fee_recipient.owner,
                subaccount: value.fee_recipient.subaccount,
            },
        }
    }
}

#[derive(CandidType, Deserialize)]
enum StorageIntegrityResultView {
    Ok(String),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
enum ProductionLifecycleView {
    Bootstrap,
    OperationalConfigSealed,
    Activated,
}

#[derive(CandidType, Deserialize)]
enum ProductionLifecycleResultView {
    Ok(ProductionLifecycleView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
struct EmergencyPauseReceiptView {
    caller: Principal,
    local_deposits_paused: bool,
    local_pause_audit_sequence: u64,
    local_pause_audit_sha256: Vec<u8>,
    base_actions_queued: bool,
    base_action_count: u8,
    base_action_plan_sha256: Vec<u8>,
}

#[derive(CandidType, Deserialize)]
enum EmergencyPauseResultView {
    Ok(EmergencyPauseReceiptView),
    Err(Reserved),
}

struct ValidatedBundle {
    root: PathBuf,
    manifest: ReleaseManifest,
    profile: Profile,
    manifest_sha256: String,
}

fn checked_ratio_ceil(value: u128, numerator: u128, denominator: u128) -> Result<u128, String> {
    value
        .checked_mul(numerator)
        .and_then(|product| product.checked_add(denominator.checked_sub(1)?))
        .map(|product| product / denominator)
        .ok_or_else(|| "rounded ratio overflow".into())
}

fn percentile(values: &[u128], numerator: usize, denominator: usize) -> Result<u128, String> {
    if values.is_empty() {
        return Err("percentile sample is empty".into());
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let rank = sorted
        .len()
        .checked_mul(numerator)
        .and_then(|value| value.checked_add(denominator - 1))
        .ok_or_else(|| "percentile rank overflow".to_string())?
        / denominator;
    Ok(sorted[rank.saturating_sub(1).min(sorted.len() - 1)])
}

fn derive(evidence: &Evidence) -> Result<DerivedParameters, String> {
    if evidence.schema_version != 3 {
        return Err("measurement evidence must use schema v3".into());
    }
    if evidence.governance_gas_samples.len() < 10
        || evidence.settlement_cycle_samples.len() < 10
        || evidence.fee_samples.len() < 10
    {
        return Err(
            "governance gas, fee, and cycle evidence must contain at least 10 samples each".into(),
        );
    }
    validate_sample_sources(
        evidence
            .governance_gas_samples
            .iter()
            .map(|sample| sample.source_ref.as_str()),
    )?;
    validate_sample_sources(
        evidence
            .settlement_cycle_samples
            .iter()
            .map(|sample| sample.source_ref.as_str()),
    )?;
    validate_sample_sources(
        evidence
            .fee_samples
            .iter()
            .map(|sample| sample.source_ref.as_str()),
    )?;
    validate_sample_sources(std::iter::once(
        evidence.baseline_cycles_sample.source_ref.as_str(),
    ))?;
    if evidence.ledger_fee == 0
        || evidence.baseline_cycles_sample.value == 0
        || evidence.baseline_cycles_sample.observed_at_unix == 0
        || evidence.expected_daily_settlements == 0
        || evidence
            .governance_gas_samples
            .iter()
            .any(|sample| sample.value == 0 || sample.observed_at_unix == 0)
        || evidence
            .settlement_cycle_samples
            .iter()
            .any(|sample| sample.value == 0 || sample.observed_at_unix == 0)
        || evidence.fee_samples.iter().any(|sample| {
            sample.base_fee_per_gas == 0
                || sample.priority_fee_per_gas == 0
                || sample.l1_fee_upper_bound_wei == 0
                || sample.observed_at_unix == 0
        })
    {
        return Err("measurement evidence values must be positive and fee samples aligned".into());
    }
    let minimum_days = match evidence.environment.as_str() {
        "base-sepolia" | "mainnet-candidate" => 7,
        _ => return Err("unsupported evidence environment".into()),
    };
    let fee_start = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.observed_at_unix)
        .min()
        .ok_or("missing fee samples")?;
    let fee_end = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.observed_at_unix)
        .max()
        .ok_or("missing fee samples")?;
    if fee_end
        .checked_sub(fee_start)
        .is_none_or(|duration| duration < minimum_days * 24 * 60 * 60)
    {
        return Err(format!(
            "Base fee evidence must cover at least {minimum_days} days"
        ));
    }
    let gas_max = evidence
        .governance_gas_samples
        .iter()
        .map(|sample| sample.value)
        .max()
        .ok_or("missing gas samples")?;
    let gas_limit_ceiling = checked_ratio_ceil(gas_max, 130, 100)?
        .checked_add(999)
        .map(|value| value / 1_000 * 1_000)
        .ok_or("gas limit overflow")?;
    let priority_fees = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.priority_fee_per_gas)
        .collect::<Vec<_>>();
    let base_fees = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.base_fee_per_gas)
        .collect::<Vec<_>>();
    let l1_fees = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.l1_fee_upper_bound_wei)
        .collect::<Vec<_>>();
    let max_priority_fee_per_gas_ceiling = percentile(&priority_fees, 95, 100)?
        .checked_mul(4)
        .ok_or("priority fee cap overflow")?;
    let max_fee_per_gas_ceiling = percentile(&base_fees, 99, 100)?
        .checked_mul(20)
        .ok_or("max fee cap overflow")?;
    let l1_fee_per_transaction_ceiling_wei = percentile(&l1_fees, 99, 100)?
        .checked_mul(10)
        .ok_or("L1 fee cap overflow")?;
    let settlement_cycles_max = evidence
        .settlement_cycle_samples
        .iter()
        .map(|sample| sample.value)
        .max()
        .ok_or("missing cycle samples")?;
    let settlement_cycle_ceiling = checked_ratio_ceil(settlement_cycles_max, 150, 100)?;
    let cycles_floor = evidence
        .expected_daily_settlements
        .checked_mul(settlement_cycles_max)
        .and_then(|settlement_daily| {
            settlement_daily.checked_add(evidence.baseline_cycles_sample.value)
        })
        .and_then(|daily| daily.checked_mul(30))
        .and_then(|thirty_days| thirty_days.checked_mul(2))
        .ok_or("cycles floor overflow")?;
    Ok(DerivedParameters {
        ledger_fee: evidence.ledger_fee,
        max_service_fee: evidence
            .ledger_fee
            .checked_mul(10_000)
            .ok_or("maximum service fee overflow")?,
        service_fee: evidence
            .ledger_fee
            .checked_mul(500)
            .ok_or("service fee overflow")?,
        gas_limit_ceiling,
        max_fee_per_gas_ceiling,
        max_priority_fee_per_gas_ceiling,
        l1_fee_per_transaction_ceiling_wei,
        cycles_floor,
        settlement_cycle_ceiling,
    })
}

fn validate_sample_sources<'a>(sources: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut unique = BTreeSet::new();
    for source in sources {
        if source.trim() != source
            || source.is_empty()
            || source.len() > 512
            || source.starts_with("replace-with-")
            || source.chars().any(char::is_control)
            || !unique.insert(source)
        {
            return Err(
                "measurement source_ref values must be non-empty, bounded, and unique per sample category"
                    .into(),
            );
        }
    }
    Ok(())
}

fn derive_initial_operational_parameters(
    evidence: &InitialOperationalParameters,
) -> Result<InitialDerivedParameters, String> {
    if evidence.schema_version != 1
        || evidence.environment != "mainnet-candidate"
        || evidence.chain_id != 8_453
        || evidence.gas_estimates.len() < 2
        || evidence.fee_samples.len() < 10
        || evidence.idle_cycles_burned_per_day == 0
        || evidence.expected_daily_settlements != 1
        || evidence.settlement_cycle_ceiling != 5_000_000_000
    {
        return Err("invalid initial operational parameter evidence".into());
    }
    let deployment_instance_id: [u8; 32] = decode_hex(&evidence.deployment_instance_id)?
        .try_into()
        .map_err(|_| "invalid initial deployment instance ID")?;
    let bridge = decode_address(&evidence.bridge_contract)?;
    let operation_salt =
        initial_activation_salt(deployment_instance_id, evidence.governance_operation_id);
    if evidence.timelock_delay_seconds != 86_400
        || evidence.governance_operation_id != 0
        || !evm_address(&evidence.governance_sender)
        || !valid_nonzero_hash32(&evidence.deployment_instance_id)
        || !evidence
            .operation_salt
            .eq_ignore_ascii_case(&format!("0x{}", hex(&operation_salt)))
    {
        return Err("invalid initial activation operation binding".into());
    }
    let actions = evidence
        .gas_estimates
        .iter()
        .map(|sample| sample.action.as_str())
        .collect::<BTreeSet<_>>();
    let finalized_blocks = evidence
        .fee_samples
        .iter()
        .map(|sample| (sample.block_number, sample.block_hash.to_ascii_lowercase()))
        .collect::<BTreeSet<_>>();
    if actions != BTreeSet::from(["execute_activation", "schedule_activation"])
        || evidence.gas_estimates.iter().any(|sample| {
            let expected_calldata = initial_activation_calldata(
                &sample.action,
                bridge,
                operation_salt,
                evidence.timelock_delay_seconds,
            );
            sample.gas == 0
                || !sample
                    .sender
                    .eq_ignore_ascii_case(&evidence.governance_sender)
                || !sample
                    .target
                    .eq_ignore_ascii_case(&evidence.timelock_contract)
                || sample.value_wei != 0
                || expected_calldata.is_err()
                || !expected_calldata
                    .as_deref()
                    .is_ok_and(|expected| sample.calldata_hex.eq_ignore_ascii_case(expected))
                || sample.block_number == 0
                || !valid_nonzero_hash32(&sample.block_hash)
                || sample.observed_at_unix == 0
                || sample.source_ref.trim() != sample.source_ref
                || sample.source_ref.is_empty()
        })
        || evidence.fee_samples.iter().any(|sample| {
            sample.base_fee_per_gas == 0
                || sample.priority_fee_per_gas == 0
                || sample.l1_fee_upper_bound_wei == 0
                || sample.block_number == 0
                || !valid_nonzero_hash32(&sample.block_hash)
                || sample.observed_at_unix == 0
        })
        || finalized_blocks.len() < 10
    {
        return Err("initial parameter samples are incomplete".into());
    }
    validate_sample_sources(
        evidence
            .gas_estimates
            .iter()
            .map(|sample| sample.source_ref.as_str()),
    )?;
    validate_sample_sources(
        evidence
            .fee_samples
            .iter()
            .map(|sample| sample.source_ref.as_str()),
    )?;
    validate_sample_sources(std::iter::once(evidence.idle_cycles_source_ref.as_str()))?;
    let gas_max = evidence
        .gas_estimates
        .iter()
        .map(|sample| sample.gas)
        .max()
        .ok_or("missing activation gas estimates")?;
    let gas_limit_ceiling = checked_ratio_ceil(gas_max, 130, 100)?
        .checked_add(999)
        .map(|value| value / 1_000 * 1_000)
        .ok_or("initial gas limit overflow")?;
    let priority = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.priority_fee_per_gas)
        .collect::<Vec<_>>();
    let base = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.base_fee_per_gas)
        .collect::<Vec<_>>();
    let l1 = evidence
        .fee_samples
        .iter()
        .map(|sample| sample.l1_fee_upper_bound_wei)
        .collect::<Vec<_>>();
    let max_priority_fee_per_gas_ceiling = percentile(&priority, 95, 100)?
        .checked_mul(4)
        .ok_or("initial priority fee overflow")?;
    let max_fee_per_gas_ceiling = percentile(&base, 99, 100)?
        .checked_mul(20)
        .ok_or("initial max fee overflow")?;
    let l1_fee_per_transaction_ceiling_wei = percentile(&l1, 99, 100)?
        .checked_mul(10)
        .ok_or("initial L1 fee overflow")?;
    let cycles_floor = evidence
        .settlement_cycle_ceiling
        .checked_mul(evidence.expected_daily_settlements)
        .and_then(|value| value.checked_add(evidence.idle_cycles_burned_per_day))
        .and_then(|value| value.checked_mul(30))
        .and_then(|value| value.checked_mul(2))
        .ok_or("initial cycles floor overflow")?;
    Ok(InitialDerivedParameters {
        gas_limit_ceiling,
        max_fee_per_gas_ceiling,
        max_priority_fee_per_gas_ceiling,
        l1_fee_per_transaction_ceiling_wei,
        quote_validity_seconds: 90,
        gas_limit_multiplier_bps: 13_000,
        base_fee_multiplier_bps: 60_000,
        l1_fee_multiplier_bps: 15_000,
        cycles_floor,
        settlement_cycle_ceiling: evidence.settlement_cycle_ceiling,
    })
}

fn validate_initial_operational_parameters(
    evidence: &InitialOperationalParameters,
    profile: &Profile,
    manifest_created_at_unix: u64,
    now: u64,
) -> Result<(), String> {
    let derived = derive_initial_operational_parameters(evidence)?;
    let sample_times = evidence
        .gas_estimates
        .iter()
        .map(|sample| sample.observed_at_unix)
        .chain(
            evidence
                .fee_samples
                .iter()
                .map(|sample| sample.observed_at_unix),
        )
        .chain(std::iter::once(evidence.idle_cycles_observed_at_unix));
    if sample_times.into_iter().any(|at| {
        at == 0
            || at > manifest_created_at_unix
            || at > now
            || now.saturating_sub(at) > MAX_EVIDENCE_AGE_SECS
    }) {
        return Err("initial operational observations are stale or future-dated".into());
    }
    if evidence.bridge_canister_id != profile.bridge_canister_id
        || !evidence
            .bridge_contract
            .eq_ignore_ascii_case(&profile.bridge_contract)
        || !evidence
            .timelock_contract
            .eq_ignore_ascii_case(&profile.timelock.address)
        || !evidence
            .governance_sender
            .eq_ignore_ascii_case(&profile.governance_operator)
        || !evidence
            .deployment_instance_id
            .eq_ignore_ascii_case(&profile.deployment_instance_id)
        || evidence.timelock_delay_seconds != profile.timelock.minimum_delay_seconds
        || !evidence
            .profile_sha256
            .eq_ignore_ascii_case(&hex(&canonical_sha256(profile)?))
        || evidence.derived != derived
        || profile.parameters.gas_limit_ceiling != derived.gas_limit_ceiling
        || profile.parameters.max_fee_per_gas_ceiling != derived.max_fee_per_gas_ceiling
        || profile.parameters.max_priority_fee_per_gas_ceiling
            != derived.max_priority_fee_per_gas_ceiling
        || profile.parameters.l1_fee_per_transaction_ceiling_wei
            != derived.l1_fee_per_transaction_ceiling_wei
        || profile.parameters.quote_validity_seconds != derived.quote_validity_seconds
        || profile.parameters.gas_limit_multiplier_bps != derived.gas_limit_multiplier_bps
        || profile.parameters.base_fee_multiplier_bps != derived.base_fee_multiplier_bps
        || profile.parameters.l1_fee_multiplier_bps != derived.l1_fee_multiplier_bps
        || profile.parameters.cycles_floor != derived.cycles_floor
        || profile.parameters.settlement_cycle_ceiling != derived.settlement_cycle_ceiling
    {
        return Err(
            "initial operational parameters do not exactly match the release profile".into(),
        );
    }
    Ok(())
}

fn validate_gate_b_operational_parameters(
    profile: &Profile,
    evidence: &Evidence,
) -> Result<(), String> {
    if evidence.environment != "mainnet-candidate" {
        return Err("Gate B measurements must use the mainnet-candidate environment".into());
    }
    let derived = derive(evidence)?;
    let expected_fee = EvmFeePolicy {
        gas_limit_ceiling: derived.gas_limit_ceiling,
        max_fee_per_gas_ceiling: derived.max_fee_per_gas_ceiling,
        max_priority_fee_per_gas_ceiling: derived.max_priority_fee_per_gas_ceiling,
        l1_fee_per_transaction_ceiling_wei: derived.l1_fee_per_transaction_ceiling_wei,
        quote_validity_seconds: 90,
        gas_limit_multiplier_bps: 13_000,
        base_fee_multiplier_bps: 60_000,
        l1_fee_multiplier_bps: 15_000,
    };
    if profile.parameters.ledger_fee != derived.ledger_fee
        || profile.parameters.max_service_fee != derived.max_service_fee
        || profile.parameters.service_fee != derived.service_fee
        || profile.parameters.governance_evm_fee() != expected_fee
        || profile.parameters.cycles_floor != derived.cycles_floor
        || profile.parameters.settlement_cycle_ceiling != derived.settlement_cycle_ceiling
    {
        return Err(
            "Gate B operational parameters must exactly match the measurement derivation".into(),
        );
    }
    Ok(())
}

fn validate_measurement_time(
    evidence: &Evidence,
    evidence_manifest_created_at_unix: u64,
    now: u64,
) -> Result<(), String> {
    let sample_times = evidence
        .governance_gas_samples
        .iter()
        .map(|sample| sample.observed_at_unix)
        .chain(
            evidence
                .fee_samples
                .iter()
                .map(|sample| sample.observed_at_unix),
        )
        .chain(
            evidence
                .settlement_cycle_samples
                .iter()
                .map(|sample| sample.observed_at_unix),
        )
        .chain(std::iter::once(
            evidence.baseline_cycles_sample.observed_at_unix,
        ))
        .collect::<Vec<_>>();
    if sample_times.is_empty()
        || evidence_manifest_created_at_unix > now
        || sample_times.iter().any(|observed_at| {
            *observed_at == 0
                || *observed_at > evidence_manifest_created_at_unix
                || now.saturating_sub(*observed_at) > MAX_EVIDENCE_AGE_SECS
        })
    {
        return Err(
            "every fee/cycles sample must predate the evidence manifest and remain current".into(),
        );
    }
    Ok(())
}

fn evm_address(value: &str) -> bool {
    value.len() == 42
        && value.starts_with("0x")
        && value[2..].bytes().all(|byte| byte.is_ascii_hexdigit())
        && value[2..].bytes().any(|byte| byte != b'0')
}

fn principal(value: &str) -> bool {
    Principal::from_text(value)
        .map(|value| value != Principal::anonymous())
        .unwrap_or(false)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
        && value.bytes().any(|byte| byte != b'0')
}

fn valid_hash32(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

fn valid_nonzero_hash32(value: &str) -> bool {
    valid_hash32(value) && value[2..].bytes().any(|byte| byte != b'0')
}

fn valid_nonempty_hex(value: &str) -> bool {
    let value = value.strip_prefix("0x").unwrap_or(value);
    !value.is_empty()
        && value.len().is_multiple_of(2)
        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn evm_selector(signature: &str) -> String {
    format!("0x{}", hex(&evm_selector_bytes(signature)))
}

fn evm_selector_bytes(signature: &str) -> [u8; 4] {
    let mut hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(signature.as_bytes());
    keccak.finalize(&mut hash);
    hash[..4].try_into().expect("selector prefix")
}

fn keccak256(value: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(value);
    keccak.finalize(&mut hash);
    hash
}

fn initial_activation_salt(deployment_instance_id: [u8; 32], operation_id: u64) -> [u8; 32] {
    let mut input = b"KINIC_BRIDGE_ACTIVATION_V2".to_vec();
    input.extend_from_slice(&deployment_instance_id);
    input.extend_from_slice(&operation_id.to_be_bytes());
    keccak256(&input)
}

fn initial_activation_operation_id(bridge: [u8; 20], salt: [u8; 32]) -> [u8; 32] {
    keccak256(&initial_activation_arguments(bridge, salt, 0, false))
}

fn evm_word_u128(value: u128) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[16..].copy_from_slice(&value.to_be_bytes());
    word
}

fn encode_initial_address_array(values: &[[u8; 20]]) -> Vec<u8> {
    let mut encoded = evm_word_u128(values.len() as u128).to_vec();
    for value in values {
        encoded.extend_from_slice(&[0; 12]);
        encoded.extend_from_slice(value);
    }
    encoded
}

fn encode_initial_u128_array(values: &[u128]) -> Vec<u8> {
    let mut encoded = evm_word_u128(values.len() as u128).to_vec();
    for value in values {
        encoded.extend_from_slice(&evm_word_u128(*value));
    }
    encoded
}

fn encode_initial_bytes(value: &[u8]) -> Vec<u8> {
    let mut encoded = evm_word_u128(value.len() as u128).to_vec();
    encoded.extend_from_slice(value);
    encoded.resize(encoded.len().next_multiple_of(32), 0);
    encoded
}

fn encode_initial_bytes_array(values: &[Vec<u8>]) -> Vec<u8> {
    let values = values
        .iter()
        .map(|value| encode_initial_bytes(value))
        .collect::<Vec<_>>();
    let mut encoded = evm_word_u128(values.len() as u128).to_vec();
    let mut offset = values.len() * 32;
    for value in &values {
        encoded.extend_from_slice(&evm_word_u128(offset as u128));
        offset += value.len();
    }
    for value in values {
        encoded.extend_from_slice(&value);
    }
    encoded
}

fn initial_activation_arguments(
    bridge: [u8; 20],
    salt: [u8; 32],
    delay_seconds: u64,
    include_delay: bool,
) -> Vec<u8> {
    let targets = encode_initial_address_array(&[bridge, bridge]);
    let values = encode_initial_u128_array(&[0, 0]);
    let payloads = encode_initial_bytes_array(&[
        evm_selector_bytes("unpauseDepositMints()").to_vec(),
        evm_selector_bytes("unpauseWithdrawals()").to_vec(),
    ]);
    let head_words = if include_delay { 6u128 } else { 5u128 };
    let mut encoded = Vec::new();
    encoded.extend_from_slice(&evm_word_u128(head_words * 32));
    encoded.extend_from_slice(&evm_word_u128(head_words * 32 + targets.len() as u128));
    encoded.extend_from_slice(&evm_word_u128(
        head_words * 32 + targets.len() as u128 + values.len() as u128,
    ));
    encoded.extend_from_slice(&[0; 32]);
    encoded.extend_from_slice(&salt);
    if include_delay {
        encoded.extend_from_slice(&evm_word_u128(delay_seconds.into()));
    }
    encoded.extend_from_slice(&targets);
    encoded.extend_from_slice(&values);
    encoded.extend_from_slice(&payloads);
    encoded
}

fn initial_activation_calldata(
    action: &str,
    bridge: [u8; 20],
    salt: [u8; 32],
    delay_seconds: u64,
) -> Result<String, String> {
    let (signature, include_delay) = match action {
        "schedule_activation" => (
            "scheduleBatch(address[],uint256[],bytes[],bytes32,bytes32,uint256)",
            true,
        ),
        "execute_activation" => (
            "executeBatch(address[],uint256[],bytes[],bytes32,bytes32)",
            false,
        ),
        _ => return Err("unknown initial activation gas action".into()),
    };
    let mut calldata = evm_selector_bytes(signature).to_vec();
    calldata.extend_from_slice(&initial_activation_arguments(
        bridge,
        salt,
        delay_seconds,
        include_delay,
    ));
    Ok(format!("0x{}", hex(&calldata)))
}

fn evm_topic(signature: &str) -> String {
    let mut hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(signature.as_bytes());
    keccak.finalize(&mut hash);
    format!("0x{}", hex(&hash))
}

fn validate_monitor_drill(
    drill: &MonitorDrill,
    manifest: &ReleaseManifest,
    profile: &Profile,
    activation_source_revision: &str,
    activation_source_tree_sha256: &str,
    activation_wasm_sha256: &str,
    now: u64,
) -> Result<(), String> {
    for at in [
        drill.fault_started_at_unix,
        drill.detected_at_unix,
        drill.acknowledged_at_unix,
        drill.base_paused_at_unix,
        drill.ic_pause.paused_at_unix,
    ] {
        validate_evidence_time(at, manifest.created_at_unix, now)?;
    }
    let expected_cancel_count = usize::from(drill.pending_timelock_operation_before);
    let count = |kind: &str| drill.base_actions.iter().filter(|a| a.kind == kind).count();
    let transactions = drill
        .base_actions
        .iter()
        .map(|action| action.transaction_hash.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let actions_valid = drill.base_actions.iter().all(|action| {
        let (target, selector, exact_length) = match action.kind.as_str() {
            "PauseDepositMints" => (
                &drill.bridge_contract,
                evm_selector("pauseDepositMints()"),
                10,
            ),
            "PauseWithdrawals" => (
                &drill.bridge_contract,
                evm_selector("pauseWithdrawals()"),
                10,
            ),
            "CancelTimelock" => (
                &drill.timelock_contract,
                evm_selector("cancel(bytes32)"),
                74,
            ),
            _ => return false,
        };
        valid_hash32(&action.transaction_hash)
            && action.block_number != 0
            && valid_hash32(&action.block_hash)
            && action.receipt_status == 1
            && action.target.eq_ignore_ascii_case(target)
            && action
                .calldata_hex
                .to_ascii_lowercase()
                .starts_with(&selector)
            && action.calldata_hex.len() == exact_length
            && action.canonical_finalized
    });
    if drill.schema_version != 4
        || drill.rehearsal_id.trim().is_empty()
        || drill.source_revision != activation_source_revision
        || !drill
            .source_tree_sha256
            .eq_ignore_ascii_case(activation_source_tree_sha256)
        || drill.ic_network != "ic"
        || drill.base_chain_id != 84_532
        || !principal(&drill.bridge_canister_id)
        || !evm_address(&drill.bridge_contract)
        || !evm_address(&drill.timelock_contract)
        || !drill
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(activation_wasm_sha256)
        || !drill
            .bridge_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&profile.bridge_runtime_bytecode_sha256)
        || !valid_sha256(&drill.rpc_provider_urls_sha256)
        || drill.detected_at_unix < drill.fault_started_at_unix
        || drill.acknowledged_at_unix < drill.detected_at_unix
        || drill.base_paused_at_unix < drill.acknowledged_at_unix
        || drill.ic_pause.paused_at_unix < drill.acknowledged_at_unix
        || count("PauseDepositMints") != 1
        || count("PauseWithdrawals") != 1
        || count("CancelTimelock") != expected_cancel_count
        || drill.base_actions.len() != 2 + expected_cancel_count
        || transactions.len() != drill.base_actions.len()
        || !actions_valid
        || !valid_nonempty_hex(&drill.ic_pause.response_hex)
        || !hex_sha256_matches(
            &drill.ic_pause.response_hex,
            &drill.ic_pause.response_sha256,
        )
        || !principal(&drill.ic_pause.pause_principal)
        || !valid_hash32(&drill.ic_pause.request_id)
        || !valid_nonempty_hex(&drill.ic_pause.certificate_hex)
        || !hex_sha256_matches(
            &drill.ic_pause.certificate_hex,
            &drill.ic_pause.certificate_sha256,
        )
        || drill.ic_pause.audit_sequence == 0
        || !valid_nonempty_hex(&drill.ic_pause.audit_raw_hex)
        || !hex_sha256_matches(&drill.ic_pause.audit_raw_hex, &drill.ic_pause.audit_sha256)
        || !drill
            .routing_sha256
            .eq_ignore_ascii_case(&profile.monitoring.routing_sha256)
    {
        return Err("monitor drill does not prove the authenticated pause/cancel path".into());
    }
    Ok(())
}

fn validate_keeper_drill(
    root: &Path,
    manifest: &ReleaseManifest,
    profile: &Profile,
    now: u64,
) -> Result<(), String> {
    let drill: KeeperDrill = read_json(&root.join("keeper-drill.json"))?;
    let monitoring_path = root.join("monitoring-receipt.json");
    let monitoring_bytes = fs::read(&monitoring_path)
        .map_err(|error| format!("{}: {error}", monitoring_path.display()))?;
    let monitoring: MonitoringReceipt = serde_json::from_slice(&monitoring_bytes)
        .map_err(|error| format!("{}: {error}", monitoring_path.display()))?;
    let monitoring_sha256 = hex(&Sha256::digest(&monitoring_bytes));
    let withdrawal_id = decode_hex(&monitoring.withdrawal_id)?;
    let paid_response = decode_hex(&monitoring.paid.response_hex)?;
    let withdrawal: Option<WithdrawalView> = Decode!(&paid_response, Option<WithdrawalView>)
        .map_err(|error| format!("invalid monitoring withdrawal response: {error}"))?;
    let paid_withdrawal = withdrawal
        .as_ref()
        .filter(|view| view.state == WithdrawalPhaseView::Paid)
        .ok_or("monitoring receipt does not contain a Paid withdrawal")?;
    let withdrawal_committed_topic = evm_topic(
        "WithdrawalCommitted(uint256,address,uint256,uint256,uint256,uint256,bytes,bytes32)",
    );
    let elapsed = drill
        .paid_at_unix
        .checked_sub(drill.burned_at_unix)
        .ok_or("keeper drill Paid time precedes burn")?;
    validate_evidence_time(drill.burned_at_unix, manifest.created_at_unix, now)?;
    validate_evidence_time(drill.paid_at_unix, manifest.created_at_unix, now)?;
    let keeper_ids = drill
        .keeper_ids
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let failure_domains = drill
        .keeper_failure_domains
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if drill.schema_version != 1
        || drill.source_revision != manifest.source_revision
        || !drill
            .source_tree_sha256
            .eq_ignore_ascii_case(&manifest.source_tree_sha256)
        || drill.bridge_canister_id != profile.bridge_canister_id
        || !valid_hash32(&drill.withdrawal_id)
        || !valid_hash32(&drill.burn_transaction_hash)
        || drill.maximum_unprocessed_seconds == 0
        || elapsed > drill.maximum_unprocessed_seconds
        || drill.keeper_ids.len() != 2
        || keeper_ids.len() != 2
        || keeper_ids
            .iter()
            .any(|value| value.is_empty() || value.len() > 128)
        || drill.keeper_failure_domains.len() != 2
        || failure_domains.len() != 2
        || failure_domains
            .iter()
            .any(|value| value.is_empty() || value.len() > 128)
        || !valid_sha256(&drill.monitoring_receipt_sha256)
        || !drill
            .monitoring_receipt_sha256
            .eq_ignore_ascii_case(&monitoring_sha256)
        || !drill.manual_fallback_drilled
        || monitoring.schema_version != 1
        || monitoring.source_revision != manifest.source_revision
        || !monitoring
            .source_tree_sha256
            .eq_ignore_ascii_case(&manifest.source_tree_sha256)
        || monitoring.bridge_canister_id != profile.bridge_canister_id
        || !monitoring
            .withdrawal_id
            .eq_ignore_ascii_case(&drill.withdrawal_id)
        || !monitoring
            .burn_transaction_hash
            .eq_ignore_ascii_case(&drill.burn_transaction_hash)
        || monitoring.burn.base_chain_id != profile.chain_id
        || !monitoring
            .burn
            .bridge_contract
            .eq_ignore_ascii_case(&profile.bridge_contract)
        || monitoring.burn.block_number == 0
        || !valid_hash32(&monitoring.burn.block_hash)
        || monitoring.burn.receipt_status != 1
        || !monitoring
            .burn
            .withdrawal_committed_topic
            .eq_ignore_ascii_case(&withdrawal_committed_topic)
        || !monitoring
            .burn
            .withdrawal_id_topic
            .eq_ignore_ascii_case(&monitoring.withdrawal_id)
        || !monitoring.burn.canonical_finalized
        || monitoring.paid.observed_at_unix != drill.paid_at_unix
        || monitoring.paid.state != "Paid"
        || !monitoring.paid.authenticated_query
        || !valid_nonempty_hex(&monitoring.paid.response_hex)
        || !hex_sha256_matches(
            &monitoring.paid.response_hex,
            &monitoring.paid.response_sha256,
        )
        || paid_withdrawal.withdrawal_id != withdrawal_id
    {
        return Err(
            "Gate B keeper drill does not prove two independent settlement paths through Paid"
                .into(),
        );
    }
    Ok(())
}

fn validate_provider_independence_receipt(
    root: &Path,
    manifest: &ReleaseManifest,
    profile: &Profile,
    now: u64,
) -> Result<(), String> {
    let receipt: ProviderIndependenceReceipt = read_json(&root.join("provider-independence.json"))?;
    validate_evidence_time(receipt.observed_at_unix, manifest.created_at_unix, now)?;
    let expected_review = hex(&canonical_sha256(&profile.rpc_providers)?);
    if receipt.schema_version != 1
        || receipt.proposal_id == 0
        || !receipt
            .provider_review_sha256
            .eq_ignore_ascii_case(&expected_review)
        || !receipt.dns_monitoring_enabled
        || !receipt.endpoint_monitoring_enabled
        || receipt.drift_action != "pause-and-require-reactivation"
        || !activation_raw_digest_matches(
            &receipt.governance_query_response_hex,
            &receipt.governance_query_response_sha256,
        )?
    {
        return Err(
            "provider independence receipt is incomplete or not bound to the RPC profile".into(),
        );
    }
    Ok(())
}

fn hex_sha256_matches(value: &str, expected: &str) -> bool {
    decode_hex(value)
        .map(|bytes| hex(&Sha256::digest(bytes)).eq_ignore_ascii_case(expected))
        .unwrap_or(false)
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

fn validate_profile(profile: &Profile, production: bool) -> Result<(), String> {
    if profile.schema_version != RELEASE_PROFILE_SCHEMA_VERSION {
        return Err("obsolete or unknown release profile schema".into());
    }
    if production && profile.test_assets_only {
        return Err("production deploy rejects test-only profiles".into());
    }
    let expected_chain = match profile.environment.as_str() {
        "mainnet-candidate" => 8453,
        "base-sepolia" => 84532,
        _ => return Err("unsupported environment".into()),
    };
    if profile.chain_id != expected_chain || profile.decimals != 8 {
        return Err("KINIC or chain identity mismatch".into());
    }
    if profile.canister_schema_version != CURRENT_STABLE_SCHEMA_VERSION {
        return Err("profile must bind the current stable schema version".into());
    }
    if !principal(&profile.bridge_canister_id) || !credential_free_https(&profile.ic_host) {
        return Err("invalid release endpoint".into());
    }
    if production {
        if profile.base_rpc_url.is_some() || !profile.rpc_providers.is_empty() {
            return Err("production uses only the built-in BaseMainnet EVM RPC providers".into());
        }
    } else {
        let base_rpc_url = profile
            .base_rpc_url
            .as_deref()
            .ok_or("staging Base RPC URL is missing")?;
        if !credential_free_https(base_rpc_url)
            || !profile
                .rpc_providers
                .iter()
                .any(|provider| provider.url.eq_ignore_ascii_case(base_rpc_url))
        {
            return Err("invalid staging release endpoint".into());
        }
    }
    if profile.evm_rpc_canister_id != OFFICIAL_EVM_RPC_CANISTER {
        return Err("profile must bind the official EVM RPC canister ID".into());
    }
    if !valid_nonzero_hash32(&profile.deployment_instance_id)
        || !valid_nonzero_hash32(&profile.minimum_withdrawal_id)
        || !valid_sha256(&profile.bridge_canister_wasm_sha256)
        || !valid_sha256(&profile.bridge_runtime_bytecode_sha256)
        || !valid_sha256(&profile.bsns_runtime_bytecode_sha256)
        || !valid_sha256(&profile.bsns_runtime_template_sha256)
    {
        return Err("profile must bind a deployment instance ID, minimum withdrawal ID, and Bridge artifact hashes".into());
    }
    let fresh_withdrawal_boundary = format!("0x{}01", "00".repeat(31));
    if production
        && profile.environment == "mainnet-candidate"
        && !profile
            .minimum_withdrawal_id
            .eq_ignore_ascii_case(&fresh_withdrawal_boundary)
    {
        return Err(
            "first production deployment must derive minimum_withdrawal_id from Bridge nextWithdrawalId == 1"
                .into(),
        );
    }
    if profile.environment == "mainnet-candidate"
        && (profile.test_assets_only
            || profile.ledger_canister_id != KINIC_LEDGER
            || profile.index_canister_id != KINIC_INDEX
            || profile.root_canister_id != KINIC_ROOT
            || profile.governance_principal != KINIC_GOVERNANCE)
    {
        return Err("mainnet profile must bind the canonical KINIC canisters".into());
    }
    if profile.environment == "base-sepolia"
        && (!profile.test_assets_only
            || !principal(&profile.ledger_canister_id)
            || !principal(&profile.index_canister_id)
            || profile.ledger_canister_id == profile.index_canister_id)
    {
        return Err(
            "Sepolia profile must use distinct test-only Ledger and Index canisters".into(),
        );
    }
    if profile.ecdsa_key_name.is_empty()
        || profile.ecdsa_derivation_path.len() > 10
        || profile
            .ecdsa_derivation_path
            .iter()
            .any(|c| c.is_empty() || c.len() > 128)
    {
        return Err("invalid threshold ECDSA key configuration".into());
    }
    let addresses = [
        &profile.bridge_contract,
        &profile.bsns_contract,
        &profile.expected_bridge_signer,
        &profile.governance_operator,
        &profile.runtime_administrator,
        &profile.independent_canceller,
        &profile.timelock.address,
    ];
    if addresses.iter().any(|value| !evm_address(value)) {
        return Err("invalid EVM role address".into());
    }
    let unique = addresses
        .iter()
        .map(|v| v.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if unique.len() != addresses.len() {
        return Err("EVM roles must be distinct".into());
    }
    if !valid_hash32(&profile.timelock.runtime_code_hash)
        || profile.timelock.runtime_code_hash[2..]
            .bytes()
            .all(|byte| byte == b'0')
        || profile.timelock.minimum_delay_seconds < 24 * 60 * 60
        || profile.timelock.external_admins != 0
        || !profile
            .timelock
            .proposer
            .eq_ignore_ascii_case(&profile.governance_operator)
        || !profile
            .timelock
            .executor
            .eq_ignore_ascii_case(&profile.governance_operator)
        || !profile
            .timelock
            .canceller
            .eq_ignore_ascii_case(&profile.independent_canceller)
    {
        return Err("unsafe Timelock configuration".into());
    }
    if profile.governance_ecdsa_derivation_path.is_empty()
        || profile.governance_ecdsa_derivation_path.len() > 10
        || profile.governance_ecdsa_derivation_path == profile.ecdsa_derivation_path
        || profile
            .governance_ecdsa_derivation_path
            .iter()
            .any(|c| c.is_empty() || c.len() > 128)
    {
        return Err("invalid or overlapping governance ECDSA derivation path".into());
    }
    let deployment = &profile.initial_base_deployment;
    let deployer = decode_address(&deployment.deployer_address)?;
    if deployment
        .deployer_address
        .eq_ignore_ascii_case(&profile.governance_operator)
        || deployment
            .deployer_address
            .eq_ignore_ascii_case(&profile.runtime_administrator)
        || deployment
            .deployer_address
            .eq_ignore_ascii_case(&profile.independent_canceller)
        || deployment
            .deployer_address
            .eq_ignore_ascii_case(&profile.expected_bridge_signer)
        || deployment.gas_limit == 0
        || deployment.max_fee_per_gas == 0
        || deployment.max_priority_fee_per_gas > deployment.max_fee_per_gas
        || !address_matches_create(
            &profile.timelock.address,
            deployer,
            deployment.starting_nonce,
        )
        || !address_matches_create(
            &profile.bridge_contract,
            deployer,
            deployment
                .starting_nonce
                .checked_add(1)
                .ok_or("deployment nonce overflow")?,
        )
        || !address_matches_create(
            &profile.bsns_contract,
            decode_address(&profile.bridge_contract)?,
            1,
        )
    {
        return Err("invalid initial Base deployment binding".into());
    }
    let principals = [
        &profile.governance_principal,
        &profile.confirmation_relayer_principal,
        &profile.pause_principal,
        &profile.fee_recipient,
    ];
    let mut unique_principals = BTreeSet::new();
    for value in principals {
        if !principal(value) || !unique_principals.insert(value) {
            return Err("invalid or overlapping IC operational principal".into());
        }
    }
    if !production && profile.rpc_providers.len() != 3 {
        return Err("exactly three RPC providers are required".into());
    }
    let urls = profile
        .rpc_providers
        .iter()
        .map(|p| p.url.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if !production
        && (urls.len() != 3
            || profile
                .rpc_providers
                .iter()
                .any(|p| !credential_free_https(p.url.trim())))
    {
        return Err("RPC providers must be three distinct credential-free HTTPS URLs".into());
    }
    let metadata_valid = profile.rpc_providers.iter().all(|provider| {
        [
            &provider.operator,
            &provider.dns_owner,
            &provider.failure_domain,
        ]
        .iter()
        .all(|value| !value.trim().is_empty() && value.len() <= 128)
    });
    let operators = profile
        .rpc_providers
        .iter()
        .map(|provider| provider.operator.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let dns_owners = profile
        .rpc_providers
        .iter()
        .map(|provider| provider.dns_owner.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let failure_domains = profile
        .rpc_providers
        .iter()
        .map(|provider| provider.failure_domain.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if !production
        && (!metadata_valid
            || operators.len() != 3
            || dns_owners.len() != 3
            || failure_domains.len() != 3)
    {
        return Err("RPC providers must bind three independently owned failure domains".into());
    }
    if profile.monitoring.detection_minutes != 5
        || profile.monitoring.acknowledgement_minutes != 15
        || profile.monitoring.pause_both_sides_minutes != 60
        || !valid_sha256(&profile.monitoring.routing_sha256)
    {
        return Err("5/15/60 monitoring SLO is invalid".into());
    }
    let r = &profile.rate_limits;
    if !(60..=300).contains(&r.deposit_window_seconds)
        || r.deposit_per_principal == 0
        || r.deposit_per_principal > r.deposit_global
        || r.deposit_global > 100
        || !(60..=3_600).contains(&r.notification_window_seconds)
        || !(1..=100).contains(&r.notification_global)
        || !(1..=100).contains(&r.notification_ingestion_global)
        || !(60..=3_600).contains(&r.settlement_window_seconds)
        || r.settlement_per_record == 0
        || r.settlement_per_record > r.settlement_per_principal
        || r.settlement_per_principal > r.settlement_global
        || !(1..=900).contains(&r.settlement_retry_interval_seconds)
    {
        return Err("unsafe rate-limit configuration".into());
    }
    let replacement = profile.governance_replacement;
    if !(1..=8).contains(&replacement.max_replacements)
        || !(1_000..=5_000).contains(&replacement.fee_bump_bps)
    {
        return Err("unsafe governance replacement policy".into());
    }
    let p = &profile.parameters;
    let _ = p
        .mint_throughput_limit
        .checked_mul(2)
        .ok_or("mint window boundary overflow")?;
    if p.ledger_fee != 100_000
        || p.per_deposit_limit == 0
        || p.mint_throughput_limit == 0
        || p.per_deposit_limit > p.mint_throughput_limit
        || !(60..=86_400).contains(&p.mint_window_duration_seconds)
        || p.max_service_fee != p.ledger_fee.saturating_mul(10_000)
        || p.service_fee != p.ledger_fee.saturating_mul(500)
        || p.service_fee > p.max_service_fee
        || p.gas_limit_ceiling == 0
        || p.max_fee_per_gas_ceiling == 0
        || p.max_priority_fee_per_gas_ceiling > p.max_fee_per_gas_ceiling
        || p.l1_fee_per_transaction_ceiling_wei == 0
        || !(30..=300).contains(&p.quote_validity_seconds)
        || !(10_000..=20_000).contains(&p.gas_limit_multiplier_bps)
        || !(10_000..=100_000).contains(&p.base_fee_multiplier_bps)
        || !(10_000..=30_000).contains(&p.l1_fee_multiplier_bps)
        || p.cycles_floor == 0
        || p.settlement_cycle_ceiling == 0
    {
        return Err("unsafe or inconsistent parameter set".into());
    }
    Ok(())
}

fn utf16_cmp(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn canonical_json(value: &Value, out: &mut Vec<u8>) -> Result<(), String> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(v) => out.extend_from_slice(if *v { b"true" } else { b"false" }),
        Value::Number(v) => {
            if !v.is_i64() && !v.is_u64() {
                return Err("canonical evidence JSON forbids floating-point numbers".into());
            }
            const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
            if v.as_u64().is_some_and(|value| value > MAX_SAFE_INTEGER)
                || v.as_i64().is_some_and(|value| {
                    value < -(MAX_SAFE_INTEGER as i64) || value > MAX_SAFE_INTEGER as i64
                })
            {
                return Err("canonical evidence JSON requires IEEE-754 safe integers".into());
            }
            out.extend_from_slice(v.to_string().as_bytes());
        }
        Value::String(v) => out.extend_from_slice(
            serde_json::to_string(v)
                .map_err(|e| e.to_string())?
                .as_bytes(),
        ),
        Value::Array(values) => {
            out.push(b'[');
            for (i, value) in values.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                canonical_json(value, out)?;
            }
            out.push(b']');
        }
        Value::Object(values) => {
            let mut keys = values.keys().collect::<Vec<_>>();
            keys.sort_by(|a, b| utf16_cmp(a, b));
            out.push(b'{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                out.extend_from_slice(
                    serde_json::to_string(key)
                        .map_err(|e| e.to_string())?
                        .as_bytes(),
                );
                out.push(b':');
                canonical_json(&values[*key], out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

fn canonical_sha256<T: Serialize>(value: &T) -> Result<[u8; 32], String> {
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    canonical_json(&value, &mut bytes)?;
    Ok(Sha256::digest(bytes).into())
}

fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    canonical_json(&value, &mut bytes)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_generated<T: Serialize>(root: &Path, name: &str, value: &T) -> Result<String, String> {
    fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let bytes = canonical_bytes(value)?;
    let path = root.join(name);
    let temporary = root.join(format!(".{name}.tmp-{}", process::id()));
    fs::write(&temporary, &bytes).map_err(|e| format!("{}: {e}", temporary.display()))?;
    fs::rename(&temporary, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(hex(&Sha256::digest(bytes)))
}

fn production_init_args(
    input: &ProductionCanisterInitInput,
) -> Result<ProductionCanisterInitArgs, String> {
    let bytes = |name: &str, value: &str, expected: usize| -> Result<Vec<u8>, String> {
        let decoded = decode_hex(value)?;
        if decoded.len() != expected || decoded.iter().all(|byte| *byte == 0) {
            return Err(format!("{name} must be a nonzero {expected}-byte value"));
        }
        Ok(decoded)
    };
    let principal = |name: &str, value: &str| -> Result<Principal, String> {
        let parsed = Principal::from_text(value).map_err(|error| format!("{name}: {error}"))?;
        if parsed == Principal::anonymous() {
            return Err(format!("{name} must not be anonymous"));
        }
        Ok(parsed)
    };
    if !input.custom_evm_rpc_urls.is_empty()
        || input.base_chain_id != 8453
        || input.ledger_canister_id != KINIC_LEDGER
        || input.index_canister_id != KINIC_INDEX
        || input.evm_rpc_canister_id != OFFICIAL_EVM_RPC_CANISTER
        || input.ecdsa_key_name != "key_1"
        || input.ecdsa_derivation_path_utf8.is_empty()
        || input.governance_ecdsa_derivation_path_utf8.is_empty()
        || input.ecdsa_derivation_path_utf8 == input.governance_ecdsa_derivation_path_utf8
        || input
            .ecdsa_derivation_path_utf8
            .iter()
            .chain(input.governance_ecdsa_derivation_path_utf8.iter())
            .any(|part| part.is_empty() || part.len() > 128)
        || !input.fee_recipient.subaccount_hex.is_empty()
        || input.expected_minimum_service_fee != 100_000
        || input.expected_bsns_decimals != 8
        || input.expected_timelock_minimum_delay_seconds < 86_400
        || input.cycles_floor == 0
        || input.settlement_cycle_ceiling == 0
        || !(60..=300).contains(&input.deposit_rate_limit_window_seconds)
        || input.deposit_rate_limit_per_principal == 0
        || input.deposit_rate_limit_per_principal > input.deposit_rate_limit_global
        || input.deposit_rate_limit_global > 100
        || !(60..=3_600).contains(&input.notification_rate_limit_window_seconds)
        || input.notification_rate_limit_global == 0
        || input.notification_ingestion_rate_limit_global == 0
        || !(60..=3_600).contains(&input.settlement_rate_limit_window_seconds)
        || input.settlement_rate_limit_per_record == 0
        || input.settlement_rate_limit_per_record > input.settlement_rate_limit_per_principal
        || input.settlement_rate_limit_per_principal > input.settlement_rate_limit_global
        || !(1..=900).contains(&input.settlement_retry_interval_seconds)
        || input.governance_evm_fee.gas_limit_ceiling == 0
        || input.governance_evm_fee.max_fee_per_gas_ceiling == 0
        || input.governance_evm_fee.max_priority_fee_per_gas_ceiling
            > input.governance_evm_fee.max_fee_per_gas_ceiling
        || !(30..=300).contains(&input.governance_evm_fee.quote_validity_seconds)
        || !(1..=8).contains(&input.governance_replacement.max_replacements)
        || !(1_000..=5_000).contains(&input.governance_replacement.fee_bump_bps)
    {
        return Err("unsafe production Canister initialization input".into());
    }
    Ok(ProductionCanisterInitArgs {
        ledger_canister_id: principal("ledger_canister_id", &input.ledger_canister_id)?,
        index_canister_id: principal("index_canister_id", &input.index_canister_id)?,
        evm_rpc_canister_id: principal("evm_rpc_canister_id", &input.evm_rpc_canister_id)?,
        custom_evm_rpc_urls: Vec::new(),
        base_chain_id: input.base_chain_id,
        bridge_contract: bytes("bridge_contract_hex", &input.bridge_contract_hex, 20)?,
        expected_bridge_runtime_sha256: bytes(
            "expected_bridge_runtime_sha256_hex",
            &input.expected_bridge_runtime_sha256_hex,
            32,
        )?,
        timelock_contract: bytes("timelock_contract_hex", &input.timelock_contract_hex, 20)?,
        expected_timelock_minimum_delay_seconds: input.expected_timelock_minimum_delay_seconds,
        expected_bsns_runtime_sha256: bytes(
            "expected_bsns_runtime_sha256_hex",
            &input.expected_bsns_runtime_sha256_hex,
            32,
        )?,
        expected_bsns_decimals: input.expected_bsns_decimals,
        expected_minimum_service_fee: input.expected_minimum_service_fee,
        deployment_instance_id: bytes(
            "deployment_instance_id_hex",
            &input.deployment_instance_id_hex,
            32,
        )?,
        minimum_withdrawal_id: bytes(
            "minimum_withdrawal_id_hex",
            &input.minimum_withdrawal_id_hex,
            32,
        )?,
        ecdsa_key_name: input.ecdsa_key_name.clone(),
        ecdsa_derivation_path: input
            .ecdsa_derivation_path_utf8
            .iter()
            .map(|part| part.as_bytes().to_vec())
            .collect(),
        governance_ecdsa_derivation_path: input
            .governance_ecdsa_derivation_path_utf8
            .iter()
            .map(|part| part.as_bytes().to_vec())
            .collect(),
        deposit_rate_limit_window_seconds: input.deposit_rate_limit_window_seconds,
        deposit_rate_limit_global: input.deposit_rate_limit_global,
        deposit_rate_limit_per_principal: input.deposit_rate_limit_per_principal,
        notification_rate_limit_window_seconds: input.notification_rate_limit_window_seconds,
        notification_rate_limit_global: input.notification_rate_limit_global,
        notification_ingestion_rate_limit_global: input.notification_ingestion_rate_limit_global,
        settlement_rate_limit_window_seconds: input.settlement_rate_limit_window_seconds,
        settlement_rate_limit_global: input.settlement_rate_limit_global,
        settlement_rate_limit_per_principal: input.settlement_rate_limit_per_principal,
        settlement_rate_limit_per_record: input.settlement_rate_limit_per_record,
        settlement_retry_interval_seconds: input.settlement_retry_interval_seconds,
        governance_evm_fee: input.governance_evm_fee,
        governance_replacement: input.governance_replacement,
        cycles_floor: input.cycles_floor,
        settlement_cycle_ceiling: input.settlement_cycle_ceiling,
        governance_principal: principal("governance_principal", &input.governance_principal)?,
        pause_principal: principal("pause_principal", &input.pause_principal)?,
        confirmation_relayer_principal: principal(
            "confirmation_relayer_principal",
            &input.confirmation_relayer_principal,
        )?,
        fee_recipient: OperationalFeeRecipientView {
            owner: principal("fee_recipient.owner", &input.fee_recipient.owner)?,
            subaccount: Vec::new(),
        },
    })
}

fn validate_production_canister_plan(plan: &ProductionCanisterPlan) -> Result<Vec<u8>, String> {
    if plan.schema_version != 2
        || plan.environment != "production"
        || plan.source_revision.trim().is_empty()
        || !valid_sha256(&plan.source_tree_sha256)
        || !principal(&plan.bridge_canister_id)
        || !valid_sha256(&plan.bridge_canister_wasm_sha256)
    {
        return Err("invalid production Canister install plan identity".into());
    }
    if plan.init.governance_evm_fee != production_bootstrap_evm_fee()
        || plan.init.cycles_floor != PRODUCTION_BOOTSTRAP_CYCLES_FLOOR
        || plan.init.settlement_cycle_ceiling != PRODUCTION_BOOTSTRAP_SETTLEMENT_CYCLE_CEILING
    {
        return Err(
            "production Canister install plan must use the fixed bootstrap operational config"
                .into(),
        );
    }
    Encode!(&production_init_args(&plan.init)?).map_err(|error| error.to_string())
}

fn validate_production_canister_plan_against_profile(
    plan: &ProductionCanisterPlan,
    profile: &Profile,
) -> Result<(), String> {
    let init = &plan.init;
    let matches = plan.bridge_canister_id == profile.bridge_canister_id
        && plan
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        && init.ledger_canister_id == profile.ledger_canister_id
        && init.index_canister_id == profile.index_canister_id
        && init.evm_rpc_canister_id == profile.evm_rpc_canister_id
        && init.base_chain_id == profile.chain_id
        && format!("0x{}", init.bridge_contract_hex).eq_ignore_ascii_case(&profile.bridge_contract)
        && init
            .expected_bridge_runtime_sha256_hex
            .eq_ignore_ascii_case(&profile.bridge_runtime_bytecode_sha256)
        && format!("0x{}", init.timelock_contract_hex)
            .eq_ignore_ascii_case(&profile.timelock.address)
        && init.expected_timelock_minimum_delay_seconds == profile.timelock.minimum_delay_seconds
        && init
            .expected_bsns_runtime_sha256_hex
            .eq_ignore_ascii_case(&profile.bsns_runtime_bytecode_sha256)
        && init.expected_bsns_decimals == profile.decimals
        && init.expected_minimum_service_fee == profile.parameters.ledger_fee
        && format!("0x{}", init.deployment_instance_id_hex)
            .eq_ignore_ascii_case(&profile.deployment_instance_id)
        && format!("0x{}", init.minimum_withdrawal_id_hex)
            .eq_ignore_ascii_case(&profile.minimum_withdrawal_id)
        && init.ecdsa_key_name == profile.ecdsa_key_name
        && init.ecdsa_derivation_path_utf8 == profile.ecdsa_derivation_path
        && init.governance_ecdsa_derivation_path_utf8 == profile.governance_ecdsa_derivation_path
        && init.deposit_rate_limit_window_seconds == profile.rate_limits.deposit_window_seconds
        && init.deposit_rate_limit_global == profile.rate_limits.deposit_global
        && init.deposit_rate_limit_per_principal == profile.rate_limits.deposit_per_principal
        && init.notification_rate_limit_window_seconds
            == profile.rate_limits.notification_window_seconds
        && init.notification_rate_limit_global == profile.rate_limits.notification_global
        && init.notification_ingestion_rate_limit_global
            == profile.rate_limits.notification_ingestion_global
        && init.settlement_rate_limit_window_seconds
            == profile.rate_limits.settlement_window_seconds
        && init.settlement_rate_limit_global == profile.rate_limits.settlement_global
        && init.settlement_rate_limit_per_principal == profile.rate_limits.settlement_per_principal
        && init.settlement_rate_limit_per_record == profile.rate_limits.settlement_per_record
        && init.settlement_retry_interval_seconds
            == profile.rate_limits.settlement_retry_interval_seconds
        && init.governance_replacement == profile.governance_replacement
        && init.governance_principal == profile.governance_principal
        && init.pause_principal == profile.pause_principal
        && init.confirmation_relayer_principal == profile.confirmation_relayer_principal
        && init.fee_recipient.owner == profile.fee_recipient
        && init.fee_recipient.subaccount_hex.is_empty();
    if !matches {
        return Err("production Canister install plan does not match the release profile".into());
    }
    Ok(())
}

fn render_production_canister_inputs(plan_path: &Path, output: &Path) -> Result<(), String> {
    let plan: ProductionCanisterPlan = read_json(plan_path)?;
    let candid = validate_production_canister_plan(&plan)?;
    let plan_sha256 = hex(&canonical_sha256(&plan)?);
    let init_sha256 = write_generated(output, "canister-init.json", &plan.init)?;
    fs::create_dir_all(output).map_err(|error| error.to_string())?;
    let temporary = output.join(format!(".canister-init.bin.tmp-{}", process::id()));
    fs::write(&temporary, &candid).map_err(|error| error.to_string())?;
    let candid_path = output.join("canister-init.bin");
    fs::rename(&temporary, &candid_path).map_err(|error| error.to_string())?;
    let candid_sha256 = hex(&Sha256::digest(&candid));
    let manifest = serde_json::json!({
        "schema_version": 2,
        "plan_sha256": plan_sha256,
        "source_revision": plan.source_revision,
        "source_tree_sha256": plan.source_tree_sha256,
        "canister_id": plan.bridge_canister_id,
        "module_sha256": plan.bridge_canister_wasm_sha256,
        "canister_init_sha256": init_sha256,
        "init_candid_sha256": candid_sha256,
    });
    write_generated(output, "production-canister-install-inputs.json", &manifest)?;
    println!("rendered production Canister install inputs plan_sha256={plan_sha256}");
    Ok(())
}

fn validate_production_canister_receipt(
    profile: &Profile,
    receipt: &ProductionCanisterInstallReceipt,
) -> Result<(), String> {
    let init_candid = validate_production_canister_plan(&receipt.plan)?;
    validate_production_canister_plan_against_profile(&receipt.plan, profile)?;
    let plan_sha256 = hex(&canonical_sha256(&receipt.plan)?);
    let rpc_url_hash = hex(&canonical_sha256(&Vec::<String>::new())?);
    let operational = expected_bootstrap_operational_config_sha256(
        &receipt.plan.init,
        &receipt.governance_operator,
        receipt.mint_authorization_ttl_seconds,
        receipt.mint_authorization_epoch,
    )?;
    validate_live_runtime_binding(
        &receipt.runtime_binding,
        profile,
        &rpc_url_hash,
        &operational,
    )?;
    if receipt.schema_version != PRODUCTION_CANISTER_INSTALL_RECEIPT_SCHEMA_VERSION
        || !receipt.plan_sha256.eq_ignore_ascii_case(&plan_sha256)
        || receipt.source_revision != receipt.plan.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&receipt.plan.source_tree_sha256)
        || receipt.canister_id != receipt.plan.bridge_canister_id
        || !receipt
            .module_sha256
            .eq_ignore_ascii_case(&receipt.plan.bridge_canister_wasm_sha256)
        || !receipt
            .init_candid_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(init_candid)))
        || receipt.source_revision.trim().is_empty()
        || !valid_sha256(&receipt.source_tree_sha256)
        || receipt.canister_id != profile.bridge_canister_id
        || !principal(&receipt.installer_principal)
        || !receipt
            .module_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !valid_sha256(&receipt.init_candid_sha256)
        || !receipt
            .governance_operator
            .eq_ignore_ascii_case(&profile.governance_operator)
        || !receipt
            .runtime_administrator
            .eq_ignore_ascii_case(&profile.runtime_administrator)
        || !receipt
            .independent_canceller
            .eq_ignore_ascii_case(&profile.independent_canceller)
        || !receipt.storage_validation_complete
        || !receipt.storage_checksum_complete
        || !receipt.deposits_paused
        || !receipt.state_is_empty
        || !receipt.cycles_reserve_sufficient
    {
        return Err(
            "production Canister install receipt does not match the release profile".into(),
        );
    }
    Ok(())
}

fn validate_production_canister_receipt_files(
    profile_path: &Path,
    receipt_path: &Path,
) -> Result<String, String> {
    let profile: Profile = read_json(profile_path)?;
    validate_profile(&profile, true)?;
    let receipt: ProductionCanisterInstallReceipt = read_json(receipt_path)?;
    validate_production_canister_receipt(&profile, &receipt)?;
    Ok(hex(&Sha256::digest(
        fs::read(receipt_path).map_err(|error| error.to_string())?,
    )))
}

fn validate_completed_gate_a_receipt(
    bundle: &ValidatedBundle,
    receipt: &GateAReceipt,
    install_receipt: &ProductionCanisterInstallReceipt,
    deployment_binding: &ProductionDeploymentBinding,
) -> Result<(), String> {
    validate_production_canister_receipt(&bundle.profile, install_receipt)?;
    validate_production_canister_receipt(&bundle.profile, &receipt.canister_install)?;
    let embedded_install_sha256 = canonical_sha256(&receipt.canister_install)?;
    let external_install_sha256 = canonical_sha256(install_receipt)?;
    let gate_a_profile_sha256 = hex(&canonical_sha256(&bundle.profile)?);
    let mut post_deploy_profile = bundle.profile.clone();
    post_deploy_profile.deployment_block = receipt.bridge_deployment_block_number;
    let post_deploy_profile_sha256 = hex(&Sha256::digest(canonical_bytes(&post_deploy_profile)?));
    if bundle.manifest.test_only
        || receipt.schema_version != 2
        || !receipt
            .gate_a_manifest_sha256
            .eq_ignore_ascii_case(&bundle.manifest_sha256)
        || receipt.release_id != bundle.manifest.release_id
        || receipt.source_revision != bundle.manifest.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || !receipt
            .gate_a_profile_sha256
            .eq_ignore_ascii_case(&gate_a_profile_sha256)
        || !receipt
            .post_deploy_profile_sha256
            .eq_ignore_ascii_case(&post_deploy_profile_sha256)
        || !receipt
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
        || !receipt
            .bridge_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&bundle.profile.bridge_runtime_bytecode_sha256)
        || !valid_hash32(&receipt.bridge_deployment_transaction_hash)
        || !valid_hash32(&receipt.bridge_deployment_block_hash)
        || !valid_hash32(&receipt.timelock_deployment_transaction_hash)
        || !valid_hash32(&receipt.timelock_deployment_block_hash)
        || receipt.bridge_deployment_block_number == 0
        || receipt.timelock_deployment_block_number == 0
        || receipt.timelock_deployment_block_number > receipt.bridge_deployment_block_number
        || !deployment_binding
            .deployer_address
            .eq_ignore_ascii_case(&bundle.profile.initial_base_deployment.deployer_address)
        || deployment_binding.starting_nonce
            != bundle.profile.initial_base_deployment.starting_nonce
        || !deployment_binding
            .timelock
            .address
            .eq_ignore_ascii_case(&bundle.profile.timelock.address)
        || !deployment_binding
            .bridge
            .address
            .eq_ignore_ascii_case(&bundle.profile.bridge_contract)
        || !deployment_binding
            .timelock
            .transaction_hash
            .eq_ignore_ascii_case(&receipt.timelock_deployment_transaction_hash)
        || deployment_binding.timelock.block_number != receipt.timelock_deployment_block_number
        || !deployment_binding
            .timelock
            .block_hash
            .eq_ignore_ascii_case(&receipt.timelock_deployment_block_hash)
        || !deployment_binding
            .bridge
            .transaction_hash
            .eq_ignore_ascii_case(&receipt.bridge_deployment_transaction_hash)
        || deployment_binding.bridge.block_number != receipt.bridge_deployment_block_number
        || !deployment_binding
            .bridge
            .block_hash
            .eq_ignore_ascii_case(&receipt.bridge_deployment_block_hash)
        || embedded_install_sha256 != external_install_sha256
        || receipt.canister_install.source_revision != bundle.manifest.source_revision
        || !receipt
            .canister_install
            .source_tree_sha256
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
    {
        return Err(
            "completed Gate A receipt is not strictly bound to the deployment and Canister install"
                .into(),
        );
    }
    Ok(())
}

fn validate_production_handover_receipt_files(
    bundle_path: &Path,
    gate_a_receipt_path: &Path,
    install_receipt_path: &Path,
    deployment_binding_path: &Path,
) -> Result<String, String> {
    let bundle = validate_bundle(bundle_path, false)?;
    let gate_a_receipt: GateAReceipt = read_json(gate_a_receipt_path)?;
    let install_receipt: ProductionCanisterInstallReceipt = read_json(install_receipt_path)?;
    let deployment_binding: ProductionDeploymentBinding = read_json(deployment_binding_path)?;
    if fs::read(deployment_binding_path).map_err(|error| error.to_string())?
        != canonical_bytes(&deployment_binding)?
    {
        return Err("deployment binding is not the canonical driver output".into());
    }
    validate_completed_gate_a_receipt(
        &bundle,
        &gate_a_receipt,
        &install_receipt,
        &deployment_binding,
    )?;
    Ok(hex(&Sha256::digest(
        fs::read(gate_a_receipt_path).map_err(|error| error.to_string())?,
    )))
}

fn validate_production_handover_candidate_files(
    bundle_path: &Path,
    final_profile_path: &Path,
    measurements_path: &Path,
    gate_a_receipt_path: &Path,
    install_receipt_path: &Path,
    deployment_binding_path: &Path,
) -> Result<
    (
        ValidatedBundle,
        GateAReceipt,
        ProductionCanisterInstallReceipt,
        Profile,
    ),
    String,
> {
    validate_production_handover_receipt_files(
        bundle_path,
        gate_a_receipt_path,
        install_receipt_path,
        deployment_binding_path,
    )?;
    let bundle = validate_bundle(bundle_path, false)?;
    let gate_a_receipt: GateAReceipt = read_json(gate_a_receipt_path)?;
    let install_receipt: ProductionCanisterInstallReceipt = read_json(install_receipt_path)?;
    let final_profile: Profile = read_json(final_profile_path)?;
    validate_profile(&final_profile, true)?;
    let measurements: Evidence = read_json(measurements_path)?;
    let now = now_unix()?;
    validate_measurement_time(&measurements, now, now)?;
    validate_gate_b_operational_parameters(&final_profile, &measurements)?;
    if final_profile.deployment_block != gate_a_receipt.bridge_deployment_block_number
        || profile_uses_production_bootstrap_operational_config(&final_profile)
    {
        return Err(
            "handover final profile must bind the deployment block and derived operational values"
                .into(),
        );
    }
    let mut normalized = final_profile.clone();
    set_production_bootstrap_operational_config(&mut normalized);
    if !hex(&Sha256::digest(canonical_bytes(&normalized)?))
        .eq_ignore_ascii_case(&gate_a_receipt.post_deploy_profile_sha256)
    {
        return Err(
            "handover final profile differs from Gate A outside the deferred operational values"
                .into(),
        );
    }
    Ok((bundle, gate_a_receipt, install_receipt, final_profile))
}

fn decode_candid_hex<T: CandidType + for<'de> Deserialize<'de>>(value: &str) -> Result<T, String> {
    let bytes = decode_hex(value.trim())?;
    Decode!(&bytes, T).map_err(|error| error.to_string())
}

fn storage_validation_complete(value: &str) -> Result<bool, String> {
    match decode_candid_hex::<StorageValidationResultView>(value)? {
        StorageValidationResultView::Ok(status) => {
            if status.phase.trim().is_empty() {
                return Err("storage validation returned an empty phase".into());
            }
            let _ = status.scanned_rows;
            Ok(status.complete)
        }
        StorageValidationResultView::Err(_) => Err("storage validation call failed".into()),
    }
}

fn storage_checksum_complete(value: &str) -> Result<bool, String> {
    match decode_candid_hex::<StorageChecksumResultView>(value)? {
        StorageChecksumResultView::Ok(status) => {
            if status.scanned_bytes > status.db_size {
                return Err("storage checksum scanned beyond the database size".into());
            }
            let _ = status.checksum;
            Ok(status.complete)
        }
        StorageChecksumResultView::Err(_) => Err("storage checksum call failed".into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_production_canister_install_receipt(
    plan_path: &Path,
    installer_principal: &str,
    module_sha256: &str,
    init_response: &str,
    validation_response: &str,
    checksum_response: &str,
    runtime_response: &str,
    operational_response: &str,
    control_plane_response: &str,
    status_response: &str,
    lifecycle_response: &str,
    integrity_response: &str,
    output: &Path,
) -> Result<(), String> {
    let plan: ProductionCanisterPlan = read_json(plan_path)?;
    validate_production_canister_plan(&plan)?;
    if !principal(installer_principal)
        || !valid_sha256(module_sha256)
        || !module_sha256.eq_ignore_ascii_case(&plan.bridge_canister_wasm_sha256)
    {
        return Err("install observation does not match the production Canister plan".into());
    }
    if !matches!(
        decode_candid_hex::<PublicConfigInitializationResultView>(init_response)?,
        PublicConfigInitializationResultView::Ok(())
    ) {
        return Err("public configuration initialization did not succeed".into());
    }
    if !storage_validation_complete(validation_response)?
        || !storage_checksum_complete(checksum_response)?
    {
        return Err("storage validation or checksum is incomplete".into());
    }
    let runtime = decode_candid_hex::<RuntimeBindingView>(runtime_response)?;
    let operational = match decode_candid_hex::<OperationalConfigResultView>(operational_response)?
    {
        OperationalConfigResultView::Ok(value) => value,
        OperationalConfigResultView::Err(_) => {
            return Err("operational configuration query failed".into())
        }
    };
    let observed_governance_operator = operational.governance_operator.clone();
    let observed_operational: OperationalConfigView = (*operational).into();
    let control_plane =
        match decode_candid_hex::<ControlPlaneAddressesResultView>(control_plane_response)? {
            ControlPlaneAddressesResultView::Ok(value) => value,
            ControlPlaneAddressesResultView::Err(_) => {
                return Err("control-plane address query failed".into())
            }
        };
    let status = decode_candid_hex::<BridgeStatusLiveView>(status_response)?;
    if !matches!(
        decode_candid_hex::<ProductionLifecycleResultView>(lifecycle_response)?,
        ProductionLifecycleResultView::Ok(ProductionLifecycleView::Bootstrap)
    ) {
        return Err("new production Canister is not in Bootstrap lifecycle".into());
    }
    match decode_candid_hex::<StorageIntegrityResultView>(integrity_response)? {
        StorageIntegrityResultView::Ok(value) if value == "ok" => {}
        _ => return Err("storage integrity check did not return ok".into()),
    }

    let init = &plan.init;
    let expected_operational = OperationalConfigView {
        mint_authorization_ttl_seconds: status.mint_authorization_ttl_seconds,
        mint_authorization_epoch: status.mint_authorization_epoch,
        governance_operator: observed_governance_operator.clone(),
        deposit_rate_limit_window_seconds: init.deposit_rate_limit_window_seconds,
        deposit_rate_limit_global: init.deposit_rate_limit_global,
        deposit_rate_limit_per_principal: init.deposit_rate_limit_per_principal,
        notification_rate_limit_window_seconds: init.notification_rate_limit_window_seconds,
        notification_rate_limit_global: init.notification_rate_limit_global,
        notification_ingestion_rate_limit_global: init.notification_ingestion_rate_limit_global,
        settlement_rate_limit_window_seconds: init.settlement_rate_limit_window_seconds,
        settlement_rate_limit_global: init.settlement_rate_limit_global,
        settlement_rate_limit_per_principal: init.settlement_rate_limit_per_principal,
        settlement_rate_limit_per_record: init.settlement_rate_limit_per_record,
        settlement_retry_interval_seconds: init.settlement_retry_interval_seconds,
        governance_evm_fee: init.governance_evm_fee,
        governance_replacement: init.governance_replacement,
        cycles_floor: init.cycles_floor,
        settlement_cycle_ceiling: init.settlement_cycle_ceiling,
        governance_principal: Principal::from_text(&init.governance_principal)
            .map_err(|error| error.to_string())?,
        pause_principal: Principal::from_text(&init.pause_principal)
            .map_err(|error| error.to_string())?,
        confirmation_relayer_principal: Principal::from_text(&init.confirmation_relayer_principal)
            .map_err(|error| error.to_string())?,
        fee_recipient: OperationalFeeRecipientView {
            owner: Principal::from_text(&init.fee_recipient.owner)
                .map_err(|error| error.to_string())?,
            subaccount: Vec::new(),
        },
    };
    let operational_digest = |value: OperationalConfigView| -> Result<Vec<u8>, String> {
        let binding = OperationalConfigBindingView {
            ledger_fee: init.expected_minimum_service_fee,
            operational_config: value,
        };
        let encoded = Encode!(&binding).map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        digest.update(OPERATIONAL_CONFIG_BINDING_DOMAIN);
        digest.update(encoded);
        Ok(digest.finalize().to_vec())
    };
    let expected_digest = operational_digest(expected_operational)?;
    let observed_digest = operational_digest(observed_operational)?;
    let empty_state = status.counts.deposits == 0
        && status.counts.withdrawals == 0
        && status.counts.reconciliation_holds == 0
        && status.counts.pending_ledger_operations == 0
        && status.counts.reserved_deposit_mint_amount == 0
        && status.counts.reserved_deposit_mint_operations == 0
        && status.counts.retained_audit_events == 0
        && status.counts.pruned_audit_events == 0
        && status.counts.retained_deposit_index_entries == 0;
    let expected_rpc_digest = canonical_sha256(&Vec::<String>::new())?;
    if runtime.base_chain_id != init.base_chain_id
        || runtime.bridge_contract != decode_hex(&init.bridge_contract_hex)?
        || runtime.expected_bridge_runtime_sha256
            != decode_hex(&init.expected_bridge_runtime_sha256_hex)?
        || runtime.timelock_contract != decode_hex(&init.timelock_contract_hex)?
        || runtime.deployment_instance_id != decode_hex(&init.deployment_instance_id_hex)?
        || runtime.minimum_withdrawal_id != decode_hex(&init.minimum_withdrawal_id_hex)?
        || runtime.ledger_canister_id.to_text() != init.ledger_canister_id
        || runtime.index_canister_id.to_text() != init.index_canister_id
        || runtime.schema_version != CURRENT_STABLE_SCHEMA_VERSION
        || runtime.expected_bridge_signer.len() != 20
        || runtime.expected_bridge_signer.iter().all(|byte| *byte == 0)
        || runtime.expected_bridge_signer != control_plane.bridge_signer
        || observed_governance_operator != control_plane.governance_operator
        || runtime.evm_rpc_canister_id.to_text() != init.evm_rpc_canister_id
        || runtime.rpc_provider_urls_sha256 != expected_rpc_digest
        || runtime.operational_config_sha256 != expected_digest
        || runtime.operational_config_sha256 != observed_digest
        || !status.deposits_paused
        || !status.reserve.sufficient
        || !empty_state
    {
        return Err("installed production Canister does not match the approved plan".into());
    }
    let candid = validate_production_canister_plan(&plan)?;
    let receipt = ProductionCanisterInstallReceipt {
        schema_version: PRODUCTION_CANISTER_INSTALL_RECEIPT_SCHEMA_VERSION,
        plan_sha256: hex(&canonical_sha256(&plan)?),
        plan: plan.clone(),
        source_revision: plan.source_revision.clone(),
        source_tree_sha256: plan.source_tree_sha256.clone(),
        canister_id: plan.bridge_canister_id.clone(),
        installer_principal: installer_principal.to_owned(),
        module_sha256: module_sha256.to_ascii_lowercase(),
        init_candid_sha256: hex(&Sha256::digest(candid)),
        runtime_binding: LiveRuntimeBinding {
            base_chain_id: runtime.base_chain_id,
            bridge_contract: format!("0x{}", hex(&runtime.bridge_contract)),
            timelock_contract: format!("0x{}", hex(&runtime.timelock_contract)),
            deployment_instance_id: format!("0x{}", hex(&runtime.deployment_instance_id)),
            minimum_withdrawal_id: format!("0x{}", hex(&runtime.minimum_withdrawal_id)),
            ledger_canister_id: runtime.ledger_canister_id.to_text(),
            index_canister_id: runtime.index_canister_id.to_text(),
            schema_version: runtime.schema_version,
            expected_bridge_signer: format!("0x{}", hex(&runtime.expected_bridge_signer)),
            evm_rpc_canister_id: runtime.evm_rpc_canister_id.to_text(),
            rpc_provider_urls_sha256: hex(&runtime.rpc_provider_urls_sha256),
            operational_config_sha256: hex(&runtime.operational_config_sha256),
        },
        governance_operator: format!("0x{}", hex(&observed_governance_operator)),
        runtime_administrator: format!("0x{}", hex(&control_plane.runtime_administrator)),
        independent_canceller: format!("0x{}", hex(&control_plane.independent_canceller)),
        mint_authorization_ttl_seconds: status.mint_authorization_ttl_seconds,
        mint_authorization_epoch: status.mint_authorization_epoch,
        storage_validation_complete: true,
        storage_checksum_complete: true,
        deposits_paused: true,
        state_is_empty: true,
        cycles_reserve_sufficient: true,
    };
    let control_plane_roles = [
        control_plane.bridge_signer,
        control_plane.governance_operator,
        control_plane.runtime_administrator,
        control_plane.independent_canceller,
    ];
    if control_plane_roles
        .iter()
        .any(|role| role.len() != 20 || role.iter().all(|byte| *byte == 0))
        || control_plane_roles.iter().collect::<BTreeSet<_>>().len() != control_plane_roles.len()
    {
        return Err("control-plane EVM roles are not nonzero and distinct".into());
    }
    if output.exists() {
        return Err(format!("{} already exists", output.display()));
    }
    let bytes = canonical_bytes(&receipt)?;
    let parent = output.parent().ok_or("receipt output has no parent")?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .map_err(|error| format!("{}: {error}", output.display()))?;
    println!(
        "production_canister_install=verified receipt={}",
        output.display()
    );
    Ok(())
}

fn render_release_inputs(
    profile_path: &Path,
    output: &Path,
    production: bool,
    gate_b_manifest_sha256: Option<&str>,
) -> Result<(), String> {
    let profile_bytes =
        fs::read(profile_path).map_err(|e| format!("{}: {e}", profile_path.display()))?;
    let profile: Profile = serde_json::from_slice(&profile_bytes)
        .map_err(|e| format!("{}: {e}", profile_path.display()))?;
    validate_profile(&profile, production)?;
    let profile_file_sha256 = hex(&Sha256::digest(&profile_bytes));
    let profile_canonical_sha256 = hex(&canonical_sha256(&profile)?);
    let canister_rpc_urls = if production {
        Vec::new()
    } else {
        profile
            .rpc_providers
            .iter()
            .map(|provider| provider.url.trim().to_string())
            .collect::<Vec<_>>()
    };
    let rpc_url_value = Value::Array(
        canister_rpc_urls
            .iter()
            .cloned()
            .map(Value::String)
            .collect(),
    );
    let mut rpc_url_bytes = Vec::new();
    canonical_json(&rpc_url_value, &mut rpc_url_bytes)?;
    let rpc_provider_urls_sha256 = hex(&Sha256::digest(rpc_url_bytes));
    let contract_hex = profile.bridge_contract.trim_start_matches("0x");
    let canister = serde_json::json!({
        "ledger_canister_id": profile.ledger_canister_id,
        "index_canister_id": profile.index_canister_id,
        "evm_rpc_canister_id": profile.evm_rpc_canister_id,
        "custom_evm_rpc_urls": canister_rpc_urls,
        "base_chain_id": profile.chain_id,
        "bridge_contract_hex": contract_hex,
        "expected_bridge_runtime_sha256_hex": profile.bridge_runtime_bytecode_sha256,
        "timelock_contract_hex": profile.timelock.address.trim_start_matches("0x"),
        "deployment_instance_id_hex": profile.deployment_instance_id.trim_start_matches("0x"),
        "minimum_withdrawal_id_hex": profile.minimum_withdrawal_id.trim_start_matches("0x"),
        "ecdsa_key_name": profile.ecdsa_key_name,
        "ecdsa_derivation_path_utf8": profile.ecdsa_derivation_path,
        "governance_ecdsa_derivation_path_utf8": profile.governance_ecdsa_derivation_path,
        "expected_timelock_minimum_delay_seconds": profile.timelock.minimum_delay_seconds,
        "expected_bsns_runtime_sha256_hex": profile.bsns_runtime_bytecode_sha256,
        "expected_bsns_decimals": profile.decimals,
        "expected_minimum_service_fee": profile.parameters.ledger_fee.to_string(),
        "deposit_rate_limit_window_seconds": profile.rate_limits.deposit_window_seconds,
        "deposit_rate_limit_global": profile.rate_limits.deposit_global,
        "deposit_rate_limit_per_principal": profile.rate_limits.deposit_per_principal,
        "notification_rate_limit_window_seconds": profile.rate_limits.notification_window_seconds,
        "notification_rate_limit_global": profile.rate_limits.notification_global,
        "notification_ingestion_rate_limit_global": profile.rate_limits.notification_ingestion_global,
        "settlement_rate_limit_window_seconds": profile.rate_limits.settlement_window_seconds,
        "settlement_rate_limit_global": profile.rate_limits.settlement_global,
        "settlement_rate_limit_per_principal": profile.rate_limits.settlement_per_principal,
        "settlement_rate_limit_per_record": profile.rate_limits.settlement_per_record,
        "settlement_retry_interval_seconds": profile.rate_limits.settlement_retry_interval_seconds,
        "governance_evm_fee": {
            "gas_limit_ceiling": profile.parameters.gas_limit_ceiling.to_string(),
            "max_fee_per_gas_ceiling": profile.parameters.max_fee_per_gas_ceiling.to_string(),
            "max_priority_fee_per_gas_ceiling": profile.parameters.max_priority_fee_per_gas_ceiling.to_string(),
            "l1_fee_per_transaction_ceiling_wei": profile.parameters.l1_fee_per_transaction_ceiling_wei.to_string(),
            "quote_validity_seconds": profile.parameters.quote_validity_seconds,
            "gas_limit_multiplier_bps": profile.parameters.gas_limit_multiplier_bps,
            "base_fee_multiplier_bps": profile.parameters.base_fee_multiplier_bps,
            "l1_fee_multiplier_bps": profile.parameters.l1_fee_multiplier_bps,
        },
        "governance_replacement": profile.governance_replacement,
        "cycles_floor": profile.parameters.cycles_floor.to_string(),
        "settlement_cycle_ceiling": profile.parameters.settlement_cycle_ceiling.to_string(),
        "governance_principal": profile.governance_principal,
        "confirmation_relayer_principal": profile.confirmation_relayer_principal,
        "pause_principal": profile.pause_principal,
        "fee_recipient": { "owner": profile.fee_recipient, "subaccount_hex": "" }
    });
    let constructors = serde_json::json!({
        "bridge": [
            profile.expected_bridge_signer, profile.runtime_administrator, profile.timelock.address,
            profile.timelock.runtime_code_hash,
            profile.parameters.per_deposit_limit.to_string(),
            profile.parameters.mint_throughput_limit.to_string(),
            profile.parameters.mint_window_duration_seconds.to_string(),
            profile.parameters.ledger_fee.to_string(),
            profile.parameters.max_service_fee.to_string(),
            profile.parameters.service_fee.to_string()
        ],
        "bsns": ["KINIC", "KINIC", profile.decimals.to_string(), profile.bridge_contract],
        "timelock": [
            profile.timelock.minimum_delay_seconds.to_string(),
            [profile.timelock.proposer], [profile.timelock.canceller], [profile.timelock.executor]
        ],
        "initial_pause_required": true,
        "deployment": {
            "deployer_address": profile.initial_base_deployment.deployer_address,
            "starting_nonce": profile.initial_base_deployment.starting_nonce,
            "gas_limit": profile.initial_base_deployment.gas_limit.to_string(),
            "max_fee_per_gas": profile.initial_base_deployment.max_fee_per_gas.to_string(),
            "max_priority_fee_per_gas": profile.initial_base_deployment.max_priority_fee_per_gas.to_string()
        }
    });
    let mut ui = serde_json::json!({
        "environment": profile.environment,
        "label": if profile.test_assets_only { "Base Sepolia" } else { "Base" },
        "testOnly": profile.test_assets_only,
        "environmentMode": null,
        "activationTimelockDelaySeconds": profile.timelock.minimum_delay_seconds,
        "gateBManifestSha256": gate_b_manifest_sha256,
        "profileFileSha256": profile_file_sha256,
        "profileCanonicalSha256": profile_canonical_sha256,
        "icHost": profile.ic_host,
        "chainId": profile.chain_id,
        "bridgeCanisterId": profile.bridge_canister_id,
        "deploymentInstanceId": profile.deployment_instance_id,
        "minimumWithdrawalId": profile.minimum_withdrawal_id,
        "ledgerCanisterId": profile.ledger_canister_id,
        "indexCanisterId": profile.index_canister_id,
        "snsRootCanisterId": profile.root_canister_id,
        "icToken": { "name": "KINIC", "symbol": "KINIC", "decimals": profile.decimals },
        "baseToken": { "symbol": "KINIC", "decimals": profile.decimals },
        "bridgeAddress": profile.bridge_contract,
        "bsnsAddress": profile.bsns_contract,
        "timelockAddress": profile.timelock.address,
        "expected_bridge_signer": profile.expected_bridge_signer,
        "evmRpcCanisterId": profile.evm_rpc_canister_id,
        "rpcProviderUrlsSha256": format!("0x{rpc_provider_urls_sha256}"),
        "deploymentBlock": profile.deployment_block.to_string(),
        "bridgeRuntimeHash": format!("0x{}", profile.bridge_runtime_bytecode_sha256),
        "bsnsRuntimeHash": format!("0x{}", profile.bsns_runtime_bytecode_sha256)
    });
    if let Some(base_rpc_url) = &profile.base_rpc_url {
        ui.as_object_mut()
            .ok_or("UI runtime profile must be an object")?
            .insert("baseRpcUrl".into(), serde_json::json!(base_rpc_url));
    }
    let mut artifacts = BTreeMap::new();
    artifacts.insert(
        "canister-init.json",
        write_generated(output, "canister-init.json", &canister)?,
    );
    artifacts.insert(
        "contract-constructor-args.json",
        write_generated(output, "contract-constructor-args.json", &constructors)?,
    );
    artifacts.insert(
        "ui-runtime-profile.json",
        write_generated(output, "ui-runtime-profile.json", &ui)?,
    );
    let manifest = serde_json::json!({
        "schema_version": 2,
        "profile_file_sha256": profile_file_sha256,
        "profile_canonical_sha256": profile_canonical_sha256,
        "artifacts": artifacts
    });
    write_generated(output, "release-inputs-manifest.json", &manifest)?;
    println!("rendered release inputs profile_sha256={profile_file_sha256}");
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex(value: &str) -> Result<Vec<u8>, String> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    if !value.len().is_multiple_of(2) {
        return Err("hex has odd length".into());
    }
    value
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            let s = std::str::from_utf8(pair).map_err(|_| "invalid hex")?;
            u8::from_str_radix(s, 16).map_err(|_| "invalid hex".into())
        })
        .collect()
}

fn decode_address(value: &str) -> Result<[u8; 20], String> {
    decode_hex(value)?
        .try_into()
        .map_err(|_| "invalid EVM address".into())
}

fn address_matches_create(value: &str, sender: [u8; 20], nonce: u64) -> bool {
    decode_address(value).is_ok_and(|expected| expected == create_address(sender, nonce))
}

fn create_address(sender: [u8; 20], nonce: u64) -> [u8; 20] {
    let mut sender_rlp = vec![0x94];
    sender_rlp.extend_from_slice(&sender);
    let nonce_bytes = nonce.to_be_bytes();
    let first = nonce_bytes.iter().position(|byte| *byte != 0).unwrap_or(8);
    let nonce_rlp = if first == 8 {
        vec![0x80]
    } else if nonce_bytes[first] < 0x80 && first == 7 {
        vec![nonce_bytes[first]]
    } else {
        let mut encoded = vec![0x80 + u8::try_from(8 - first).unwrap_or(u8::MAX)];
        encoded.extend_from_slice(&nonce_bytes[first..]);
        encoded
    };
    let payload_length = sender_rlp.len() + nonce_rlp.len();
    let mut encoded = vec![0xc0 + u8::try_from(payload_length).unwrap_or(u8::MAX)];
    encoded.extend(sender_rlp);
    encoded.extend(nonce_rlp);
    let mut digest = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(&encoded);
    hasher.finalize(&mut digest);
    digest[12..].try_into().expect("CREATE digest suffix")
}

fn unsigned_manifest_hash(manifest: &ReleaseManifest) -> Result<[u8; 32], String> {
    let value = serde_json::to_value(manifest).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    canonical_json(&value, &mut bytes)?;
    Ok(Sha256::digest(bytes).into())
}

fn valid_release_id(release_id: &str) -> bool {
    (8..=64).contains(&release_id.len())
        && release_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

const REHEARSAL_VALIDATOR: &str = include_str!("../../../scripts/evm-rpc-rehearsal/rehearsal.py");

fn validate_rpc_rehearsal(bundle: &ValidatedBundle) -> Result<(), String> {
    let path = bundle.root.join("rpc-e2e.json");
    let output = Command::new("python3")
        .arg("-c")
        .arg("import sys; source=sys.argv[1]; sys.argv=[sys.argv[0]]+sys.argv[2:]; path='/reviewed-source/scripts/evm-rpc-rehearsal/rehearsal.py'; scope={'__file__':path,'__name__':'__main__'}; exec(compile(source,path,'exec'),scope)")
        .arg(REHEARSAL_VALIDATOR)
        .arg("verify")
        .arg(&path)
        .output()
        .map_err(|e| format!("cannot run EVM RPC rehearsal verifier: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "EVM RPC rehearsal manifest verification failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let value: Value = read_json(&path)?;
    let string = |pointer: &str| {
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("rehearsal manifest is missing {pointer}"))
    };
    let rehearsal_created = rfc3339_utc_unix(string("/created_at")?)?;
    let rehearsal_updated = rfc3339_utc_unix(string("/updated_at")?)?;
    let now = now_unix()?;
    validate_evidence_time(rehearsal_created, bundle.manifest.created_at_unix, now)?;
    validate_evidence_time(rehearsal_updated, bundle.manifest.created_at_unix, now)?;
    if rehearsal_updated < rehearsal_created {
        return Err("rehearsal update precedes creation".into());
    }
    let scenarios = value
        .pointer("/scenarios")
        .and_then(Value::as_object)
        .ok_or("rehearsal scenarios are missing")?;
    for scenario in scenarios.values() {
        if scenario.is_null() {
            continue;
        }
        let observed = scenario
            .get("observed_at")
            .and_then(Value::as_str)
            .ok_or("rehearsal scenario observation time is missing")?;
        let observed = rfc3339_utc_unix(observed)?;
        validate_evidence_time(observed, bundle.manifest.created_at_unix, now)?;
        if observed < rehearsal_created || observed > rehearsal_updated {
            return Err("rehearsal scenario observation is outside its manifest interval".into());
        }
        let raw_artifacts = scenario
            .get("artifacts")
            .and_then(Value::as_array)
            .ok_or("rehearsal raw artifact references are missing")?;
        for reference in raw_artifacts {
            let relative = reference
                .get("path")
                .and_then(Value::as_str)
                .ok_or("rehearsal raw artifact path is missing")?;
            let artifact: Value = read_json(&safe_artifact_path(&bundle.root, relative)?)?;
            let captured = artifact
                .get("captured_at")
                .and_then(Value::as_str)
                .ok_or("raw artifact capture time is missing")?;
            let captured = rfc3339_utc_unix(captured)?;
            validate_evidence_time(captured, bundle.manifest.created_at_unix, now)?;
            if captured < rehearsal_created || captured > rehearsal_updated {
                return Err("raw artifact capture is outside its rehearsal interval".into());
            }
        }
    }
    let rehearsal_url_hashes = value
        .pointer("/binding/rpc_endpoints")
        .and_then(Value::as_array)
        .ok_or("rehearsal RPC endpoint bindings are missing")?
        .iter()
        .filter_map(|endpoint| endpoint.get("url_sha256").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if rehearsal_url_hashes.len() != 3 {
        return Err("rehearsal must bind three distinct Base Sepolia RPC URLs".into());
    }
    if value.pointer("/launch_ready") != Some(&Value::Bool(true))
        || !matches!(string("/state")?, "LAUNCH_READY" | "EXTENDED_COMPLETE")
        || string("/source/revision")? != bundle.manifest.source_revision
        || !string("/source/source_tree_sha256")?
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || value
            .pointer("/binding/base_chain_id")
            .and_then(Value::as_u64)
            != Some(84532)
        || string("/binding/evm_rpc_canister_id")? != OFFICIAL_EVM_RPC_CANISTER
        || !string("/binding/bridge_canister_wasm_sha256")?
            .eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
        || !string("/binding/bridge_runtime_bytecode_sha256")?
            .eq_ignore_ascii_case(&bundle.profile.bridge_runtime_bytecode_sha256)
    {
        return Err("EVM RPC rehearsal manifest is not bound to this reviewed release".into());
    }
    Ok(())
}

fn safe_artifact_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("unsafe artifact path: {relative}"));
    }
    let joined = root.join(path);
    if fs::symlink_metadata(&joined)
        .map_err(|e| format!("{relative}: {e}"))?
        .file_type()
        .is_symlink()
    {
        return Err(format!("artifact must not be a symlink: {relative}"));
    }
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("{relative}: {e}"))?;
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.starts_with(canonical_root) || !canonical.is_file() {
        return Err(format!("artifact escapes bundle: {relative}"));
    }
    Ok(canonical)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn now_unix() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|e| e.to_string())
}

fn rfc3339_utc_unix(value: &str) -> Result<u64, String> {
    if value.len() != 20
        || &value[4..5] != "-"
        || &value[7..8] != "-"
        || &value[10..11] != "T"
        || &value[13..14] != ":"
        || &value[16..17] != ":"
        || &value[19..20] != "Z"
    {
        return Err("evidence timestamp must be YYYY-MM-DDTHH:MM:SSZ".into());
    }
    let number = |range: std::ops::Range<usize>| {
        value[range]
            .parse::<u64>()
            .map_err(|_| "invalid evidence timestamp")
    };
    let year = number(0..4)?;
    let month = number(5..7)?;
    let day = number(8..10)?;
    let hour = number(11..13)?;
    let minute = number(14..16)?;
    let second = number(17..19)?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if year < 1970
        || !(1..=12).contains(&month)
        || day == 0
        || day > month_days[(month - 1) as usize]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err("invalid evidence timestamp".into());
    }
    let mut days = 0u64;
    for y in 1970..year {
        days += if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) {
            366
        } else {
            365
        };
    }
    for days_in_month in month_days.iter().take((month - 1) as usize) {
        days += days_in_month;
    }
    days += day - 1;
    days.checked_mul(86_400)
        .and_then(|v| v.checked_add(hour * 3_600 + minute * 60 + second))
        .ok_or_else(|| "evidence timestamp overflow".into())
}

fn validate_evidence_time(at: u64, manifest_created: u64, now: u64) -> Result<(), String> {
    if at > manifest_created || at > now || now - at > MAX_EVIDENCE_AGE_SECS {
        return Err("evidence observation is future-dated or older than 90 days".into());
    }
    Ok(())
}

fn validate_activation_time(at: u64, manifest_created: u64, now: u64) -> Result<(), String> {
    if at < manifest_created || at > now || now - at > MAX_EVIDENCE_AGE_SECS {
        return Err("activation timestamp predates Gate B, is future-dated, or is too old".into());
    }
    Ok(())
}

fn validate_activation_attestation_time(
    observed_at_ns: u64,
    manifest_created: u64,
    now: u64,
) -> Result<(), String> {
    if observed_at_ns == 0 {
        return Err("activation attestation timestamp is missing".into());
    }
    let observed = observed_at_ns / 1_000_000_000;
    if observed < manifest_created
        || observed > now
        || now - observed > MAX_ACTIVATION_ATTESTATION_AGE_SECS
    {
        return Err("activation attestation predates Gate B, is future-dated, or is stale".into());
    }
    Ok(())
}

fn validate_plan006_evidence(
    root: &Path,
    manifest: &ReleaseManifest,
    profile: &Profile,
    now: u64,
) -> Result<(), String> {
    let handover: ControllerHandover = read_json(&root.join("controller-handover.json"))?;
    validate_evidence_time(handover.observed_at_unix, manifest.created_at_unix, now)?;
    let required_prefix = ["icp", "canister", "settings", "update", "bridge-canister"];
    let add_controller_positions = handover
        .command_argv
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (value == "--add-controller").then_some(index))
        .collect::<Vec<_>>();
    let remove_all_count = handover
        .command_argv
        .iter()
        .filter(|value| value.as_str() == "--remove-all-controllers")
        .count();
    let environment_is_production = handover
        .command_argv
        .windows(2)
        .any(|pair| pair == ["-e", "production"] || pair == ["--environment", "production"]);
    let identity_is_explicit = handover
        .command_argv
        .windows(2)
        .any(|pair| (pair[0] == "--identity") && !pair[1].is_empty() && !pair[1].starts_with('-'));
    let expected_freezing_cycles = handover
        .idle_cycles_burned_per_day
        .checked_mul(u128::from(handover.freezing_threshold_seconds))
        .and_then(|value| value.checked_add(86_399))
        .map(|value| value / 86_400)
        .ok_or("freezing cycles requirement overflow")?;
    let response_stdout = decode_hex(&handover.response_stdout_hex)?;
    let response_stderr = decode_hex(&handover.response_stderr_hex)?;
    let mut response_transcript = response_stdout;
    response_transcript.extend_from_slice(&response_stderr);
    let response_digest = hex(&Sha256::digest(&response_transcript));
    let response_text = String::from_utf8_lossy(&response_transcript).to_ascii_lowercase();
    let request_id_text = handover
        .request_id
        .trim_start_matches("0x")
        .to_ascii_lowercase();
    if handover.schema_version != 2
        || handover.stage != "complete"
        || handover.bridge_canister_id != profile.bridge_canister_id
        || handover.sns_root_canister_id != KINIC_ROOT
        || !principal(&handover.executing_principal)
        || handover.command_argv.len() < required_prefix.len()
        || handover.command_argv[..required_prefix.len()] != required_prefix
        || remove_all_count != 1
        || add_controller_positions.len() != 1
        || handover
            .command_argv
            .get(add_controller_positions[0] + 1)
            .is_none_or(|value| value != KINIC_ROOT)
        || !environment_is_production
        || !identity_is_explicit
        || !handover.command_argv.iter().any(|value| value == "--force")
        || handover
            .command_argv
            .iter()
            .any(|value| value == "--network" || value == "-n")
        || !(valid_sha256(&handover.request_id) || valid_hash32(&handover.request_id))
        || handover.response_exit_code != 0
        || !response_digest.eq_ignore_ascii_case(&handover.response_sha256)
        || !response_text.contains(&request_id_text)
        || !valid_sha256(&handover.response_sha256)
        || handover.final_controllers != [KINIC_ROOT.to_string()]
        || handover.freezing_threshold_seconds == 0
        || handover.idle_cycles_burned_per_day == 0
        || handover.required_freezing_cycles != expected_freezing_cycles
        || handover.cycles_balance < profile.parameters.cycles_floor
        || handover.cycles_balance < handover.required_freezing_cycles
    {
        return Err("controller handover evidence is not an atomic SNS Root-only transfer".into());
    }

    let upgrade: SnsUpgrade = read_json(&root.join("sns-upgrade.json"))?;
    validate_evidence_time(upgrade.observed_at_unix, manifest.created_at_unix, now)?;
    validate_evidence_time(upgrade.executed_at_unix, manifest.created_at_unix, now)?;
    if upgrade.schema_version != 3
        || upgrade.proposal_id == 0
        || upgrade.governance_canister_id != KINIC_GOVERNANCE
        || upgrade.root_canister_id != KINIC_ROOT
        || upgrade.bridge_canister_id != profile.bridge_canister_id
        || upgrade.status != "Executed"
        || upgrade.executed_at_unix < handover.observed_at_unix
        || upgrade.executed_at_unix > upgrade.observed_at_unix
        || !upgrade
            .wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !upgrade
            .before_module_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !upgrade
            .after_module_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !valid_sha256(&upgrade.before_public_state_sha256)
        || !upgrade
            .before_public_state_sha256
            .eq_ignore_ascii_case(&upgrade.after_public_state_sha256)
        || upgrade.proposal_action != "UpgradeSnsControlledCanister"
        || upgrade.install_mode != "upgrade"
        || upgrade.proposal_target_canister_id != profile.bridge_canister_id
        || !upgrade
            .proposal_wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !valid_nonempty_hex(&upgrade.governance_query_response_hex)
        || !valid_sha256(&upgrade.governance_query_response_sha256)
        || !hex_sha256_matches(
            &upgrade.governance_query_response_hex,
            &upgrade.governance_query_response_sha256,
        )
    {
        return Err("SNS upgrade evidence is incomplete or not bound to the release Wasm".into());
    }

    Ok(())
}

fn validate_ui_assets_receipt(root: &Path, manifest: &ReleaseManifest) -> Result<(), String> {
    let receipt: UiAssetsReceipt = read_json(&root.join("ui-assets.json"))?;
    if receipt.schema_version != 1
        || receipt.source_revision != manifest.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&manifest.source_tree_sha256)
        || receipt.files.is_empty()
        || !valid_sha256(&receipt.artifact_set_sha256)
    {
        return Err("UI artifact receipt is not bound to the release source".into());
    }
    let mut previous = None;
    let mut seen = BTreeSet::new();
    for file in &receipt.files {
        let path = Path::new(&file.path);
        if file.path == "deployment-profile.js"
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || !valid_sha256(&file.sha256)
            || !seen.insert(file.path.as_str())
            || previous.is_some_and(|value: &str| value >= file.path.as_str())
        {
            return Err("UI artifact receipt contains an unsafe or unordered file entry".into());
        }
        previous = Some(file.path.as_str());
    }
    let encoded = serde_json::to_vec(&receipt.files).map_err(|e| e.to_string())?;
    if !hex(&Sha256::digest(encoded)).eq_ignore_ascii_case(&receipt.artifact_set_sha256) {
        return Err("UI artifact receipt aggregate digest is invalid".into());
    }
    Ok(())
}

fn validate_post_gate_a_policy_transition(
    root: &Path,
    manifest: &ReleaseManifest,
    profile: &Profile,
    receipt: &GateAReceipt,
    now: u64,
) -> Result<(), String> {
    let transition: PostGateAPolicyTransition =
        read_json(&root.join("post-gate-a-policy-transition.json"))?;
    validate_evidence_time(transition.observed_at_unix, manifest.created_at_unix, now)?;
    let receipt_bytes = fs::read(root.join("gate-a-receipt.json")).map_err(|e| e.to_string())?;
    if transition.schema_version != 1
        || transition.reason != "activate-before-production-measurements"
        || !transition
            .gate_a_manifest_sha256
            .eq_ignore_ascii_case(&receipt.gate_a_manifest_sha256)
        || !transition
            .gate_a_receipt_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(receipt_bytes)))
        || transition.from_source_revision != receipt.source_revision
        || !transition
            .from_source_tree_sha256
            .eq_ignore_ascii_case(&receipt.source_tree_sha256)
        || transition.to_source_revision != manifest.source_revision
        || !transition
            .to_source_tree_sha256
            .eq_ignore_ascii_case(&manifest.source_tree_sha256)
        || transition.bridge_canister_id != profile.bridge_canister_id
        || !transition
            .bridge_contract
            .eq_ignore_ascii_case(&profile.bridge_contract)
        || !transition
            .bsns_contract
            .eq_ignore_ascii_case(&profile.bsns_contract)
        || !transition
            .timelock_contract
            .eq_ignore_ascii_case(&profile.timelock.address)
        || !transition
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&receipt.bridge_canister_wasm_sha256)
        || !transition
            .bridge_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&receipt.bridge_runtime_bytecode_sha256)
        || !transition
            .bridge_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&profile.bridge_runtime_bytecode_sha256)
        || !transition
            .bsns_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&profile.bsns_runtime_bytecode_sha256)
        || !transition
            .bsns_runtime_template_sha256
            .eq_ignore_ascii_case(&profile.bsns_runtime_template_sha256)
        || !transition
            .bridge_deployment_transaction_hash
            .eq_ignore_ascii_case(&receipt.bridge_deployment_transaction_hash)
        || !transition
            .timelock_deployment_transaction_hash
            .eq_ignore_ascii_case(&receipt.timelock_deployment_transaction_hash)
    {
        return Err(
            "post-Gate-A policy transition is incomplete or changes deployed identity".into(),
        );
    }
    Ok(())
}

fn validate_bundle(root: &Path, gate_b: bool) -> Result<ValidatedBundle, String> {
    if root.join("proof-attestation.json").exists() {
        return Err(
            "obsolete self-asserted proof attestation is forbidden; release drivers rerun proofs"
                .into(),
        );
    }
    let manifest: ReleaseManifest = read_json(&root.join("release-manifest.json"))?;
    let expected_manifest_schema = if gate_b { 4 } else { 3 };
    if manifest.schema_version != expected_manifest_schema
        || !valid_release_id(&manifest.release_id)
        || manifest.source_revision.trim().is_empty()
        || !valid_sha256(&manifest.source_tree_sha256)
    {
        return Err("invalid release manifest identity".into());
    }
    if gate_b {
        if !manifest
            .parent_gate_a_manifest_sha256
            .as_deref()
            .is_some_and(valid_sha256)
        {
            return Err("Gate B must bind a Gate A manifest hash".into());
        }
    } else if manifest.parent_gate_a_manifest_sha256.is_some() {
        return Err("Gate A manifest must not contain a parent Gate A hash".into());
    }
    let lifetime = manifest
        .expires_at_unix
        .checked_sub(manifest.created_at_unix)
        .ok_or("manifest expiry precedes creation")?;
    let now = now_unix()?;
    if lifetime == 0
        || lifetime > MAX_EVIDENCE_AGE_SECS
        || now < manifest.created_at_unix
        || now > manifest.expires_at_unix
    {
        return Err("evidence bundle is not current or exceeds 90 days".into());
    }
    let artifacts = manifest
        .artifacts
        .iter()
        .map(|a| (a.path.as_str(), a))
        .collect::<BTreeMap<_, _>>();
    let required = if gate_b {
        GATE_B_ARTIFACTS.as_slice()
    } else {
        GATE_A_ARTIFACTS.as_slice()
    };
    if artifacts.len() != manifest.artifacts.len()
        || artifacts.len() != required.len()
        || required
            .iter()
            .any(|required| !artifacts.contains_key(required))
    {
        return Err("manifest must contain each required evidence artifact exactly once".into());
    }
    for artifact in &manifest.artifacts {
        if !valid_sha256(&artifact.sha256) {
            return Err(format!("invalid artifact hash: {}", artifact.path));
        }
        let path = safe_artifact_path(root, &artifact.path)?;
        let actual = hex(&Sha256::digest(fs::read(path).map_err(|e| e.to_string())?));
        if !actual.eq_ignore_ascii_case(&artifact.sha256) {
            return Err(format!("artifact hash mismatch: {}", artifact.path));
        }
    }
    let profile: Profile = read_json(&root.join("profile.json"))?;
    validate_profile(&profile, !manifest.test_only)?;
    if gate_b {
        let initial: InitialOperationalParameters =
            read_json(&root.join("initial-operational-parameters.json"))?;
        validate_initial_operational_parameters(&initial, &profile, manifest.created_at_unix, now)?;
        if profile.deployment_block == 0 {
            return Err("Gate B profile must bind the actual Bridge deployment block".into());
        }
        if profile_uses_production_bootstrap_operational_config(&profile) {
            return Err(
                "Gate B must replace the bootstrap operational config with reviewed final values"
                    .into(),
            );
        }
    } else if profile.deployment_block != 0 {
        return Err("Gate A profile must leave deployment_block unbound until deployment".into());
    } else if !profile_uses_production_bootstrap_operational_config(&profile) {
        return Err("Gate A profile must use the fixed bootstrap operational config".into());
    }
    let wasm_hash = artifacts["bridge-canister.wasm"].sha256.as_str();
    let bytecode_hash = artifacts["bridge-runtime.bin"].sha256.as_str();
    if !wasm_hash.eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !bytecode_hash.eq_ignore_ascii_case(&profile.bridge_runtime_bytecode_sha256)
        || !artifacts["bsns-runtime.bin"]
            .sha256
            .eq_ignore_ascii_case(&profile.bsns_runtime_template_sha256)
    {
        return Err("release artifacts do not match profile code hashes".into());
    }
    if gate_b {
        let receipt: GateAReceipt = read_json(&root.join("gate-a-receipt.json"))?;
        let mut gate_a_profile = profile.clone();
        gate_a_profile.deployment_block = 0;
        set_production_bootstrap_operational_config(&mut gate_a_profile);
        let expected_gate_a_profile_hash = hex(&canonical_sha256(&gate_a_profile)?);
        let mut expected_post_deploy_profile = profile.clone();
        set_production_bootstrap_operational_config(&mut expected_post_deploy_profile);
        let expected_post_deploy_profile_hash = hex(&Sha256::digest(canonical_bytes(
            &expected_post_deploy_profile,
        )?));
        if receipt.schema_version != 2
            || !receipt.gate_a_manifest_sha256.eq_ignore_ascii_case(
                manifest
                    .parent_gate_a_manifest_sha256
                    .as_deref()
                    .unwrap_or_default(),
            )
            || receipt.release_id != manifest.release_id
            || !receipt
                .post_deploy_profile_sha256
                .eq_ignore_ascii_case(&expected_post_deploy_profile_hash)
            || !receipt
                .gate_a_profile_sha256
                .eq_ignore_ascii_case(&expected_gate_a_profile_hash)
            || !receipt
                .bridge_canister_wasm_sha256
                .eq_ignore_ascii_case(wasm_hash)
            || !receipt
                .bridge_runtime_bytecode_sha256
                .eq_ignore_ascii_case(bytecode_hash)
            || !valid_hash32(&receipt.bridge_deployment_transaction_hash)
            || !valid_hash32(&receipt.bridge_deployment_block_hash)
            || !valid_hash32(&receipt.timelock_deployment_transaction_hash)
            || !valid_hash32(&receipt.timelock_deployment_block_hash)
            || receipt.bridge_deployment_block_number != profile.deployment_block
            || receipt.timelock_deployment_block_number == 0
            || receipt.timelock_deployment_block_number > receipt.bridge_deployment_block_number
        {
            return Err("Gate B evidence is not bound to the Gate A release".into());
        }
        let mut installed_profile = profile.clone();
        set_production_bootstrap_operational_config(&mut installed_profile);
        validate_production_canister_receipt(&installed_profile, &receipt.canister_install)?;
        validate_post_gate_a_policy_transition(root, &manifest, &profile, &receipt, now)?;
    }
    if profile.test_assets_only != manifest.test_only {
        return Err("manifest/profile test-only mismatch".into());
    }
    if gate_b {
        let drill: MonitorDrill = read_json(&root.join("monitor-drill.json"))?;
        let receipt: GateAReceipt = read_json(&root.join("gate-a-receipt.json"))?;
        let transition: PostGateAPolicyTransition =
            read_json(&root.join("post-gate-a-policy-transition.json"))?;
        validate_monitor_drill(
            &drill,
            &manifest,
            &profile,
            &transition.to_source_revision,
            &transition.to_source_tree_sha256,
            &receipt.bridge_canister_wasm_sha256,
            now,
        )?;
        validate_provider_independence_receipt(root, &manifest, &profile, now)?;
        validate_ui_assets_receipt(root, &manifest)?;
    }
    let hash = unsigned_manifest_hash(&manifest)?;
    Ok(ValidatedBundle {
        root: root.to_path_buf(),
        manifest,
        profile,
        manifest_sha256: hex(&hash),
    })
}

fn validate_live_runtime_binding(
    observed: &LiveRuntimeBinding,
    profile: &Profile,
    rpc_url_hash: &str,
    operational_config_sha256: &[u8],
) -> Result<(), String> {
    if observed.base_chain_id != profile.chain_id
        || !observed
            .bridge_contract
            .eq_ignore_ascii_case(&profile.bridge_contract)
        || !observed
            .timelock_contract
            .eq_ignore_ascii_case(&profile.timelock.address)
        || !observed
            .deployment_instance_id
            .eq_ignore_ascii_case(&profile.deployment_instance_id)
        || !observed
            .minimum_withdrawal_id
            .eq_ignore_ascii_case(&profile.minimum_withdrawal_id)
        || observed.ledger_canister_id != profile.ledger_canister_id
        || observed.index_canister_id != profile.index_canister_id
        || observed.schema_version != profile.canister_schema_version
        || !observed
            .expected_bridge_signer
            .eq_ignore_ascii_case(&profile.expected_bridge_signer)
        || observed.evm_rpc_canister_id != profile.evm_rpc_canister_id
        || !observed
            .rpc_provider_urls_sha256
            .eq_ignore_ascii_case(rpc_url_hash)
        || !observed
            .operational_config_sha256
            .eq_ignore_ascii_case(&hex(operational_config_sha256))
    {
        return Err("live Canister RuntimeBinding does not exactly match the profile".into());
    }
    Ok(())
}

fn live_runtime_binding_from_view(observed: &RuntimeBindingView) -> LiveRuntimeBinding {
    LiveRuntimeBinding {
        base_chain_id: observed.base_chain_id,
        bridge_contract: format!("0x{}", hex(&observed.bridge_contract)),
        timelock_contract: format!("0x{}", hex(&observed.timelock_contract)),
        deployment_instance_id: format!("0x{}", hex(&observed.deployment_instance_id)),
        minimum_withdrawal_id: format!("0x{}", hex(&observed.minimum_withdrawal_id)),
        ledger_canister_id: observed.ledger_canister_id.to_text(),
        index_canister_id: observed.index_canister_id.to_text(),
        schema_version: observed.schema_version,
        expected_bridge_signer: format!("0x{}", hex(&observed.expected_bridge_signer)),
        evm_rpc_canister_id: observed.evm_rpc_canister_id.to_text(),
        rpc_provider_urls_sha256: hex(&observed.rpc_provider_urls_sha256),
        operational_config_sha256: hex(&observed.operational_config_sha256),
    }
}

fn validate_empty_paused_production_status(status: &BridgeStatusLiveView) -> Result<(), String> {
    let counts = &status.counts;
    if !status.deposits_paused
        || !status.reserve.sufficient
        || counts.deposits != 0
        || counts.withdrawals != 0
        || counts.reconciliation_holds != 0
        || counts.pending_ledger_operations != 0
        || counts.reserved_deposit_mint_amount != 0
        || counts.reserved_deposit_mint_operations != 0
        || counts.retained_audit_events != 0
        || counts.pruned_audit_events != 0
        || counts.retained_deposit_index_entries != 0
    {
        return Err("production Canister state is not paused, empty, and reserved".into());
    }
    Ok(())
}

fn expected_bootstrap_operational_config_sha256(
    init: &ProductionCanisterInitInput,
    governance_operator: &str,
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
) -> Result<[u8; 32], String> {
    let binding = OperationalConfigBindingView {
        ledger_fee: init.expected_minimum_service_fee,
        operational_config: OperationalConfigView {
            mint_authorization_ttl_seconds,
            mint_authorization_epoch,
            governance_operator: decode_hex(governance_operator)?,
            deposit_rate_limit_window_seconds: init.deposit_rate_limit_window_seconds,
            deposit_rate_limit_global: init.deposit_rate_limit_global,
            deposit_rate_limit_per_principal: init.deposit_rate_limit_per_principal,
            notification_rate_limit_window_seconds: init.notification_rate_limit_window_seconds,
            notification_rate_limit_global: init.notification_rate_limit_global,
            notification_ingestion_rate_limit_global: init.notification_ingestion_rate_limit_global,
            settlement_rate_limit_window_seconds: init.settlement_rate_limit_window_seconds,
            settlement_rate_limit_global: init.settlement_rate_limit_global,
            settlement_rate_limit_per_principal: init.settlement_rate_limit_per_principal,
            settlement_rate_limit_per_record: init.settlement_rate_limit_per_record,
            settlement_retry_interval_seconds: init.settlement_retry_interval_seconds,
            governance_evm_fee: init.governance_evm_fee,
            governance_replacement: init.governance_replacement,
            cycles_floor: init.cycles_floor,
            settlement_cycle_ceiling: init.settlement_cycle_ceiling,
            governance_principal: Principal::from_text(&init.governance_principal)
                .map_err(|error| error.to_string())?,
            pause_principal: Principal::from_text(&init.pause_principal)
                .map_err(|error| error.to_string())?,
            confirmation_relayer_principal: Principal::from_text(
                &init.confirmation_relayer_principal,
            )
            .map_err(|error| error.to_string())?,
            fee_recipient: OperationalFeeRecipientView {
                owner: Principal::from_text(&init.fee_recipient.owner)
                    .map_err(|error| error.to_string())?,
                subaccount: Vec::new(),
            },
        },
    };
    let encoded = Encode!(&binding).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    digest.update(OPERATIONAL_CONFIG_BINDING_DOMAIN);
    digest.update(encoded);
    Ok(digest.finalize().into())
}

fn expected_operational_config_sha256(
    profile: &Profile,
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
) -> Result<[u8; 32], String> {
    let binding = OperationalConfigBindingView {
        ledger_fee: profile.parameters.ledger_fee,
        operational_config: OperationalConfigView {
            mint_authorization_ttl_seconds,
            mint_authorization_epoch,
            governance_operator: decode_hex(&profile.governance_operator)?,
            deposit_rate_limit_window_seconds: profile.rate_limits.deposit_window_seconds,
            deposit_rate_limit_global: profile.rate_limits.deposit_global,
            deposit_rate_limit_per_principal: profile.rate_limits.deposit_per_principal,
            notification_rate_limit_window_seconds: profile.rate_limits.notification_window_seconds,
            notification_rate_limit_global: profile.rate_limits.notification_global,
            notification_ingestion_rate_limit_global: profile
                .rate_limits
                .notification_ingestion_global,
            settlement_rate_limit_window_seconds: profile.rate_limits.settlement_window_seconds,
            settlement_rate_limit_global: profile.rate_limits.settlement_global,
            settlement_rate_limit_per_principal: profile.rate_limits.settlement_per_principal,
            settlement_rate_limit_per_record: profile.rate_limits.settlement_per_record,
            settlement_retry_interval_seconds: profile
                .rate_limits
                .settlement_retry_interval_seconds,
            governance_evm_fee: profile.parameters.governance_evm_fee(),
            governance_replacement: profile.governance_replacement,
            cycles_floor: profile.parameters.cycles_floor,
            settlement_cycle_ceiling: profile.parameters.settlement_cycle_ceiling,
            governance_principal: Principal::from_text(&profile.governance_principal)
                .map_err(|error| error.to_string())?,
            pause_principal: Principal::from_text(&profile.pause_principal)
                .map_err(|error| error.to_string())?,
            confirmation_relayer_principal: Principal::from_text(
                &profile.confirmation_relayer_principal,
            )
            .map_err(|error| error.to_string())?,
            fee_recipient: OperationalFeeRecipientView {
                owner: Principal::from_text(&profile.fee_recipient)
                    .map_err(|error| error.to_string())?,
                subaccount: Vec::new(),
            },
        },
    };
    let encoded = Encode!(&binding).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    digest.update(OPERATIONAL_CONFIG_BINDING_DOMAIN);
    digest.update(encoded);
    Ok(digest.finalize().into())
}

fn verify_live_inputs(
    bundle: &ValidatedBundle,
    expected_deposits_paused: bool,
) -> Result<(), String> {
    let rpc_url_hash = hex(&canonical_sha256(
        &bundle
            .profile
            .rpc_providers
            .iter()
            .map(|provider| provider.url.clone())
            .collect::<Vec<_>>(),
    )?);
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (public_raw, status_raw) = async_runtime()?.block_on(async {
        let public = agent
            .query(&bridge, "get_runtime_binding")
            .with_arg(Encode!().map_err(|error| error.to_string())?)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let status = agent
            .query(&bridge, "get_bridge_status")
            .with_arg(Encode!().map_err(|error| error.to_string())?)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((public, status))
    })?;
    let public = Decode!(&public_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let observed = live_runtime_binding_from_view(&public);
    let operational_config_sha256 = expected_operational_config_sha256(
        &bundle.profile,
        status.mint_authorization_ttl_seconds,
        status.mint_authorization_epoch,
    )?;
    validate_live_runtime_binding(
        &observed,
        &bundle.profile,
        &rpc_url_hash,
        &operational_config_sha256,
    )?;
    if public.expected_bridge_runtime_sha256
        != decode_hex(&bundle.profile.bridge_runtime_bytecode_sha256)?
        || status.deposits_paused != expected_deposits_paused
        || !status.reserve.sufficient
    {
        return Err("authenticated live Canister state does not satisfy Gate B".into());
    }
    validate_rpc_rehearsal(bundle)?;
    Ok(())
}

fn validate_production_canister_management_state(
    profile: &Profile,
    receipt: &ProductionCanisterInstallReceipt,
    controllers: &[Principal],
    module_hash: &[u8],
) -> Result<(), String> {
    let installer =
        Principal::from_text(&receipt.installer_principal).map_err(|error| error.to_string())?;
    if controllers != [installer]
        || !hex(module_hash).eq_ignore_ascii_case(&receipt.module_sha256)
        || !hex(module_hash).eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
    {
        return Err(
            "certified Canister module hash or controller set differs from the install receipt"
                .into(),
        );
    }
    Ok(())
}

fn validate_control_plane_addresses(
    profile: &Profile,
    receipt: &ProductionCanisterInstallReceipt,
    observed: &ControlPlaneAddressesCallView,
) -> Result<(), String> {
    let expected = [
        &profile.expected_bridge_signer,
        &profile.governance_operator,
        &profile.runtime_administrator,
        &profile.independent_canceller,
    ];
    let receipt_values = [
        &receipt.runtime_binding.expected_bridge_signer,
        &receipt.governance_operator,
        &receipt.runtime_administrator,
        &receipt.independent_canceller,
    ];
    let observed_values = [
        &observed.bridge_signer,
        &observed.governance_operator,
        &observed.runtime_administrator,
        &observed.independent_canceller,
    ];
    for ((profile_value, receipt_value), observed_value) in expected
        .iter()
        .zip(receipt_values.iter())
        .zip(observed_values.iter())
    {
        if !profile_value.eq_ignore_ascii_case(receipt_value)
            || !profile_value
                .trim_start_matches("0x")
                .eq_ignore_ascii_case(&hex(observed_value))
        {
            return Err(
                "live control-plane addresses differ from the profile or install receipt".into(),
            );
        }
    }
    Ok(())
}

fn verify_production_canister_predeploy(
    profile_path: &Path,
    receipt_path: &Path,
) -> Result<(), String> {
    let profile: Profile = read_json(profile_path)?;
    validate_profile(&profile, true)?;
    let receipt: ProductionCanisterInstallReceipt = read_json(receipt_path)?;
    validate_production_canister_receipt(&profile, &receipt)?;
    let bridge =
        Principal::from_text(&profile.bridge_canister_id).map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&profile.ic_host, false)?;
    let (runtime_raw, control_plane_raw, status_raw, lifecycle_raw, controllers, module_hash) =
        async_runtime()?.block_on(async {
            let empty = Encode!().map_err(|error| error.to_string())?;
            let runtime = agent
                .query(&bridge, "get_runtime_binding")
                .with_arg(empty.clone())
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let status = agent
                .query(&bridge, "get_bridge_status")
                .with_arg(empty.clone())
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let control_plane = agent
                .query(&bridge, "get_control_plane_addresses")
                .with_arg(empty.clone())
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let lifecycle = agent
                .query(&bridge, "get_production_lifecycle")
                .with_arg(empty)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let controllers = agent
                .read_state_canister_controllers(bridge)
                .await
                .map_err(|error| error.to_string())?;
            let module_hash = agent
                .read_state_canister_module_hash(bridge)
                .await
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((
                runtime,
                control_plane,
                status,
                lifecycle,
                controllers,
                module_hash,
            ))
        })?;
    validate_production_canister_management_state(&profile, &receipt, &controllers, &module_hash)?;
    let runtime = Decode!(&runtime_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let control_plane = match Decode!(&control_plane_raw, ControlPlaneAddressesResultView)
        .map_err(|error| error.to_string())?
    {
        ControlPlaneAddressesResultView::Ok(value) => value,
        ControlPlaneAddressesResultView::Err(_) => {
            return Err("live control-plane address query failed".into())
        }
    };
    validate_control_plane_addresses(&profile, &receipt, &control_plane)?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    if !matches!(
        Decode!(&lifecycle_raw, ProductionLifecycleResultView).map_err(|error| error.to_string())?,
        ProductionLifecycleResultView::Ok(ProductionLifecycleView::Bootstrap)
    ) {
        return Err("production Canister left Bootstrap before Base deployment".into());
    }
    let observed = live_runtime_binding_from_view(&runtime);
    let operational = expected_bootstrap_operational_config_sha256(
        &receipt.plan.init,
        &receipt.governance_operator,
        status.mint_authorization_ttl_seconds,
        status.mint_authorization_epoch,
    )?;
    validate_live_runtime_binding(
        &observed,
        &profile,
        &hex(&canonical_sha256(&Vec::<String>::new())?),
        &operational,
    )?;
    if runtime.expected_bridge_runtime_sha256
        != decode_hex(&profile.bridge_runtime_bytecode_sha256)?
    {
        return Err("production Canister runtime code binding differs from the profile".into());
    }
    validate_empty_paused_production_status(&status)?;
    println!(
        "production_canister_predeploy=verified canister={}",
        profile.bridge_canister_id
    );
    Ok(())
}

struct ProductionHandoverCanisterObservation<'a> {
    lifecycle: &'a ProductionLifecycleView,
    attestation: Option<&'a ActivationAttestationView>,
    runtime: &'a RuntimeBindingView,
    status: &'a BridgeStatusLiveView,
    controllers: &'a [Principal],
    module_hash: &'a [u8],
}

fn validate_production_handover_canister_state(
    profile: &Profile,
    install_receipt: &ProductionCanisterInstallReceipt,
    gate_a_receipt: &GateAReceipt,
    observation: &ProductionHandoverCanisterObservation<'_>,
    manifest_created_at_unix: u64,
    now: u64,
) -> Result<(), String> {
    if !matches!(
        observation.lifecycle,
        ProductionLifecycleView::OperationalConfigSealed
    ) {
        return Err("production Canister must be OperationalConfigSealed before handover".into());
    }
    validate_production_canister_management_state(
        profile,
        install_receipt,
        observation.controllers,
        observation.module_hash,
    )?;
    let operational_config_sha256 = expected_operational_config_sha256(
        profile,
        observation.status.mint_authorization_ttl_seconds,
        observation.status.mint_authorization_epoch,
    )?;
    validate_live_runtime_binding(
        &live_runtime_binding_from_view(observation.runtime),
        profile,
        &hex(&canonical_sha256(&Vec::<String>::new())?),
        &operational_config_sha256,
    )?;
    if observation.runtime.expected_bridge_runtime_sha256
        != decode_hex(&profile.bridge_runtime_bytecode_sha256)?
    {
        return Err("production Canister runtime code binding differs from the profile".into());
    }
    validate_empty_paused_production_status(observation.status)?;
    let attestation = observation.attestation.ok_or(
        "authenticated activation attestation is unavailable after operational configuration seal",
    )?;
    validate_activation_attestation(
        profile,
        attestation,
        manifest_created_at_unix,
        gate_a_receipt
            .bridge_deployment_block_number
            .max(gate_a_receipt.timelock_deployment_block_number),
        now,
    )
}

fn verify_production_canister_handover(
    bundle_path: &Path,
    final_profile_path: &Path,
    measurements_path: &Path,
    gate_a_receipt_path: &Path,
    install_receipt_path: &Path,
    deployment_binding_path: &Path,
) -> Result<(), String> {
    let (bundle, gate_a_receipt, install_receipt, final_profile) =
        validate_production_handover_candidate_files(
            bundle_path,
            final_profile_path,
            measurements_path,
            gate_a_receipt_path,
            install_receipt_path,
            deployment_binding_path,
        )?;
    let bridge = Principal::from_text(&final_profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&final_profile.ic_host, false)?;
    let (lifecycle_raw, attestation_raw, runtime_raw, status_raw, controllers, module_hash) =
        async_runtime()?.block_on(async {
            let empty = Encode!().map_err(|error| error.to_string())?;
            let lifecycle = agent
                .query(&bridge, "get_production_lifecycle")
                .with_arg(empty.clone())
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let attestation = agent
                .query(&bridge, "get_activation_attestation")
                .with_arg(empty)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let runtime = agent
                .query(&bridge, "get_runtime_binding")
                .with_arg(Encode!().map_err(|error| error.to_string())?)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let status = agent
                .query(&bridge, "get_bridge_status")
                .with_arg(Encode!().map_err(|error| error.to_string())?)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let controllers = agent
                .read_state_canister_controllers(bridge)
                .await
                .map_err(|error| error.to_string())?;
            let module_hash = agent
                .read_state_canister_module_hash(bridge)
                .await
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((
                lifecycle,
                attestation,
                runtime,
                status,
                controllers,
                module_hash,
            ))
        })?;
    let lifecycle = match Decode!(&lifecycle_raw, ProductionLifecycleResultView)
        .map_err(|error| error.to_string())?
    {
        ProductionLifecycleResultView::Ok(value) => value,
        ProductionLifecycleResultView::Err(_) => {
            return Err("authenticated production lifecycle is unavailable".into())
        }
    };
    let attestation = match Decode!(&attestation_raw, ActivationAttestationResultView)
        .map_err(|error| error.to_string())?
    {
        ActivationAttestationResultView::Ok(value) => Some(value),
        ActivationAttestationResultView::Err(_) => None,
    };
    let runtime = Decode!(&runtime_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let observation = ProductionHandoverCanisterObservation {
        lifecycle: &lifecycle,
        attestation: attestation.as_deref(),
        runtime: &runtime,
        status: &status,
        controllers: &controllers,
        module_hash: &module_hash,
    };
    validate_production_handover_canister_state(
        &final_profile,
        &install_receipt,
        &gate_a_receipt,
        &observation,
        bundle.manifest.created_at_unix,
        now_unix()?,
    )?;
    println!(
        "production_canister_handover=verified canister={}",
        final_profile.bridge_canister_id
    );
    Ok(())
}

fn mainnet_agent(host: &str, evidence_window: bool) -> Result<Agent, String> {
    let mut builder = Agent::builder()
        .with_url(host)
        .with_verify_query_signatures(true);
    if evidence_window {
        builder =
            builder.with_ingress_expiry(std::time::Duration::from_secs(MAX_EVIDENCE_AGE_SECS));
    }
    builder.build().map_err(|error| error.to_string())
}

fn async_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

fn verify_monitor_ic_certificate(bundle: &ValidatedBundle) -> Result<(), String> {
    let drill: MonitorDrill = read_json(&bundle.root.join("monitor-drill.json"))?;
    let certificate_bytes = decode_hex(&drill.ic_pause.certificate_hex)?;
    let certificate: ic_agent::Certificate = ciborium::from_reader(certificate_bytes.as_slice())
        .map_err(|error| format!("invalid IC certificate CBOR: {error}"))?;
    let canister = Principal::from_text(&drill.bridge_canister_id).map_err(|e| e.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, true)?;
    agent
        .verify(&certificate, canister)
        .map_err(|error| format!("IC pause certificate verification failed: {error}"))?;
    let request_bytes = decode_hex(&drill.ic_pause.request_id)?;
    let request_hash: [u8; 32] = request_bytes
        .try_into()
        .map_err(|_| "IC pause request ID must be 32 bytes")?;
    let request_id = ic_agent::RequestId::new(&request_hash);
    let status = ic_agent::lookup_value(
        &certificate,
        [
            "request_status".as_bytes(),
            request_id.as_slice(),
            "status".as_bytes(),
        ],
    )
    .map_err(|error| format!("IC pause status is not certified: {error}"))?;
    if status != b"replied" {
        return Err("IC pause request did not have a certified replied status".into());
    }
    let reply = ic_agent::lookup_value(
        &certificate,
        [
            "request_status".as_bytes(),
            request_id.as_slice(),
            "reply".as_bytes(),
        ],
    )
    .map_err(|error| format!("IC pause reply is not certified: {error}"))?;
    let expected_reply = decode_hex(&drill.ic_pause.response_hex)?;
    if reply != expected_reply {
        return Err("certified IC pause reply differs from monitor evidence".into());
    }
    let decoded = Decode!(reply, EmergencyPauseResultView).map_err(|error| error.to_string())?;
    let EmergencyPauseResultView::Ok(receipt) = decoded else {
        return Err("certified emergency_pause reply is an error".into());
    };
    let pause_principal =
        Principal::from_text(&drill.ic_pause.pause_principal).map_err(|error| error.to_string())?;
    let audit_sha = decode_hex(&drill.ic_pause.audit_sha256)?;
    let action_plan = drill
        .base_actions
        .iter()
        .map(|action| action.kind.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let action_plan_sha256 = Sha256::digest(action_plan.as_bytes());
    if receipt.caller != pause_principal
        || drill.ic_pause.pause_principal != bundle.profile.pause_principal
        || !receipt.local_deposits_paused
        || receipt.local_pause_audit_sequence != drill.ic_pause.audit_sequence
        || receipt.local_pause_audit_sha256 != audit_sha
        || !receipt.base_actions_queued
        || usize::from(receipt.base_action_count) != drill.base_actions.len()
        || receipt.base_action_plan_sha256 != action_plan_sha256.as_slice()
    {
        return Err("certified emergency_pause receipt is not bound to the drill evidence".into());
    }
    Ok(())
}

fn verify_keeper_authenticity(bundle: &ValidatedBundle) -> Result<(), String> {
    let monitoring: MonitoringReceipt = read_json(&bundle.root.join("monitoring-receipt.json"))?;
    let withdrawal_id = decode_hex(&monitoring.withdrawal_id)?;
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let arg = Encode!(&withdrawal_id).map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let response = async_runtime()?
        .block_on(async {
            agent
                .query(&bridge, "get_withdrawal")
                .with_arg(arg)
                .call()
                .await
        })
        .map_err(|error| format!("authenticated get_withdrawal query failed: {error}"))?;
    let expected = decode_hex(&monitoring.paid.response_hex)?;
    if response != expected
        || !hex(&Sha256::digest(&response)).eq_ignore_ascii_case(&monitoring.paid.response_sha256)
    {
        return Err("live get_withdrawal response differs from monitoring evidence".into());
    }
    let withdrawal = Decode!(&response, Option<WithdrawalView>)
        .map_err(|error| format!("invalid live get_withdrawal response: {error}"))?
        .ok_or("live monitoring withdrawal is missing")?;
    if withdrawal.withdrawal_id != withdrawal_id || withdrawal.state != WithdrawalPhaseView::Paid {
        return Err("live monitoring withdrawal is not the bound Paid record".into());
    }
    Ok(())
}

fn validate_activation_attestation(
    profile: &Profile,
    attestation: &ActivationAttestationView,
    manifest_created_at_unix: u64,
    minimum_finalized_block: u64,
    now: u64,
) -> Result<(), String> {
    validate_activation_attestation_time(
        attestation.observed_at_ns,
        manifest_created_at_unix,
        now,
    )?;
    let expected_signer = decode_address(&profile.expected_bridge_signer)?;
    let expected_runtime = decode_hex(&profile.bridge_runtime_bytecode_sha256)?;
    let expected_timelock = decode_address(&profile.timelock.address)?;
    let expected_operator = decode_address(&profile.governance_operator)?;
    let expected_runtime_administrator = decode_address(&profile.runtime_administrator)?;
    let expected_independent_canceller = decode_address(&profile.independent_canceller)?;
    if attestation.chain_id != profile.chain_id
        || attestation.finalized_block_number < minimum_finalized_block
        || attestation.finalized_block_hash.len() != 32
        || attestation.bridge_signer != expected_signer
        || attestation.bridge_runtime_sha256 != expected_runtime
        || !attestation.deposits_paused
        || !attestation.withdrawals_paused
        || attestation.bridge_timelock != expected_timelock
        || attestation.runtime_administrator != expected_runtime_administrator
        || attestation.timelock_admin != expected_timelock
        || attestation.timelock_proposer != expected_operator
        || attestation.timelock_canceller != expected_independent_canceller
        || attestation.timelock_executor != expected_operator
        || attestation.timelock_runtime_code_hash
            != decode_hex(&profile.timelock.runtime_code_hash)?
        || attestation.bridge_approved_timelock_runtime_code_hash
            != decode_hex(&profile.timelock.runtime_code_hash)?
        || attestation.timelock_minimum_delay_seconds != profile.timelock.minimum_delay_seconds
        || attestation.bsns_address != decode_address(&profile.bsns_contract)?
        || attestation.bsns_runtime_sha256 != decode_hex(&profile.bsns_runtime_bytecode_sha256)?
        || attestation.bsns_name != "KINIC"
        || attestation.bsns_symbol != "KINIC"
        || attestation.bsns_decimals != profile.decimals
        || attestation.bsns_bridge != decode_address(&profile.bridge_contract)?
        || attestation.base_service_fee != profile.parameters.service_fee
    {
        return Err("authenticated activation attestation does not match the release".into());
    }
    Ok(())
}

fn verify_activation_attestation_authenticity(bundle: &ValidatedBundle) -> Result<(), String> {
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let raw = async_runtime()?.block_on(async {
        agent
            .query(&bridge, "get_activation_attestation")
            .with_arg(Encode!().map_err(|error| error.to_string())?)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())
    })?;
    let ActivationAttestationResultView::Ok(attestation) =
        Decode!(&raw, ActivationAttestationResultView).map_err(|error| error.to_string())?
    else {
        return Err("authenticated activation attestation is unavailable".into());
    };
    validate_activation_attestation(
        &bundle.profile,
        &attestation,
        bundle.manifest.created_at_unix,
        1,
        now_unix()?,
    )
}

fn verify_sns_upgrade_authenticity(bundle: &ValidatedBundle) -> Result<(), String> {
    let upgrade: SnsUpgrade = read_json(&bundle.root.join("sns-upgrade.json"))?;
    let governance = Principal::from_text(KINIC_GOVERNANCE).map_err(|e| e.to_string())?;
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let arg = Encode!(&GetProposalRequest {
        proposal_id: Some(ProposalId {
            id: upgrade.proposal_id,
        }),
    })
    .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (response, controllers, module_hash) = async_runtime()?.block_on(async {
        let response = agent
            .query(&governance, "get_proposal")
            .with_arg(arg)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let controllers = agent
            .read_state_canister_controllers(bridge)
            .await
            .map_err(|error| error.to_string())?;
        let module_hash = agent
            .read_state_canister_module_hash(bridge)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((response, controllers, module_hash))
    })?;
    if response != decode_hex(&upgrade.governance_query_response_hex)? {
        return Err("authenticated SNS proposal response differs from the evidence".into());
    }
    let decoded = Decode!(&response, GetProposalResponse).map_err(|error| error.to_string())?;
    let proposal = match decoded.result {
        Some(GetProposalResult::Proposal(proposal)) => proposal,
        Some(GetProposalResult::Error(error)) => {
            return Err(format!(
                "SNS get_proposal returned {}: {}",
                error.error_type, error.error_message
            ));
        }
        None => return Err("SNS get_proposal returned no result".into()),
    };
    let proposal_id = proposal.id.as_ref().map(|id| id.id);
    let action = proposal
        .proposal
        .and_then(|proposal| proposal.action)
        .ok_or("SNS proposal has no action")?;
    let SnsProposalAction::UpgradeSnsControlledCanister(action) = action else {
        return Err("SNS proposal is not UpgradeSnsControlledCanister".into());
    };
    let wasm_hash = hex(&Sha256::digest(&action.new_canister_wasm));
    if proposal_id != Some(upgrade.proposal_id)
        || proposal.executed_timestamp_seconds == 0
        || proposal.executed_timestamp_seconds != upgrade.executed_at_unix
        || proposal.failed_timestamp_seconds != 0
        || proposal.failure_reason.is_some()
        || proposal.decided_timestamp_seconds == 0
        || action.canister_id != Some(bridge)
        || !wasm_hash.eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
        || controllers != [Principal::from_text(KINIC_ROOT).map_err(|e| e.to_string())?]
        || !hex(&module_hash).eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
    {
        return Err(
            "authenticated SNS upgrade or live controller/module state does not match the release"
                .into(),
        );
    }
    Ok(())
}

fn verify_provider_independence_authenticity(bundle: &ValidatedBundle) -> Result<(), String> {
    let receipt: ProviderIndependenceReceipt =
        read_json(&bundle.root.join("provider-independence.json"))?;
    let governance = Principal::from_text(KINIC_GOVERNANCE).map_err(|e| e.to_string())?;
    let arg = Encode!(&GetProposalRequest {
        proposal_id: Some(ProposalId {
            id: receipt.proposal_id,
        }),
    })
    .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let response = async_runtime()?.block_on(async {
        agent
            .query(&governance, "get_proposal")
            .with_arg(arg)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())
    })?;
    if response != decode_hex(&receipt.governance_query_response_hex)? {
        return Err("authenticated provider review response differs from the receipt".into());
    }
    let decoded = Decode!(&response, GetProposalResponse).map_err(|error| error.to_string())?;
    let proposal = match decoded.result {
        Some(GetProposalResult::Proposal(proposal)) => proposal,
        _ => return Err("SNS provider independence proposal is unavailable".into()),
    };
    let proposal_id = proposal.id.as_ref().map(|id| id.id);
    let content = proposal
        .proposal
        .ok_or("provider review proposal has no content")?;
    if proposal_id != Some(receipt.proposal_id)
        || proposal.executed_timestamp_seconds == 0
        || proposal.failed_timestamp_seconds != 0
        || proposal.failure_reason.is_some()
        || proposal.decided_timestamp_seconds == 0
        || !matches!(content.action, Some(SnsProposalAction::Motion(_)))
        || !content
            .summary
            .to_ascii_lowercase()
            .contains(&receipt.provider_review_sha256.to_ascii_lowercase())
    {
        return Err("SNS Governance did not execute the exact provider independence review".into());
    }
    Ok(())
}

fn gate_b_controller(bundle: &ValidatedBundle) -> Result<Principal, String> {
    let receipt: GateAReceipt = read_json(&bundle.root.join("gate-a-receipt.json"))?;
    Principal::from_text(&receipt.canister_install.installer_principal)
        .map_err(|error| error.to_string())
}

fn validate_gate_b_management_snapshot(
    bundle: &ValidatedBundle,
    controllers: &[Principal],
    module_hash: &[u8],
) -> Result<(), String> {
    let installer = gate_b_controller(bundle)?;
    if controllers != [installer]
        || !hex(module_hash).eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
    {
        return Err("Gate B requires the production identity as sole controller".into());
    }
    Ok(())
}

fn verify_gate_b_management_state(bundle: &ValidatedBundle) -> Result<(), String> {
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (controllers, module_hash) = async_runtime()?.block_on(async {
        let controllers = agent
            .read_state_canister_controllers(bridge)
            .await
            .map_err(|error| error.to_string())?;
        let module_hash = agent
            .read_state_canister_module_hash(bridge)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((controllers, module_hash))
    })?;
    validate_gate_b_management_snapshot(bundle, &controllers, &module_hash)
}

fn verify_live(bundle: &ValidatedBundle, expected_deposits_paused: bool) -> Result<(), String> {
    verify_live_inputs(bundle, expected_deposits_paused)?;
    verify_gate_b_management_state(bundle)?;
    verify_activation_attestation_authenticity(bundle)?;
    verify_monitor_drill_authenticity(bundle)?;
    verify_provider_independence_authenticity(bundle)
}

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err("receipt parent directory does not exist".into());
    }
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    output
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    output.sync_all().map_err(|error| error.to_string())
}

fn activation_raw_digest_matches(raw: &str, digest: &str) -> Result<bool, String> {
    Ok(valid_sha256(digest) && hex(&Sha256::digest(decode_hex(raw)?)).eq_ignore_ascii_case(digest))
}

fn validate_schedule_receipt_binding(
    receipt: &ActivationReceipt,
    bundle: &ValidatedBundle,
) -> Result<(), String> {
    let canonical_payload = [0x44, 0x49, 0x44, 0x4c, 0x00, 0x00];
    let payload_sha256 = hex(&Sha256::digest(canonical_payload));
    let now = now_unix()?;
    let initial_path = bundle.root.join("initial-operational-parameters.json");
    let initial_bytes = fs::read(&initial_path).map_err(|error| error.to_string())?;
    let expected_initial_sha256 = bundle
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "initial-operational-parameters.json")
        .ok_or("Gate B manifest has no initial operational parameter evidence")?
        .sha256
        .as_str();
    if !hex(&Sha256::digest(&initial_bytes)).eq_ignore_ascii_case(expected_initial_sha256) {
        return Err(
            "initial operational parameter evidence changed after Gate B validation".into(),
        );
    }
    let initial: InitialOperationalParameters =
        serde_json::from_slice(&initial_bytes).map_err(|error| error.to_string())?;
    validate_initial_operational_parameters(
        &initial,
        &bundle.profile,
        bundle.manifest.created_at_unix,
        now,
    )?;
    let deployment_instance_id: [u8; 32] = decode_hex(&initial.deployment_instance_id)?
        .try_into()
        .map_err(|_| "invalid initial deployment instance ID")?;
    let expected_salt = initial_activation_salt(deployment_instance_id, 0);
    let expected_operation_id =
        initial_activation_operation_id(decode_address(&initial.bridge_contract)?, expected_salt);
    if receipt.schema_version != 4
        || receipt.phase != "schedule"
        || receipt.release_id != bundle.manifest.release_id
        || receipt.source_revision != bundle.manifest.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || !valid_sha256(&receipt.gate_b_manifest_sha256)
        || !receipt
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(&bundle.manifest_sha256)
        || receipt.proposal_id == 0
        || receipt.function_id == 0
        || receipt.target_method_name != "schedule_activation"
        || !receipt.payload_sha256.eq_ignore_ascii_case(&payload_sha256)
        || receipt.executed_at_unix == 0
        || receipt.verified_at_unix < receipt.executed_at_unix
        || receipt.verified_at_unix < bundle.manifest.created_at_unix
        || receipt.verified_at_unix > now
        || now - receipt.verified_at_unix > MAX_EVIDENCE_AGE_SECS
        || !activation_raw_digest_matches(
            &receipt.governance_query_response_hex,
            &receipt.governance_query_response_sha256,
        )?
        || !activation_raw_digest_matches(
            &receipt.function_registry_response_hex,
            &receipt.function_registry_response_sha256,
        )?
        || !activation_raw_digest_matches(
            &receipt.activation_status_response_hex,
            &receipt.activation_status_response_sha256,
        )?
        || !valid_hash32(&receipt.operation_id)
        || !valid_hash32(&receipt.operation_salt)
        || !receipt
            .operation_id
            .eq_ignore_ascii_case(&format!("0x{}", hex(&expected_operation_id)))
        || !receipt
            .operation_salt
            .eq_ignore_ascii_case(&format!("0x{}", hex(&expected_salt)))
        || receipt.prior_schedule_receipt_sha256.is_some()
    {
        return Err("prior schedule receipt is malformed or not bound to this release".into());
    }
    Ok(())
}

struct LiveActivationSnapshot {
    proposal_raw: Vec<u8>,
    registry_raw: Vec<u8>,
    activation_raw: Vec<u8>,
    controllers: Vec<Principal>,
    module_hash: Vec<u8>,
}

fn fetch_live_activation_snapshot(
    host: &str,
    bridge: Principal,
    proposal_id: u64,
    canonical_payload: &[u8],
) -> Result<LiveActivationSnapshot, String> {
    let governance = Principal::from_text(KINIC_GOVERNANCE).map_err(|error| error.to_string())?;
    let proposal_arg = Encode!(&GetProposalRequest {
        proposal_id: Some(ProposalId { id: proposal_id }),
    })
    .map_err(|error| error.to_string())?;
    let empty_arg = canonical_payload.to_vec();
    let agent = mainnet_agent(host, false)?;
    async_runtime()?.block_on(async {
        let proposal_raw = agent
            .query(&governance, "get_proposal")
            .with_arg(proposal_arg)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let registry_raw = agent
            .query(&governance, "list_nervous_system_functions")
            .with_arg(empty_arg.clone())
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let activation_raw = agent
            .query(&bridge, "get_activation_status")
            .with_arg(empty_arg)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let controllers = agent
            .read_state_canister_controllers(bridge)
            .await
            .map_err(|error| error.to_string())?;
        let module_hash = agent
            .read_state_canister_module_hash(bridge)
            .await
            .map_err(|error| error.to_string())?;
        Ok(LiveActivationSnapshot {
            proposal_raw,
            registry_raw,
            activation_raw,
            controllers,
            module_hash,
        })
    })
}

fn verify_activation(
    phase: &str,
    bundle: &ValidatedBundle,
    submission_path: &Path,
    prior_path: Option<&Path>,
    receipt_path: &Path,
) -> Result<(), String> {
    if phase != "schedule" && phase != "execute" {
        return Err("activation phase must be schedule or execute".into());
    }
    let submission: ActivationSubmission = read_json(submission_path)?;
    let now = now_unix()?;
    validate_activation_time(
        submission.submitted_at_unix,
        bundle.manifest.created_at_unix,
        now,
    )?;
    let method = if phase == "schedule" {
        "schedule_activation"
    } else {
        "execute_activation"
    };
    let canonical_payload = [0x44, 0x49, 0x44, 0x4c, 0x00, 0x00];
    let proposal_response = decode_hex(&submission.proposal_response_hex)?;
    if submission.schema_version != 3
        || submission.phase != phase
        || submission.release_id != bundle.manifest.release_id
        || submission.source_revision != bundle.manifest.source_revision
        || !submission
            .source_tree_sha256
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || !submission
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(&bundle.manifest_sha256)
        || submission.governance_canister_id != KINIC_GOVERNANCE
        || submission.bridge_canister_id != bundle.profile.bridge_canister_id
        || submission.function_id == 0
        || submission.target_method_name != method
        || decode_hex(&submission.payload_hex)? != canonical_payload
        || !submission
            .payload_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(canonical_payload)))
        || !principal(&submission.proposer_principal)
        || submission.neuron_subaccount.len() != 64
        || !submission
            .neuron_subaccount
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || submission.proposal_id == 0
        || !valid_sha256(&submission.registry_response_sha256)
        || !submission
            .proposal_response_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(&proposal_response)))
        || submission.registry_command_argv.is_empty()
        || submission.proposal_command_argv.is_empty()
    {
        return Err(
            "activation submission is not exactly bound to the reviewed Gate B release".into(),
        );
    }

    let prior = match (phase, prior_path) {
        ("schedule", None) => None,
        ("schedule", Some(_)) => return Err("schedule verification forbids a prior receipt".into()),
        ("execute", Some(path)) => {
            let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
            if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                return Err("execute predecessor receipt must be an ordinary file".into());
            }
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            let digest = hex(&Sha256::digest(&bytes));
            let receipt = serde_json::from_slice::<ActivationReceipt>(&bytes)
                .map_err(|error| error.to_string())?;
            Some((receipt, digest))
        }
        ("execute", None) => {
            return Err("execute verification requires the schedule receipt".into())
        }
        _ => unreachable!(),
    };
    if let Some((receipt, _)) = prior.as_ref() {
        validate_schedule_receipt_binding(receipt, bundle)?;
    }

    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let snapshot = fetch_live_activation_snapshot(
        &bundle.profile.ic_host,
        bridge,
        submission.proposal_id,
        &canonical_payload,
    )?;
    let LiveActivationSnapshot {
        proposal_raw,
        registry_raw,
        activation_raw,
        controllers,
        module_hash,
    } = snapshot;
    validate_gate_b_management_snapshot(bundle, &controllers, &module_hash)?;

    let decoded = Decode!(&proposal_raw, GetProposalResponse).map_err(|error| error.to_string())?;
    let proposal = match decoded.result {
        Some(GetProposalResult::Proposal(proposal)) => proposal,
        Some(GetProposalResult::Error(error)) => {
            return Err(format!(
                "SNS get_proposal returned {}: {}",
                error.error_type, error.error_message
            ));
        }
        None => return Err("SNS get_proposal returned no result".into()),
    };
    let proposal_id = proposal.id.as_ref().map(|id| id.id);
    let executed_at = proposal.executed_timestamp_seconds;
    let action = proposal
        .proposal
        .and_then(|proposal| proposal.action)
        .ok_or("activation proposal has no action")?;
    let SnsProposalAction::ExecuteGenericNervousSystemFunction(action) = action else {
        return Err("activation proposal is not ExecuteGenericNervousSystemFunction".into());
    };
    if proposal_id != Some(submission.proposal_id)
        || executed_at == 0
        || executed_at < submission.submitted_at_unix
        || executed_at > now
        || proposal.failed_timestamp_seconds != 0
        || proposal.failure_reason.is_some()
        || proposal.decided_timestamp_seconds == 0
        || action.function_id != submission.function_id
        || action.payload != canonical_payload
    {
        return Err(
            "authenticated activation proposal or live Canister state does not match the release"
                .into(),
        );
    }

    let registry = Decode!(&registry_raw, ListNervousSystemFunctionsResponseView)
        .map_err(|error| error.to_string())?;
    let matching_functions = registry
        .functions
        .iter()
        .filter(|function| {
            function.id == submission.function_id
                && matches!(
                    function.function_type.as_ref(),
                    Some(FunctionTypeView::GenericNervousSystemFunction(generic))
                        if generic.target_canister_id == Some(bridge)
                            && generic.target_method_name.as_deref() == Some(method)
                )
        })
        .count();
    if matching_functions != 1 {
        return Err(
            "authenticated SNS function registry has no unique exact activation target".into(),
        );
    }

    let activation =
        Decode!(&activation_raw, ActivationStatusResultView).map_err(|error| error.to_string())?;
    let ActivationStatusResultView::Ok(activation) = activation else {
        return Err("authenticated get_activation_status returned an error".into());
    };
    let (operation_id, operation_salt) = if phase == "schedule" {
        let pending = activation
            .pending_timelock_operation
            .as_ref()
            .ok_or("scheduled activation has no pending Timelock operation")?;
        if !activation.deposits_paused
            || pending.operation_id.len() != 32
            || pending.salt.len() != 32
        {
            return Err("scheduled activation status is unsafe or malformed".into());
        }
        (
            format!("0x{}", hex(&pending.operation_id)),
            format!("0x{}", hex(&pending.salt)),
        )
    } else {
        if activation.deposits_paused || activation.pending_timelock_operation.is_some() {
            return Err(
                "executed activation did not unpause and clear the Timelock operation".into(),
            );
        }
        let prior = &prior.as_ref().expect("execute prior checked").0;
        (prior.operation_id.clone(), prior.operation_salt.clone())
    };
    let confirmation = activation
        .last_confirmed_activation
        .as_ref()
        .ok_or("activation has no Finalized Canister confirmation")?;
    if confirmation.phase != phase
        || confirmation.receipt_block_number == 0
        || confirmation.transaction_hash.len() != 32
        || format!("0x{}", hex(&confirmation.timelock_operation_id)) != operation_id.to_lowercase()
    {
        return Err(
            "authenticated activation confirmation does not match the requested phase".into(),
        );
    }

    let prior_schedule_receipt_sha256 = prior.as_ref().map(|(_, digest)| digest.clone());
    let receipt = ActivationReceipt {
        schema_version: 4,
        phase: phase.into(),
        release_id: bundle.manifest.release_id.clone(),
        source_revision: bundle.manifest.source_revision.clone(),
        source_tree_sha256: bundle.manifest.source_tree_sha256.clone(),
        gate_b_manifest_sha256: bundle.manifest_sha256.clone(),
        proposal_id: submission.proposal_id,
        function_id: submission.function_id,
        target_method_name: method.into(),
        payload_sha256: submission.payload_sha256.clone(),
        executed_at_unix: executed_at,
        verified_at_unix: now,
        governance_query_response_hex: hex(&proposal_raw),
        governance_query_response_sha256: hex(&Sha256::digest(&proposal_raw)),
        function_registry_response_hex: hex(&registry_raw),
        function_registry_response_sha256: hex(&Sha256::digest(&registry_raw)),
        activation_status_response_hex: hex(&activation_raw),
        activation_status_response_sha256: hex(&Sha256::digest(&activation_raw)),
        operation_id,
        operation_salt,
        prior_schedule_receipt_sha256,
    };
    write_json_new(receipt_path, &receipt)
}

fn verify_schedule_receipt_live(
    bundle: &ValidatedBundle,
    receipt_path: &Path,
) -> Result<(), String> {
    let receipt: ActivationReceipt = read_json(receipt_path)?;
    let canonical_payload = [0x44, 0x49, 0x44, 0x4c, 0x00, 0x00];
    validate_schedule_receipt_binding(&receipt, bundle)?;

    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let snapshot = fetch_live_activation_snapshot(
        &bundle.profile.ic_host,
        bridge,
        receipt.proposal_id,
        &canonical_payload,
    )?;
    let LiveActivationSnapshot {
        proposal_raw,
        registry_raw,
        activation_raw,
        controllers,
        module_hash,
    } = snapshot;
    validate_gate_b_management_snapshot(bundle, &controllers, &module_hash)?;

    let decoded = Decode!(&proposal_raw, GetProposalResponse).map_err(|error| error.to_string())?;
    let proposal = match decoded.result {
        Some(GetProposalResult::Proposal(proposal)) => proposal,
        Some(GetProposalResult::Error(error)) => {
            return Err(format!(
                "SNS get_proposal returned {}: {}",
                error.error_type, error.error_message
            ));
        }
        None => return Err("SNS get_proposal returned no result".into()),
    };
    let executed_at = proposal.executed_timestamp_seconds;
    let action = proposal
        .proposal
        .and_then(|proposal| proposal.action)
        .ok_or("schedule proposal has no action")?;
    let SnsProposalAction::ExecuteGenericNervousSystemFunction(action) = action else {
        return Err("schedule proposal is not ExecuteGenericNervousSystemFunction".into());
    };
    if proposal.id.as_ref().map(|id| id.id) != Some(receipt.proposal_id)
        || executed_at != receipt.executed_at_unix
        || proposal.failed_timestamp_seconds != 0
        || proposal.failure_reason.is_some()
        || proposal.decided_timestamp_seconds == 0
        || action.function_id != receipt.function_id
        || action.payload != canonical_payload
    {
        return Err(
            "authenticated schedule proposal or Canister state does not match the receipt".into(),
        );
    }

    let registry = Decode!(&registry_raw, ListNervousSystemFunctionsResponseView)
        .map_err(|error| error.to_string())?;
    let matching_functions = registry
        .functions
        .iter()
        .filter(|function| {
            function.id == receipt.function_id
                && matches!(
                    function.function_type.as_ref(),
                    Some(FunctionTypeView::GenericNervousSystemFunction(generic))
                        if generic.target_canister_id == Some(bridge)
                            && generic.target_method_name.as_deref()
                                == Some("schedule_activation")
                )
        })
        .count();
    if matching_functions != 1 {
        return Err("authenticated SNS registry no longer has the exact schedule function".into());
    }

    let activation =
        Decode!(&activation_raw, ActivationStatusResultView).map_err(|error| error.to_string())?;
    let ActivationStatusResultView::Ok(activation) = activation else {
        return Err("authenticated get_activation_status returned an error".into());
    };
    let pending = activation
        .pending_timelock_operation
        .as_ref()
        .ok_or("schedule receipt operation is no longer pending in the Canister")?;
    if !activation.deposits_paused
        || format!("0x{}", hex(&pending.operation_id)) != receipt.operation_id.to_lowercase()
        || format!("0x{}", hex(&pending.salt)) != receipt.operation_salt.to_lowercase()
    {
        return Err("live Canister activation state does not match the schedule receipt".into());
    }
    let confirmation = activation
        .last_confirmed_activation
        .as_ref()
        .ok_or("schedule receipt has no Finalized Canister confirmation")?;
    if confirmation.phase != "schedule"
        || confirmation.receipt_block_number == 0
        || confirmation.transaction_hash.len() != 32
        || format!("0x{}", hex(&confirmation.timelock_operation_id))
            != receipt.operation_id.to_lowercase()
    {
        return Err("live Finalized schedule confirmation does not match the receipt".into());
    }

    Ok(())
}

fn verify_monitor_drill_authenticity(bundle: &ValidatedBundle) -> Result<(), String> {
    verify_monitor_ic_certificate(bundle)?;
    let verifier =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/production-live-preflight.sh");
    let status = Command::new(verifier)
        .arg("verify-monitor-drill")
        .arg(&bundle.root)
        .status()
        .map_err(|error| format!("failed to execute monitor drill Base verifier: {error}"))?;
    if !status.success() {
        return Err("monitor drill Base receipt/log verifier rejected the evidence".into());
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("derive") if args.len() == 3 => {
            let evidence: Evidence = read_json(Path::new(&args[2]))?;
            println!("{}", serde_json::to_string_pretty(&derive(&evidence)?).map_err(|e| e.to_string())?);
        }
        Some("validate") | Some("validate-test") if args.len() == 3 => {
            let profile: Profile = read_json(Path::new(&args[2]))?;
            validate_profile(&profile, args[1] == "validate")?;
            println!("{}", hex(&canonical_sha256(&profile)?));
        }
        Some("render-release-inputs") if args.len() == 4 => {
            render_release_inputs(Path::new(&args[2]), Path::new(&args[3]), true, None)?;
        }
        Some("render-test-inputs") if args.len() == 4 => {
            render_release_inputs(Path::new(&args[2]), Path::new(&args[3]), false, None)?;
        }
        Some("render-bundle-inputs") if args.len() == 4 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only {
                return Err("production release inputs reject test-only bundles".into());
            }
            render_release_inputs(
                &Path::new(&args[2]).join("profile.json"),
                Path::new(&args[3]),
                true,
                Some(&bundle.manifest_sha256),
            )?;
        }
        Some("validate-production-canister-plan") if args.len() == 3 => {
            let plan: ProductionCanisterPlan = read_json(Path::new(&args[2]))?;
            validate_production_canister_plan(&plan)?;
            println!("{}", hex(&canonical_sha256(&plan)?));
        }
        Some("render-production-canister-inputs") if args.len() == 4 => {
            render_production_canister_inputs(Path::new(&args[2]), Path::new(&args[3]))?;
        }
        Some("validate-production-canister-receipt") if args.len() == 4 => {
            println!(
                "{}",
                validate_production_canister_receipt_files(
                    Path::new(&args[2]),
                    Path::new(&args[3]),
                )?
            );
        }
        Some("validate-production-handover-receipt") if args.len() == 6 => {
            println!(
                "{}",
                validate_production_handover_receipt_files(
                    Path::new(&args[2]),
                    Path::new(&args[3]),
                    Path::new(&args[4]),
                    Path::new(&args[5]),
                )?
            );
        }
        Some("verify-production-canister-predeploy") if args.len() == 4 => {
            verify_production_canister_predeploy(Path::new(&args[2]), Path::new(&args[3]))?;
        }
        Some("validate-production-handover-candidate") if args.len() == 8 => {
            validate_production_handover_candidate_files(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
            )?;
        }
        Some("verify-production-canister-handover") if args.len() == 8 => {
            verify_production_canister_handover(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
            )?;
        }
        Some("storage-validation-complete") if args.len() == 3 => {
            println!("{}", storage_validation_complete(&args[2])?);
        }
        Some("storage-checksum-complete") if args.len() == 3 => {
            println!("{}", storage_checksum_complete(&args[2])?);
        }
        Some("write-production-canister-receipt") if args.len() == 15 => {
            write_production_canister_install_receipt(
                Path::new(&args[2]),
                &args[3],
                &args[4],
                &args[5],
                &args[6],
                &args[7],
                &args[8],
                &args[9],
                &args[10],
                &args[11],
                &args[12],
                &args[13],
                Path::new(&args[14]),
            )?;
        }
        Some("validate-bundle") if args.len() == 4 && args[2] == "--offline" => {
            let bundle = validate_bundle(Path::new(&args[3]), false)?;
            println!(
                "gate_a=pass authorizing=true manifest_sha256={}",
                bundle.manifest_sha256
            );
        }
        Some("validate-bundle")
            if args.len() == 5 && args[2] == "--offline" && args[3] == "--gate-b" =>
        {
            let bundle = validate_bundle(Path::new(&args[4]), true)?;
            println!(
                "gate_b=structural-pass authorizing=false manifest_sha256={}",
                bundle.manifest_sha256
            );
        }
        Some("verify-live") if args.len() == 3 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only { return Err("Gate B rejects test-only bundles".into()); }
            verify_live(&bundle, true)?;
            println!("gate_b=pass manifest_sha256={}", bundle.manifest_sha256);
        }
        Some("verify-activation") if args.len() == 7 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err("activation verification rejects test-only bundles".into());
            }
            match args[2].as_str() {
                "schedule" => verify_live(&bundle, true)?,
                "execute" => verify_live_inputs(&bundle, false)?,
                _ => return Err("activation phase must be schedule or execute".into()),
            }
            let prior = if args[5] == "-" {
                None
            } else {
                Some(Path::new(&args[5]))
            };
            verify_activation(
                &args[2],
                &bundle,
                Path::new(&args[4]),
                prior,
                Path::new(&args[6]),
            )?;
            println!("activation=verified phase={} receipt={}", args[2], args[6]);
        }
        Some("verify-schedule-receipt-live") if args.len() == 4 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only {
                return Err("schedule receipt verification rejects test-only bundles".into());
            }
            verify_live(&bundle, true)?;
            verify_schedule_receipt_live(&bundle, Path::new(&args[3]))?;
            println!(
                "schedule_receipt=verified manifest_sha256={} receipt={}",
                bundle.manifest_sha256, args[3]
            );
        }
        _ => return Err("usage: bridge-profile <derive|validate|validate-test> <json-file> | validate-production-canister-plan <plan.json> | render-production-canister-inputs <plan.json> <output-dir> | validate-production-canister-receipt <profile.json> <receipt.json> | validate-production-handover-receipt <gate-a-bundle-dir> <gate-a-receipt.json> <install-receipt.json> <deployment-binding.json> | validate-production-handover-candidate <gate-a-bundle-dir> <final-profile.json> <measurements.json> <gate-a-receipt.json> <install-receipt.json> <deployment-binding.json> | verify-production-canister-predeploy <profile.json> <receipt.json> | verify-production-canister-handover <gate-a-bundle-dir> <final-profile.json> <measurements.json> <gate-a-receipt.json> <install-receipt.json> <deployment-binding.json> | render-release-inputs <profile.json> <output-dir> | render-test-inputs <profile.json> <output-dir> | render-bundle-inputs <bundle-dir> <output-dir> | validate-bundle --offline <bundle-dir> | validate-bundle --offline --gate-b <bundle-dir> | verify-live <bundle-dir> | verify-schedule-receipt-live <bundle-dir> <schedule-receipt.json> | verify-activation <schedule|execute> <bundle-dir> <submission.json> <prior-schedule-receipt.json|-> <receipt.json>".into()),
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("bridge-profile: {error}");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_activation_calldata_matches_the_solidity_abi_vectors() {
        let bridge = [7; 20];
        let salt = [0x11; 32];
        let schedule =
            initial_activation_calldata("schedule_activation", bridge, salt, 86_400).unwrap();
        let execute =
            initial_activation_calldata("execute_activation", bridge, salt, 86_400).unwrap();
        assert_eq!(
            hex(&Sha256::digest(decode_hex(&schedule).unwrap())),
            "791b06b957a214ad82713256c3f1031c952c74029e28cb43f71cde76dc26c8c4"
        );
        assert_eq!(
            hex(&Sha256::digest(decode_hex(&execute).unwrap())),
            "5f84cf3c08bbb0b9fda03854d0cbd697ca22aa5d94e94ad220db58111904fc37"
        );
    }

    #[test]
    fn activation_timestamps_must_follow_gate_b_and_precede_verification() {
        let created = MAX_EVIDENCE_AGE_SECS + 1_000_000;
        let now = created + 120;
        assert!(validate_activation_time(created, created, now).is_ok());
        assert!(validate_activation_time(created + 60, created, now).is_ok());
        assert!(validate_activation_time(created - 1, created, now).is_err());
        assert!(validate_activation_time(now + 1, created, now).is_err());
        assert!(validate_activation_time(
            now - MAX_EVIDENCE_AGE_SECS - 1,
            now - MAX_EVIDENCE_AGE_SECS - 1,
            now,
        )
        .is_err());
    }

    #[test]
    fn activation_attestation_must_be_fresh_and_follow_gate_b() {
        let created = 1_000_000;
        let now = created + 300;
        let ns = |seconds: u64| seconds * 1_000_000_000;
        assert!(validate_activation_attestation_time(ns(created), created, now).is_ok());
        assert!(validate_activation_attestation_time(ns(created - 1), created, now).is_err());
        assert!(validate_activation_attestation_time(ns(now + 1), created, now).is_err());
        assert!(validate_activation_attestation_time(ns(created), created, now + 1).is_err());
        assert!(validate_activation_attestation_time(0, created, now).is_err());
    }

    fn test_principal(seed: u8) -> String {
        Principal::self_authenticating([seed; 32]).to_text()
    }
    fn address(seed: u8) -> String {
        format!("0x{seed:040x}")
    }
    fn address_bytes(seed: u8) -> [u8; 20] {
        let mut value = [0; 20];
        value[19] = seed;
        value
    }

    fn measurement_samples(value: u128, start: u64) -> Vec<MeasurementSample> {
        (0..10)
            .map(|index| MeasurementSample {
                value,
                observed_at_unix: start + index,
                source_ref: format!("measurement-{index}"),
            })
            .collect()
    }

    fn fee_samples(base: u128, priority: u128, l1: u128, start: u64) -> Vec<FeeMeasurementSample> {
        (0..10)
            .map(|index| FeeMeasurementSample {
                base_fee_per_gas: base,
                priority_fee_per_gas: priority,
                l1_fee_upper_bound_wei: l1,
                observed_at_unix: if index == 9 {
                    start + 7 * 24 * 60 * 60
                } else {
                    start + index
                },
                source_ref: format!("fee-{index}"),
            })
            .collect()
    }

    fn measurement_evidence(start: u64) -> Evidence {
        Evidence {
            schema_version: 3,
            environment: "mainnet-candidate".into(),
            ledger_fee: 100_000,
            governance_gas_samples: measurement_samples(30_001, start),
            fee_samples: fee_samples(10, 2, 5, start),
            settlement_cycle_samples: measurement_samples(1_000, start),
            baseline_cycles_sample: MeasurementSample {
                value: 10_000,
                observed_at_unix: start,
                source_ref: "baseline-cycles".into(),
            },
            expected_daily_settlements: 4,
        }
    }

    #[test]
    fn release_id_is_strictly_bounded_and_manifest_safe() {
        assert!(valid_release_id("release-1"));
        assert!(valid_release_id("12345678"));
        assert!(!valid_release_id("short-1"));
        assert!(!valid_release_id("Release-1"));
        assert!(!valid_release_id("release_1"));
        assert!(!valid_release_id("release-1\naddress=0x00"));
        assert!(!valid_release_id(&"a".repeat(65)));
    }

    #[test]
    fn deployment_and_withdrawal_boundary_ids_must_be_nonzero() {
        assert!(valid_nonzero_hash32(&format!("0x{}", "11".repeat(32))));
        assert!(!valid_nonzero_hash32(&format!("0x{}", "00".repeat(32))));

        let mut profile = valid_profile();
        profile.minimum_withdrawal_id = format!("0x{}", "00".repeat(32));
        assert!(validate_profile(&profile, true).is_err());

        profile.minimum_withdrawal_id = format!("0x{}02", "00".repeat(31));
        assert!(validate_profile(&profile, true).is_err());

        profile.minimum_withdrawal_id = format!("0x{}01", "00".repeat(31));
        assert!(
            validate_profile(&profile, true).is_ok(),
            "{:?}",
            validate_profile(&profile, true)
        );
    }

    fn valid_profile() -> Profile {
        let bridge_contract = create_address(address_bytes(7), 1);
        Profile {
            schema_version: RELEASE_PROFILE_SCHEMA_VERSION,
            environment: "mainnet-candidate".into(),
            test_assets_only: false,
            chain_id: 8453,
            evm_rpc_canister_id: OFFICIAL_EVM_RPC_CANISTER.into(),
            ledger_canister_id: KINIC_LEDGER.into(),
            index_canister_id: KINIC_INDEX.into(),
            root_canister_id: KINIC_ROOT.into(),
            governance_principal: KINIC_GOVERNANCE.into(),
            confirmation_relayer_principal: test_principal(8),
            decimals: 8,
            bridge_canister_id: test_principal(9),
            canister_schema_version: CURRENT_STABLE_SCHEMA_VERSION,
            ic_host: "https://icp-api.io".into(),
            base_rpc_url: None,
            bridge_contract: format!("0x{}", hex(&bridge_contract)),
            bsns_contract: format!("0x{}", hex(&create_address(bridge_contract, 1))),
            deployment_instance_id: format!("0x{}", "11".repeat(32)),
            minimum_withdrawal_id: format!("0x{}01", "00".repeat(31)),
            deployment_block: 1,
            expected_bridge_signer: address(2),
            bridge_canister_wasm_sha256: "3".repeat(64),
            bridge_runtime_bytecode_sha256: "4".repeat(64),
            bsns_runtime_bytecode_sha256: "5".repeat(64),
            bsns_runtime_template_sha256: "6".repeat(64),
            ecdsa_key_name: "key_1".into(),
            ecdsa_derivation_path: vec!["KINIC-BASE-BRIDGE".into()],
            governance_ecdsa_derivation_path: vec!["KINIC-BASE-GOVERNANCE".into()],
            governance_operator: address(3),
            runtime_administrator: address(4),
            independent_canceller: address(5),
            initial_base_deployment: InitialBaseDeployment {
                deployer_address: address(7),
                starting_nonce: 0,
                gas_limit: 5_000_000,
                max_fee_per_gas: 200,
                max_priority_fee_per_gas: 10,
            },
            timelock: Timelock {
                address: format!("0x{}", hex(&create_address(address_bytes(7), 0))),
                runtime_code_hash: format!("0x{}", "ab".repeat(32)),
                minimum_delay_seconds: 86_400,
                proposer: address(3),
                canceller: address(5),
                executor: address(3),
                external_admins: 0,
            },
            pause_principal: test_principal(2),
            fee_recipient: test_principal(4),
            rpc_providers: vec![],
            monitoring: Monitoring {
                routing_sha256: "5".repeat(64),
                detection_minutes: 5,
                acknowledgement_minutes: 15,
                pause_both_sides_minutes: 60,
            },
            parameters: Parameters {
                ledger_fee: 100_000,
                per_deposit_limit: 1,
                mint_throughput_limit: 1,
                mint_window_duration_seconds: 3_600,
                max_service_fee: 1_000_000_000,
                service_fee: 50_000_000,
                gas_limit_ceiling: 100_000,
                max_fee_per_gas_ceiling: 200,
                max_priority_fee_per_gas_ceiling: 10,
                l1_fee_per_transaction_ceiling_wei: 100,
                quote_validity_seconds: 90,
                gas_limit_multiplier_bps: 13_000,
                base_fee_multiplier_bps: 60_000,
                l1_fee_multiplier_bps: 15_000,
                cycles_floor: 1,
                settlement_cycle_ceiling: 1,
            },
            rate_limits: RateLimits {
                deposit_window_seconds: 60,
                deposit_global: 30,
                deposit_per_principal: 3,
                notification_window_seconds: 600,
                notification_global: 60,
                notification_ingestion_global: 30,
                settlement_window_seconds: 600,
                settlement_global: 60,
                settlement_per_principal: 6,
                settlement_per_record: 3,
                settlement_retry_interval_seconds: 60,
            },
            governance_replacement: GovernanceReplacementPolicy {
                max_replacements: 3,
                fee_bump_bps: 1_250,
            },
        }
    }

    fn live_runtime_binding(profile: &Profile) -> LiveRuntimeBinding {
        let rpc_url_hash = hex(&canonical_sha256(
            &profile
                .rpc_providers
                .iter()
                .map(|provider| provider.url.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap());
        let operational_config_sha256 = expected_operational_config_sha256(profile, 900, 7)
            .map(|digest| hex(&digest))
            .unwrap();
        LiveRuntimeBinding {
            base_chain_id: profile.chain_id,
            bridge_contract: profile.bridge_contract.clone(),
            timelock_contract: profile.timelock.address.clone(),
            deployment_instance_id: profile.deployment_instance_id.clone(),
            minimum_withdrawal_id: profile.minimum_withdrawal_id.clone(),
            ledger_canister_id: profile.ledger_canister_id.clone(),
            index_canister_id: profile.index_canister_id.clone(),
            schema_version: profile.canister_schema_version,
            expected_bridge_signer: profile.expected_bridge_signer.clone(),
            evm_rpc_canister_id: profile.evm_rpc_canister_id.clone(),
            rpc_provider_urls_sha256: rpc_url_hash,
            operational_config_sha256,
        }
    }

    fn production_canister_plan(profile: &Profile) -> ProductionCanisterPlan {
        ProductionCanisterPlan {
            schema_version: 2,
            environment: "production".into(),
            source_revision: "a".repeat(40),
            source_tree_sha256: "b".repeat(64),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            init: ProductionCanisterInitInput {
                ledger_canister_id: profile.ledger_canister_id.clone(),
                index_canister_id: profile.index_canister_id.clone(),
                evm_rpc_canister_id: profile.evm_rpc_canister_id.clone(),
                custom_evm_rpc_urls: Vec::new(),
                base_chain_id: profile.chain_id,
                bridge_contract_hex: profile.bridge_contract.trim_start_matches("0x").into(),
                expected_bridge_runtime_sha256_hex: profile.bridge_runtime_bytecode_sha256.clone(),
                timelock_contract_hex: profile.timelock.address.trim_start_matches("0x").into(),
                expected_timelock_minimum_delay_seconds: profile.timelock.minimum_delay_seconds,
                expected_bsns_runtime_sha256_hex: profile.bsns_runtime_bytecode_sha256.clone(),
                expected_bsns_decimals: profile.decimals,
                expected_minimum_service_fee: profile.parameters.ledger_fee,
                deployment_instance_id_hex: profile
                    .deployment_instance_id
                    .trim_start_matches("0x")
                    .into(),
                minimum_withdrawal_id_hex: profile
                    .minimum_withdrawal_id
                    .trim_start_matches("0x")
                    .into(),
                ecdsa_key_name: profile.ecdsa_key_name.clone(),
                ecdsa_derivation_path_utf8: profile.ecdsa_derivation_path.clone(),
                governance_ecdsa_derivation_path_utf8: profile
                    .governance_ecdsa_derivation_path
                    .clone(),
                deposit_rate_limit_window_seconds: profile.rate_limits.deposit_window_seconds,
                deposit_rate_limit_global: profile.rate_limits.deposit_global,
                deposit_rate_limit_per_principal: profile.rate_limits.deposit_per_principal,
                notification_rate_limit_window_seconds: profile
                    .rate_limits
                    .notification_window_seconds,
                notification_rate_limit_global: profile.rate_limits.notification_global,
                notification_ingestion_rate_limit_global: profile
                    .rate_limits
                    .notification_ingestion_global,
                settlement_rate_limit_window_seconds: profile.rate_limits.settlement_window_seconds,
                settlement_rate_limit_global: profile.rate_limits.settlement_global,
                settlement_rate_limit_per_principal: profile.rate_limits.settlement_per_principal,
                settlement_rate_limit_per_record: profile.rate_limits.settlement_per_record,
                settlement_retry_interval_seconds: profile
                    .rate_limits
                    .settlement_retry_interval_seconds,
                governance_evm_fee: production_bootstrap_evm_fee(),
                governance_replacement: profile.governance_replacement,
                cycles_floor: PRODUCTION_BOOTSTRAP_CYCLES_FLOOR,
                settlement_cycle_ceiling: PRODUCTION_BOOTSTRAP_SETTLEMENT_CYCLE_CEILING,
                governance_principal: profile.governance_principal.clone(),
                pause_principal: profile.pause_principal.clone(),
                confirmation_relayer_principal: profile.confirmation_relayer_principal.clone(),
                fee_recipient: ProductionFeeRecipientInput {
                    owner: profile.fee_recipient.clone(),
                    subaccount_hex: String::new(),
                },
            },
        }
    }

    fn production_canister_receipt(profile: &Profile) -> ProductionCanisterInstallReceipt {
        let plan = production_canister_plan(profile);
        let init_candid_sha256 = hex(&Sha256::digest(
            validate_production_canister_plan(&plan).unwrap(),
        ));
        let mut runtime_binding = live_runtime_binding(profile);
        runtime_binding.operational_config_sha256 =
            hex(&expected_bootstrap_operational_config_sha256(
                &plan.init,
                &profile.governance_operator,
                900,
                7,
            )
            .unwrap());
        ProductionCanisterInstallReceipt {
            schema_version: PRODUCTION_CANISTER_INSTALL_RECEIPT_SCHEMA_VERSION,
            plan_sha256: hex(&canonical_sha256(&plan).unwrap()),
            plan: plan.clone(),
            source_revision: plan.source_revision.clone(),
            source_tree_sha256: plan.source_tree_sha256.clone(),
            canister_id: profile.bridge_canister_id.clone(),
            installer_principal: test_principal(31),
            module_sha256: profile.bridge_canister_wasm_sha256.clone(),
            init_candid_sha256,
            runtime_binding,
            governance_operator: profile.governance_operator.clone(),
            runtime_administrator: profile.runtime_administrator.clone(),
            independent_canceller: profile.independent_canceller.clone(),
            mint_authorization_ttl_seconds: 900,
            mint_authorization_epoch: 7,
            storage_validation_complete: true,
            storage_checksum_complete: true,
            deposits_paused: true,
            state_is_empty: true,
            cycles_reserve_sufficient: true,
        }
    }

    #[test]
    fn production_canister_plan_generates_the_typed_candid_init_argument() {
        let profile = valid_profile();
        let plan = production_canister_plan(&profile);
        let encoded = validate_production_canister_plan(&plan).unwrap();
        assert!(encoded.starts_with(b"DIDL"));
        let decoded = Decode!(&encoded, ProductionCanisterInitArgsCallView).unwrap();
        assert_eq!(decoded.base_chain_id, 8453);
        assert_eq!(decoded.bridge_contract.len(), 20);
        assert_eq!(decoded.deployment_instance_id.len(), 32);
        assert!(decoded.custom_evm_rpc_urls.is_empty());

        let mut obsolete_plan = plan.clone();
        obsolete_plan.schema_version = 1;
        assert!(validate_production_canister_plan(&obsolete_plan).is_err());

        let mut premature_final_values = plan.clone();
        premature_final_values.init.governance_evm_fee = profile.parameters.governance_evm_fee();
        premature_final_values.init.cycles_floor = profile.parameters.cycles_floor;
        premature_final_values.init.settlement_cycle_ceiling =
            profile.parameters.settlement_cycle_ceiling;
        assert!(validate_production_canister_plan(&premature_final_values).is_err());

        let mut unsafe_plan = plan;
        unsafe_plan.init.custom_evm_rpc_urls = vec!["https://unreviewed.example".into()];
        assert!(validate_production_canister_plan(&unsafe_plan).is_err());
    }

    #[test]
    fn production_canister_receipt_fails_closed_on_postcondition_drift() {
        let profile = valid_profile();
        let mut receipt = production_canister_receipt(&profile);
        assert!(validate_production_canister_receipt(&profile, &receipt).is_ok());
        let mut obsolete_receipt = receipt.clone();
        obsolete_receipt.schema_version = 2;
        assert!(validate_production_canister_receipt(&profile, &obsolete_receipt).is_err());
        receipt.deposits_paused = false;
        assert!(validate_production_canister_receipt(&profile, &receipt).is_err());
        receipt.deposits_paused = true;
        receipt.plan.init.cycles_floor += 1;
        receipt.plan_sha256 = hex(&canonical_sha256(&receipt.plan).unwrap());
        assert!(validate_production_canister_receipt(&profile, &receipt).is_err());
    }

    #[test]
    fn production_canister_predeploy_rejects_every_control_plane_role_drift() {
        let profile = valid_profile();
        let receipt = production_canister_receipt(&profile);
        let observed = ControlPlaneAddressesCallView {
            bridge_signer: decode_hex(&profile.expected_bridge_signer).unwrap(),
            governance_operator: decode_hex(&profile.governance_operator).unwrap(),
            runtime_administrator: decode_hex(&profile.runtime_administrator).unwrap(),
            independent_canceller: decode_hex(&profile.independent_canceller).unwrap(),
        };
        assert!(validate_control_plane_addresses(&profile, &receipt, &observed).is_ok());

        for index in 0..4 {
            let mut drifted = observed.clone();
            match index {
                0 => drifted.bridge_signer[0] ^= 1,
                1 => drifted.governance_operator[0] ^= 1,
                2 => drifted.runtime_administrator[0] ^= 1,
                _ => drifted.independent_canceller[0] ^= 1,
            }
            assert!(validate_control_plane_addresses(&profile, &receipt, &drifted).is_err());
        }

        let mut reordered = observed.clone();
        std::mem::swap(
            &mut reordered.runtime_administrator,
            &mut reordered.independent_canceller,
        );
        assert!(validate_control_plane_addresses(&profile, &receipt, &reordered).is_err());

        for index in 0..4 {
            let mut drifted_receipt = receipt.clone();
            match index {
                0 => drifted_receipt.runtime_binding.expected_bridge_signer = address(9),
                1 => drifted_receipt.governance_operator = address(9),
                2 => drifted_receipt.runtime_administrator = address(9),
                _ => drifted_receipt.independent_canceller = address(9),
            }
            assert!(
                validate_control_plane_addresses(&profile, &drifted_receipt, &observed).is_err()
            );
        }
    }

    #[test]
    fn production_canister_predeploy_rejects_certified_module_drift() {
        let profile = valid_profile();
        let receipt = production_canister_receipt(&profile);
        let controllers = [Principal::from_text(&receipt.installer_principal).unwrap()];
        let mut module_hash = decode_hex(&receipt.module_sha256).unwrap();
        assert!(validate_production_canister_management_state(
            &profile,
            &receipt,
            &controllers,
            &module_hash,
        )
        .is_ok());
        module_hash[0] ^= 1;
        assert!(validate_production_canister_management_state(
            &profile,
            &receipt,
            &controllers,
            &module_hash,
        )
        .is_err());
    }

    #[test]
    fn production_canister_predeploy_rejects_certified_extra_controller() {
        let profile = valid_profile();
        let receipt = production_canister_receipt(&profile);
        let controllers = [
            Principal::from_text(&receipt.installer_principal).unwrap(),
            Principal::anonymous(),
        ];
        let module_hash = decode_hex(&receipt.module_sha256).unwrap();
        assert!(validate_production_canister_management_state(
            &profile,
            &receipt,
            &controllers,
            &module_hash,
        )
        .is_err());
    }

    fn matching_activation_attestation(
        profile: &Profile,
        observed_at_unix: u64,
        finalized_block_number: u64,
    ) -> ActivationAttestationView {
        let operator = decode_address(&profile.governance_operator).unwrap();
        let runtime_administrator = decode_address(&profile.runtime_administrator).unwrap();
        let independent_canceller = decode_address(&profile.independent_canceller).unwrap();
        let timelock = decode_address(&profile.timelock.address).unwrap();
        ActivationAttestationView {
            chain_id: profile.chain_id,
            finalized_block_number,
            finalized_block_hash: vec![0xaa; 32],
            observed_at_ns: observed_at_unix * 1_000_000_000,
            bridge_signer: decode_address(&profile.expected_bridge_signer)
                .unwrap()
                .to_vec(),
            bridge_runtime_sha256: decode_hex(&profile.bridge_runtime_bytecode_sha256).unwrap(),
            deposits_paused: true,
            withdrawals_paused: true,
            bridge_timelock: timelock.to_vec(),
            runtime_administrator: runtime_administrator.to_vec(),
            timelock_admin: timelock.to_vec(),
            timelock_proposer: operator.to_vec(),
            timelock_canceller: independent_canceller.to_vec(),
            timelock_executor: operator.to_vec(),
            timelock_runtime_code_hash: decode_hex(&profile.timelock.runtime_code_hash).unwrap(),
            bridge_approved_timelock_runtime_code_hash: decode_hex(
                &profile.timelock.runtime_code_hash,
            )
            .unwrap(),
            timelock_minimum_delay_seconds: profile.timelock.minimum_delay_seconds,
            bsns_address: decode_address(&profile.bsns_contract).unwrap().to_vec(),
            bsns_runtime_sha256: decode_hex(&profile.bsns_runtime_bytecode_sha256).unwrap(),
            bsns_name: "KINIC".into(),
            bsns_symbol: "KINIC".into(),
            bsns_decimals: profile.decimals,
            bsns_bridge: decode_address(&profile.bridge_contract).unwrap().to_vec(),
            base_service_fee: profile.parameters.service_fee,
        }
    }

    fn matching_handover_status() -> BridgeStatusLiveView {
        BridgeStatusLiveView {
            reserve: ReserveStatusView { sufficient: true },
            deposits_paused: true,
            mint_authorization_ttl_seconds: 900,
            mint_authorization_epoch: 7,
            counts: ProductionStatusCountsView {
                deposits: 0,
                withdrawals: 0,
                reconciliation_holds: 0,
                pending_ledger_operations: 0,
                reserved_deposit_mint_amount: 0,
                reserved_deposit_mint_operations: 0,
                retained_audit_events: 0,
                pruned_audit_events: 0,
                retained_deposit_index_entries: 0,
            },
        }
    }

    fn matching_handover_runtime(
        profile: &Profile,
        status: &BridgeStatusLiveView,
    ) -> RuntimeBindingView {
        RuntimeBindingView {
            base_chain_id: profile.chain_id,
            bridge_contract: decode_address(&profile.bridge_contract).unwrap().to_vec(),
            expected_bridge_runtime_sha256: decode_hex(&profile.bridge_runtime_bytecode_sha256)
                .unwrap(),
            timelock_contract: decode_address(&profile.timelock.address).unwrap().to_vec(),
            deployment_instance_id: decode_hex(&profile.deployment_instance_id).unwrap(),
            minimum_withdrawal_id: decode_hex(&profile.minimum_withdrawal_id).unwrap(),
            ledger_canister_id: Principal::from_text(&profile.ledger_canister_id).unwrap(),
            index_canister_id: Principal::from_text(&profile.index_canister_id).unwrap(),
            schema_version: profile.canister_schema_version,
            expected_bridge_signer: decode_address(&profile.expected_bridge_signer)
                .unwrap()
                .to_vec(),
            evm_rpc_canister_id: Principal::from_text(&profile.evm_rpc_canister_id).unwrap(),
            rpc_provider_urls_sha256: canonical_sha256(&Vec::<String>::new()).unwrap().to_vec(),
            operational_config_sha256: expected_operational_config_sha256(
                profile,
                status.mint_authorization_ttl_seconds,
                status.mint_authorization_epoch,
            )
            .unwrap()
            .to_vec(),
        }
    }

    fn handover_gate_a_receipt(
        profile: &Profile,
        install_receipt: ProductionCanisterInstallReceipt,
    ) -> GateAReceipt {
        GateAReceipt {
            schema_version: 2,
            gate_a_manifest_sha256: "a".repeat(64),
            release_id: "release".into(),
            source_revision: "b".repeat(40),
            source_tree_sha256: "c".repeat(64),
            gate_a_profile_sha256: "d".repeat(64),
            post_deploy_profile_sha256: "e".repeat(64),
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            bridge_runtime_bytecode_sha256: profile.bridge_runtime_bytecode_sha256.clone(),
            bridge_deployment_transaction_hash: format!("0x{}", "11".repeat(32)),
            bridge_deployment_block_number: 101,
            bridge_deployment_block_hash: format!("0x{}", "22".repeat(32)),
            timelock_deployment_transaction_hash: format!("0x{}", "33".repeat(32)),
            timelock_deployment_block_number: 100,
            timelock_deployment_block_hash: format!("0x{}", "44".repeat(32)),
            canister_install: install_receipt,
        }
    }

    #[test]
    fn production_handover_requires_sealed_fresh_post_deployment_attestation() {
        let profile = valid_profile();
        assert!(profile.rpc_providers.is_empty());
        let install_receipt = production_canister_receipt(&profile);
        let gate_a_receipt = handover_gate_a_receipt(&profile, install_receipt.clone());
        let controllers = [Principal::from_text(&install_receipt.installer_principal).unwrap()];
        let module_hash = decode_hex(&install_receipt.module_sha256).unwrap();
        let created = 1_000_000;
        let now = created + 600;
        let attestation = matching_activation_attestation(&profile, now, 101);
        let status = matching_handover_status();
        let runtime = matching_handover_runtime(&profile, &status);
        let validate = |lifecycle: &ProductionLifecycleView,
                        attestation: Option<&ActivationAttestationView>,
                        controllers: &[Principal],
                        module_hash: &[u8]| {
            let observation = ProductionHandoverCanisterObservation {
                lifecycle,
                attestation,
                runtime: &runtime,
                status: &status,
                controllers,
                module_hash,
            };
            validate_production_handover_canister_state(
                &profile,
                &install_receipt,
                &gate_a_receipt,
                &observation,
                created,
                now,
            )
        };
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            Some(&attestation),
            &controllers,
            &module_hash,
        )
        .is_ok());
        assert!(validate(
            &ProductionLifecycleView::Bootstrap,
            Some(&attestation),
            &controllers,
            &module_hash,
        )
        .is_err());
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            None,
            &controllers,
            &module_hash,
        )
        .is_err());

        let mut stale = matching_activation_attestation(
            &profile,
            now - MAX_ACTIVATION_ATTESTATION_AGE_SECS - 1,
            101,
        );
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            Some(&stale),
            &controllers,
            &module_hash,
        )
        .is_err());
        stale.observed_at_ns = now * 1_000_000_000;
        stale.finalized_block_number = 100;
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            Some(&stale),
            &controllers,
            &module_hash,
        )
        .is_err());

        let mut profile_drift = attestation;
        profile_drift.chain_id = 84532;
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            Some(&profile_drift),
            &controllers,
            &module_hash,
        )
        .is_err());
        let extra_controllers = [controllers[0], Principal::anonymous()];
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            Some(&matching_activation_attestation(&profile, now, 101)),
            &extra_controllers,
            &module_hash,
        )
        .is_err());
        let mut drifted_module = module_hash.clone();
        drifted_module[0] ^= 1;
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            Some(&matching_activation_attestation(&profile, now, 101)),
            &controllers,
            &drifted_module,
        )
        .is_err());

        let sealed_lifecycle = ProductionLifecycleView::OperationalConfigSealed;
        let validate_runtime =
            |candidate_profile: &Profile,
             candidate_runtime: &RuntimeBindingView,
             candidate_status: &BridgeStatusLiveView| {
                let candidate_attestation =
                    matching_activation_attestation(candidate_profile, now, 101);
                let observation = ProductionHandoverCanisterObservation {
                    lifecycle: &sealed_lifecycle,
                    attestation: Some(&candidate_attestation),
                    runtime: candidate_runtime,
                    status: candidate_status,
                    controllers: &controllers,
                    module_hash: &module_hash,
                };
                validate_production_handover_canister_state(
                    candidate_profile,
                    &install_receipt,
                    &gate_a_receipt,
                    &observation,
                    created,
                    now,
                )
            };
        let mut fee_drift_profile = profile.clone();
        fee_drift_profile.parameters.gas_limit_ceiling += 1;
        assert!(validate_runtime(&fee_drift_profile, &runtime, &status).is_err());
        let mut cycles_drift_profile = profile.clone();
        cycles_drift_profile.parameters.cycles_floor += 1;
        assert!(validate_runtime(&cycles_drift_profile, &runtime, &status).is_err());
        let mut settlement_cycles_drift_profile = profile.clone();
        settlement_cycles_drift_profile
            .parameters
            .settlement_cycle_ceiling += 1;
        assert!(validate_runtime(&settlement_cycles_drift_profile, &runtime, &status).is_err());

        let mut runtime_drift = matching_handover_runtime(&profile, &status);
        runtime_drift.rpc_provider_urls_sha256[0] ^= 1;
        assert!(validate_runtime(&profile, &runtime_drift, &status).is_err());
        let mut runtime_code_drift = matching_handover_runtime(&profile, &status);
        runtime_code_drift.expected_bridge_runtime_sha256[0] ^= 1;
        assert!(validate_runtime(&profile, &runtime_code_drift, &status).is_err());
        let mut unpaused = matching_handover_status();
        unpaused.deposits_paused = false;
        assert!(validate_runtime(&profile, &runtime, &unpaused).is_err());
        let mut insufficient_reserve = matching_handover_status();
        insufficient_reserve.reserve.sufficient = false;
        assert!(validate_runtime(&profile, &runtime, &insufficient_reserve).is_err());
        let mut nonempty = matching_handover_status();
        nonempty.counts.deposits = 1;
        assert!(validate_runtime(&profile, &runtime, &nonempty).is_err());
    }

    #[test]
    fn live_runtime_binding_must_exactly_match_the_profile() {
        let profile = valid_profile();
        let rpc_url_hash = hex(&canonical_sha256(
            &profile
                .rpc_providers
                .iter()
                .map(|provider| provider.url.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap());
        let operational_config_sha256 =
            expected_operational_config_sha256(&profile, 900, 7).unwrap();
        let mut observed = live_runtime_binding(&profile);
        assert!(validate_live_runtime_binding(
            &observed,
            &profile,
            &rpc_url_hash,
            &operational_config_sha256,
        )
        .is_ok());
        observed.schema_version -= 1;
        assert!(validate_live_runtime_binding(
            &observed,
            &profile,
            &rpc_url_hash,
            &operational_config_sha256,
        )
        .is_err());
    }

    #[test]
    fn live_runtime_binding_rejects_operational_profile_drift() {
        let mut profile = valid_profile();
        assert_eq!(
            hex(&expected_operational_config_sha256(&profile, 900, 7).unwrap()),
            "5b28cf270243b84dd41cb18918f79d0e4457c10852bd6fa8b866431e67d7fa48"
        );
        let observed = live_runtime_binding(&profile);
        let rpc_url_hash = observed.rpc_provider_urls_sha256.clone();
        profile.rate_limits.notification_global -= 1;
        let changed = expected_operational_config_sha256(&profile, 900, 7).unwrap();
        assert!(
            validate_live_runtime_binding(&observed, &profile, &rpc_url_hash, &changed).is_err()
        );
    }

    #[test]
    fn conservative_derivation_uses_exact_boundaries() {
        let evidence = measurement_evidence(1_700_000_000);
        let result = derive(&evidence).unwrap();
        assert_eq!(result.gas_limit_ceiling, 40_000);
        assert_eq!(result.max_fee_per_gas_ceiling, 200);
        assert_eq!(result.max_priority_fee_per_gas_ceiling, 8);
        assert_eq!(result.l1_fee_per_transaction_ceiling_wei, 50);
        assert_eq!(result.settlement_cycle_ceiling, 1_500);
        assert_eq!(result.cycles_floor, 840_000);
    }

    #[test]
    fn gate_b_operational_parameters_are_fully_bound_to_measurements() {
        let evidence = measurement_evidence(1_700_000_000);
        let derived = derive(&evidence).unwrap();
        let mut profile = valid_profile();
        profile.parameters.gas_limit_ceiling = derived.gas_limit_ceiling;
        profile.parameters.max_fee_per_gas_ceiling = derived.max_fee_per_gas_ceiling;
        profile.parameters.max_priority_fee_per_gas_ceiling =
            derived.max_priority_fee_per_gas_ceiling;
        profile.parameters.l1_fee_per_transaction_ceiling_wei =
            derived.l1_fee_per_transaction_ceiling_wei;
        profile.parameters.cycles_floor = derived.cycles_floor;
        profile.parameters.settlement_cycle_ceiling = derived.settlement_cycle_ceiling;
        assert!(validate_gate_b_operational_parameters(&profile, &evidence).is_ok());

        macro_rules! assert_drift_rejected {
            ($field:ident) => {{
                let mut drift = profile.clone();
                drift.parameters.$field += 1;
                assert!(validate_gate_b_operational_parameters(&drift, &evidence).is_err());
            }};
        }
        assert_drift_rejected!(ledger_fee);
        assert_drift_rejected!(max_service_fee);
        assert_drift_rejected!(service_fee);
        assert_drift_rejected!(gas_limit_ceiling);
        assert_drift_rejected!(max_fee_per_gas_ceiling);
        assert_drift_rejected!(max_priority_fee_per_gas_ceiling);
        assert_drift_rejected!(l1_fee_per_transaction_ceiling_wei);
        assert_drift_rejected!(quote_validity_seconds);
        assert_drift_rejected!(gas_limit_multiplier_bps);
        assert_drift_rejected!(base_fee_multiplier_bps);
        assert_drift_rejected!(l1_fee_multiplier_bps);
        assert_drift_rejected!(cycles_floor);
        assert_drift_rejected!(settlement_cycle_ceiling);

        let mut wrong_environment = evidence;
        wrong_environment.environment = "base-sepolia".into();
        assert!(validate_gate_b_operational_parameters(&profile, &wrong_environment).is_err());
    }

    #[test]
    fn derivation_rejects_incomplete_stale_and_obsolete_measurement_shapes() {
        let mut evidence = measurement_evidence(1_700_000_000);
        evidence.governance_gas_samples = measurement_samples(10_000, 1_700_000_000);
        evidence.settlement_cycle_samples = measurement_samples(1_001, 1_700_000_000);
        let mut value = serde_json::to_value(&evidence).unwrap();
        value["observed_daily_cycles"] = Value::from(10_000);
        assert!(serde_json::from_value::<Evidence>(value).is_err());
        assert!(serde_json::from_str::<Evidence>(
            r#"{"schema_version":2,"sample_count":10,"observation_days":7}"#
        )
        .is_err());

        let mut short = serde_json::to_value(&evidence).unwrap();
        short["governance_gas_samples"] =
            serde_json::to_value(&evidence.governance_gas_samples[..9]).unwrap();
        assert!(derive(&serde_json::from_value(short).unwrap()).is_err());

        let mut obsolete_schema = serde_json::to_value(&evidence).unwrap();
        obsolete_schema["schema_version"] = Value::from(2);
        assert!(derive(&serde_json::from_value(obsolete_schema).unwrap()).is_err());

        let mut obsolete_field = serde_json::to_value(&evidence).unwrap();
        obsolete_field["base_fee_per_gas"] = serde_json::to_value(vec![10u128; 10]).unwrap();
        assert!(serde_json::from_value::<Evidence>(obsolete_field).is_err());

        let mut short_period = serde_json::to_value(&evidence).unwrap();
        short_period["fee_samples"][9]["observed_at_unix"] = Value::from(1_700_000_001u64);
        assert!(derive(&serde_json::from_value(short_period).unwrap()).is_err());

        let mut duplicate_source = serde_json::to_value(&evidence).unwrap();
        duplicate_source["fee_samples"][1]["source_ref"] =
            duplicate_source["fee_samples"][0]["source_ref"].clone();
        assert!(derive(&serde_json::from_value(duplicate_source).unwrap()).is_err());

        let mut empty_source = serde_json::to_value(&evidence).unwrap();
        empty_source["settlement_cycle_samples"][0]["source_ref"] = Value::from("");
        assert!(derive(&serde_json::from_value(empty_source).unwrap()).is_err());

        let measurement_end = evidence
            .fee_samples
            .iter()
            .map(|sample| sample.observed_at_unix)
            .max()
            .unwrap();
        let manifest_created = measurement_end + 10;
        assert!(validate_measurement_time(&evidence, manifest_created, manifest_created).is_ok());
        assert!(
            validate_measurement_time(&evidence, measurement_end - 1, manifest_created,).is_err()
        );
        assert!(
            validate_measurement_time(&evidence, manifest_created + 1, manifest_created).is_err()
        );
        evidence.governance_gas_samples[0].observed_at_unix = manifest_created + 1;
        assert!(validate_measurement_time(&evidence, manifest_created, manifest_created).is_err());
        evidence.governance_gas_samples[0].observed_at_unix = 1_700_000_000;
        assert!(validate_measurement_time(
            &evidence,
            manifest_created,
            measurement_end + MAX_EVIDENCE_AGE_SECS + 1,
        )
        .is_err());

        let mut zero = serde_json::to_value(&evidence).unwrap();
        zero["baseline_cycles_sample"]["value"] = Value::from("0");
        assert!(derive(&serde_json::from_value(zero).unwrap()).is_err());

        let mut placeholder = serde_json::to_value(&evidence).unwrap();
        placeholder["baseline_cycles_sample"]["source_ref"] =
            Value::from("replace-with-baseline-reference");
        assert!(derive(&serde_json::from_value(placeholder).unwrap()).is_err());

        let mut whitespace = serde_json::to_value(&evidence).unwrap();
        whitespace["governance_gas_samples"][0]["source_ref"] = Value::from(" evidence ");
        assert!(derive(&serde_json::from_value(whitespace).unwrap()).is_err());

        let mut control = serde_json::to_value(&evidence).unwrap();
        control["fee_samples"][0]["source_ref"] = Value::from("evidence\nref");
        assert!(derive(&serde_json::from_value(control).unwrap()).is_err());

        evidence.expected_daily_settlements = u128::MAX;
        assert!(derive(&evidence).is_err());
    }

    #[test]
    fn profile_has_no_self_asserted_status_and_requires_provider_independence() {
        let mut profile = valid_profile();
        profile.schema_version = 4;
        assert!(validate_profile(&profile, true).is_err());
        profile = valid_profile();
        profile.base_rpc_url = Some("https://rpc.example".into());
        assert!(validate_profile(&profile, true).is_err());
        profile = valid_profile();
        let mut value = serde_json::to_value(profile).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("status".into(), Value::String("validated".into()));
        assert!(serde_json::from_value::<Profile>(value).is_err());
    }

    #[test]
    fn profile_rejects_credentials_duplicate_urls_and_role_overlap() {
        let mut profile = valid_profile();
        profile.rpc_providers.push(RpcProvider {
            url: "https://rpc.example".into(),
            operator: "operator".into(),
            dns_owner: "dns".into(),
            failure_domain: "upstream".into(),
        });
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.governance_operator = profile.expected_bridge_signer.clone();
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.independent_canceller = profile.runtime_administrator.clone();
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.timelock.runtime_code_hash = format!("0x{}", "00".repeat(32));
        assert!(validate_profile(&profile, true).is_err());
    }

    #[test]
    fn profile_rejects_notification_rate_limits_outside_canister_bounds() {
        let mut profile = valid_profile();
        profile.rate_limits.notification_window_seconds = 59;
        assert!(validate_profile(&profile, true).is_err());

        let mut profile = valid_profile();
        profile.rate_limits.notification_window_seconds = 3_601;
        assert!(validate_profile(&profile, true).is_err());

        let mut profile = valid_profile();
        profile.rate_limits.notification_global = 0;
        assert!(validate_profile(&profile, true).is_err());

        let mut profile = valid_profile();
        profile.rate_limits.notification_global = 101;
        assert!(validate_profile(&profile, true).is_err());

        let mut profile = valid_profile();
        profile.rate_limits.notification_ingestion_global = 0;
        assert!(validate_profile(&profile, true).is_err());

        let mut profile = valid_profile();
        profile.rate_limits.notification_ingestion_global = 101;
        assert!(validate_profile(&profile, true).is_err());
    }

    #[test]
    fn canonical_json_sorts_utf16_keys_and_rejects_floats() {
        let value = serde_json::json!({"z": 1, "a": {"b": true, "a": "x"}});
        let mut out = Vec::new();
        canonical_json(&value, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            r#"{"a":{"a":"x","b":true},"z":1}"#
        );
        assert!(canonical_json(&serde_json::json!(1.5), &mut Vec::new()).is_err());
        let mut safe = Vec::new();
        canonical_json(&serde_json::json!(9_007_199_254_740_991u64), &mut safe).unwrap();
        assert_eq!(safe, b"9007199254740991");
        assert!(canonical_json(
            &serde_json::json!(9_007_199_254_740_992u64),
            &mut Vec::new()
        )
        .is_err());
    }

    #[test]
    fn release_inputs_are_deterministic_and_bound_to_profile() {
        let root = env::temp_dir().join(format!("bridge-inputs-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let profile_path = root.join("profile.json");
        fs::write(&profile_path, serde_json::to_vec(&valid_profile()).unwrap()).unwrap();
        let first = root.join("first");
        let second = root.join("second");
        render_release_inputs(&profile_path, &first, true, None).unwrap();
        render_release_inputs(&profile_path, &second, true, None).unwrap();
        for name in [
            "canister-init.json",
            "contract-constructor-args.json",
            "ui-runtime-profile.json",
            "release-inputs-manifest.json",
        ] {
            assert_eq!(
                fs::read(first.join(name)).unwrap(),
                fs::read(second.join(name)).unwrap()
            );
        }
        let manifest: Value = read_json(&first.join("release-inputs-manifest.json")).unwrap();
        assert_eq!(manifest["schema_version"], 2);
        assert_eq!(
            manifest["profile_file_sha256"],
            hex(&Sha256::digest(fs::read(&profile_path).unwrap()))
        );
        let canister: Value = read_json(&first.join("canister-init.json")).unwrap();
        assert_eq!(canister["evm_rpc_canister_id"], OFFICIAL_EVM_RPC_CANISTER);
        assert_eq!(canister["custom_evm_rpc_urls"], serde_json::json!([]));
        assert_eq!(canister["governance_replacement"]["max_replacements"], 3);
        let expected_init_keys = [
            "ledger_canister_id",
            "index_canister_id",
            "evm_rpc_canister_id",
            "custom_evm_rpc_urls",
            "base_chain_id",
            "bridge_contract_hex",
            "expected_bridge_runtime_sha256_hex",
            "timelock_contract_hex",
            "expected_timelock_minimum_delay_seconds",
            "expected_bsns_runtime_sha256_hex",
            "expected_bsns_decimals",
            "expected_minimum_service_fee",
            "deployment_instance_id_hex",
            "minimum_withdrawal_id_hex",
            "ecdsa_key_name",
            "ecdsa_derivation_path_utf8",
            "governance_ecdsa_derivation_path_utf8",
            "deposit_rate_limit_window_seconds",
            "deposit_rate_limit_global",
            "deposit_rate_limit_per_principal",
            "notification_rate_limit_window_seconds",
            "notification_rate_limit_global",
            "notification_ingestion_rate_limit_global",
            "settlement_rate_limit_window_seconds",
            "settlement_rate_limit_global",
            "settlement_rate_limit_per_principal",
            "settlement_rate_limit_per_record",
            "settlement_retry_interval_seconds",
            "governance_evm_fee",
            "governance_replacement",
            "cycles_floor",
            "settlement_cycle_ceiling",
            "governance_principal",
            "confirmation_relayer_principal",
            "pause_principal",
            "fee_recipient",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        assert_eq!(
            canister
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected_init_keys
        );
        let constructors: Value = read_json(&first.join("contract-constructor-args.json")).unwrap();
        assert_eq!(
            constructors["bridge"][3],
            valid_profile().timelock.runtime_code_hash
        );
        let ui: Value = read_json(&first.join("ui-runtime-profile.json")).unwrap();
        assert_eq!(ui["environmentMode"], Value::Null);
        assert_eq!(
            ui["activationTimelockDelaySeconds"],
            valid_profile().timelock.minimum_delay_seconds
        );
        assert_eq!(ui["timelockAddress"], valid_profile().timelock.address);
        assert_eq!(ui["evmRpcCanisterId"], OFFICIAL_EVM_RPC_CANISTER);
        assert_eq!(ui["snsRootCanisterId"], valid_profile().root_canister_id);
        assert_eq!(
            ui["rpcProviderUrlsSha256"],
            format!("0x{}", hex(&Sha256::digest(b"[]")))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundle_gate_validates_hashes_and_slo() {
        let root = env::temp_dir().join(format!(
            "bridge-profile-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let now = now_unix().unwrap();
        let mut profile = valid_profile();
        profile.timelock.proposer = profile.governance_operator.clone();
        profile.timelock.executor = profile.governance_operator.clone();
        profile.timelock.canceller = profile.independent_canceller.clone();
        profile.bridge_canister_wasm_sha256 = hex(&Sha256::digest(b"wasm"));
        profile.bridge_runtime_bytecode_sha256 = hex(&Sha256::digest(b"runtime"));
        profile.deployment_block = 0;
        let test_helper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/evm-rpc-rehearsal/test_rehearsal.py");
        let python = r###"import importlib.util,json,os,sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('fixture',sys.argv[1]); m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
m.SIGNER=sys.argv[3]; m.SHA_A=sys.argv[4]; m.SHA_B=sys.argv[5]; binding=m.rehearsal.validate_config(m.config()); value=m.manifest(binding); value['source']['revision']='a'*40; value['source']['source_tree_sha256']='2'*64
root=Path(sys.argv[2]).parent; tool=root/'tool'
os.environ['PATH']=str(root)+os.pathsep+os.environ.get('PATH','')
items=m.all_evidence(binding)
for scenario in ('preflight','authorization_mint','withdrawal_release','quorum_loss','final_pause'):
 item=items[scenario]
 m.rehearsal.now=lambda: item['observed_at']
 fault_fields={'configured_provider_count','required_provider_threshold','injected_provider_failures','fault_injection_reference'}; command_details={k:v for k,v in item['details'].items() if scenario not in {'single_provider_failure','quorum_loss'} or k not in fault_fields}; audit_event=None
 if item['canister_decision'] is not None:
  timestamp_ns=int(m.rehearsal.datetime.fromisoformat(item['observed_at'].replace('Z','+00:00')).timestamp()*1_000_000_000); audit_event={'sequence':7,'timestamp_ns':timestamp_ns,'kind':{'EvmRpcDecision':item['canister_decision']}}
 payload=json.dumps({**command_details,'canister_audit':item['canister_audit'],'audit_events':[audit_event] if audit_event else []},separators=(',',':')); tool.write_text("#!/bin/sh\nprintf '%s' '"+payload+"'\n"); tool.chmod(0o755)
 base_provider_index=0
 for reference in item['artifacts']:
  kind=reference['kind']; output=root/reference['path']
  if kind=='fault':
   m.write_fault_artifact(item,scenario,output); reference['sha256']=m.rehearsal.hashlib.sha256(output.read_bytes()).hexdigest(); continue
  executable=root/('cast' if kind=='base' else 'icp')
  if kind=='base': executable.write_text("#!/bin/sh\nif [ \"$1\" = \"chain-id\" ]; then printf '84532\\n'; else printf '%s' '"+payload+"'; fi\n")
  else: executable.write_bytes(tool.read_bytes())
  executable.chmod(0o755)
  if kind=='base': command=['cast','receipt',m.H32_A]
  elif kind=='module': command=['icp','canister','status',binding['bridge_canister_id'],'-n','ic','--public','--json']
  else:
   method='icrc1_fee' if kind=='ledger' else ('get_audit_events' if kind=='audit' else 'get_bridge_status')
   command=['icp','canister','call',binding['ledger_canister_id'] if kind=='ledger' else binding['bridge_canister_id'],method,'()','-n','ic','--json']
  m.rehearsal.capture_artifact(value,m.config(),scenario,kind,output,command,base_provider_index if kind=='base' else None); reference['sha256']=m.rehearsal.hashlib.sha256(output.read_bytes()).hexdigest()
  if kind=='base': base_provider_index+=1
 request_records=[]; response_records=[]
 for reference in item['artifacts']:
  artifact=json.loads((root/reference['path']).read_text(encoding='utf-8')); request_records.append([artifact['tool'],*artifact['argv'],artifact['transport']]); response_records.append(artifact['stdout'])
 item['request_sha256']=m.rehearsal.hashlib.sha256(json.dumps(request_records,separators=(',',':')).encode()).hexdigest()
 item['response_sha256']=m.rehearsal.hashlib.sha256(json.dumps(response_records,separators=(',',':')).encode()).hexdigest()
 m.rehearsal.record(value,item,scenario,root)
with open(sys.argv[2],'w',encoding='utf-8') as f: json.dump(value,f,sort_keys=True)
"###;
        let generated = Command::new("python3")
            .arg("-c")
            .arg(python)
            .arg(test_helper)
            .arg(root.join("rpc-e2e.json"))
            .arg(&profile.expected_bridge_signer)
            .arg(&profile.bridge_canister_wasm_sha256)
            .arg(&profile.bridge_runtime_bytecode_sha256)
            .status()
            .unwrap();
        assert!(generated.success());
        let drill = MonitorDrill {
            schema_version: 4,
            rehearsal_id: "rehearsal-1".into(),
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            ic_network: "ic".into(),
            base_chain_id: 84_532,
            bridge_canister_id: profile.bridge_canister_id.clone(),
            bridge_contract: profile.bridge_contract.clone(),
            timelock_contract: profile.timelock.address.clone(),
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            bridge_runtime_bytecode_sha256: profile.bridge_runtime_bytecode_sha256.clone(),
            rpc_provider_urls_sha256: "6".repeat(64),
            routing_sha256: profile.monitoring.routing_sha256.clone(),
            fault_started_at_unix: now - 5_000,
            detected_at_unix: now - 4_000,
            acknowledged_at_unix: now - 3_000,
            base_paused_at_unix: now - 1_000,
            pending_timelock_operation_before: false,
            base_actions: vec![
                MonitorBaseAction {
                    kind: "PauseDepositMints".into(),
                    transaction_hash: format!("0x{}", "10".repeat(32)),
                    block_number: 1,
                    block_hash: format!("0x{}", "11".repeat(32)),
                    receipt_status: 1,
                    target: profile.bridge_contract.clone(),
                    calldata_hex: evm_selector("pauseDepositMints()"),
                    canonical_finalized: true,
                },
                MonitorBaseAction {
                    kind: "PauseWithdrawals".into(),
                    transaction_hash: format!("0x{}", "14".repeat(32)),
                    block_number: 2,
                    block_hash: format!("0x{}", "15".repeat(32)),
                    receipt_status: 1,
                    target: profile.bridge_contract.clone(),
                    calldata_hex: evm_selector("pauseWithdrawals()"),
                    canonical_finalized: true,
                },
            ],
            ic_pause: MonitorIcPause {
                paused_at_unix: now - 900,
                response_hex: hex(b"pause response"),
                response_sha256: hex(&Sha256::digest(b"pause response")),
                pause_principal: profile.pause_principal.clone(),
                request_id: format!("0x{}", "12".repeat(32)),
                certificate_hex: hex(b"certificate"),
                certificate_sha256: hex(&Sha256::digest(b"certificate")),
                audit_sequence: 1,
                audit_sha256: hex(&Sha256::digest([0x13; 32])),
                audit_raw_hex: hex(&[0x13; 32]),
            },
        };
        let handover = ControllerHandover {
            schema_version: 2,
            stage: "complete".into(),
            observed_at_unix: now - 95,
            bridge_canister_id: profile.bridge_canister_id.clone(),
            sns_root_canister_id: profile.root_canister_id.clone(),
            executing_principal: test_principal(31),
            command_argv: vec![
                "icp",
                "canister",
                "settings",
                "update",
                "bridge-canister",
                "-e",
                "production",
                "--remove-all-controllers",
                "--add-controller",
                KINIC_ROOT,
                "--force",
                "--identity",
                "production",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            request_id: "3".repeat(64),
            response_exit_code: 0,
            response_stdout_hex: String::new(),
            response_stderr_hex: hex(format!("request_id={}\n", "3".repeat(64)).as_bytes()),
            response_sha256: hex(&Sha256::digest(
                format!("request_id={}\n", "3".repeat(64)).as_bytes(),
            )),
            final_controllers: vec![profile.root_canister_id.clone()],
            cycles_balance: 10_000_000,
            freezing_threshold_seconds: 86_400,
            idle_cycles_burned_per_day: 1_000,
            required_freezing_cycles: 1_000,
        };
        let upgrade = SnsUpgrade {
            schema_version: 3,
            observed_at_unix: now - 90,
            executed_at_unix: now - 91,
            proposal_id: 1,
            governance_canister_id: KINIC_GOVERNANCE.into(),
            root_canister_id: profile.root_canister_id.clone(),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            status: "Executed".into(),
            before_module_sha256: profile.bridge_canister_wasm_sha256.clone(),
            after_module_sha256: profile.bridge_canister_wasm_sha256.clone(),
            before_public_state_sha256: "5".repeat(64),
            after_public_state_sha256: "5".repeat(64),
            proposal_action: "UpgradeSnsControlledCanister".into(),
            install_mode: "upgrade".into(),
            proposal_target_canister_id: profile.bridge_canister_id.clone(),
            proposal_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            governance_query_response_hex: hex(b"governance raw"),
            governance_query_response_sha256: hex(&Sha256::digest(b"governance raw")),
        };
        let withdrawal_id = format!("0x{}", "7".repeat(64));
        let burn_transaction_hash = format!("0x{}", "8".repeat(64));
        let paid_response = Encode!(&Some(WithdrawalView {
            charged_service_fee: Nat::from(10u64),
            withdrawal_id: vec![0x77; 32],
            max_service_fee: Nat::from(10u64),
            release_ledger_block_index: Some(Nat::from(7u64)),
            last_settlement_stop_reason: None,
            amount_out: Nat::from(90u64),
            state: WithdrawalPhaseView::Paid,
            ledger_fee: Nat::from(1u64),
            amount: Nat::from(100u64),
        }))
        .unwrap();
        let monitoring_receipt = MonitoringReceipt {
            schema_version: 1,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            withdrawal_id: withdrawal_id.clone(),
            burn_transaction_hash: burn_transaction_hash.clone(),
            burn: MonitoringBurnReceipt {
                base_chain_id: profile.chain_id,
                bridge_contract: profile.bridge_contract.clone(),
                block_number: 3,
                block_hash: format!("0x{}", "16".repeat(32)),
                receipt_status: 1,
                withdrawal_committed_topic: evm_topic(
                    "WithdrawalCommitted(uint256,address,uint256,uint256,uint256,uint256,bytes,bytes32)",
                ),
                withdrawal_id_topic: withdrawal_id.clone(),
                canonical_finalized: true,
            },
            paid: MonitoringPaidObservation {
                observed_at_unix: now - 40,
                state: "Paid".into(),
                response_hex: hex(&paid_response),
                response_sha256: hex(&Sha256::digest(&paid_response)),
                authenticated_query: true,
            },
        };
        let monitoring_receipt_bytes = serde_json::to_vec(&monitoring_receipt).unwrap();
        let keeper_drill = KeeperDrill {
            schema_version: 1,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            withdrawal_id,
            burn_transaction_hash,
            burned_at_unix: now - 80,
            paid_at_unix: now - 40,
            maximum_unprocessed_seconds: 300,
            keeper_ids: vec!["keeper-primary".into(), "keeper-secondary".into()],
            keeper_failure_domains: vec!["operator-a".into(), "operator-b".into()],
            monitoring_receipt_sha256: hex(&Sha256::digest(&monitoring_receipt_bytes)),
            manual_fallback_drilled: true,
        };
        let provider_independence = ProviderIndependenceReceipt {
            schema_version: 1,
            observed_at_unix: now - 30,
            proposal_id: 2,
            provider_review_sha256: hex(&canonical_sha256(&profile.rpc_providers).unwrap()),
            dns_monitoring_enabled: true,
            endpoint_monitoring_enabled: true,
            drift_action: "pause-and-require-reactivation".into(),
            governance_query_response_hex: hex(b"provider governance raw"),
            governance_query_response_sha256: hex(&Sha256::digest(b"provider governance raw")),
        };
        let ui_files = vec![UiAssetDigest {
            path: "assets/index.js".into(),
            sha256: hex(&Sha256::digest(b"ui")),
        }];
        let ui_assets = UiAssetsReceipt {
            schema_version: 1,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            artifact_set_sha256: hex(&Sha256::digest(serde_json::to_vec(&ui_files).unwrap())),
            files: ui_files,
        };
        let measurement_start = now - 7 * 24 * 60 * 60 - 10;
        let mut measurements = measurement_evidence(measurement_start);
        measurements.ledger_fee = profile.parameters.ledger_fee;
        measurements.governance_gas_samples = measurement_samples(1, measurement_start);
        measurements.fee_samples = fee_samples(1, 1, 1, measurement_start);
        measurements.settlement_cycle_samples = measurement_samples(1, measurement_start);
        measurements.baseline_cycles_sample.value = 1;
        measurements.expected_daily_settlements = 1;
        let initial_observed_at = now - 6_000;
        let governance_operation_id = 0;
        let deployment_instance_id: [u8; 32] = decode_hex(&profile.deployment_instance_id)
            .unwrap()
            .try_into()
            .unwrap();
        let activation_bridge = decode_address(&profile.bridge_contract).unwrap();
        let operation_salt =
            initial_activation_salt(deployment_instance_id, governance_operation_id);
        let mut initial_parameters = InitialOperationalParameters {
            schema_version: 1,
            environment: "mainnet-candidate".into(),
            chain_id: 8_453,
            bridge_canister_id: profile.bridge_canister_id.clone(),
            bridge_contract: profile.bridge_contract.clone(),
            timelock_contract: profile.timelock.address.clone(),
            governance_sender: profile.governance_operator.clone(),
            deployment_instance_id: profile.deployment_instance_id.clone(),
            governance_operation_id,
            operation_salt: format!("0x{}", hex(&operation_salt)),
            timelock_delay_seconds: profile.timelock.minimum_delay_seconds,
            profile_sha256: String::new(),
            gas_estimates: vec![
                InitialGasEstimate {
                    action: "schedule_activation".into(),
                    sender: profile.governance_operator.clone(),
                    target: profile.timelock.address.clone(),
                    value_wei: 0,
                    calldata_hex: initial_activation_calldata(
                        "schedule_activation",
                        activation_bridge,
                        operation_salt,
                        profile.timelock.minimum_delay_seconds,
                    )
                    .unwrap(),
                    gas: 100_000,
                    block_number: 10,
                    block_hash: format!("0x{}", "31".repeat(32)),
                    observed_at_unix: initial_observed_at,
                    source_ref: "schedule-estimate".into(),
                },
                InitialGasEstimate {
                    action: "execute_activation".into(),
                    sender: profile.governance_operator.clone(),
                    target: profile.timelock.address.clone(),
                    value_wei: 0,
                    calldata_hex: initial_activation_calldata(
                        "execute_activation",
                        activation_bridge,
                        operation_salt,
                        profile.timelock.minimum_delay_seconds,
                    )
                    .unwrap(),
                    gas: 120_000,
                    block_number: 10,
                    block_hash: format!("0x{}", "31".repeat(32)),
                    observed_at_unix: initial_observed_at,
                    source_ref: "execute-estimate".into(),
                },
            ],
            fee_samples: (0..10)
                .map(|index| InitialFeeSample {
                    base_fee_per_gas: 100,
                    priority_fee_per_gas: 10,
                    l1_fee_upper_bound_wei: 1_000,
                    block_number: 100 + index,
                    block_hash: format!("0x{:064x}", index + 1),
                    observed_at_unix: initial_observed_at + index,
                    source_ref: format!("initial-fee-{index}"),
                })
                .collect(),
            idle_cycles_burned_per_day: 1_000,
            idle_cycles_observed_at_unix: initial_observed_at,
            idle_cycles_source_ref: "icp-canister-status".into(),
            expected_daily_settlements: 1,
            settlement_cycle_ceiling: 5_000_000_000,
            derived: InitialDerivedParameters {
                gas_limit_ceiling: 0,
                max_fee_per_gas_ceiling: 0,
                max_priority_fee_per_gas_ceiling: 0,
                l1_fee_per_transaction_ceiling_wei: 0,
                quote_validity_seconds: 0,
                gas_limit_multiplier_bps: 0,
                base_fee_multiplier_bps: 0,
                l1_fee_multiplier_bps: 0,
                cycles_floor: 0,
                settlement_cycle_ceiling: 0,
            },
        };
        initial_parameters.derived =
            derive_initial_operational_parameters(&initial_parameters).unwrap();
        let mut candid_payload: InitialOperationalParameters =
            serde_json::from_value(serde_json::to_value(&initial_parameters).unwrap()).unwrap();
        candid_payload.gas_estimates[0].calldata_hex = "4449444c0000".into();
        assert!(derive_initial_operational_parameters(&candid_payload).is_err());
        let mut wrong_sender: InitialOperationalParameters =
            serde_json::from_value(serde_json::to_value(&initial_parameters).unwrap()).unwrap();
        wrong_sender.gas_estimates[0].sender = format!("0x{}", "99".repeat(20));
        assert!(derive_initial_operational_parameters(&wrong_sender).is_err());
        let mut wrong_target: InitialOperationalParameters =
            serde_json::from_value(serde_json::to_value(&initial_parameters).unwrap()).unwrap();
        wrong_target.gas_estimates[0].target = profile.bridge_contract.clone();
        assert!(derive_initial_operational_parameters(&wrong_target).is_err());
        let mut wrong_salt: InitialOperationalParameters =
            serde_json::from_value(serde_json::to_value(&initial_parameters).unwrap()).unwrap();
        wrong_salt.operation_salt = format!("0x{}", "88".repeat(32));
        assert!(derive_initial_operational_parameters(&wrong_salt).is_err());
        let mut wrong_operation_id: InitialOperationalParameters =
            serde_json::from_value(serde_json::to_value(&initial_parameters).unwrap()).unwrap();
        wrong_operation_id.governance_operation_id = 1;
        wrong_operation_id.operation_salt = format!(
            "0x{}",
            hex(&initial_activation_salt(deployment_instance_id, 1))
        );
        assert!(derive_initial_operational_parameters(&wrong_operation_id).is_err());
        profile.parameters.gas_limit_ceiling = initial_parameters.derived.gas_limit_ceiling;
        profile.parameters.max_fee_per_gas_ceiling =
            initial_parameters.derived.max_fee_per_gas_ceiling;
        profile.parameters.max_priority_fee_per_gas_ceiling =
            initial_parameters.derived.max_priority_fee_per_gas_ceiling;
        profile.parameters.l1_fee_per_transaction_ceiling_wei = initial_parameters
            .derived
            .l1_fee_per_transaction_ceiling_wei;
        profile.parameters.quote_validity_seconds =
            initial_parameters.derived.quote_validity_seconds;
        profile.parameters.gas_limit_multiplier_bps =
            initial_parameters.derived.gas_limit_multiplier_bps;
        profile.parameters.base_fee_multiplier_bps =
            initial_parameters.derived.base_fee_multiplier_bps;
        profile.parameters.l1_fee_multiplier_bps = initial_parameters.derived.l1_fee_multiplier_bps;
        profile.parameters.cycles_floor = initial_parameters.derived.cycles_floor;
        profile.parameters.settlement_cycle_ceiling =
            initial_parameters.derived.settlement_cycle_ceiling;
        profile.bsns_runtime_template_sha256 = hex(&Sha256::digest(b"bsns-runtime"));
        let final_operational_parameters = profile.parameters.clone();
        set_production_bootstrap_operational_config(&mut profile);
        let mut docs = vec![
            ("profile.json", serde_json::to_vec(&profile).unwrap()),
            ("rpc-e2e.json", fs::read(root.join("rpc-e2e.json")).unwrap()),
            (
                "controller-handover.json",
                serde_json::to_vec(&handover).unwrap(),
            ),
            ("sns-upgrade.json", serde_json::to_vec(&upgrade).unwrap()),
            ("monitor-drill.json", serde_json::to_vec(&drill).unwrap()),
            (
                "keeper-drill.json",
                serde_json::to_vec(&keeper_drill).unwrap(),
            ),
            ("monitoring-receipt.json", monitoring_receipt_bytes),
            (
                "fee-cycles-measurements.json",
                serde_json::to_vec(&measurements).unwrap(),
            ),
            (
                "provider-independence.json",
                serde_json::to_vec(&provider_independence).unwrap(),
            ),
            (
                "ui-assets.json",
                serde_json::to_vec(&ui_assets).unwrap(),
            ),
            ("bridge-canister.wasm", b"wasm".to_vec()),
            ("bridge-runtime.bin", b"runtime".to_vec()),
            ("bsns-creation.bin", b"bsns-creation".to_vec()),
            ("bsns-runtime.bin", b"bsns-runtime".to_vec()),
            (
                "bsns-runtime-layout.json",
                br#"{"byte_length":12,"immutable_ranges":[{"length":1,"start":0}],"schema_version":1}"#.to_vec(),
            ),
        ];
        docs[0].1 = serde_json::to_vec(&profile).unwrap();
        let mut artifacts = Vec::new();
        for (name, bytes) in docs {
            fs::write(root.join(name), &bytes).unwrap();
            artifacts.push(ArtifactDigest {
                path: name.into(),
                sha256: hex(&Sha256::digest(bytes)),
            });
        }
        let manifest_created = now_unix().unwrap();
        let gate_a_artifacts = artifacts
            .iter()
            .filter(|artifact| GATE_A_ARTIFACTS.contains(&artifact.path.as_str()))
            .map(|artifact| ArtifactDigest {
                path: artifact.path.clone(),
                sha256: artifact.sha256.clone(),
            })
            .collect();
        let gate_a_manifest = ReleaseManifest {
            schema_version: 3,
            release_id: "release-1".into(),
            test_only: false,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            created_at_unix: manifest_created,
            expires_at_unix: manifest_created + 100,
            parent_gate_a_manifest_sha256: None,
            artifacts: gate_a_artifacts,
        };
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&gate_a_manifest).unwrap(),
        )
        .unwrap();
        let planned_profile = fs::read(root.join("profile.json")).unwrap();
        let mut premature_profile = profile.clone();
        premature_profile.deployment_block = 1;
        let premature_bytes = serde_json::to_vec(&premature_profile).unwrap();
        fs::write(root.join("profile.json"), &premature_bytes).unwrap();
        let mut premature_manifest = serde_json::to_value(&gate_a_manifest).unwrap();
        let profile_artifact = premature_manifest["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["path"] == "profile.json")
            .unwrap();
        profile_artifact["sha256"] = Value::String(hex(&Sha256::digest(&premature_bytes)));
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&premature_manifest).unwrap(),
        )
        .unwrap();
        let premature_error = match validate_bundle(&root, false) {
            Ok(_) => panic!("Gate A accepted a predeclared deployment block"),
            Err(error) => error,
        };
        assert!(premature_error.contains("leave deployment_block unbound"));
        fs::write(root.join("profile.json"), &planned_profile).unwrap();
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&gate_a_manifest).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join("proof-attestation.json"),
            br#"{"lean_result":"passed"}"#,
        )
        .unwrap();
        let obsolete_error = match validate_bundle(&root, false) {
            Ok(_) => panic!("Gate A accepted an obsolete self-asserted proof attestation"),
            Err(error) => error,
        };
        assert!(obsolete_error.contains("obsolete self-asserted proof attestation"));
        fs::remove_file(root.join("proof-attestation.json")).unwrap();
        let gate_a = validate_bundle(&root, false).unwrap();
        let gate_a_profile_sha256 = hex(&canonical_sha256(&profile).unwrap());
        let bridge_deployment_transaction_hash = format!("0x{}", "aa".repeat(32));
        let bridge_deployment_block_number = 1;
        let bridge_deployment_block_hash = format!("0x{}", "cc".repeat(32));
        let timelock_deployment_transaction_hash = format!("0x{}", "bb".repeat(32));
        let timelock_deployment_block_number = 1;
        let timelock_deployment_block_hash = format!("0x{}", "dd".repeat(32));
        profile.deployment_block = bridge_deployment_block_number;
        let gate_a_post_deploy_profile = profile.clone();
        let post_deploy_profile = canonical_bytes(&profile).unwrap();
        fs::write(root.join("profile.json"), &post_deploy_profile).unwrap();
        let post_deploy_profile_sha256 = hex(&Sha256::digest(&post_deploy_profile));
        artifacts
            .iter_mut()
            .find(|a| a.path == "profile.json")
            .unwrap()
            .sha256 = post_deploy_profile_sha256.clone();
        let mut canister_plan = production_canister_plan(&profile);
        canister_plan.source_tree_sha256 = "2".repeat(64);
        let canister_plan_sha256 = hex(&canonical_sha256(&canister_plan).unwrap());
        let canister_init_candid_sha256 = hex(&Sha256::digest(
            validate_production_canister_plan(&canister_plan).unwrap(),
        ));
        let mut canister_runtime_binding = live_runtime_binding(&profile);
        canister_runtime_binding.operational_config_sha256 =
            hex(&expected_bootstrap_operational_config_sha256(
                &canister_plan.init,
                &profile.governance_operator,
                900,
                7,
            )
            .unwrap());
        let receipt = GateAReceipt {
            schema_version: 2,
            gate_a_manifest_sha256: gate_a.manifest_sha256.clone(),
            release_id: "release-1".into(),
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            gate_a_profile_sha256,
            post_deploy_profile_sha256,
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            bridge_runtime_bytecode_sha256: profile.bridge_runtime_bytecode_sha256.clone(),
            bridge_deployment_transaction_hash,
            bridge_deployment_block_number,
            bridge_deployment_block_hash,
            timelock_deployment_transaction_hash,
            timelock_deployment_block_number,
            timelock_deployment_block_hash,
            canister_install: ProductionCanisterInstallReceipt {
                schema_version: PRODUCTION_CANISTER_INSTALL_RECEIPT_SCHEMA_VERSION,
                plan_sha256: canister_plan_sha256,
                plan: canister_plan,
                source_revision: "a".repeat(40),
                source_tree_sha256: "2".repeat(64),
                canister_id: profile.bridge_canister_id.clone(),
                installer_principal: test_principal(31),
                module_sha256: profile.bridge_canister_wasm_sha256.clone(),
                init_candid_sha256: canister_init_candid_sha256,
                runtime_binding: canister_runtime_binding,
                governance_operator: profile.governance_operator.clone(),
                runtime_administrator: profile.runtime_administrator.clone(),
                independent_canceller: profile.independent_canceller.clone(),
                mint_authorization_ttl_seconds: 900,
                mint_authorization_epoch: 7,
                storage_validation_complete: true,
                storage_checksum_complete: true,
                deposits_paused: true,
                state_is_empty: true,
                cycles_reserve_sufficient: true,
            },
        };
        let deployment_binding = ProductionDeploymentBinding {
            deployer_address: profile.initial_base_deployment.deployer_address.clone(),
            starting_nonce: profile.initial_base_deployment.starting_nonce,
            timelock: ProductionContractDeploymentBinding {
                transaction_hash: receipt.timelock_deployment_transaction_hash.clone(),
                address: profile.timelock.address.clone(),
                block_number: receipt.timelock_deployment_block_number,
                block_hash: receipt.timelock_deployment_block_hash.clone(),
            },
            bridge: ProductionContractDeploymentBinding {
                transaction_hash: receipt.bridge_deployment_transaction_hash.clone(),
                address: profile.bridge_contract.clone(),
                block_number: receipt.bridge_deployment_block_number,
                block_hash: receipt.bridge_deployment_block_hash.clone(),
            },
        };
        assert!(validate_completed_gate_a_receipt(
            &gate_a,
            &receipt,
            &receipt.canister_install,
            &deployment_binding,
        )
        .is_ok());
        let mut predeploy_receipt = receipt.clone();
        predeploy_receipt.bridge_deployment_block_number = 0;
        assert!(validate_completed_gate_a_receipt(
            &gate_a,
            &predeploy_receipt,
            &receipt.canister_install,
            &deployment_binding,
        )
        .is_err());
        let mut drifted_install_receipt = receipt.canister_install.clone();
        drifted_install_receipt.installer_principal = test_principal(30);
        assert!(validate_completed_gate_a_receipt(
            &gate_a,
            &receipt,
            &drifted_install_receipt,
            &deployment_binding,
        )
        .is_err());
        let mut forged_binding = ProductionDeploymentBinding {
            deployer_address: deployment_binding.deployer_address.clone(),
            starting_nonce: deployment_binding.starting_nonce,
            timelock: ProductionContractDeploymentBinding {
                transaction_hash: deployment_binding.timelock.transaction_hash.clone(),
                address: deployment_binding.timelock.address.clone(),
                block_number: deployment_binding.timelock.block_number,
                block_hash: deployment_binding.timelock.block_hash.clone(),
            },
            bridge: ProductionContractDeploymentBinding {
                transaction_hash: deployment_binding.bridge.transaction_hash.clone(),
                address: deployment_binding.bridge.address.clone(),
                block_number: deployment_binding.bridge.block_number,
                block_hash: deployment_binding.bridge.block_hash.clone(),
            },
        };
        forged_binding.bridge.transaction_hash = format!("0x{}", "ee".repeat(32));
        assert!(validate_completed_gate_a_receipt(
            &gate_a,
            &receipt,
            &receipt.canister_install,
            &forged_binding,
        )
        .is_err());
        profile.parameters = final_operational_parameters;
        let final_profile_bytes = serde_json::to_vec(&profile).unwrap();
        fs::write(root.join("profile.json"), &final_profile_bytes).unwrap();
        artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "profile.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&final_profile_bytes));
        let receipt_bytes = serde_json::to_vec(&receipt).unwrap();
        fs::write(root.join("gate-a-receipt.json"), &receipt_bytes).unwrap();
        initial_parameters.profile_sha256 = hex(&canonical_sha256(&profile).unwrap());
        let initial_parameters_bytes = serde_json::to_vec(&initial_parameters).unwrap();
        fs::write(
            root.join("initial-operational-parameters.json"),
            &initial_parameters_bytes,
        )
        .unwrap();
        let transition = PostGateAPolicyTransition {
            schema_version: 1,
            reason: "activate-before-production-measurements".into(),
            observed_at_unix: now - 20,
            gate_a_manifest_sha256: receipt.gate_a_manifest_sha256.clone(),
            gate_a_receipt_sha256: hex(&Sha256::digest(&receipt_bytes)),
            from_source_revision: receipt.source_revision.clone(),
            from_source_tree_sha256: receipt.source_tree_sha256.clone(),
            to_source_revision: "a".repeat(40),
            to_source_tree_sha256: "2".repeat(64),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            bridge_contract: profile.bridge_contract.clone(),
            bsns_contract: profile.bsns_contract.clone(),
            timelock_contract: profile.timelock.address.clone(),
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            bridge_runtime_bytecode_sha256: profile.bridge_runtime_bytecode_sha256.clone(),
            bsns_runtime_bytecode_sha256: profile.bsns_runtime_bytecode_sha256.clone(),
            bsns_runtime_template_sha256: profile.bsns_runtime_template_sha256.clone(),
            bridge_deployment_transaction_hash: receipt.bridge_deployment_transaction_hash.clone(),
            timelock_deployment_transaction_hash: receipt
                .timelock_deployment_transaction_hash
                .clone(),
        };
        let transition_bytes = serde_json::to_vec(&transition).unwrap();
        fs::write(
            root.join("post-gate-a-policy-transition.json"),
            &transition_bytes,
        )
        .unwrap();
        artifacts.push(ArtifactDigest {
            path: "gate-a-receipt.json".into(),
            sha256: hex(&Sha256::digest(receipt_bytes)),
        });
        artifacts.push(ArtifactDigest {
            path: "initial-operational-parameters.json".into(),
            sha256: hex(&Sha256::digest(initial_parameters_bytes)),
        });
        artifacts.push(ArtifactDigest {
            path: "post-gate-a-policy-transition.json".into(),
            sha256: hex(&Sha256::digest(transition_bytes)),
        });
        artifacts.retain(|artifact| GATE_B_ARTIFACTS.contains(&artifact.path.as_str()));
        let manifest = ReleaseManifest {
            schema_version: 4,
            release_id: "release-1".into(),
            test_only: false,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            created_at_unix: manifest_created,
            expires_at_unix: manifest_created + 100,
            parent_gate_a_manifest_sha256: Some(gate_a.manifest_sha256),
            artifacts,
        };
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let mut expected_gate_a_profile = profile.clone();
        expected_gate_a_profile.deployment_block = 0;
        set_production_bootstrap_operational_config(&mut expected_gate_a_profile);
        assert_eq!(
            receipt.gate_a_profile_sha256,
            hex(&canonical_sha256(&expected_gate_a_profile).unwrap())
        );
        let mut expected_post_deploy_profile = profile.clone();
        set_production_bootstrap_operational_config(&mut expected_post_deploy_profile);
        assert_eq!(
            serde_json::to_value(&gate_a_post_deploy_profile).unwrap(),
            serde_json::to_value(&expected_post_deploy_profile).unwrap()
        );
        assert_eq!(
            receipt.post_deploy_profile_sha256,
            hex(&Sha256::digest(
                canonical_bytes(&expected_post_deploy_profile).unwrap()
            ))
        );
        let bundle = validate_bundle(&root, true).unwrap();
        // Cryptographic live inputs are verified against the network by `verify-live`;
        // this fixture exercises only deterministic bundle inputs.
        let installer =
            Principal::from_text(&receipt.canister_install.installer_principal).unwrap();
        let module_hash = decode_hex(&bundle.profile.bridge_canister_wasm_sha256).unwrap();
        assert!(validate_gate_b_management_snapshot(&bundle, &[installer], &module_hash).is_ok());
        assert!(validate_gate_b_management_snapshot(
            &bundle,
            &[Principal::from_text(KINIC_ROOT).unwrap()],
            &module_hash,
        )
        .is_err());
        assert!(validate_gate_b_management_snapshot(&bundle, &[installer], &[0; 32]).is_err());

        let valid_monitoring_bytes = fs::read(root.join("monitoring-receipt.json")).unwrap();
        let valid_keeper_bytes = fs::read(root.join("keeper-drill.json")).unwrap();
        let mut mismatched_withdrawal: Value =
            serde_json::from_slice(&valid_monitoring_bytes).unwrap();
        mismatched_withdrawal["withdrawal_id"] = Value::String(format!("0x{}", "9".repeat(64)));
        fs::write(
            root.join("monitoring-receipt.json"),
            serde_json::to_vec(&mismatched_withdrawal).unwrap(),
        )
        .unwrap();
        assert!(validate_keeper_drill(&root, &bundle.manifest, &bundle.profile, now).is_err());

        let mut noncanonical_burn: Value = serde_json::from_slice(&valid_monitoring_bytes).unwrap();
        noncanonical_burn["burn"]["canonical_finalized"] = Value::Bool(false);
        fs::write(
            root.join("monitoring-receipt.json"),
            serde_json::to_vec(&noncanonical_burn).unwrap(),
        )
        .unwrap();
        assert!(validate_keeper_drill(&root, &bundle.manifest, &bundle.profile, now).is_err());

        let mut unpaid_observation: Value =
            serde_json::from_slice(&valid_monitoring_bytes).unwrap();
        unpaid_observation["paid"]["state"] = Value::String("ReleasePending".into());
        fs::write(
            root.join("monitoring-receipt.json"),
            serde_json::to_vec(&unpaid_observation).unwrap(),
        )
        .unwrap();
        assert!(validate_keeper_drill(&root, &bundle.manifest, &bundle.profile, now).is_err());
        fs::write(
            root.join("monitoring-receipt.json"),
            &valid_monitoring_bytes,
        )
        .unwrap();

        let mut arbitrary_digest: Value = serde_json::from_slice(&valid_keeper_bytes).unwrap();
        arbitrary_digest["monitoring_receipt_sha256"] = Value::String("9".repeat(64));
        fs::write(
            root.join("keeper-drill.json"),
            serde_json::to_vec(&arbitrary_digest).unwrap(),
        )
        .unwrap();
        assert!(validate_keeper_drill(&root, &bundle.manifest, &bundle.profile, now).is_err());
        fs::write(root.join("keeper-drill.json"), &valid_keeper_bytes).unwrap();

        let payload_sha256 = hex(&Sha256::digest([0x44, 0x49, 0x44, 0x4c, 0x00, 0x00]));
        let mut schedule_receipt = ActivationReceipt {
            schema_version: 4,
            phase: "schedule".into(),
            release_id: bundle.manifest.release_id.clone(),
            source_revision: bundle.manifest.source_revision.clone(),
            source_tree_sha256: bundle.manifest.source_tree_sha256.clone(),
            gate_b_manifest_sha256: bundle.manifest_sha256.clone(),
            proposal_id: 1,
            function_id: 1,
            target_method_name: "schedule_activation".into(),
            payload_sha256,
            executed_at_unix: manifest_created,
            verified_at_unix: manifest_created,
            governance_query_response_hex: hex(b"proposal"),
            governance_query_response_sha256: hex(&Sha256::digest(b"proposal")),
            function_registry_response_hex: hex(b"registry"),
            function_registry_response_sha256: hex(&Sha256::digest(b"registry")),
            activation_status_response_hex: hex(b"activation"),
            activation_status_response_sha256: hex(&Sha256::digest(b"activation")),
            operation_id: format!(
                "0x{}",
                hex(&initial_activation_operation_id(
                    activation_bridge,
                    operation_salt
                ))
            ),
            operation_salt: format!("0x{}", hex(&operation_salt)),
            prior_schedule_receipt_sha256: None,
        };
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_ok());
        let valid_operation_id = schedule_receipt.operation_id.clone();
        schedule_receipt.operation_id = format!("0x{}", "1".repeat(64));
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());
        schedule_receipt.operation_id = valid_operation_id;
        let valid_operation_salt = schedule_receipt.operation_salt.clone();
        schedule_receipt.operation_salt = format!("0x{}", "2".repeat(64));
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());
        schedule_receipt.operation_salt = valid_operation_salt;
        schedule_receipt.schema_version = 3;
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());
        schedule_receipt.schema_version = 5;
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());
        schedule_receipt.schema_version = 4;
        let mut obsolete_receipt = serde_json::to_value(&schedule_receipt).unwrap();
        obsolete_receipt["base_postcondition_sha256"] = Value::String("3".repeat(64));
        assert!(serde_json::from_value::<ActivationReceipt>(obsolete_receipt).is_err());
        schedule_receipt.gate_b_manifest_sha256 = "9".repeat(64);
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());
        schedule_receipt.gate_b_manifest_sha256 = bundle.manifest_sha256.clone();
        schedule_receipt.activation_status_response_sha256 = "4".repeat(64);
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());

        let valid_handover_bytes = fs::read(root.join("controller-handover.json")).unwrap();
        let mut tampered_response: Value = serde_json::from_slice(&valid_handover_bytes).unwrap();
        tampered_response["response_sha256"] = Value::String("0".repeat(64));
        fs::write(
            root.join("controller-handover.json"),
            serde_json::to_vec(&tampered_response).unwrap(),
        )
        .unwrap();
        assert!(
            validate_plan006_evidence(&bundle.root, &bundle.manifest, &bundle.profile, now)
                .is_err()
        );

        let mut mismatched_request: Value = serde_json::from_slice(&valid_handover_bytes).unwrap();
        mismatched_request["request_id"] = Value::String("4".repeat(64));
        fs::write(
            root.join("controller-handover.json"),
            serde_json::to_vec(&mismatched_request).unwrap(),
        )
        .unwrap();
        assert!(
            validate_plan006_evidence(&bundle.root, &bundle.manifest, &bundle.profile, now)
                .is_err()
        );

        let mut extra_controller: Value = serde_json::from_slice(&valid_handover_bytes).unwrap();
        extra_controller["final_controllers"] =
            serde_json::json!([profile.root_canister_id.clone(), test_principal(32)]);
        fs::write(
            root.join("controller-handover.json"),
            serde_json::to_vec(&extra_controller).unwrap(),
        )
        .unwrap();
        assert!(
            validate_plan006_evidence(&bundle.root, &bundle.manifest, &bundle.profile, now)
                .is_err()
        );
        fs::write(root.join("controller-handover.json"), &valid_handover_bytes).unwrap();

        let valid_upgrade_bytes = fs::read(root.join("sns-upgrade.json")).unwrap();
        let mut pending_upgrade: Value = serde_json::from_slice(&valid_upgrade_bytes).unwrap();
        pending_upgrade["status"] = Value::String("Pending".into());
        fs::write(
            root.join("sns-upgrade.json"),
            serde_json::to_vec(&pending_upgrade).unwrap(),
        )
        .unwrap();
        assert!(
            validate_plan006_evidence(&bundle.root, &bundle.manifest, &bundle.profile, now)
                .is_err()
        );
        let mut reinstall_upgrade: Value = serde_json::from_slice(&valid_upgrade_bytes).unwrap();
        reinstall_upgrade["install_mode"] = Value::String("reinstall".into());
        fs::write(
            root.join("sns-upgrade.json"),
            serde_json::to_vec(&reinstall_upgrade).unwrap(),
        )
        .unwrap();
        assert!(
            validate_plan006_evidence(&bundle.root, &bundle.manifest, &bundle.profile, now)
                .is_err()
        );
        let mut forged_upgrade: Value = serde_json::from_slice(&valid_upgrade_bytes).unwrap();
        forged_upgrade["governance_query_response_hex"] = Value::String(hex(b"forged"));
        fs::write(
            root.join("sns-upgrade.json"),
            serde_json::to_vec(&forged_upgrade).unwrap(),
        )
        .unwrap();
        assert!(
            validate_plan006_evidence(&bundle.root, &bundle.manifest, &bundle.profile, now)
                .is_err()
        );
        fs::write(root.join("sns-upgrade.json"), &valid_upgrade_bytes).unwrap();

        fs::remove_file(root.join("controller-handover.json")).unwrap();
        assert!(
            validate_plan006_evidence(&bundle.root, &bundle.manifest, &bundle.profile, now)
                .is_err()
        );
        fs::write(
            root.join("controller-handover.json"),
            serde_json::to_vec(&handover).unwrap(),
        )
        .unwrap();
        let valid_profile_bytes = fs::read(root.join("profile.json")).unwrap();
        let valid_receipt_bytes = fs::read(root.join("gate-a-receipt.json")).unwrap();
        let valid_manifest_bytes = fs::read(root.join("release-manifest.json")).unwrap();
        profile.pause_principal = test_principal(30);
        let drifted_profile_bytes = serde_json::to_vec(&profile).unwrap();
        fs::write(root.join("profile.json"), &drifted_profile_bytes).unwrap();
        let mut drifted_receipt = receipt;
        drifted_receipt.post_deploy_profile_sha256 = hex(&Sha256::digest(&drifted_profile_bytes));
        let drifted_receipt_bytes = serde_json::to_vec(&drifted_receipt).unwrap();
        fs::write(root.join("gate-a-receipt.json"), &drifted_receipt_bytes).unwrap();
        let mut drifted_manifest: ReleaseManifest =
            serde_json::from_slice(&valid_manifest_bytes).unwrap();
        for artifact in &mut drifted_manifest.artifacts {
            if artifact.path == "profile.json" {
                artifact.sha256 = hex(&Sha256::digest(&drifted_profile_bytes));
            } else if artifact.path == "gate-a-receipt.json" {
                artifact.sha256 = hex(&Sha256::digest(&drifted_receipt_bytes));
            }
        }
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&drifted_manifest).unwrap(),
        )
        .unwrap();
        let drift_error = match validate_bundle(&root, true) {
            Ok(_) => panic!("Gate B accepted non-deployment profile drift"),
            Err(error) => error,
        };
        assert!(
            drift_error.contains("initial operational parameters do not exactly match"),
            "unexpected error: {drift_error}"
        );
        fs::write(root.join("profile.json"), valid_profile_bytes).unwrap();
        fs::write(root.join("gate-a-receipt.json"), valid_receipt_bytes).unwrap();
        fs::write(root.join("release-manifest.json"), valid_manifest_bytes).unwrap();
        let valid_rehearsal = fs::read(root.join("rpc-e2e.json")).unwrap();
        let mut incomplete: Value = serde_json::from_slice(&valid_rehearsal).unwrap();
        incomplete["scenarios"]["quorum_loss"] = Value::Null;
        incomplete["scenarios"]["final_pause"] = Value::Null;
        incomplete["state"] = Value::String("READY_FOR_QUORUM_LOSS".into());
        incomplete["launch_ready"] = Value::Bool(false);
        incomplete["extended_complete"] = Value::Bool(false);
        fs::write(
            root.join("rpc-e2e.json"),
            serde_json::to_vec(&incomplete).unwrap(),
        )
        .unwrap();
        assert!(validate_rpc_rehearsal(&bundle).is_err());
        fs::write(root.join("rpc-e2e.json"), valid_rehearsal).unwrap();
        fs::write(root.join("rpc-e2e.json"), b"{}").unwrap();
        assert!(validate_bundle(&root, true)
            .err()
            .unwrap()
            .contains("artifact hash mismatch"));
        fs::remove_dir_all(root).unwrap();
    }
}
