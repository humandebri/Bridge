#![recursion_limit = "256"]

use candid::{CandidType, Decode, Encode, Principal, Reserved};
use ic_agent::{
    agent::CallResponse,
    identity::{BasicIdentity, Identity, Secp256k1Identity},
    Agent,
};
use ic_transport_types::{Envelope, EnvelopeContent};
use k256::ecdsa::{signature::Verifier, RecoveryId, Signature as Secp256k1Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    env, fs,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
    process::{self, Command},
    time::{SystemTime, UNIX_EPOCH},
};
use tiny_keccak::{Hasher, Keccak};

const KINIC_LEDGER: &str = "73mez-iiaaa-aaaaq-aaasq-cai";
const KINIC_INDEX: &str = "7vojr-tyaaa-aaaaq-aaatq-cai";
const KINIC_ROOT: &str = "7jkta-eyaaa-aaaaq-aaarq-cai";
const KINIC_GOVERNANCE: &str = "74ncn-fqaaa-aaaaq-aaasa-cai";
const PRODUCTION_BRIDGE_CANISTER: &str = "lb5i5-ziaaa-aaaar-qcgwq-cai";
const PRODUCTION_PAUSE_PRINCIPAL: &str =
    "lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe";
const OFFICIAL_EVM_RPC_CANISTER: &str = "7hfb6-caaaa-aaaar-qadga-cai";
const MAX_EVIDENCE_AGE_SECS: u64 = 90 * 24 * 60 * 60;
const MAX_ACTIVATION_ATTESTATION_AGE_SECS: u64 = 5 * 60;
const CURRENT_STABLE_SCHEMA_VERSION: u16 = 36;
const PREVIOUS_STABLE_SCHEMA_VERSION: u16 = 35;
const RELEASE_PROFILE_SCHEMA_VERSION: u8 = 5;
const PRODUCTION_CANISTER_INSTALL_RECEIPT_SCHEMA_VERSION: u8 = 3;
const PRODUCTION_UPGRADE_CHUNK_SIZE: usize = 1024 * 1024;
const GATE_A_ARTIFACTS: [&str; 6] = [
    "profile.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "bsns-creation.bin",
    "bsns-runtime.bin",
    "bsns-runtime-layout.json",
];
const GATE_B_ARTIFACTS: [&str; 11] = [
    "profile.json",
    "initial-operational-parameters.json",
    "provider-independence.json",
    "ui-assets.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "bsns-creation.bin",
    "bsns-runtime.bin",
    "bsns-runtime-layout.json",
    "gate-a-receipt.json",
    "gate-a-profile.json",
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

#[derive(CandidType, Deserialize)]
enum ManagementInstallMode {
    #[serde(rename = "upgrade")]
    Upgrade,
}

#[derive(CandidType, Deserialize)]
struct ManagementUploadChunkArgument {
    canister_id: Principal,
    chunk: Vec<u8>,
}

#[derive(CandidType)]
struct ManagementStoredChunksArgument {
    canister_id: Principal,
}

#[derive(CandidType, Deserialize, Clone)]
struct ManagementChunkHash {
    hash: Vec<u8>,
}

#[derive(CandidType)]
struct ManagementInstallChunkedCodeArgument {
    mode: ManagementInstallMode,
    target_canister: Principal,
    store_canister: Option<Principal>,
    chunk_hashes_list: Vec<ManagementChunkHash>,
    wasm_module_hash: Vec<u8>,
    arg: Vec<u8>,
    sender_canister_version: Option<u64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionUpgradeChunkSubmission {
    index: u32,
    offset: u64,
    size_bytes: u64,
    sha256: String,
    argument_hex: String,
    argument_sha256: String,
    ingress_expiry: u64,
    request_id: String,
    signed_update_hex: String,
    signed_update_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionUpgradeSignedUpdate {
    argument_hex: String,
    argument_sha256: String,
    ingress_expiry: u64,
    request_id: String,
    signed_update_hex: String,
    signed_update_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionUpgradeSubmission {
    schema_version: u8,
    install_method: String,
    ic_host: String,
    effective_canister_id: String,
    sender_principal: String,
    wasm_sha256: String,
    chunk_size_bytes: u64,
    stored_chunks: ProductionUpgradeSignedUpdate,
    chunks: Vec<ProductionUpgradeChunkSubmission>,
    argument_hex: String,
    argument_sha256: String,
    ingress_expiry: u64,
    request_id: String,
    signed_update_hex: String,
    signed_update_sha256: String,
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
    walletconnect_project_id: String,
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

fn set_initial_operational_config(target: &mut Parameters, source: &Parameters) {
    target.gas_limit_ceiling = source.gas_limit_ceiling;
    target.max_fee_per_gas_ceiling = source.max_fee_per_gas_ceiling;
    target.max_priority_fee_per_gas_ceiling = source.max_priority_fee_per_gas_ceiling;
    target.l1_fee_per_transaction_ceiling_wei = source.l1_fee_per_transaction_ceiling_wei;
    target.quote_validity_seconds = source.quote_validity_seconds;
    target.gas_limit_multiplier_bps = source.gas_limit_multiplier_bps;
    target.base_fee_multiplier_bps = source.base_fee_multiplier_bps;
    target.l1_fee_multiplier_bps = source.l1_fee_multiplier_bps;
    target.cycles_floor = source.cycles_floor;
    target.settlement_cycle_ceiling = source.settlement_cycle_ceiling;
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

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq)]
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

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct ProviderIndependenceReceipt {
    schema_version: u8,
    reviewed_at_unix: u64,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    profile_sha256: String,
    bridge_canister_wasm_sha256: String,
    evm_rpc_canister_id: String,
    base_chain_id: u64,
    rpc_service: String,
    provider_selection: String,
    custom_evm_rpc_urls_sha256: String,
    consensus_strategy: RpcConsensusStrategyBinding,
    guarantee_boundary: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct RpcConsensusStrategyBinding {
    kind: String,
    total: u8,
    min: u8,
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
    validator_canister_id: String,
    validator_method_name: String,
    previous_governance_operation_id: u64,
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
    validator_canister_id: String,
    validator_method_name: String,
    previous_governance_operation_id: u64,
    payload_sha256: String,
    executed_at_unix: u64,
    verified_at_unix: u64,
    governance_query_response_hex: String,
    governance_query_response_sha256: String,
    function_registry_response_hex: String,
    function_registry_response_sha256: String,
    activation_status_response_hex: String,
    activation_status_response_sha256: String,
    governance_operation_id: String,
    operation_id: String,
    operation_salt: String,
    prior_schedule_receipt_sha256: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectActivationArtifact {
    operation_id: String,
    kind: DirectActivationKind,
    chain_id: String,
    sender: String,
    nonce: String,
    target: String,
    calldata: String,
    gas_limit: String,
    max_fee_per_gas: String,
    max_priority_fee_per_gas: String,
    raw_transaction: String,
    transaction_hash: String,
    generation: u8,
    signed_at_ns: String,
}

#[derive(Clone, Deserialize, Serialize)]
enum DirectActivationKind {
    ScheduleActivation(DirectActivationOperation),
    ExecuteActivation(DirectActivationOperation),
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectActivationOperation {
    operation_id: String,
    salt: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ControllerActivationAuthorizationReceipt {
    schema_version: u8,
    phase: String,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    gate_b_manifest_sha256: String,
    operational_config_seal_receipt_sha256: String,
    controller_principal: String,
    certified_controller_set: Vec<String>,
    certified_module_sha256: String,
    authorized_at_unix: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControllerActivationPrepareReceipt {
    schema_version: u8,
    phase: String,
    gate_b_manifest_sha256: String,
    artifact_sha256: String,
    authorization_receipt_sha256: String,
    bound_at_unix: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectActivationConfirmationFile {
    schema_version: u8,
    confirmed_at_unix: u64,
    artifact_sha256: String,
    operation_id: String,
    transaction_hash: String,
    response: DirectActivationConfirmation,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectActivationConfirmation {
    operation_id: String,
    transaction_hash: String,
    receipt_block_number: String,
    succeeded: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ControllerActivationReceipt {
    schema_version: u8,
    phase: String,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    bridge_canister_wasm_sha256: String,
    gate_b_manifest_sha256: String,
    artifact_sha256: String,
    authorization_receipt_hex: String,
    authorization_receipt_sha256: String,
    prepare_receipt_hex: String,
    prepare_receipt_sha256: String,
    controller_principal: String,
    certified_controller_set: Vec<String>,
    governance_operation_id: String,
    timelock_operation_id: String,
    operation_salt: String,
    transaction_hash: String,
    confirmed_generation: u8,
    confirmed_signed_at_ns: String,
    finalized_block_number: String,
    deposits_paused: bool,
    activation_status_response_hex: String,
    activation_status_response_sha256: String,
    prior_schedule_receipt_sha256: Option<String>,
    confirmed_at_unix: u64,
    verified_at_unix: u64,
}

fn controller_activation_confirmation_fields_match(
    confirmed_generation: u8,
    confirmed_signed_at_ns: &str,
    confirmation: &ActivationConfirmationStatusView,
) -> bool {
    let Ok(signed_at_ns) = parse_decimal_u128(confirmed_signed_at_ns, "confirmed signed timestamp")
        .and_then(|value| {
            u64::try_from(value).map_err(|_| "confirmed signed timestamp exceeds nat64".into())
        })
    else {
        return false;
    };
    bridge_core::kernel::confirmed_activation_metadata_matches(
        confirmed_generation,
        signed_at_ns,
        confirmation.generation,
        confirmation.signed_at_ns,
    )
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OperationalConfigSealReservation {
    schema_version: u8,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    gate_b_manifest_sha256: String,
    bridge_canister_id: String,
    controller_principal: String,
    certified_module_sha256: String,
    parameters_sha256: String,
    operational_args_sha256: String,
    reserved_at_unix: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OperationalConfigSealReceiptEvidence {
    schema_version: u8,
    release_id: String,
    source_revision: String,
    source_tree_sha256: String,
    gate_b_manifest_sha256: String,
    reservation_hex: String,
    reservation_sha256: String,
    parameters_sha256: String,
    operational_args_sha256: String,
    bridge_canister_id: String,
    controller_principal: String,
    certified_controller_set: Vec<String>,
    certified_module_sha256: String,
    expected_operational_config_sha256: String,
    observed_operational_config_sha256: String,
    recovered: bool,
    attempt_hex: Option<String>,
    attempt_sha256: Option<String>,
    lifecycle_response_hex: String,
    lifecycle_response_sha256: String,
    activation_attestation_response_hex: String,
    activation_attestation_response_sha256: String,
    runtime_binding_response_hex: String,
    runtime_binding_response_sha256: String,
    bridge_status_response_hex: String,
    bridge_status_response_sha256: String,
    pending_transactions_response_hex: String,
    pending_transactions_response_sha256: String,
    verified_at_unix: u64,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ProposalId {
    id: u64,
}

#[derive(CandidType, Serialize)]
struct GetProposalRequest {
    proposal_id: Option<ProposalId>,
}

#[derive(CandidType, Deserialize, Serialize)]
struct GetProposalResponse {
    result: Option<GetProposalResult>,
}

#[derive(CandidType, Deserialize, Serialize)]
enum GetProposalResult {
    Error(GovernanceErrorView),
    Proposal(Box<ProposalDataView>),
}

#[derive(CandidType, Deserialize, Serialize)]
struct GovernanceErrorView {
    error_message: String,
    error_type: i32,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ProposalDataView {
    id: Option<ProposalId>,
    failure_reason: Option<GovernanceErrorView>,
    failed_timestamp_seconds: u64,
    decided_timestamp_seconds: u64,
    proposal: Option<ProposalView>,
    executed_timestamp_seconds: u64,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ProposalView {
    action: Option<SnsProposalAction>,
    summary: String,
}

#[allow(clippy::large_enum_variant)]
#[derive(CandidType, Deserialize, Serialize)]
enum SnsProposalAction {
    ManageNervousSystemParameters(Reserved),
    AddGenericNervousSystemFunction(Reserved),
    SetTopicsForCustomProposals(Reserved),
    ManageDappCanisterSettings(Reserved),
    RemoveGenericNervousSystemFunction(Reserved),
    UpgradeSnsToNextVersion(Reserved),
    AdvanceSnsTargetVersion(Reserved),
    RegisterDappCanisters(RegisterDappCanistersView),
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

#[derive(CandidType, Deserialize, Serialize)]
struct UpgradeSnsControlledCanisterView {
    mode: Option<i32>,
    canister_upgrade_arg: Option<Vec<u8>>,
    canister_upgrade_options: Option<Reserved>,
    chunked_canister_wasm: Option<ChunkedSnsWasmView>,
    new_canister_wasm: Vec<u8>,
    canister_id: Option<Principal>,
}

#[derive(CandidType, Deserialize, Serialize)]
struct RegisterDappCanistersView {
    canister_ids: Vec<Principal>,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ChunkedSnsWasmView {
    wasm_module_hash: Vec<u8>,
    store_canister_id: Option<Principal>,
    chunk_hashes_list: Vec<Vec<u8>>,
}

#[derive(CandidType, Deserialize, Serialize)]
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
    validator_canister_id: Option<Principal>,
    validator_method_name: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ActivationOperationStatusView {
    operation_id: Vec<u8>,
    salt: Vec<u8>,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ActivationStatusView {
    deposits_paused: bool,
    pending_timelock_operation: Option<ActivationOperationStatusView>,
    last_confirmed_activation: Option<ActivationConfirmationStatusView>,
}

#[derive(CandidType, Deserialize, Serialize)]
struct ActivationConfirmationStatusView {
    phase: String,
    governance_operation_id: u64,
    timelock_operation_id: Vec<u8>,
    transaction_hash: Vec<u8>,
    receipt_block_number: u64,
    generation: u8,
    signed_at_ns: u64,
}

#[derive(CandidType, Deserialize, Serialize)]
enum ActivationStatusResultView {
    Ok(ActivationStatusView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
enum PendingGovernanceTransactionsView {
    Ok(Vec<Reserved>),
    Err(Reserved),
}

#[derive(CandidType, Deserialize, Serialize)]
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

#[derive(CandidType, Deserialize, Serialize)]
enum ActivationAttestationResultView {
    Ok(Box<ActivationAttestationView>),
    Err(Reserved),
}

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq, Serialize)]
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

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq, Serialize)]
struct ReserveStatusView {
    sufficient: bool,
}

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq, Serialize)]
struct BridgeStatusLiveView {
    reserve: ReserveStatusView,
    deposits_paused: bool,
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
    counts: ProductionStatusCountsView,
}

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq, Serialize)]
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

#[derive(CandidType, Deserialize, Serialize)]
enum StorageIntegrityResultView {
    Ok(String),
    Err(Reserved),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, CandidType, Deserialize, Serialize)]
enum ProductionLifecycleView {
    Bootstrap,
    OperationalConfigSealed,
    Activated,
}

#[derive(CandidType, Deserialize, Serialize)]
enum ProductionLifecycleResultView {
    Ok(ProductionLifecycleView),
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
        || evidence.governance_operation_id != 0
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
    if sample_times
        .into_iter()
        .any(|at| at > now || now.saturating_sub(at) > MAX_EVIDENCE_AGE_SECS)
    {
        return Err("initial operational observations are stale or future-dated".into());
    }
    validate_initial_operational_parameter_lineage(evidence, profile, manifest_created_at_unix)
}

fn validate_initial_operational_parameter_lineage(
    evidence: &InitialOperationalParameters,
    profile: &Profile,
    manifest_created_at_unix: u64,
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
    if sample_times
        .into_iter()
        .any(|at| at == 0 || at > manifest_created_at_unix)
    {
        return Err("initial operational observations do not predate Gate B".into());
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

#[cfg(test)]
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

#[cfg(test)]
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

fn validate_provider_independence_receipt(
    root: &Path,
    manifest: &ReleaseManifest,
    profile: &Profile,
    now: u64,
) -> Result<(), String> {
    let receipt: ProviderIndependenceReceipt = read_json(&root.join("provider-independence.json"))?;
    validate_evidence_time(receipt.reviewed_at_unix, manifest.created_at_unix, now)?;
    validate_provider_independence_binding(&receipt, manifest, profile)
}

fn validate_provider_independence_binding(
    receipt: &ProviderIndependenceReceipt,
    manifest: &ReleaseManifest,
    profile: &Profile,
) -> Result<(), String> {
    validate_production_rpc_profile(profile)?;
    let artifacts = manifest
        .artifacts
        .iter()
        .map(|artifact| (artifact.path.as_str(), artifact.sha256.as_str()))
        .collect::<BTreeMap<_, _>>();
    let profile_sha256 = artifacts
        .get("profile.json")
        .ok_or("provider binding manifest has no profile artifact")?;
    let wasm_sha256 = artifacts
        .get("bridge-canister.wasm")
        .ok_or("provider binding manifest has no Wasm artifact")?;
    let empty_custom_urls_sha256 = hex(&canonical_sha256(&profile.rpc_providers)?);
    if receipt.schema_version != 2
        || receipt.release_id != manifest.release_id
        || receipt.source_revision != manifest.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&manifest.source_tree_sha256)
        || !receipt.profile_sha256.eq_ignore_ascii_case(profile_sha256)
        || !receipt
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !receipt
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(wasm_sha256)
        || receipt.evm_rpc_canister_id != OFFICIAL_EVM_RPC_CANISTER
        || receipt.base_chain_id != 8_453
        || receipt.rpc_service != "BaseMainnet"
        || receipt.provider_selection != "default-pool"
        || !receipt
            .custom_evm_rpc_urls_sha256
            .eq_ignore_ascii_case(&empty_custom_urls_sha256)
        || receipt.consensus_strategy.kind != "Threshold"
        || receipt.consensus_strategy.total != 3
        || receipt.consensus_strategy.min != 2
        || receipt.guarantee_boundary
            != "evm-rpc-default-provider-registry-and-upstream-chain-are-external"
    {
        return Err(
            "provider independence receipt is not bound to the immutable production RPC service"
                .into(),
        );
    }
    Ok(())
}

fn validate_production_rpc_profile(profile: &Profile) -> Result<(), String> {
    if profile.environment != "mainnet-candidate"
        || profile.chain_id != 8_453
        || profile.evm_rpc_canister_id != OFFICIAL_EVM_RPC_CANISTER
        || profile.base_rpc_url.is_some()
        || !profile.rpc_providers.is_empty()
    {
        return Err("production RPC binding requires the official BaseMainnet defaults".into());
    }
    Ok(())
}

fn write_provider_independence_receipt(
    profile_path: &Path,
    release_id: &str,
    source_revision: &str,
    source_tree_sha256: &str,
    output_path: &Path,
) -> Result<(), String> {
    let profile: Profile = read_json(profile_path)?;
    validate_profile(&profile, true)?;
    if !valid_release_id(release_id)
        || source_revision.trim().is_empty()
        || !valid_sha256(source_tree_sha256)
    {
        return Err("invalid provider binding release identity".into());
    }
    let profile_bytes = fs::read(profile_path).map_err(|error| error.to_string())?;
    let receipt = provider_independence_receipt(
        &profile,
        now_unix()?,
        release_id,
        source_revision,
        source_tree_sha256,
        &hex(&Sha256::digest(profile_bytes)),
    )?;
    write_json_new(output_path, &receipt)
}

fn provider_independence_receipt(
    profile: &Profile,
    reviewed_at_unix: u64,
    release_id: &str,
    source_revision: &str,
    source_tree_sha256: &str,
    profile_sha256: &str,
) -> Result<ProviderIndependenceReceipt, String> {
    validate_production_rpc_profile(profile)?;
    Ok(ProviderIndependenceReceipt {
        schema_version: 2,
        reviewed_at_unix,
        release_id: release_id.into(),
        source_revision: source_revision.into(),
        source_tree_sha256: source_tree_sha256.into(),
        profile_sha256: profile_sha256.into(),
        bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
        evm_rpc_canister_id: profile.evm_rpc_canister_id.clone(),
        base_chain_id: profile.chain_id,
        rpc_service: "BaseMainnet".into(),
        provider_selection: "default-pool".into(),
        custom_evm_rpc_urls_sha256: hex(&canonical_sha256(&profile.rpc_providers)?),
        consensus_strategy: RpcConsensusStrategyBinding {
            kind: "Threshold".into(),
            total: 3,
            min: 2,
        },
        guarantee_boundary: "evm-rpc-default-provider-registry-and-upstream-chain-are-external"
            .into(),
    })
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

#[derive(Clone, Copy)]
enum ProfileSchemaPolicy {
    Current,
    Historical,
}

fn validate_profile_with_schema_policy(
    profile: &Profile,
    production: bool,
    schema_policy: ProfileSchemaPolicy,
) -> Result<(), String> {
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
    let schema_version_valid = match schema_policy {
        ProfileSchemaPolicy::Current => {
            profile.canister_schema_version == CURRENT_STABLE_SCHEMA_VERSION
        }
        ProfileSchemaPolicy::Historical => matches!(
            profile.canister_schema_version,
            PREVIOUS_STABLE_SCHEMA_VERSION | CURRENT_STABLE_SCHEMA_VERSION
        ),
    };
    if !schema_version_valid {
        return Err(match schema_policy {
            ProfileSchemaPolicy::Current => {
                "profile must bind the current stable schema version".into()
            }
            ProfileSchemaPolicy::Historical => {
                "historical profile must bind stable schema version 35 or 36".into()
            }
        });
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

fn validate_profile(profile: &Profile, production: bool) -> Result<(), String> {
    validate_profile_with_schema_policy(profile, production, ProfileSchemaPolicy::Current)
}

fn validate_gate_b_profile_schema_convergence(
    profile: &Profile,
    gate_a_profile: &Profile,
) -> Result<(), String> {
    if gate_a_profile.canister_schema_version != profile.canister_schema_version {
        return Err("Gate A and Gate B profiles must bind the same stable schema version".into());
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

fn post_deploy_profile_sha256(
    profile_source: &[u8],
    deployment_block: u64,
) -> Result<String, String> {
    let mut profile: Value = serde_json::from_slice(profile_source).map_err(|e| e.to_string())?;
    let fields = profile
        .as_object_mut()
        .ok_or_else(|| "Gate A profile must be a JSON object".to_string())?;
    match fields.get("deployment_block") {
        Some(Value::Number(_)) => {}
        _ => return Err("Gate A profile deployment_block must be a number".into()),
    }
    fields.insert("deployment_block".into(), Value::from(deployment_block));
    let mut bytes = Vec::new();
    canonical_json(&profile, &mut bytes)?;
    bytes.push(b'\n');
    Ok(hex(&Sha256::digest(bytes)))
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

fn ui_runtime_profile(
    profile: &Profile,
    profile_bytes: &[u8],
    production: bool,
    gate_b_manifest_sha256: Option<&str>,
) -> Result<Value, String> {
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
    let mut ui = serde_json::json!({
        "environment": profile.environment,
        "label": if profile.test_assets_only { "Base Sepolia" } else { "Base" },
        "testOnly": profile.test_assets_only,
        "environmentMode": null,
        "activationTimelockDelaySeconds": profile.timelock.minimum_delay_seconds,
        "gateBManifestSha256": gate_b_manifest_sha256,
        "profileFileSha256": hex(&Sha256::digest(profile_bytes)),
        "profileCanonicalSha256": hex(&canonical_sha256(profile)?),
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
    Ok(ui)
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
    let ui = ui_runtime_profile(&profile, &profile_bytes, production, gate_b_manifest_sha256)?;
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

fn validate_ui_assets_receipt(root: &Path, manifest: &ReleaseManifest) -> Result<(), String> {
    let receipt: UiAssetsReceipt = read_json(&root.join("ui-assets.json"))?;
    if receipt.schema_version != 2
        || receipt.source_revision != manifest.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&manifest.source_tree_sha256)
        || receipt.files.is_empty()
        || receipt.walletconnect_project_id.len() != 32
        || !receipt
            .walletconnect_project_id
            .bytes()
            .all(|value| value.is_ascii_hexdigit())
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

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OperationalEpochEvidence {
    response_hex: String,
    response_sha256: String,
}

impl OperationalEpochEvidence {
    fn from_response(response_hex: &str) -> Result<Self, String> {
        let bytes = decode_hex(response_hex)?;
        Ok(Self {
            response_hex: hex(&bytes),
            response_sha256: hex(&Sha256::digest(&bytes)),
        })
    }
}

fn validate_operational_epoch_snapshot(
    evidence: &OperationalEpochEvidence,
    status: &BridgeStatusLiveView,
    runtime: &LiveRuntimeBinding,
    ledger_fee: u128,
) -> Result<(), String> {
    if !activation_raw_digest_matches(&evidence.response_hex, &evidence.response_sha256)? {
        return Err("operational epoch evidence raw digest mismatch".into());
    }
    let config = match decode_candid_hex::<OperationalConfigResultView>(&evidence.response_hex)? {
        OperationalConfigResultView::Ok(value) => value,
        OperationalConfigResultView::Err(_) => {
            return Err("operational epoch preimage is unavailable".into())
        }
    };
    if config.mint_authorization_epoch != status.mint_authorization_epoch
        || config.mint_authorization_ttl_seconds != status.mint_authorization_ttl_seconds
        || status.mint_authorization_epoch == 0
        || operational_epoch_digest(
            &evidence.response_hex,
            ledger_fee,
            status.mint_authorization_epoch,
        )? != runtime.operational_config_sha256.to_ascii_lowercase()
    {
        return Err("operational epoch preimage differs from the observed snapshot".into());
    }
    Ok(())
}

fn operational_epoch_digest(
    response_hex: &str,
    ledger_fee: u128,
    epoch: u64,
) -> Result<String, String> {
    let config = match decode_candid_hex::<OperationalConfigResultView>(response_hex)? {
        OperationalConfigResultView::Ok(value) => *value,
        OperationalConfigResultView::Err(_) => {
            return Err("operational config preimage is unavailable".into())
        }
    };
    let mut operational_config: OperationalConfigView = config.into();
    operational_config.mint_authorization_epoch = epoch;
    operational_config_digest(operational_config, ledger_fee)
}

fn operational_config_digest(
    operational_config: OperationalConfigView,
    ledger_fee: u128,
) -> Result<String, String> {
    let encoded = Encode!(&OperationalConfigBindingView {
        ledger_fee,
        operational_config,
    })
    .map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    digest.update(OPERATIONAL_CONFIG_BINDING_DOMAIN);
    digest.update(encoded);
    Ok(hex(&digest.finalize()))
}

fn validate_bundle(root: &Path, gate_b: bool) -> Result<ValidatedBundle, String> {
    validate_bundle_with_freshness(root, gate_b, true)
}

fn validate_historical_gate_b_bundle(root: &Path) -> Result<ValidatedBundle, String> {
    validate_bundle_with_freshness(root, true, false)
}

fn validate_bundle_with_freshness(
    root: &Path,
    gate_b: bool,
    require_current: bool,
) -> Result<ValidatedBundle, String> {
    validate_bundle_with_freshness_at(root, gate_b, require_current, now_unix()?)
}

fn validate_bundle_with_freshness_at(
    root: &Path,
    gate_b: bool,
    require_current: bool,
    wall_now: u64,
) -> Result<ValidatedBundle, String> {
    let schema_policy = if gate_b && !require_current {
        ProfileSchemaPolicy::Historical
    } else {
        ProfileSchemaPolicy::Current
    };
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
    let now = if require_current {
        wall_now
    } else {
        manifest.created_at_unix
    };
    if lifetime == 0
        || lifetime > MAX_EVIDENCE_AGE_SECS
        || (require_current && now < manifest.created_at_unix)
        || (require_current && now > manifest.expires_at_unix)
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
    validate_profile_with_schema_policy(&profile, !manifest.test_only, schema_policy)?;
    if gate_b {
        let initial: InitialOperationalParameters =
            read_json(&root.join("initial-operational-parameters.json"))?;
        if require_current {
            validate_initial_operational_parameters(
                &initial,
                &profile,
                manifest.created_at_unix,
                now,
            )?;
        } else {
            validate_initial_operational_parameter_lineage(
                &initial,
                &profile,
                manifest.created_at_unix,
            )?;
        }
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
        let gate_a_profile_source =
            fs::read(root.join("gate-a-profile.json")).map_err(|e| e.to_string())?;
        let gate_a_profile: Profile =
            serde_json::from_slice(&gate_a_profile_source).map_err(|e| e.to_string())?;
        validate_profile_with_schema_policy(&gate_a_profile, !manifest.test_only, schema_policy)?;
        validate_gate_b_profile_schema_convergence(&profile, &gate_a_profile)?;
        if gate_a_profile.deployment_block != 0
            || !profile_uses_production_bootstrap_operational_config(&gate_a_profile)
        {
            return Err("Gate A profile artifact is not the immutable predeploy profile".into());
        }
        let expected_gate_a_profile_hash = hex(&canonical_sha256(&gate_a_profile)?);
        let mut expected_post_deploy_profile = gate_a_profile.clone();
        expected_post_deploy_profile.deployment_block = profile.deployment_block;
        set_production_bootstrap_operational_config(&mut expected_post_deploy_profile);
        let expected_post_deploy_profile_hash =
            post_deploy_profile_sha256(&gate_a_profile_source, profile.deployment_block)?;
        let mut expected_current_profile = expected_post_deploy_profile.clone();
        set_initial_operational_config(
            &mut expected_current_profile.parameters,
            &profile.parameters,
        );
        expected_current_profile.bridge_canister_wasm_sha256 =
            profile.bridge_canister_wasm_sha256.clone();
        if gate_a_profile.pause_principal == KINIC_ROOT
            && profile.pause_principal == PRODUCTION_PAUSE_PRINCIPAL
        {
            expected_current_profile.pause_principal = profile.pause_principal.clone();
        }
        if canonical_bytes(&expected_current_profile)? != canonical_bytes(&profile)? {
            return Err("Gate B profile changes fields outside the reviewed operational config and Wasm upgrade".into());
        }
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
                .eq_ignore_ascii_case(&gate_a_profile.bridge_canister_wasm_sha256)
            || !receipt
                .bridge_runtime_bytecode_sha256
                .eq_ignore_ascii_case(&gate_a_profile.bridge_runtime_bytecode_sha256)
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
        validate_production_canister_receipt(
            &expected_post_deploy_profile,
            &receipt.canister_install,
        )?;
    }
    if profile.test_assets_only != manifest.test_only {
        return Err("manifest/profile test-only mismatch".into());
    }
    if gate_b {
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

fn operational_config_binding(
    profile: &Profile,
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
) -> Result<OperationalConfigBindingView, String> {
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
    Ok(binding)
}

fn expected_operational_config_sha256(
    profile: &Profile,
    ttl: u64,
    epoch: u64,
) -> Result<[u8; 32], String> {
    let binding = operational_config_binding(profile, ttl, epoch)?;
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
    verify_live_inputs_for_services_hash(bundle, expected_deposits_paused, &rpc_url_hash)
}

fn verify_live_inputs_for_services_hash(
    bundle: &ValidatedBundle,
    expected_deposits_paused: bool,
    rpc_url_hash: &str,
) -> Result<(), String> {
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (public_raw, status_raw, pending_raw) = async_runtime()?.block_on(async {
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
        let pending = agent
            .query(&bridge, "get_pending_base_governance_transaction")
            .with_arg(Encode!().map_err(|error| error.to_string())?)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((public, status, pending))
    })?;
    let public = Decode!(&public_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let pending = Decode!(&pending_raw, PendingGovernanceTransactionsView)
        .map_err(|error| error.to_string())?;
    let observed = live_runtime_binding_from_view(&public);
    let operational_config_sha256 = expected_operational_config_sha256(
        &bundle.profile,
        status.mint_authorization_ttl_seconds,
        status.mint_authorization_epoch,
    )?;
    validate_live_runtime_binding(
        &observed,
        &bundle.profile,
        rpc_url_hash,
        &operational_config_sha256,
    )?;
    if !matches!(pending, PendingGovernanceTransactionsView::Ok(ref values) if values.is_empty())
        || public.expected_bridge_runtime_sha256
            != decode_hex(&bundle.profile.bridge_runtime_bytecode_sha256)?
        || status.deposits_paused != expected_deposits_paused
        || !status.reserve.sufficient
    {
        return Err("authenticated live Canister state does not satisfy Gate B".into());
    }
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

#[derive(CandidType, Deserialize)]
struct ProductionUiHistoryProbe {
    requester: Vec<u8>,
    before_cursor: Option<Vec<u8>>,
    limit: u16,
}

#[derive(CandidType, Deserialize)]
enum ProductionUiHistoryResult {
    Ok(Reserved),
    Err(Reserved),
}

#[derive(CandidType, Deserialize, Serialize)]
struct SnsCanistersView {
    dapps: Vec<Principal>,
}
#[derive(CandidType)]
struct SnsRootQuery {}

fn verify_production_current_state(
    profile_path: &Path,
    expected_controller_text: &str,
    expected_module_sha256: &str,
    controller_mode: &str,
) -> Result<(), String> {
    if !valid_sha256(expected_module_sha256) {
        return Err("expected production module SHA-256 is invalid".into());
    }
    let mut profile: Profile = read_json(profile_path)?;
    let bridge =
        Principal::from_text(&profile.bridge_canister_id).map_err(|error| error.to_string())?;
    let expected_controller =
        Principal::from_text(expected_controller_text).map_err(|error| error.to_string())?;
    let root = Principal::from_text(KINIC_ROOT).map_err(|error| error.to_string())?;
    let expected_controllers = match controller_mode {
        "sole" => BTreeSet::from([expected_controller]),
        "joint" => BTreeSet::from([expected_controller, root]),
        _ => return Err("controller mode must be sole or joint".into()),
    };
    let agent = mainnet_agent(&profile.ic_host, false)?;
    let (
        lifecycle_raw,
        attestation_raw,
        activation_status_raw,
        runtime_raw,
        status_raw,
        pending_raw,
        history_raw,
        root_raw,
        controllers,
        module_hash,
    ) = async_runtime()?.block_on(async {
        let empty = Encode!().map_err(|error| error.to_string())?;
        let lifecycle = agent
            .query(&bridge, "get_production_lifecycle")
            .with_arg(empty.clone())
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let attestation = agent
            .query(&bridge, "get_activation_attestation")
            .with_arg(empty.clone())
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let activation_status = agent
            .query(&bridge, "get_activation_status")
            .with_arg(empty.clone())
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
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
        let pending = agent
            .query(&bridge, "get_pending_base_governance_transaction")
            .with_arg(empty)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let history = agent
            .query(&bridge, "list_withdrawals")
            .with_arg(
                Encode!(&ProductionUiHistoryProbe {
                    requester: vec![0; 20],
                    before_cursor: None,
                    limit: 1,
                })
                .map_err(|error| error.to_string())?,
            )
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let root_canister = Principal::from_text(KINIC_ROOT).map_err(|error| error.to_string())?;
        let root = agent
            .query(&root_canister, "list_sns_canisters")
            .with_arg(Encode!(&SnsRootQuery {}).map_err(|error| error.to_string())?)
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
            activation_status,
            runtime,
            status,
            pending,
            history,
            root,
            controllers,
            module_hash,
        ))
    })?;
    let ProductionLifecycleResultView::Ok(lifecycle) =
        Decode!(&lifecycle_raw, ProductionLifecycleResultView)
            .map_err(|error| error.to_string())?
    else {
        return Err("authenticated production lifecycle is unavailable".into());
    };
    let ActivationAttestationResultView::Ok(attestation) =
        Decode!(&attestation_raw, ActivationAttestationResultView)
            .map_err(|error| error.to_string())?
    else {
        return Err("authenticated activation attestation is unavailable".into());
    };
    let ActivationStatusResultView::Ok(activation_status) =
        Decode!(&activation_status_raw, ActivationStatusResultView)
            .map_err(|error| error.to_string())?
    else {
        return Err("authenticated activation status is unavailable".into());
    };
    let runtime = Decode!(&runtime_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let pending = Decode!(&pending_raw, PendingGovernanceTransactionsView)
        .map_err(|error| error.to_string())?;
    let root_state = Decode!(&root_raw, SnsCanistersView).map_err(|error| error.to_string())?;
    let history_ready = matches!(
        Decode!(&history_raw, ProductionUiHistoryResult).map_err(|error| error.to_string())?,
        ProductionUiHistoryResult::Ok(_)
    );
    let storage_ok = matches!(
        production_installer_storage_integrity(bridge, expected_controller)?,
        StorageIntegrityResultView::Ok(ref value) if value == "ok"
    );
    validate_production_current_state_core(
        &controllers,
        &expected_controllers,
        &module_hash,
        expected_module_sha256,
        lifecycle,
        &activation_status,
        &runtime,
        &status,
        &pending,
        history_ready,
        root_state.dapps.contains(&bridge),
        storage_ok,
    )?;
    profile.canister_schema_version = CURRENT_STABLE_SCHEMA_VERSION;
    profile.bridge_canister_wasm_sha256 = expected_module_sha256.to_ascii_lowercase();
    let rpc_url_hash = hex(&canonical_sha256(
        &profile
            .rpc_providers
            .iter()
            .map(|provider| provider.url.clone())
            .collect::<Vec<_>>(),
    )?);
    let operational_config_sha256 = expected_operational_config_sha256(
        &profile,
        status.mint_authorization_ttl_seconds,
        status.mint_authorization_epoch,
    )?;
    validate_live_runtime_binding(
        &live_runtime_binding_from_view(&runtime),
        &profile,
        &rpc_url_hash,
        &operational_config_sha256,
    )?;
    if runtime.expected_bridge_runtime_sha256
        != decode_hex(&profile.bridge_runtime_bytecode_sha256)?
    {
        return Err("production runtime code binding differs from policy".into());
    }
    validate_activation_attestation_with_pause(
        &profile,
        &attestation,
        0,
        1,
        now_unix()?,
        Some(false),
    )?;
    let operational_response =
        production_installer_query(bridge, expected_controller, "get_operational_config")?;
    let operational_evidence = OperationalEpochEvidence::from_response(
        std::str::from_utf8(&operational_response)
            .map_err(|_| "invalid operational config output")?
            .trim(),
    )?;
    validate_operational_epoch_snapshot(
        &operational_evidence,
        &status,
        &live_runtime_binding_from_view(&runtime),
        profile.parameters.ledger_fee,
    )?;
    println!(
        "production_current_state=verified canister={} module_sha256={} controllers={}",
        profile.bridge_canister_id,
        expected_module_sha256.to_ascii_lowercase(),
        controller_mode
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn validate_production_current_state_core(
    controllers: &[Principal],
    expected_controllers: &BTreeSet<Principal>,
    module_hash: &[u8],
    expected_module_sha256: &str,
    lifecycle: ProductionLifecycleView,
    activation_status: &ActivationStatusView,
    runtime: &RuntimeBindingView,
    status: &BridgeStatusLiveView,
    pending: &PendingGovernanceTransactionsView,
    history_ready: bool,
    registered_with_root: bool,
    storage_ok: bool,
) -> Result<(), String> {
    let observed_controllers = controllers.iter().copied().collect::<BTreeSet<_>>();
    if observed_controllers.len() != controllers.len()
        || &observed_controllers != expected_controllers
        || !hex(module_hash).eq_ignore_ascii_case(expected_module_sha256)
    {
        return Err("certified production controller set or module hash differs".into());
    }
    if lifecycle != ProductionLifecycleView::Activated {
        return Err("production Canister is not Activated".into());
    }
    if activation_status.deposits_paused
        || activation_status.pending_timelock_operation.is_some()
        || !matches!(
            activation_status.last_confirmed_activation.as_ref(),
            Some(last) if last.phase == "execute"
        )
    {
        return Err("production activation is not in a completed active state".into());
    }
    if runtime.schema_version != CURRENT_STABLE_SCHEMA_VERSION
        || status.deposits_paused
        || !status.reserve.sufficient
        || !matches!(pending, PendingGovernanceTransactionsView::Ok(values) if values.is_empty())
        || !history_ready
        || registered_with_root
        || !storage_ok
    {
        return Err("authenticated production state is not ready".into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProductionUiRpcConfig {
    schema_version: u16,
    base_rpc_url: String,
}

fn production_current_ui_runtime_profile(
    bundle_path: &Path,
    module_sha256: &str,
    rpc_config_path: &Path,
) -> Result<(ValidatedBundle, Value), String> {
    if !valid_sha256(module_sha256) {
        return Err("production UI module SHA-256 is invalid".into());
    }
    let mut bundle = validate_historical_gate_b_bundle(bundle_path)?;
    bundle.profile.canister_schema_version = CURRENT_STABLE_SCHEMA_VERSION;
    bundle.profile.bridge_canister_wasm_sha256 = module_sha256.to_ascii_lowercase();
    let profile_bytes = canonical_bytes(&bundle.profile)?;
    let rpc_bytes = fs::read(rpc_config_path).map_err(|error| error.to_string())?;
    let ui = production_current_ui_runtime_profile_value(
        &bundle.profile,
        &profile_bytes,
        &bundle.manifest_sha256,
        module_sha256,
        &rpc_bytes,
    )?;
    Ok((bundle, ui))
}

fn production_current_ui_runtime_profile_value(
    profile: &Profile,
    profile_bytes: &[u8],
    manifest_sha256: &str,
    module_sha256: &str,
    rpc_bytes: &[u8],
) -> Result<Value, String> {
    if profile.canister_schema_version != CURRENT_STABLE_SCHEMA_VERSION {
        return Err("production UI profile must use current schema v36".into());
    }
    let rpc: ProductionUiRpcConfig = serde_json::from_slice(rpc_bytes)
        .map_err(|_| "invalid reviewed production UI RPC configuration")?;
    let key = rpc
        .base_rpc_url
        .strip_prefix("https://base-mainnet.g.alchemy.com/v2/")
        .ok_or("production UI RPC must be the reviewed Base mainnet Alchemy endpoint")?;
    if rpc.schema_version != 1
        || key.is_empty()
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("invalid reviewed production UI RPC configuration".into());
    }
    let mut ui = ui_runtime_profile(profile, profile_bytes, true, Some(manifest_sha256))?;
    let fields = ui
        .as_object_mut()
        .ok_or("UI runtime profile must be an object")?;
    fields.insert("baseRpcUrl".into(), serde_json::json!(rpc.base_rpc_url));
    fields.insert(
        "mintRecoveryUrl".into(),
        serde_json::json!("https://recovery.bridge.kinic.xyz/v1/mint-recovery"),
    );
    fields.insert(
        "canisterSchemaVersion".into(),
        serde_json::json!(CURRENT_STABLE_SCHEMA_VERSION),
    );
    fields.insert(
        "canisterModuleSha256".into(),
        serde_json::json!(module_sha256.to_ascii_lowercase()),
    );
    fields.insert(
        "uiRpcConfigSha256".into(),
        serde_json::json!(hex(&Sha256::digest(rpc_bytes))),
    );
    Ok(ui)
}

fn render_production_current_ui_runtime(
    bundle_path: &Path,
    module_sha256: &str,
    rpc_config_path: &Path,
    output_path: &Path,
) -> Result<(), String> {
    let (_, rendered) =
        production_current_ui_runtime_profile(bundle_path, module_sha256, rpc_config_path)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(output_path)
        .map_err(|error| error.to_string())?
        .write_all(&canonical_bytes(&rendered)?)
        .map_err(|error| error.to_string())?;
    println!("production_ui_runtime=rendered schema=36");
    Ok(())
}

fn verify_production_current_ui_live(
    bundle_path: &Path,
    module_sha256: &str,
    rpc_config_path: &Path,
    runtime_profile_path: &Path,
    controller_mode: &str,
) -> Result<(), String> {
    let (bundle, rendered) =
        production_current_ui_runtime_profile(bundle_path, module_sha256, rpc_config_path)?;
    if fs::read(runtime_profile_path).map_err(|error| error.to_string())?
        != canonical_bytes(&rendered)?
    {
        return Err("supplied UI runtime profile differs from current production state".into());
    }
    verify_production_current_state(
        &bundle.root.join("profile.json"),
        &gate_b_controller(&bundle)?.to_text(),
        module_sha256,
        controller_mode,
    )?;
    println!(
        "production_ui=current-live-pass schema=36 module_sha256={} manifest_sha256={}",
        module_sha256.to_ascii_lowercase(),
        bundle.manifest_sha256
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

fn validate_production_installer_principal(
    expected_installer: Principal,
    principal_output: &[u8],
) -> Result<(), String> {
    let resolved = Principal::from_text(
        std::str::from_utf8(principal_output)
            .map_err(|_| "production installer identity returned non-UTF-8 principal")?
            .trim(),
    )
    .map_err(|_| "production installer identity returned an invalid principal")?;
    if resolved != expected_installer {
        return Err("production installer identity differs from the Gate B sole controller".into());
    }
    Ok(())
}

fn decode_production_storage_integrity(
    response_hex: &[u8],
) -> Result<StorageIntegrityResultView, String> {
    let response_hex = std::str::from_utf8(response_hex)
        .map_err(|_| "production storage integrity query returned non-UTF-8 output")?;
    let response = decode_hex(response_hex.trim())?;
    Decode!(&response, StorageIntegrityResultView)
        .map_err(|error| format!("invalid production storage integrity response: {error}"))
}

fn decode_handover_query(method: &str, path: &Path) -> Result<Value, String> {
    let raw: Value = read_json(path)?;
    let bytes = decode_hex(
        raw.get("response_bytes")
            .and_then(Value::as_str)
            .ok_or("query response lacks raw Candid")?,
    )?;
    macro_rules! decoded {
        ($ty:ty) => {
            serde_json::to_value(Decode!(&bytes, $ty).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        };
    }
    let decoded = match method {
        "get_bridge_status" => decoded!(BridgeStatusLiveView),
        "get_runtime_binding" => decoded!(RuntimeBindingView),
        "get_production_lifecycle" => decoded!(ProductionLifecycleResultView),
        "get_activation_status" => decoded!(ActivationStatusResultView),
        "get_activation_attestation" => decoded!(ActivationAttestationResultView),
        "storage_integrity_check" | "get_release_storage_integrity" => {
            decoded!(StorageIntegrityResultView)
        }
        "get_registration_proposal" => decoded!(GetProposalResponse),
        "list_sns_canisters" => decoded!(SnsCanistersView),
        _ => return Err("unsupported handover query".into()),
    };
    Ok(serde_json::json!({"response_bytes":hex(&bytes), "decoded":decoded}))
}

fn production_installer_storage_integrity(
    bridge: Principal,
    expected_installer: Principal,
) -> Result<StorageIntegrityResultView, String> {
    decode_production_storage_integrity(&production_installer_query(
        bridge,
        expected_installer,
        "storage_integrity_check",
    )?)
}

fn production_installer_query(
    bridge: Principal,
    expected_installer: Principal,
    method: &str,
) -> Result<Vec<u8>, String> {
    if expected_installer.to_text() == KINIC_ROOT {
        let public_method = match method {
            "get_operational_config" => "get_release_operational_config",
            "storage_integrity_check" => "get_release_storage_integrity",
            _ => return Err("unsupported SNS release query".into()),
        };
        let agent = mainnet_agent("https://icp-api.io", false)?;
        let bytes = async_runtime()?.block_on(async {
            agent
                .query(&bridge, public_method)
                .with_arg(Encode!().map_err(|error| error.to_string())?)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())
        })?;
        return Ok(hex(&bytes).into_bytes());
    }
    let identity = env::var("BRIDGE_PRODUCTION_INSTALLER_IDENTITY")
        .map_err(|_| "missing BRIDGE_PRODUCTION_INSTALLER_IDENTITY for controller-authenticated storage integrity query")?;
    if identity.trim().is_empty() {
        return Err("BRIDGE_PRODUCTION_INSTALLER_IDENTITY must not be empty".into());
    }
    let principal_output = Command::new("icp")
        .args(["identity", "principal", "--identity", identity.as_str()])
        .output()
        .map_err(|error| format!("failed to resolve production installer identity: {error}"))?;
    if !principal_output.status.success() {
        return Err("failed to resolve production installer identity".into());
    }
    validate_production_installer_principal(expected_installer, &principal_output.stdout)?;

    let candid =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../canister/bridge-canister/bridge.did");
    let bridge_text = bridge.to_text();
    let integrity_output = Command::new("icp")
        .args([
            "canister",
            "call",
            bridge_text.as_str(),
            method,
            "()",
            "-n",
            "ic",
            "--identity",
            identity.as_str(),
            "--query",
            "--candid",
        ])
        .arg(candid)
        .args(["-o", "hex"])
        .output()
        .map_err(|error| format!("failed to query production storage integrity: {error}"))?;
    if !integrity_output.status.success() {
        return Err("controller-authenticated production storage integrity query failed".into());
    }
    Ok(integrity_output.stdout)
}

fn async_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

fn validate_activation_attestation(
    profile: &Profile,
    attestation: &ActivationAttestationView,
    manifest_created_at_unix: u64,
    minimum_finalized_block: u64,
    now: u64,
) -> Result<(), String> {
    validate_activation_attestation_with_pause(
        profile,
        attestation,
        manifest_created_at_unix,
        minimum_finalized_block,
        now,
        Some(true),
    )
}

fn validate_activation_attestation_with_pause(
    profile: &Profile,
    attestation: &ActivationAttestationView,
    manifest_created_at_unix: u64,
    minimum_finalized_block: u64,
    now: u64,
    expected_paused: Option<bool>,
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
        || expected_paused.is_some_and(|paused| {
            attestation.deposits_paused != paused || attestation.withdrawals_paused != paused
        })
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

fn verify_production_rpc_binding(bundle: &ValidatedBundle) -> Result<(), String> {
    validate_provider_independence_receipt(
        &bundle.root,
        &bundle.manifest,
        &bundle.profile,
        now_unix()?,
    )
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
    validate_current_profile_management_snapshot(
        &bundle.profile,
        installer,
        controllers,
        module_hash,
    )
}

fn validate_dao_activation_management_snapshot(
    bundle: &ValidatedBundle,
    controllers: &[Principal],
    module_hash: &[u8],
) -> Result<(), String> {
    if env::var("BRIDGE_DAO_JOINT_CONTROL").as_deref() != Ok("1") {
        return validate_gate_b_management_snapshot(bundle, controllers, module_hash);
    }
    let installer = gate_b_controller(bundle)?;
    if !dao_activation_controller_set_matches(installer, controllers)?
        || !hex(module_hash).eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
    {
        return Err("DAO activation requires the exact certified production identity and SNS Root controller set".into());
    }
    Ok(())
}

fn dao_activation_controller_set_matches(
    installer: Principal,
    controllers: &[Principal],
) -> Result<bool, String> {
    let expected = BTreeSet::from([
        installer,
        Principal::from_text(KINIC_ROOT).map_err(|error| error.to_string())?,
    ]);
    Ok(controllers.len() == 2 && controllers.iter().copied().collect::<BTreeSet<_>>() == expected)
}

fn validate_current_profile_management_snapshot(
    profile: &Profile,
    installer: Principal,
    controllers: &[Principal],
    module_hash: &[u8],
) -> Result<(), String> {
    if controllers != [installer]
        || !hex(module_hash).eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
    {
        return Err("Gate B requires the production identity as sole controller".into());
    }
    Ok(())
}

fn verify_gate_b_management_state(bundle: &ValidatedBundle) -> Result<(), String> {
    gate_b_management_snapshot(bundle).map(|_| ())
}

fn gate_b_management_snapshot(
    bundle: &ValidatedBundle,
) -> Result<(Vec<Principal>, Vec<u8>), String> {
    let (controllers, module_hash) = live_management_snapshot(bundle)?;
    validate_gate_b_management_snapshot(bundle, &controllers, &module_hash)?;
    Ok((controllers, module_hash))
}

fn live_management_snapshot(bundle: &ValidatedBundle) -> Result<(Vec<Principal>, Vec<u8>), String> {
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
    Ok((controllers, module_hash))
}

fn verify_live(bundle: &ValidatedBundle, expected_deposits_paused: bool) -> Result<(), String> {
    verify_live_inputs(bundle, expected_deposits_paused)?;
    verify_gate_b_management_state(bundle)?;
    verify_activation_attestation_authenticity(bundle)?;
    verify_production_rpc_binding(bundle)
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
        .mode(0o400)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    output
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    output.write_all(b"\n").map_err(|error| error.to_string())?;
    output.sync_all().map_err(|error| error.to_string())?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn operational_config_seal_reservation(
    bundle: &ValidatedBundle,
    expected_gate_hash: &str,
    path: &Path,
) -> Result<bool, String> {
    if !expected_gate_hash.eq_ignore_ascii_case(&bundle.manifest_sha256) {
        return Err("seal reservation Gate B hash differs from the bundle".into());
    }
    let parameters_path = bundle.root.join("initial-operational-parameters.json");
    let parameters_bytes = fs::read(&parameters_path).map_err(|error| error.to_string())?;
    let parameters: InitialOperationalParameters =
        serde_json::from_slice(&parameters_bytes).map_err(|error| error.to_string())?;
    validate_initial_operational_parameters(
        &parameters,
        &bundle.profile,
        bundle.manifest.created_at_unix,
        now_unix()?,
    )?;
    let controller = gate_b_controller(bundle)?.to_text();
    let expected = OperationalConfigSealReservation {
        schema_version: 1,
        release_id: bundle.manifest.release_id.clone(),
        source_revision: bundle.manifest.source_revision.clone(),
        source_tree_sha256: bundle.manifest.source_tree_sha256.clone(),
        gate_b_manifest_sha256: bundle.manifest_sha256.clone(),
        bridge_canister_id: bundle.profile.bridge_canister_id.clone(),
        controller_principal: controller,
        certified_module_sha256: bundle.profile.bridge_canister_wasm_sha256.clone(),
        parameters_sha256: hex(&Sha256::digest(&parameters_bytes)),
        operational_args_sha256: hex(&canonical_sha256(&parameters.derived)?),
        reserved_at_unix: now_unix()?,
    };
    if path.exists() {
        let existing: OperationalConfigSealReservation = read_json(path)?;
        let mut comparable = expected;
        comparable.reserved_at_unix = existing.reserved_at_unix;
        if existing != comparable || existing.reserved_at_unix == 0 {
            return Err("existing seal reservation differs from the reviewed release".into());
        }
        return Ok(false);
    }
    write_json_new(path, &expected)?;
    Ok(true)
}

fn write_operational_config_seal_receipt(
    bundle: &ValidatedBundle,
    reservation_path: &Path,
    attempt_path: Option<&Path>,
    output_path: &Path,
) -> Result<(), String> {
    let reservation_bytes = fs::read(reservation_path).map_err(|error| error.to_string())?;
    let reservation: OperationalConfigSealReservation =
        serde_json::from_slice(&reservation_bytes).map_err(|error| error.to_string())?;
    operational_config_seal_reservation(bundle, &bundle.manifest_sha256, reservation_path)?;
    let parameters_bytes = fs::read(bundle.root.join("initial-operational-parameters.json"))
        .map_err(|error| error.to_string())?;
    let attempt_bytes = if let Some(path) = attempt_path {
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        Some(bytes)
    } else {
        None
    };

    verify_live(bundle, true)?;
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (lifecycle_raw, attestation_raw, runtime_raw, status_raw, pending_raw) =
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
                .with_arg(empty.clone())
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
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
            let pending = agent
                .query(&bridge, "get_pending_base_governance_transaction")
                .with_arg(empty)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((lifecycle, attestation, runtime, status, pending))
        })?;
    let lifecycle = Decode!(&lifecycle_raw, ProductionLifecycleResultView)
        .map_err(|error| error.to_string())?;
    let attestation = Decode!(&attestation_raw, ActivationAttestationResultView)
        .map_err(|error| error.to_string())?;
    let runtime = Decode!(&runtime_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let pending = Decode!(&pending_raw, PendingGovernanceTransactionsView)
        .map_err(|error| error.to_string())?;
    let expected_operational_config_sha256 = expected_operational_config_sha256(
        &bundle.profile,
        status.mint_authorization_ttl_seconds,
        status.mint_authorization_epoch,
    )?;
    if !matches!(
        lifecycle,
        ProductionLifecycleResultView::Ok(ProductionLifecycleView::OperationalConfigSealed)
    ) || !matches!(attestation, ActivationAttestationResultView::Ok(_))
        || !status.deposits_paused
        || !status.reserve.sufficient
        || !matches!(pending, PendingGovernanceTransactionsView::Ok(ref values) if values.is_empty())
        || runtime.operational_config_sha256 != expected_operational_config_sha256
    {
        return Err("live Canister state cannot prove the reviewed operational config seal".into());
    }
    let (controllers, module_hash) = gate_b_management_snapshot(bundle)?;
    if controllers
        .iter()
        .map(Principal::to_text)
        .collect::<Vec<_>>()
        != [reservation.controller_principal.clone()]
        || !hex(&module_hash).eq_ignore_ascii_case(&reservation.certified_module_sha256)
        || reservation.parameters_sha256 != hex(&Sha256::digest(&parameters_bytes))
    {
        return Err("seal receipt management or parameter binding drifted".into());
    }
    let verified_at_unix = now_unix()?;
    let attempt_sha256 = attempt_bytes
        .as_deref()
        .map(|bytes| {
            validate_operational_config_seal_attempt(bytes, &reservation, verified_at_unix)
        })
        .transpose()?;
    let receipt = OperationalConfigSealReceiptEvidence {
        schema_version: 1,
        release_id: reservation.release_id,
        source_revision: reservation.source_revision,
        source_tree_sha256: reservation.source_tree_sha256,
        gate_b_manifest_sha256: reservation.gate_b_manifest_sha256,
        reservation_hex: hex(&reservation_bytes),
        reservation_sha256: hex(&Sha256::digest(&reservation_bytes)),
        parameters_sha256: reservation.parameters_sha256,
        operational_args_sha256: reservation.operational_args_sha256,
        bridge_canister_id: reservation.bridge_canister_id,
        controller_principal: reservation.controller_principal.clone(),
        certified_controller_set: vec![reservation.controller_principal],
        certified_module_sha256: hex(&module_hash),
        expected_operational_config_sha256: hex(&expected_operational_config_sha256),
        observed_operational_config_sha256: hex(&runtime.operational_config_sha256),
        recovered: attempt_path.is_none(),
        attempt_hex: attempt_bytes.as_deref().map(hex),
        attempt_sha256,
        lifecycle_response_hex: hex(&lifecycle_raw),
        lifecycle_response_sha256: hex(&Sha256::digest(&lifecycle_raw)),
        activation_attestation_response_hex: hex(&attestation_raw),
        activation_attestation_response_sha256: hex(&Sha256::digest(&attestation_raw)),
        runtime_binding_response_hex: hex(&runtime_raw),
        runtime_binding_response_sha256: hex(&Sha256::digest(&runtime_raw)),
        bridge_status_response_hex: hex(&status_raw),
        bridge_status_response_sha256: hex(&Sha256::digest(&status_raw)),
        pending_transactions_response_hex: hex(&pending_raw),
        pending_transactions_response_sha256: hex(&Sha256::digest(&pending_raw)),
        verified_at_unix,
    };
    write_json_new(output_path, &receipt)
}

fn validate_operational_config_seal_attempt(
    bytes: &[u8],
    reservation: &OperationalConfigSealReservation,
    verified_at_unix: u64,
) -> Result<String, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let object = value
        .as_object()
        .ok_or("seal attempt must be a JSON object")?;
    let mut keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    keys.sort_unstable();
    let response = object.get("response").and_then(Value::as_object);
    let response_keys = response.map(|value| {
        let mut keys = value.keys().map(String::as_str).collect::<Vec<_>>();
        keys.sort_unstable();
        keys
    });
    let lifecycle = response
        .and_then(|value| value.get("lifecycle"))
        .and_then(Value::as_object);
    let sealed_at_unix = object.get("sealed_at_unix").and_then(Value::as_u64);
    if keys
        != [
            "parameters_sha256",
            "response",
            "schema_version",
            "sealed_at_unix",
        ]
        || object.get("schema_version").and_then(Value::as_u64) != Some(1)
        || object.get("parameters_sha256").and_then(Value::as_str)
            != Some(reservation.parameters_sha256.as_str())
        || response_keys.as_deref() != Some(&["activation_attestation", "lifecycle"])
        || lifecycle.and_then(|value| value.get("OperationalConfigSealed")) != Some(&Value::Null)
        || !sealed_at_unix
            .is_some_and(|value| value >= reservation.reserved_at_unix && value <= verified_at_unix)
    {
        return Err(
            "seal attempt differs from the durable reservation or verified timeline".into(),
        );
    }
    Ok(hex(&Sha256::digest(bytes)))
}

fn validate_historical_evidence_window(
    manifest_created: u64,
    manifest_expires: u64,
    timestamps: &[u64],
) -> Result<(), String> {
    if manifest_expires < manifest_created
        || timestamps
            .iter()
            .any(|timestamp| *timestamp < manifest_created || *timestamp > manifest_expires)
    {
        return Err("historical evidence was created outside the Gate B validity window".into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
enum SealReceiptLiveContext {
    PrePrepare,
    PendingResume,
    ConfirmationInput,
    ScheduleFinalization,
    ExecuteFinalization,
    HistoricalEvidence,
}

fn live_activation_pause_requirement(context: SealReceiptLiveContext) -> Option<bool> {
    match context {
        SealReceiptLiveContext::PrePrepare
        | SealReceiptLiveContext::ConfirmationInput
        | SealReceiptLiveContext::HistoricalEvidence => None,
        SealReceiptLiveContext::PendingResume | SealReceiptLiveContext::ScheduleFinalization => {
            Some(true)
        }
        SealReceiptLiveContext::ExecuteFinalization => Some(false),
    }
}

fn validate_operational_config_seal_receipt(
    bundle: &ValidatedBundle,
    path: &Path,
    live_context: SealReceiptLiveContext,
) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let receipt: OperationalConfigSealReceiptEvidence =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    let reservation_bytes = decode_hex(&receipt.reservation_hex)?;
    if !hex(&Sha256::digest(&reservation_bytes)).eq_ignore_ascii_case(&receipt.reservation_sha256) {
        return Err("seal receipt reservation digest does not match its bytes".into());
    }
    let reservation: OperationalConfigSealReservation =
        serde_json::from_slice(&reservation_bytes).map_err(|error| error.to_string())?;
    if matches!(live_context, SealReceiptLiveContext::HistoricalEvidence) {
        validate_historical_evidence_window(
            bundle.manifest.created_at_unix,
            bundle.manifest.expires_at_unix,
            &[reservation.reserved_at_unix, receipt.verified_at_unix],
        )?;
    }
    let parameters_bytes = fs::read(bundle.root.join("initial-operational-parameters.json"))
        .map_err(|error| error.to_string())?;
    let parameters: InitialOperationalParameters =
        serde_json::from_slice(&parameters_bytes).map_err(|error| error.to_string())?;
    if matches!(live_context, SealReceiptLiveContext::HistoricalEvidence) {
        validate_initial_operational_parameter_lineage(
            &parameters,
            &bundle.profile,
            bundle.manifest.created_at_unix,
        )?;
    } else {
        validate_initial_operational_parameters(
            &parameters,
            &bundle.profile,
            bundle.manifest.created_at_unix,
            now_unix()?,
        )?;
    }
    let controller = gate_b_controller(bundle)?.to_text();
    if reservation.schema_version != 1
        || reservation.release_id != bundle.manifest.release_id
        || reservation.source_revision != bundle.manifest.source_revision
        || !reservation
            .source_tree_sha256
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || !reservation
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(&bundle.manifest_sha256)
        || reservation.bridge_canister_id != bundle.profile.bridge_canister_id
        || reservation.controller_principal != controller
        || !reservation
            .certified_module_sha256
            .eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
        || reservation.parameters_sha256 != hex(&Sha256::digest(&parameters_bytes))
        || reservation.operational_args_sha256 != hex(&canonical_sha256(&parameters.derived)?)
        || reservation.reserved_at_unix == 0
        || receipt.schema_version != 1
        || receipt.release_id != reservation.release_id
        || receipt.source_revision != reservation.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&reservation.source_tree_sha256)
        || !receipt
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(&reservation.gate_b_manifest_sha256)
        || receipt.parameters_sha256 != reservation.parameters_sha256
        || receipt.operational_args_sha256 != reservation.operational_args_sha256
        || receipt.bridge_canister_id != reservation.bridge_canister_id
        || receipt.controller_principal != reservation.controller_principal
        || receipt.certified_controller_set != [reservation.controller_principal.clone()]
        || !receipt
            .certified_module_sha256
            .eq_ignore_ascii_case(&reservation.certified_module_sha256)
        || receipt.recovered != receipt.attempt_sha256.is_none()
        || receipt.recovered != receipt.attempt_hex.is_none()
        || receipt.verified_at_unix < reservation.reserved_at_unix
        || receipt.verified_at_unix > now_unix()?
    {
        return Err("operational config seal receipt is not bound to this Gate B release".into());
    }
    if let (Some(attempt_hex), Some(expected_attempt_sha256)) =
        (&receipt.attempt_hex, &receipt.attempt_sha256)
    {
        let attempt_bytes = decode_hex(attempt_hex)?;
        let actual_attempt_sha256 = validate_operational_config_seal_attempt(
            &attempt_bytes,
            &reservation,
            receipt.verified_at_unix,
        )?;
        if !actual_attempt_sha256.eq_ignore_ascii_case(expected_attempt_sha256) {
            return Err("seal receipt attempt digest does not match its bytes".into());
        }
    }
    let raw_fields = [
        (
            &receipt.lifecycle_response_hex,
            &receipt.lifecycle_response_sha256,
        ),
        (
            &receipt.activation_attestation_response_hex,
            &receipt.activation_attestation_response_sha256,
        ),
        (
            &receipt.runtime_binding_response_hex,
            &receipt.runtime_binding_response_sha256,
        ),
        (
            &receipt.bridge_status_response_hex,
            &receipt.bridge_status_response_sha256,
        ),
        (
            &receipt.pending_transactions_response_hex,
            &receipt.pending_transactions_response_sha256,
        ),
    ];
    for (raw, digest) in raw_fields {
        if !activation_raw_digest_matches(raw, digest)? {
            return Err("operational config seal receipt raw response digest drifted".into());
        }
    }
    let lifecycle_raw = decode_hex(&receipt.lifecycle_response_hex)?;
    let attestation_raw = decode_hex(&receipt.activation_attestation_response_hex)?;
    let runtime_raw = decode_hex(&receipt.runtime_binding_response_hex)?;
    let status_raw = decode_hex(&receipt.bridge_status_response_hex)?;
    let pending_raw = decode_hex(&receipt.pending_transactions_response_hex)?;
    let lifecycle = Decode!(&lifecycle_raw, ProductionLifecycleResultView)
        .map_err(|error| error.to_string())?;
    let attestation = Decode!(&attestation_raw, ActivationAttestationResultView)
        .map_err(|error| error.to_string())?;
    let runtime = Decode!(&runtime_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let pending = Decode!(&pending_raw, PendingGovernanceTransactionsView)
        .map_err(|error| error.to_string())?;
    let ActivationAttestationResultView::Ok(attestation) = attestation else {
        return Err("seal receipt has no activation attestation".into());
    };
    let gate_a: GateAReceipt = read_json(&bundle.root.join("gate-a-receipt.json"))?;
    validate_activation_attestation(
        &bundle.profile,
        &attestation,
        bundle.manifest.created_at_unix,
        gate_a
            .bridge_deployment_block_number
            .max(gate_a.timelock_deployment_block_number),
        receipt.verified_at_unix,
    )?;
    let historical_expected_operational_config_sha256 = expected_operational_config_sha256(
        &bundle.profile,
        status.mint_authorization_ttl_seconds,
        status.mint_authorization_epoch,
    )?;
    if !matches!(
        lifecycle,
        ProductionLifecycleResultView::Ok(ProductionLifecycleView::OperationalConfigSealed)
    ) || !status.deposits_paused
        || !status.reserve.sufficient
        || !matches!(pending, PendingGovernanceTransactionsView::Ok(ref values) if values.is_empty())
        || runtime.operational_config_sha256 != historical_expected_operational_config_sha256
        || !receipt
            .expected_operational_config_sha256
            .eq_ignore_ascii_case(&hex(&historical_expected_operational_config_sha256))
        || !receipt
            .observed_operational_config_sha256
            .eq_ignore_ascii_case(&hex(&runtime.operational_config_sha256))
    {
        return Err("operational config seal receipt contains unsafe live state".into());
    }
    // Candidate generation validates historical observations at receipt time.
    // It grants no live authorization; production/UI paths still query current state.
    if matches!(live_context, SealReceiptLiveContext::HistoricalEvidence) {
        return Ok(hex(&Sha256::digest(&bytes)));
    }
    if matches!(live_context, SealReceiptLiveContext::PrePrepare) {
        verify_live(bundle, true)?;
    } else if let Some(expected_paused) = live_activation_pause_requirement(live_context) {
        let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
            .map_err(|error| error.to_string())?;
        let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
        let (live_lifecycle_raw, live_attestation_raw, live_runtime_raw, live_status_raw) =
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
                    .with_arg(empty.clone())
                    .call_with_verification()
                    .await
                    .map_err(|error| error.to_string())?;
                let runtime = agent
                    .query(&bridge, "get_runtime_binding")
                    .with_arg(empty.clone())
                    .call_with_verification()
                    .await
                    .map_err(|error| error.to_string())?;
                let status = agent
                    .query(&bridge, "get_bridge_status")
                    .with_arg(empty)
                    .call_with_verification()
                    .await
                    .map_err(|error| error.to_string())?;
                Ok::<_, String>((lifecycle, attestation, runtime, status))
            })?;
        let live_lifecycle = Decode!(&live_lifecycle_raw, ProductionLifecycleResultView)
            .map_err(|error| error.to_string())?;
        let live_attestation = Decode!(&live_attestation_raw, ActivationAttestationResultView)
            .map_err(|error| error.to_string())?;
        let live_runtime =
            Decode!(&live_runtime_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
        let live_status =
            Decode!(&live_status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
        let ActivationAttestationResultView::Ok(live_attestation) = live_attestation else {
            return Err("live activation attestation is unavailable for seal recovery".into());
        };
        validate_activation_attestation_with_pause(
            &bundle.profile,
            &live_attestation,
            bundle.manifest.created_at_unix,
            gate_a
                .bridge_deployment_block_number
                .max(gate_a.timelock_deployment_block_number),
            now_unix()?,
            Some(expected_paused),
        )?;
        let live_expected_digest = expected_operational_config_sha256(
            &bundle.profile,
            live_status.mint_authorization_ttl_seconds,
            live_status.mint_authorization_epoch,
        )?;
        let lifecycle_matches = match live_context {
            SealReceiptLiveContext::PendingResume
            | SealReceiptLiveContext::ScheduleFinalization => {
                matches!(
                    live_lifecycle,
                    ProductionLifecycleResultView::Ok(
                        ProductionLifecycleView::OperationalConfigSealed
                    )
                ) && live_status.deposits_paused
            }
            SealReceiptLiveContext::ExecuteFinalization => {
                matches!(
                    live_lifecycle,
                    ProductionLifecycleResultView::Ok(ProductionLifecycleView::Activated)
                ) && !live_status.deposits_paused
            }
            SealReceiptLiveContext::PrePrepare
            | SealReceiptLiveContext::ConfirmationInput
            | SealReceiptLiveContext::HistoricalEvidence => {
                unreachable!()
            }
        };
        if !lifecycle_matches
            || !live_status.reserve.sufficient
            || live_runtime.operational_config_sha256 != live_expected_digest
        {
            return Err("live activation phase is inconsistent with the seal receipt".into());
        }
    }
    let (controllers, module_hash) = gate_b_management_snapshot(bundle)?;
    if controllers
        .iter()
        .map(Principal::to_text)
        .collect::<Vec<_>>()
        != receipt.certified_controller_set
        || !hex(&module_hash).eq_ignore_ascii_case(&receipt.certified_module_sha256)
    {
        return Err("live management state differs from the seal receipt".into());
    }
    Ok(hex(&Sha256::digest(&bytes)))
}

fn activation_raw_digest_matches(raw: &str, digest: &str) -> Result<bool, String> {
    Ok(valid_sha256(digest) && hex(&Sha256::digest(decode_hex(raw)?)).eq_ignore_ascii_case(digest))
}

fn validate_controller_activation_authorization(
    phase: &str,
    bundle: &ValidatedBundle,
    expected_gate_hash: &str,
    expected_seal_receipt_hash: &str,
    receipt: &ControllerActivationAuthorizationReceipt,
    freshness: ActivationReceiptFreshness,
) -> Result<(), String> {
    let installer = gate_b_controller(bundle)?;
    let now = now_unix()?;
    if !controller_activation_authorization_fields_match(
        receipt,
        phase,
        &bundle.manifest.release_id,
        &bundle.manifest.source_revision,
        &bundle.manifest.source_tree_sha256,
        expected_gate_hash,
        expected_seal_receipt_hash,
        &installer.to_text(),
        &bundle.profile.bridge_canister_wasm_sha256,
    ) || !expected_gate_hash.eq_ignore_ascii_case(&bundle.manifest_sha256)
        || match freshness {
            ActivationReceiptFreshness::Current => validate_activation_time(
                receipt.authorized_at_unix,
                bundle.manifest.created_at_unix,
                now,
            )
            .is_err(),
            ActivationReceiptFreshness::Historical => validate_historical_evidence_window(
                bundle.manifest.created_at_unix,
                bundle.manifest.expires_at_unix,
                &[receipt.authorized_at_unix],
            )
            .is_err(),
        }
    {
        return Err("controller activation authorization is not bound to this live Gate B".into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn controller_activation_authorization_fields_match(
    receipt: &ControllerActivationAuthorizationReceipt,
    phase: &str,
    release_id: &str,
    source_revision: &str,
    source_tree_sha256: &str,
    gate_hash: &str,
    seal_receipt_hash: &str,
    controller: &str,
    module_sha256: &str,
) -> bool {
    (phase == "schedule" || phase == "execute")
        && valid_sha256(gate_hash)
        && receipt.schema_version == 1
        && receipt.phase == phase
        && receipt.release_id == release_id
        && receipt.source_revision == source_revision
        && receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(source_tree_sha256)
        && receipt
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(gate_hash)
        && valid_sha256(seal_receipt_hash)
        && receipt
            .operational_config_seal_receipt_sha256
            .eq_ignore_ascii_case(seal_receipt_hash)
        && receipt.controller_principal == controller
        && receipt.certified_controller_set == [controller]
        && receipt
            .certified_module_sha256
            .eq_ignore_ascii_case(module_sha256)
}

fn validate_controller_activation_timeline(
    manifest_created: u64,
    authorized: u64,
    bound: u64,
    confirmed: u64,
    verified: u64,
    now: u64,
) -> Result<(), String> {
    validate_activation_time(authorized, manifest_created, now)?;
    if bound < authorized || confirmed < bound || verified < confirmed || verified > now {
        return Err("controller activation receipt timestamps are out of order".into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
#[cfg_attr(not(test), allow(dead_code))]
enum ActivationReceiptFreshness {
    Current,
    Historical,
}

fn validate_controller_activation_receipt_timeline(
    freshness: ActivationReceiptFreshness,
    manifest_window: (u64, u64),
    authorized: u64,
    bound: u64,
    confirmed: u64,
    verified: u64,
    now: u64,
) -> Result<(), String> {
    let (manifest_created, manifest_expires) = manifest_window;
    if matches!(freshness, ActivationReceiptFreshness::Current) {
        return validate_controller_activation_timeline(
            manifest_created,
            authorized,
            bound,
            confirmed,
            verified,
            now,
        );
    }
    if validate_historical_evidence_window(
        manifest_created,
        manifest_expires,
        &[authorized, bound, confirmed, verified],
    )
    .is_err()
        || bound < authorized
        || confirmed < bound
        || verified < confirmed
    {
        return Err("historical controller activation receipt timestamps are out of order".into());
    }
    Ok(())
}

fn controller_activation_prepare_fields_match(
    receipt: &ControllerActivationPrepareReceipt,
    phase: &str,
    gate_hash: &str,
    artifact_sha256: &str,
    authorization_sha256: &str,
) -> bool {
    receipt.schema_version == 1
        && receipt.phase == phase
        && receipt
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(gate_hash)
        && receipt
            .artifact_sha256
            .eq_ignore_ascii_case(artifact_sha256)
        && receipt
            .authorization_receipt_sha256
            .eq_ignore_ascii_case(authorization_sha256)
        && receipt.bound_at_unix > 0
}

fn controller_activation_authorization(
    phase: &str,
    bundle: &ValidatedBundle,
    expected_gate_hash: &str,
    seal_receipt_path: &Path,
    output: Option<&Path>,
    existing: Option<&Path>,
) -> Result<(), String> {
    let seal_receipt_sha256 = validate_operational_config_seal_receipt(
        bundle,
        seal_receipt_path,
        if existing.is_some() {
            SealReceiptLiveContext::PendingResume
        } else {
            SealReceiptLiveContext::PrePrepare
        },
    )?;
    let receipt = if let Some(path) = existing {
        read_json(path)?
    } else {
        // Establish the authorization time before the final pending-transaction
        // observation. Any transaction already signed before authorization is
        // therefore visible to `verify_live` and rejected; a transaction signed
        // after that observation is ordered after this authorization.
        let authorized_at_unix = now_unix()?;
        verify_live(bundle, true)?;
        let (controllers, module_hash) = gate_b_management_snapshot(bundle)?;
        let installer = gate_b_controller(bundle)?;
        ControllerActivationAuthorizationReceipt {
            schema_version: 1,
            phase: phase.into(),
            release_id: bundle.manifest.release_id.clone(),
            source_revision: bundle.manifest.source_revision.clone(),
            source_tree_sha256: bundle.manifest.source_tree_sha256.clone(),
            gate_b_manifest_sha256: expected_gate_hash.to_ascii_lowercase(),
            operational_config_seal_receipt_sha256: seal_receipt_sha256.clone(),
            controller_principal: installer.to_text(),
            certified_controller_set: controllers.iter().map(Principal::to_text).collect(),
            certified_module_sha256: hex(&module_hash),
            authorized_at_unix,
        }
    };
    validate_controller_activation_authorization(
        phase,
        bundle,
        expected_gate_hash,
        &seal_receipt_sha256,
        &receipt,
        ActivationReceiptFreshness::Current,
    )?;
    gate_b_management_snapshot(bundle)?;
    if let Some(path) = output {
        write_json_new(path, &receipt)?;
    }
    Ok(())
}

fn verify_controller_activation_authorization_fresh(
    phase: &str,
    bundle: &ValidatedBundle,
    expected_gate_hash: &str,
    seal_receipt_path: &Path,
    path: &Path,
) -> Result<(), String> {
    controller_activation_authorization(
        phase,
        bundle,
        expected_gate_hash,
        seal_receipt_path,
        None,
        Some(path),
    )?;
    let receipt: ControllerActivationAuthorizationReceipt = read_json(path)?;
    let now = now_unix()?;
    if !controller_activation_authorization_is_fresh(receipt.authorized_at_unix, now) {
        return Err("controller activation authorization is too old for a new prepare".into());
    }
    Ok(())
}

fn controller_activation_authorization_is_fresh(authorized_at_unix: u64, now: u64) -> bool {
    authorized_at_unix <= now && now - authorized_at_unix <= MAX_ACTIVATION_ATTESTATION_AGE_SECS
}

fn direct_activation_operation<'a>(
    phase: &str,
    kind: &'a DirectActivationKind,
) -> Result<&'a DirectActivationOperation, String> {
    match (phase, kind) {
        ("schedule", DirectActivationKind::ScheduleActivation(operation))
        | ("execute", DirectActivationKind::ExecuteActivation(operation)) => Ok(operation),
        ("schedule" | "execute", _) => Err("fixed artifact has the wrong activation phase".into()),
        _ => Err("activation phase must be schedule or execute".into()),
    }
}

#[allow(clippy::too_many_arguments)]
fn verify_controller_activation_artifact_binding(
    phase: &str,
    bundle: &ValidatedBundle,
    artifact_path: &Path,
    seal_receipt_path: &Path,
    authorization_path: &Path,
    prepare_receipt_path: &Path,
    prior_path: Option<&Path>,
    live_context: SealReceiptLiveContext,
) -> Result<(), String> {
    if phase != "schedule" && phase != "execute" {
        return Err("activation phase must be schedule or execute".into());
    }
    let artifact_bytes = fs::read(artifact_path).map_err(|error| error.to_string())?;
    let artifact: DirectActivationArtifact =
        serde_json::from_slice(&artifact_bytes).map_err(|error| error.to_string())?;
    let artifact_sha256 = hex(&Sha256::digest(&artifact_bytes));
    let authorization_bytes = fs::read(authorization_path).map_err(|error| error.to_string())?;
    let authorization: ControllerActivationAuthorizationReceipt =
        serde_json::from_slice(&authorization_bytes).map_err(|error| error.to_string())?;
    let seal_receipt_sha256 =
        validate_operational_config_seal_receipt(bundle, seal_receipt_path, live_context)?;
    validate_controller_activation_authorization(
        phase,
        bundle,
        &bundle.manifest_sha256,
        &seal_receipt_sha256,
        &authorization,
        ActivationReceiptFreshness::Current,
    )?;
    let prepare_receipt: ControllerActivationPrepareReceipt = read_json(prepare_receipt_path)?;
    let now = now_unix()?;
    if authorization.authorized_at_unix > prepare_receipt.bound_at_unix
        || prepare_receipt.bound_at_unix > now
        || !controller_activation_prepare_fields_match(
            &prepare_receipt,
            phase,
            &bundle.manifest_sha256,
            &artifact_sha256,
            &hex(&Sha256::digest(&authorization_bytes)),
        )
    {
        return Err("activation artifact is not bound to its Gate B authorization".into());
    }
    let operation = direct_activation_operation(phase, &artifact.kind)?;
    let timelock_operation_id = operation.operation_id.as_str();
    let operation_salt = operation.salt.as_str();
    let governance_operation_id = artifact
        .operation_id
        .parse::<u64>()
        .map_err(|_| "invalid governance operation ID")?;
    if !valid_hash32(timelock_operation_id)
        || !valid_hash32(operation_salt)
        || !valid_hash32(&artifact.transaction_hash)
    {
        return Err("fixed activation artifact contains a malformed hash".into());
    }
    validate_direct_activation_transaction_fields(
        phase,
        &bundle.profile,
        operation_salt,
        &artifact,
    )?;
    match (phase, prior_path) {
        ("schedule", None) => {
            let (expected_governance_operation_id, expected_operation_id, expected_salt) =
                gate_b_initial_activation_binding(bundle, ActivationReceiptFreshness::Current)?;
            if governance_operation_id != expected_governance_operation_id
                || !timelock_operation_id
                    .eq_ignore_ascii_case(&format!("0x{}", hex(&expected_operation_id)))
                || !operation_salt.eq_ignore_ascii_case(&format!("0x{}", hex(&expected_salt)))
            {
                return Err(
                    "controller schedule artifact differs from the Gate B activation binding"
                        .into(),
                );
            }
        }
        ("schedule", Some(_)) => return Err("schedule forbids a prior receipt".into()),
        ("execute", Some(path)) => {
            let receipt: ControllerActivationReceipt = read_json(path)?;
            let (schedule_governance_operation_id, _) = validate_controller_schedule_receipt(
                bundle,
                &receipt,
                &seal_receipt_sha256,
                ActivationReceiptFreshness::Current,
            )?;
            if !receipt
                .timelock_operation_id
                .eq_ignore_ascii_case(timelock_operation_id)
                || !receipt.operation_salt.eq_ignore_ascii_case(operation_salt)
                || governance_operation_id <= schedule_governance_operation_id
            {
                return Err("execute artifact is not bound to the prior schedule".into());
            }
        }
        ("execute", None) => return Err("execute requires the controller schedule receipt".into()),
        _ => unreachable!(),
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum RlpItem<'a> {
    Bytes(&'a [u8]),
    List(&'a [u8]),
}

fn rlp_length(bytes: &[u8]) -> Result<usize, String> {
    if bytes.is_empty() || bytes[0] == 0 {
        return Err("signed activation transaction has non-canonical RLP length".into());
    }
    bytes.iter().try_fold(0usize, |value, byte| {
        value
            .checked_mul(256)
            .and_then(|value| value.checked_add(usize::from(*byte)))
            .ok_or_else(|| "signed activation transaction has oversized RLP length".into())
    })
}

fn decode_rlp_item(input: &[u8]) -> Result<(RlpItem<'_>, usize), String> {
    let first = *input
        .first()
        .ok_or("signed activation transaction has truncated RLP")?;
    match first {
        0x00..=0x7f => Ok((RlpItem::Bytes(&input[..1]), 1)),
        0x80..=0xb7 => {
            let length = usize::from(first - 0x80);
            let end = 1usize
                .checked_add(length)
                .ok_or("signed activation transaction has oversized RLP bytes")?;
            let value = input
                .get(1..end)
                .ok_or("signed activation transaction has truncated RLP bytes")?;
            if length == 1 && value[0] < 0x80 {
                return Err("signed activation transaction has non-canonical RLP bytes".into());
            }
            Ok((RlpItem::Bytes(value), end))
        }
        0xb8..=0xbf => {
            let length_of_length = usize::from(first - 0xb7);
            let prefix_end = 1usize
                .checked_add(length_of_length)
                .ok_or("signed activation transaction has oversized RLP prefix")?;
            let length = rlp_length(
                input
                    .get(1..prefix_end)
                    .ok_or("signed activation transaction has truncated RLP length")?,
            )?;
            if length < 56 {
                return Err(
                    "signed activation transaction has non-canonical long RLP bytes".into(),
                );
            }
            let end = prefix_end
                .checked_add(length)
                .ok_or("signed activation transaction has oversized RLP bytes")?;
            Ok((
                RlpItem::Bytes(
                    input
                        .get(prefix_end..end)
                        .ok_or("signed activation transaction has truncated RLP bytes")?,
                ),
                end,
            ))
        }
        0xc0..=0xf7 => {
            let length = usize::from(first - 0xc0);
            let end = 1usize
                .checked_add(length)
                .ok_or("signed activation transaction has oversized RLP list")?;
            Ok((
                RlpItem::List(
                    input
                        .get(1..end)
                        .ok_or("signed activation transaction has truncated RLP list")?,
                ),
                end,
            ))
        }
        0xf8..=0xff => {
            let length_of_length = usize::from(first - 0xf7);
            let prefix_end = 1usize
                .checked_add(length_of_length)
                .ok_or("signed activation transaction has oversized RLP prefix")?;
            let length = rlp_length(
                input
                    .get(1..prefix_end)
                    .ok_or("signed activation transaction has truncated RLP length")?,
            )?;
            if length < 56 {
                return Err("signed activation transaction has non-canonical long RLP list".into());
            }
            let end = prefix_end
                .checked_add(length)
                .ok_or("signed activation transaction has oversized RLP list")?;
            Ok((
                RlpItem::List(
                    input
                        .get(prefix_end..end)
                        .ok_or("signed activation transaction has truncated RLP list")?,
                ),
                end,
            ))
        }
    }
}

fn rlp_uint(item: RlpItem<'_>, maximum_bytes: usize) -> Result<u128, String> {
    let RlpItem::Bytes(bytes) = item else {
        return Err("signed activation transaction integer is an RLP list".into());
    };
    if bytes.len() > maximum_bytes || bytes.first() == Some(&0) {
        return Err("signed activation transaction has a non-canonical integer".into());
    }
    Ok(bytes
        .iter()
        .fold(0u128, |value, byte| (value << 8) | u128::from(*byte)))
}

fn rlp_bytes(item: RlpItem<'_>) -> Result<&[u8], String> {
    match item {
        RlpItem::Bytes(bytes) => Ok(bytes),
        RlpItem::List(_) => Err("signed activation transaction field is an RLP list".into()),
    }
}

fn encode_rlp_list_payload(payload: &[u8]) -> Vec<u8> {
    if payload.len() <= 55 {
        let mut encoded = Vec::with_capacity(payload.len() + 1);
        encoded.push(0xc0 + payload.len() as u8);
        encoded.extend_from_slice(payload);
        return encoded;
    }
    let length = payload.len().to_be_bytes();
    let first = length
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(length.len() - 1);
    let length = &length[first..];
    let mut encoded = Vec::with_capacity(payload.len() + length.len() + 1);
    encoded.push(0xf7 + length.len() as u8);
    encoded.extend_from_slice(length);
    encoded.extend_from_slice(payload);
    encoded
}

#[cfg(test)]
fn encode_rlp_bytes(value: &[u8]) -> Vec<u8> {
    if value.len() == 1 && value[0] < 0x80 {
        return value.to_vec();
    }
    if value.len() <= 55 {
        let mut encoded = Vec::with_capacity(value.len() + 1);
        encoded.push(0x80 + value.len() as u8);
        encoded.extend_from_slice(value);
        return encoded;
    }
    let length = value.len().to_be_bytes();
    let first = length
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(length.len() - 1);
    let length = &length[first..];
    let mut encoded = Vec::with_capacity(value.len() + length.len() + 1);
    encoded.push(0xb7 + length.len() as u8);
    encoded.extend_from_slice(length);
    encoded.extend_from_slice(value);
    encoded
}

#[cfg(test)]
fn encode_rlp_uint(value: u128) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let first = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    encode_rlp_bytes(&bytes[first..])
}

fn parse_decimal_u128(value: &str, field: &str) -> Result<u128, String> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(format!("signed activation artifact has invalid {field}"));
    }
    value
        .parse()
        .map_err(|_| format!("signed activation artifact has invalid {field}"))
}

fn validate_signed_eip1559_artifact(artifact: &DirectActivationArtifact) -> Result<(), String> {
    let raw = decode_hex(&artifact.raw_transaction)?;
    if raw.first() != Some(&0x02) {
        return Err("signed activation transaction is not EIP-1559".into());
    }
    let (outer, consumed) = decode_rlp_item(&raw[1..])?;
    if consumed != raw.len() - 1 {
        return Err("signed activation transaction has trailing bytes".into());
    }
    let RlpItem::List(payload) = outer else {
        return Err("signed activation transaction payload is not an RLP list".into());
    };
    let mut fields = Vec::with_capacity(12);
    let mut encoded_fields = Vec::with_capacity(12);
    let mut offset = 0usize;
    while offset < payload.len() {
        let (field, used) = decode_rlp_item(&payload[offset..])?;
        fields.push(field);
        encoded_fields.push(&payload[offset..offset + used]);
        offset += used;
    }
    if fields.len() != 12 {
        return Err("signed activation transaction must contain exactly 12 fields".into());
    }
    let chain_id = rlp_uint(fields[0], 8)?;
    let nonce = rlp_uint(fields[1], 8)?;
    let max_priority_fee_per_gas = rlp_uint(fields[2], 16)?;
    let max_fee_per_gas = rlp_uint(fields[3], 16)?;
    let gas_limit = rlp_uint(fields[4], 16)?;
    let target = rlp_bytes(fields[5])?;
    if target.len() != 20 {
        return Err("signed activation transaction target is not 20 bytes".into());
    }
    if rlp_uint(fields[6], 16)? != 0 || !rlp_bytes(fields[6])?.is_empty() {
        return Err("signed activation transaction value must be zero".into());
    }
    let calldata = rlp_bytes(fields[7])?;
    if !matches!(fields[8], RlpItem::List(items) if items.is_empty()) {
        return Err("signed activation transaction access list must be empty".into());
    }
    let parity = rlp_uint(fields[9], 1)?;
    if parity > 1 {
        return Err("signed activation transaction has invalid recovery parity".into());
    }
    let r = rlp_bytes(fields[10])?;
    let s = rlp_bytes(fields[11])?;
    if r.is_empty()
        || r.len() > 32
        || r.first() == Some(&0)
        || s.is_empty()
        || s.len() > 32
        || s.first() == Some(&0)
    {
        return Err("signed activation transaction has invalid signature scalars".into());
    }
    let mut signature_bytes = [0u8; 64];
    signature_bytes[32 - r.len()..32].copy_from_slice(r);
    signature_bytes[64 - s.len()..].copy_from_slice(s);
    let signature = Secp256k1Signature::from_slice(&signature_bytes)
        .map_err(|_| "signed activation transaction has an invalid signature")?;
    if signature.normalize_s().is_some() {
        return Err("signed activation transaction signature is not low-s".into());
    }
    let mut unsigned_payload = Vec::new();
    for encoded in &encoded_fields[..9] {
        unsigned_payload.extend_from_slice(encoded);
    }
    let mut unsigned = vec![0x02];
    unsigned.extend_from_slice(&encode_rlp_list_payload(&unsigned_payload));
    let recovered = VerifyingKey::recover_from_prehash(
        &keccak256(&unsigned),
        &signature,
        RecoveryId::new(parity == 1, false),
    )
    .map_err(|_| "signed activation transaction sender recovery failed")?;
    let public_key = recovered.to_encoded_point(false);
    let recovered_hash = keccak256(&public_key.as_bytes()[1..]);
    let recovered_sender = &recovered_hash[12..];

    if keccak256(&raw).as_slice() != decode_hex(&artifact.transaction_hash)?.as_slice()
        || recovered_sender != decode_hex(&artifact.sender)?.as_slice()
        || chain_id != parse_decimal_u128(&artifact.chain_id, "chain ID")?
        || nonce != parse_decimal_u128(&artifact.nonce, "nonce")?
        || target != decode_hex(&artifact.target)?.as_slice()
        || calldata != decode_hex(&artifact.calldata)?.as_slice()
        || gas_limit != parse_decimal_u128(&artifact.gas_limit, "gas limit")?
        || max_fee_per_gas != parse_decimal_u128(&artifact.max_fee_per_gas, "max fee")?
        || max_priority_fee_per_gas
            != parse_decimal_u128(&artifact.max_priority_fee_per_gas, "priority fee")?
        || !(1..=u128::from(u64::MAX)).contains(&parse_decimal_u128(
            &artifact.signed_at_ns,
            "signed timestamp",
        )?)
    {
        return Err("signed activation transaction differs from the artifact fields".into());
    }
    Ok(())
}

fn validate_direct_activation_transaction_fields(
    phase: &str,
    profile: &Profile,
    operation_salt: &str,
    artifact: &DirectActivationArtifact,
) -> Result<(), String> {
    validate_signed_eip1559_artifact(artifact)?;
    let action = match phase {
        "schedule" => "schedule_activation",
        "execute" => "execute_activation",
        _ => return Err("activation phase must be schedule or execute".into()),
    };
    let salt: [u8; 32] = decode_hex(operation_salt)?
        .try_into()
        .map_err(|_| "fixed activation artifact contains a malformed salt")?;
    let expected_calldata = initial_activation_calldata(
        action,
        decode_address(&profile.bridge_contract)?,
        salt,
        profile.timelock.minimum_delay_seconds,
    )?;
    if artifact.chain_id.parse::<u64>().ok() != Some(profile.chain_id)
        || !artifact
            .sender
            .eq_ignore_ascii_case(&profile.governance_operator)
        || !artifact
            .target
            .eq_ignore_ascii_case(&profile.timelock.address)
        || !artifact.calldata.eq_ignore_ascii_case(&expected_calldata)
    {
        return Err(
            "controller activation artifact transaction differs from the release profile".into(),
        );
    }
    Ok(())
}

fn activation_confirmation_artifact_metadata_matches(
    confirmation: &ActivationConfirmationStatusView,
    artifact: &DirectActivationArtifact,
) -> Result<bool, String> {
    let signed_at_ns = u64::try_from(parse_decimal_u128(
        &artifact.signed_at_ns,
        "signed timestamp",
    )?)
    .map_err(|_| "signed timestamp exceeds nat64")?;
    Ok(bridge_core::kernel::confirmed_activation_metadata_matches(
        confirmation.generation,
        confirmation.signed_at_ns,
        artifact.generation,
        signed_at_ns,
    ))
}

#[allow(clippy::too_many_arguments)]
fn verify_controller_activation(
    phase: &str,
    bundle: &ValidatedBundle,
    artifact_path: &Path,
    seal_receipt_path: &Path,
    authorization_path: &Path,
    prepare_receipt_path: &Path,
    confirmation_path: &Path,
    prior_path: Option<&Path>,
    receipt_path: &Path,
) -> Result<(), String> {
    if phase != "schedule" && phase != "execute" {
        return Err("activation phase must be schedule or execute".into());
    }
    let artifact_bytes = fs::read(artifact_path).map_err(|error| error.to_string())?;
    let artifact: DirectActivationArtifact =
        serde_json::from_slice(&artifact_bytes).map_err(|error| error.to_string())?;
    let artifact_sha256 = hex(&Sha256::digest(&artifact_bytes));
    let authorization_bytes = fs::read(authorization_path).map_err(|error| error.to_string())?;
    let authorization: ControllerActivationAuthorizationReceipt =
        serde_json::from_slice(&authorization_bytes).map_err(|error| error.to_string())?;
    let seal_receipt_sha256 = validate_operational_config_seal_receipt(
        bundle,
        seal_receipt_path,
        if phase == "schedule" {
            SealReceiptLiveContext::ScheduleFinalization
        } else {
            SealReceiptLiveContext::ExecuteFinalization
        },
    )?;
    validate_controller_activation_authorization(
        phase,
        bundle,
        &bundle.manifest_sha256,
        &seal_receipt_sha256,
        &authorization,
        ActivationReceiptFreshness::Current,
    )?;
    let authorization_sha256 = hex(&Sha256::digest(&authorization_bytes));
    let prepare_receipt_bytes =
        fs::read(prepare_receipt_path).map_err(|error| error.to_string())?;
    let prepare_receipt: ControllerActivationPrepareReceipt =
        serde_json::from_slice(&prepare_receipt_bytes).map_err(|error| error.to_string())?;
    let confirmation: DirectActivationConfirmationFile = read_json(confirmation_path)?;
    let installer = gate_b_controller(bundle)?;
    let verification_started_at = now_unix()?;
    if validate_controller_activation_timeline(
        bundle.manifest.created_at_unix,
        authorization.authorized_at_unix,
        prepare_receipt.bound_at_unix,
        confirmation.confirmed_at_unix,
        verification_started_at,
        verification_started_at,
    )
    .is_err()
        || !controller_activation_prepare_fields_match(
            &prepare_receipt,
            phase,
            &bundle.manifest_sha256,
            &artifact_sha256,
            &authorization_sha256,
        )
    {
        return Err("activation prepare receipt is not bound to the Gate B artifact".into());
    }
    if confirmation.schema_version != 1
        || !confirmation
            .artifact_sha256
            .eq_ignore_ascii_case(&artifact_sha256)
        || confirmation.operation_id != artifact.operation_id
        || !confirmation
            .transaction_hash
            .eq_ignore_ascii_case(&artifact.transaction_hash)
        || confirmation.response.operation_id != artifact.operation_id
        || !confirmation
            .response
            .transaction_hash
            .eq_ignore_ascii_case(&artifact.transaction_hash)
        || !confirmation.response.succeeded
    {
        return Err("controller activation confirmation differs from the fixed artifact".into());
    }
    let operation = direct_activation_operation(phase, &artifact.kind)?;
    let timelock_operation_id = operation.operation_id.as_str();
    let operation_salt = operation.salt.as_str();
    if !valid_hash32(timelock_operation_id)
        || !valid_hash32(operation_salt)
        || !valid_hash32(&artifact.transaction_hash)
    {
        return Err("fixed activation artifact contains a malformed hash".into());
    }
    validate_direct_activation_transaction_fields(
        phase,
        &bundle.profile,
        operation_salt,
        &artifact,
    )?;
    let governance_operation_id = artifact
        .operation_id
        .parse::<u64>()
        .map_err(|_| "invalid governance operation ID")?;
    if phase == "schedule" {
        let (expected_governance_operation_id, expected_operation_id, expected_salt) =
            gate_b_initial_activation_binding(bundle, ActivationReceiptFreshness::Current)?;
        if governance_operation_id != expected_governance_operation_id
            || !timelock_operation_id
                .eq_ignore_ascii_case(&format!("0x{}", hex(&expected_operation_id)))
            || !operation_salt.eq_ignore_ascii_case(&format!("0x{}", hex(&expected_salt)))
        {
            return Err(
                "controller schedule artifact differs from the Gate B activation binding".into(),
            );
        }
    }
    let finalized_block_number = confirmation
        .response
        .receipt_block_number
        .parse::<u64>()
        .map_err(|_| "invalid Finalized receipt block number")?;
    if finalized_block_number == 0 {
        return Err("activation confirmation has no Finalized block".into());
    }

    let prior = match (phase, prior_path) {
        ("schedule", None) => None,
        ("schedule", Some(_)) => return Err("schedule forbids a prior receipt".into()),
        ("execute", Some(path)) => {
            let bytes = fs::read(path).map_err(|error| error.to_string())?;
            let receipt: ControllerActivationReceipt =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            let (schedule_governance_operation_id, _) = validate_controller_schedule_receipt(
                bundle,
                &receipt,
                &seal_receipt_sha256,
                ActivationReceiptFreshness::Current,
            )?;
            if !receipt
                .timelock_operation_id
                .eq_ignore_ascii_case(timelock_operation_id)
                || !receipt.operation_salt.eq_ignore_ascii_case(operation_salt)
                || governance_operation_id <= schedule_governance_operation_id
            {
                return Err("execute prior schedule receipt is not bound to this operation".into());
            }
            Some(hex(&Sha256::digest(&bytes)))
        }
        ("execute", None) => return Err("execute requires the controller schedule receipt".into()),
        _ => unreachable!(),
    };

    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let empty_arg = [0x44, 0x49, 0x44, 0x4c, 0x00, 0x00];
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (activation_raw, controllers, module_hash) = async_runtime()?.block_on(async {
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
        Ok::<_, String>((activation_raw, controllers, module_hash))
    })?;
    validate_gate_b_management_snapshot(bundle, &controllers, &module_hash)?;
    let activation =
        Decode!(&activation_raw, ActivationStatusResultView).map_err(|error| error.to_string())?;
    let ActivationStatusResultView::Ok(activation) = activation else {
        return Err("authenticated activation status returned an error".into());
    };
    let last = activation
        .last_confirmed_activation
        .as_ref()
        .ok_or("activation status has no confirmed activation")?;
    if last.phase != phase
        || last.governance_operation_id != governance_operation_id
        || last.receipt_block_number != finalized_block_number
        || !activation_confirmation_artifact_metadata_matches(last, &artifact)?
        || !format!("0x{}", hex(&last.transaction_hash))
            .eq_ignore_ascii_case(&artifact.transaction_hash)
        || !format!("0x{}", hex(&last.timelock_operation_id))
            .eq_ignore_ascii_case(timelock_operation_id)
    {
        return Err("authenticated activation status differs from the confirmation".into());
    }
    match phase {
        "schedule" => {
            let pending = activation
                .pending_timelock_operation
                .as_ref()
                .ok_or("scheduled activation has no pending Timelock operation")?;
            if !activation.deposits_paused
                || !format!("0x{}", hex(&pending.operation_id))
                    .eq_ignore_ascii_case(timelock_operation_id)
                || !format!("0x{}", hex(&pending.salt)).eq_ignore_ascii_case(operation_salt)
            {
                return Err("scheduled activation live state is unsafe".into());
            }
        }
        "execute" => {
            if activation.deposits_paused || activation.pending_timelock_operation.is_some() {
                return Err("executed activation did not unpause and clear the operation".into());
            }
        }
        _ => unreachable!(),
    }
    let verified_at_unix = now_unix()?;
    validate_controller_activation_timeline(
        bundle.manifest.created_at_unix,
        authorization.authorized_at_unix,
        prepare_receipt.bound_at_unix,
        confirmation.confirmed_at_unix,
        verified_at_unix,
        verified_at_unix,
    )?;
    let receipt = ControllerActivationReceipt {
        schema_version: 1,
        phase: phase.into(),
        release_id: bundle.manifest.release_id.clone(),
        source_revision: bundle.manifest.source_revision.clone(),
        source_tree_sha256: bundle.manifest.source_tree_sha256.clone(),
        bridge_canister_wasm_sha256: bundle.profile.bridge_canister_wasm_sha256.clone(),
        gate_b_manifest_sha256: bundle.manifest_sha256.clone(),
        artifact_sha256,
        authorization_receipt_hex: hex(&authorization_bytes),
        authorization_receipt_sha256: authorization_sha256,
        prepare_receipt_hex: hex(&prepare_receipt_bytes),
        prepare_receipt_sha256: hex(&Sha256::digest(&prepare_receipt_bytes)),
        controller_principal: installer.to_text(),
        certified_controller_set: controllers.iter().map(Principal::to_text).collect(),
        governance_operation_id: governance_operation_id.to_string(),
        timelock_operation_id: timelock_operation_id.into(),
        operation_salt: operation_salt.into(),
        transaction_hash: artifact.transaction_hash,
        confirmed_generation: last.generation,
        confirmed_signed_at_ns: last.signed_at_ns.to_string(),
        finalized_block_number: finalized_block_number.to_string(),
        deposits_paused: activation.deposits_paused,
        activation_status_response_hex: hex(&activation_raw),
        activation_status_response_sha256: hex(&Sha256::digest(&activation_raw)),
        prior_schedule_receipt_sha256: prior,
        confirmed_at_unix: confirmation.confirmed_at_unix,
        verified_at_unix,
    };
    write_json_new(receipt_path, &receipt)
}

fn validate_controller_schedule_receipt(
    bundle: &ValidatedBundle,
    receipt: &ControllerActivationReceipt,
    expected_seal_receipt_sha256: &str,
    freshness: ActivationReceiptFreshness,
) -> Result<(u64, u64), String> {
    let installer = gate_b_controller(bundle)?;
    let authorization_bytes = decode_hex(&receipt.authorization_receipt_hex)?;
    let authorization: ControllerActivationAuthorizationReceipt =
        serde_json::from_slice(&authorization_bytes).map_err(|error| error.to_string())?;
    let prepare_receipt_bytes = decode_hex(&receipt.prepare_receipt_hex)?;
    let prepare_receipt: ControllerActivationPrepareReceipt =
        serde_json::from_slice(&prepare_receipt_bytes).map_err(|error| error.to_string())?;
    let now = now_unix()?;
    if receipt.schema_version != 1
        || receipt.phase != "schedule"
        || receipt.release_id != bundle.manifest.release_id
        || receipt.source_revision != bundle.manifest.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || !receipt
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
        || !receipt
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(&bundle.manifest_sha256)
        || !valid_sha256(&receipt.artifact_sha256)
        || !valid_sha256(&receipt.authorization_receipt_sha256)
        || !hex(&Sha256::digest(&authorization_bytes))
            .eq_ignore_ascii_case(&receipt.authorization_receipt_sha256)
        || validate_controller_activation_authorization(
            "schedule",
            bundle,
            &bundle.manifest_sha256,
            expected_seal_receipt_sha256,
            &authorization,
            freshness,
        )
        .is_err()
        || validate_controller_activation_receipt_timeline(
            freshness,
            (
                bundle.manifest.created_at_unix,
                bundle.manifest.expires_at_unix,
            ),
            authorization.authorized_at_unix,
            prepare_receipt.bound_at_unix,
            receipt.confirmed_at_unix,
            receipt.verified_at_unix,
            now,
        )
        .is_err()
        || !valid_sha256(&receipt.prepare_receipt_sha256)
        || !hex(&Sha256::digest(&prepare_receipt_bytes))
            .eq_ignore_ascii_case(&receipt.prepare_receipt_sha256)
        || !controller_activation_prepare_fields_match(
            &prepare_receipt,
            "schedule",
            &bundle.manifest_sha256,
            &receipt.artifact_sha256,
            &receipt.authorization_receipt_sha256,
        )
        || receipt.controller_principal != installer.to_text()
        || receipt.certified_controller_set != [installer.to_text()]
        || receipt.prior_schedule_receipt_sha256.is_some()
        || !receipt.deposits_paused
        || !valid_hash32(&receipt.timelock_operation_id)
        || !valid_hash32(&receipt.operation_salt)
        || !valid_hash32(&receipt.transaction_hash)
        || !activation_raw_digest_matches(
            &receipt.activation_status_response_hex,
            &receipt.activation_status_response_sha256,
        )?
    {
        return Err("controller schedule receipt is not bound to this Gate B release".into());
    }
    let governance_operation_id = receipt
        .governance_operation_id
        .parse::<u64>()
        .map_err(|_| "invalid schedule governance operation ID")?;
    let (expected_governance_operation_id, expected_operation_id, expected_salt) =
        gate_b_initial_activation_binding(bundle, freshness)?;
    let finalized_block = receipt
        .finalized_block_number
        .parse::<u64>()
        .map_err(|_| "invalid schedule Finalized block")?;
    if finalized_block == 0
        || governance_operation_id != expected_governance_operation_id
        || !receipt
            .timelock_operation_id
            .eq_ignore_ascii_case(&format!("0x{}", hex(&expected_operation_id)))
        || !receipt
            .operation_salt
            .eq_ignore_ascii_case(&format!("0x{}", hex(&expected_salt)))
    {
        return Err("controller schedule receipt has no Finalized block".into());
    }
    let raw = decode_hex(&receipt.activation_status_response_hex)?;
    let ActivationStatusResultView::Ok(status) =
        Decode!(&raw, ActivationStatusResultView).map_err(|error| error.to_string())?
    else {
        return Err("controller schedule receipt contains an error response".into());
    };
    let pending = status
        .pending_timelock_operation
        .as_ref()
        .ok_or("controller schedule receipt has no pending operation")?;
    let last = status
        .last_confirmed_activation
        .as_ref()
        .ok_or("controller schedule receipt has no confirmation")?;
    if !status.deposits_paused
        || last.phase != "schedule"
        || last.governance_operation_id != governance_operation_id
        || last.receipt_block_number != finalized_block
        || !controller_activation_confirmation_fields_match(
            receipt.confirmed_generation,
            &receipt.confirmed_signed_at_ns,
            last,
        )
        || !format!("0x{}", hex(&pending.operation_id))
            .eq_ignore_ascii_case(&receipt.timelock_operation_id)
        || !format!("0x{}", hex(&pending.salt)).eq_ignore_ascii_case(&receipt.operation_salt)
        || !format!("0x{}", hex(&last.timelock_operation_id))
            .eq_ignore_ascii_case(&receipt.timelock_operation_id)
        || !format!("0x{}", hex(&last.transaction_hash))
            .eq_ignore_ascii_case(&receipt.transaction_hash)
    {
        return Err("controller schedule receipt response disagrees with its fields".into());
    }
    Ok((governance_operation_id, finalized_block))
}

fn gate_b_initial_activation_binding(
    bundle: &ValidatedBundle,
    freshness: ActivationReceiptFreshness,
) -> Result<(u64, [u8; 32], [u8; 32]), String> {
    let path = bundle.root.join("initial-operational-parameters.json");
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    let expected_sha256 = bundle
        .manifest
        .artifacts
        .iter()
        .find(|artifact| artifact.path == "initial-operational-parameters.json")
        .ok_or("Gate B manifest has no initial operational parameter evidence")?
        .sha256
        .as_str();
    if !hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(expected_sha256) {
        return Err(
            "initial operational parameter evidence changed after Gate B validation".into(),
        );
    }
    let initial: InitialOperationalParameters =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    match freshness {
        ActivationReceiptFreshness::Current => validate_initial_operational_parameters(
            &initial,
            &bundle.profile,
            bundle.manifest.created_at_unix,
            now_unix()?,
        )?,
        ActivationReceiptFreshness::Historical => validate_initial_operational_parameter_lineage(
            &initial,
            &bundle.profile,
            bundle.manifest.created_at_unix,
        )?,
    }
    let deployment_instance_id: [u8; 32] = decode_hex(&initial.deployment_instance_id)?
        .try_into()
        .map_err(|_| "invalid initial deployment instance ID")?;
    let salt = initial_activation_salt(deployment_instance_id, initial.governance_operation_id);
    let operation_id =
        initial_activation_operation_id(decode_address(&initial.bridge_contract)?, salt);
    Ok((initial.governance_operation_id, operation_id, salt))
}

fn verify_controller_schedule_receipt_live(
    bundle: &ValidatedBundle,
    seal_receipt_path: &Path,
    receipt_path: &Path,
) -> Result<(), String> {
    let seal_receipt_sha256 = validate_operational_config_seal_receipt(
        bundle,
        seal_receipt_path,
        SealReceiptLiveContext::PendingResume,
    )?;
    let receipt: ControllerActivationReceipt = read_json(receipt_path)?;
    let (governance_operation_id, finalized_block) = validate_controller_schedule_receipt(
        bundle,
        &receipt,
        &seal_receipt_sha256,
        ActivationReceiptFreshness::Current,
    )?;
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let empty_arg = [0x44, 0x49, 0x44, 0x4c, 0x00, 0x00];
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (activation_raw, controllers, module_hash) = async_runtime()?.block_on(async {
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
        Ok::<_, String>((activation_raw, controllers, module_hash))
    })?;
    validate_gate_b_management_snapshot(bundle, &controllers, &module_hash)?;
    let ActivationStatusResultView::Ok(status) =
        Decode!(&activation_raw, ActivationStatusResultView).map_err(|error| error.to_string())?
    else {
        return Err("authenticated activation status returned an error".into());
    };
    let pending = status
        .pending_timelock_operation
        .as_ref()
        .ok_or("live schedule operation is missing")?;
    let last = status
        .last_confirmed_activation
        .as_ref()
        .ok_or("live schedule confirmation is missing")?;
    if !status.deposits_paused
        || last.phase != "schedule"
        || last.governance_operation_id != governance_operation_id
        || last.receipt_block_number != finalized_block
        || !controller_activation_confirmation_fields_match(
            receipt.confirmed_generation,
            &receipt.confirmed_signed_at_ns,
            last,
        )
        || !format!("0x{}", hex(&pending.operation_id))
            .eq_ignore_ascii_case(&receipt.timelock_operation_id)
        || !format!("0x{}", hex(&pending.salt)).eq_ignore_ascii_case(&receipt.operation_salt)
        || !format!("0x{}", hex(&last.timelock_operation_id))
            .eq_ignore_ascii_case(&receipt.timelock_operation_id)
        || !format!("0x{}", hex(&last.transaction_hash))
            .eq_ignore_ascii_case(&receipt.transaction_hash)
    {
        return Err("live schedule state differs from the controller receipt".into());
    }
    Ok(())
}

#[derive(CandidType, Deserialize, Serialize)]
struct SnsActivationProposal {
    previous_governance_operation_id: u64,
}

fn dao_activation_bundle(root: &Path) -> Result<ValidatedBundle, String> {
    let mut bundle = validate_historical_gate_b_bundle(root)?;
    let module = env::var("BRIDGE_CURRENT_MODULE_SHA256")
        .map_err(|_| "DAO activation verification requires BRIDGE_CURRENT_MODULE_SHA256")?;
    if bundle.manifest.test_only || !valid_sha256(&module) {
        return Err("DAO activation current module binding is invalid".into());
    }
    bundle.profile.bridge_canister_wasm_sha256 = module.to_ascii_lowercase();
    bundle.profile.canister_schema_version = CURRENT_STABLE_SCHEMA_VERSION;
    Ok(bundle)
}

fn verify_dao_live_inputs(
    bundle: &ValidatedBundle,
    expected_deposits_paused: bool,
) -> Result<(), String> {
    verify_live_inputs_for_services_hash(
        bundle,
        expected_deposits_paused,
        &hex(&canonical_sha256(&Vec::<String>::new())?),
    )
}

fn sns_activation_payload(previous_governance_operation_id: u64) -> Result<Vec<u8>, String> {
    Encode!(&SnsActivationProposal {
        previous_governance_operation_id
    })
    .map_err(|error| error.to_string())
}

fn sns_activation_binding(
    bundle: &ValidatedBundle,
    id: u64,
) -> Result<(u64, [u8; 32], [u8; 32]), String> {
    let instance: [u8; 32] = decode_hex(&bundle.profile.deployment_instance_id)?
        .try_into()
        .map_err(|_| "invalid deployment instance")?;
    let salt = initial_activation_salt(instance, id);
    Ok((
        id,
        initial_activation_operation_id(decode_address(&bundle.profile.bridge_contract)?, salt),
        salt,
    ))
}

fn validate_schedule_receipt_binding(
    receipt: &ActivationReceipt,
    bundle: &ValidatedBundle,
) -> Result<(), String> {
    let canonical_payload = sns_activation_payload(receipt.previous_governance_operation_id)?;
    let payload_sha256 = hex(&Sha256::digest(&canonical_payload));
    let now = now_unix()?;
    let (expected_governance_operation_id, expected_operation_id, expected_salt) =
        sns_activation_binding(
            bundle,
            receipt
                .governance_operation_id
                .parse()
                .map_err(|_| "invalid schedule operation ID")?,
        )?;
    if receipt.schema_version != 5
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
        || receipt.target_method_name != "sns_schedule_activation"
        || receipt.validator_canister_id != bundle.profile.bridge_canister_id
        || receipt.validator_method_name != "validate_sns_schedule_activation"
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
        || expected_governance_operation_id <= receipt.previous_governance_operation_id
        || receipt.governance_operation_id != expected_governance_operation_id.to_string()
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
    let empty_arg = Encode!().map_err(|error| error.to_string())?;
    let _ = canonical_payload;
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
        "sns_schedule_activation"
    } else {
        "sns_execute_activation"
    };
    let canonical_payload = sns_activation_payload(submission.previous_governance_operation_id)?;
    let proposal_response = decode_hex(&submission.proposal_response_hex)?;
    if submission.schema_version != 4
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
        || submission.validator_canister_id != bundle.profile.bridge_canister_id
        || submission.validator_method_name != format!("validate_{method}")
        || decode_hex(&submission.payload_hex)? != canonical_payload
        || !submission
            .payload_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(&canonical_payload)))
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
    validate_dao_activation_management_snapshot(bundle, &controllers, &module_hash)?;

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
                            && generic.validator_canister_id == Some(bridge)
                            && generic.validator_method_name.as_deref() == Some(submission.validator_method_name.as_str())
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
    let confirmation_governance_operation_id = confirmation.governance_operation_id;
    if confirmation.phase != phase
        || confirmation.receipt_block_number == 0
        || confirmation.transaction_hash.len() != 32
        || format!("0x{}", hex(&confirmation.timelock_operation_id)) != operation_id.to_lowercase()
    {
        return Err(
            "authenticated activation confirmation does not match the requested phase".into(),
        );
    }
    if phase == "schedule" {
        let (expected_governance_operation_id, expected_operation_id, expected_salt) =
            sns_activation_binding(bundle, confirmation_governance_operation_id)?;
        if confirmation_governance_operation_id <= submission.previous_governance_operation_id
            || confirmation_governance_operation_id != expected_governance_operation_id
            || operation_id != format!("0x{}", hex(&expected_operation_id))
            || operation_salt != format!("0x{}", hex(&expected_salt))
        {
            return Err(
                "SNS schedule confirmation differs from the Gate B activation binding".into(),
            );
        }
    } else {
        let prior_governance_operation_id = prior
            .as_ref()
            .expect("execute prior checked")
            .0
            .governance_operation_id
            .parse::<u64>()
            .map_err(|_| "invalid prior schedule governance operation ID")?;
        if submission.previous_governance_operation_id != prior_governance_operation_id
            || confirmation_governance_operation_id <= prior_governance_operation_id
        {
            return Err("SNS execute governance operation ID does not follow schedule".into());
        }
    }

    let prior_schedule_receipt_sha256 = prior.as_ref().map(|(_, digest)| digest.clone());
    let receipt = ActivationReceipt {
        schema_version: 5,
        phase: phase.into(),
        release_id: bundle.manifest.release_id.clone(),
        source_revision: bundle.manifest.source_revision.clone(),
        source_tree_sha256: bundle.manifest.source_tree_sha256.clone(),
        gate_b_manifest_sha256: bundle.manifest_sha256.clone(),
        proposal_id: submission.proposal_id,
        function_id: submission.function_id,
        target_method_name: method.into(),
        validator_canister_id: submission.validator_canister_id.clone(),
        validator_method_name: submission.validator_method_name.clone(),
        previous_governance_operation_id: submission.previous_governance_operation_id,
        payload_sha256: submission.payload_sha256.clone(),
        executed_at_unix: executed_at,
        verified_at_unix: now,
        governance_query_response_hex: hex(&proposal_raw),
        governance_query_response_sha256: hex(&Sha256::digest(&proposal_raw)),
        function_registry_response_hex: hex(&registry_raw),
        function_registry_response_sha256: hex(&Sha256::digest(&registry_raw)),
        activation_status_response_hex: hex(&activation_raw),
        activation_status_response_sha256: hex(&Sha256::digest(&activation_raw)),
        governance_operation_id: confirmation_governance_operation_id.to_string(),
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
    let canonical_payload = sns_activation_payload(receipt.previous_governance_operation_id)?;
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
    validate_dao_activation_management_snapshot(bundle, &controllers, &module_hash)?;

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
                                == Some("sns_schedule_activation")
                            && generic.validator_canister_id == Some(bridge)
                            && generic.validator_method_name.as_deref() == Some("validate_sns_schedule_activation")
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
    let governance_operation_id = receipt
        .governance_operation_id
        .parse::<u64>()
        .map_err(|_| "schedule receipt has an invalid governance operation ID")?;
    if confirmation.phase != "schedule"
        || confirmation.governance_operation_id != governance_operation_id
        || confirmation.receipt_block_number == 0
        || confirmation.transaction_hash.len() != 32
        || format!("0x{}", hex(&confirmation.timelock_operation_id))
            != receipt.operation_id.to_lowercase()
    {
        return Err("live Finalized schedule confirmation does not match the receipt".into());
    }

    Ok(())
}

fn production_upgrade_identity(pem_path: &Path) -> Result<Box<dyn Identity>, String> {
    let pem = fs::read(pem_path).map_err(|error| error.to_string())?;
    if let Ok(identity) = Secp256k1Identity::from_pem(&pem) {
        return Ok(Box::new(identity));
    }
    if let Ok(identity) = BasicIdentity::from_pem(&pem) {
        return Ok(Box::new(identity));
    }
    Err("production controller PEM is not a supported secp256k1 or Ed25519 identity".into())
}

fn verify_production_upgrade_signature(
    public_key_der: &[u8],
    signature: &[u8],
    message: &[u8],
) -> Result<(), String> {
    const ED25519_SPKI_PREFIX: &[u8] = &[
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    const SECP256K1_SPKI_PREFIX: &[u8] = &[
        0x30, 0x56, 0x30, 0x10, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x05,
        0x2b, 0x81, 0x04, 0x00, 0x0a, 0x03, 0x42, 0x00,
    ];
    if let Some(public_key) = public_key_der.strip_prefix(ED25519_SPKI_PREFIX) {
        if public_key.len() != 32 {
            return Err("production upgrade Ed25519 public key is malformed".into());
        }
        return ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key)
            .verify(message, signature)
            .map_err(|_| "production upgrade Ed25519 signature is invalid".into());
    }
    if let Some(public_key) = public_key_der.strip_prefix(SECP256K1_SPKI_PREFIX) {
        let verifying_key =
            VerifyingKey::from_sec1_bytes(public_key).map_err(|error| error.to_string())?;
        let signature =
            Secp256k1Signature::from_slice(signature).map_err(|error| error.to_string())?;
        return verifying_key
            .verify(message, &signature)
            .map_err(|error| error.to_string());
    }
    Err("production upgrade signed envelope uses an unsupported public key algorithm".into())
}

#[allow(clippy::too_many_arguments)]
fn validate_production_upgrade_signed_update(
    sender: Principal,
    method_name: &str,
    argument: &[u8],
    ingress_expiry: u64,
    request_id_hex: &str,
    signed_update_hex: &str,
    signed_update_sha256: &str,
) -> Result<(), String> {
    if !hex_sha256_matches(signed_update_hex, signed_update_sha256) || !valid_sha256(request_id_hex)
    {
        return Err("production upgrade signed update metadata is invalid".into());
    }
    let management = Principal::management_canister();
    let signed_update = decode_hex(signed_update_hex)?;
    ic_agent::agent::signed_update_inspect(
        sender,
        management,
        method_name,
        argument,
        ingress_expiry,
        signed_update.clone(),
    )
    .map_err(|error| error.to_string())?;
    let envelope: Envelope<'_> =
        serde_cbor::from_slice(&signed_update).map_err(|error| error.to_string())?;
    let EnvelopeContent::Call { .. } = envelope.content.as_ref() else {
        return Err("production upgrade submission is not an update call".into());
    };
    let public_key = envelope
        .sender_pubkey
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("production upgrade submission has no sender public key")?;
    let signature = envelope
        .sender_sig
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or("production upgrade submission has no sender signature")?;
    let request_id = envelope.content.to_request_id();
    if envelope.sender_delegation.is_some()
        || Principal::self_authenticating(public_key) != sender
        || !hex(request_id.as_slice()).eq_ignore_ascii_case(request_id_hex)
    {
        return Err("production upgrade signed envelope identity or request ID is invalid".into());
    }
    verify_production_upgrade_signature(public_key, signature, &request_id.signable())
}

fn validate_production_upgrade_submission_bytes(
    host: &str,
    canister: Principal,
    sender: Principal,
    wasm: &[u8],
    submission_bytes: &[u8],
) -> Result<ProductionUpgradeSubmission, String> {
    if wasm.is_empty() {
        return Err("production upgrade Wasm must not be empty".into());
    }
    let wasm_sha256 = hex(&Sha256::digest(wasm));
    let chunks = wasm
        .chunks(PRODUCTION_UPGRADE_CHUNK_SIZE)
        .collect::<Vec<_>>();
    let chunk_hashes_list = chunks
        .iter()
        .map(|chunk| ManagementChunkHash {
            hash: Sha256::digest(chunk).to_vec(),
        })
        .collect::<Vec<_>>();
    let argument = Encode!(&ManagementInstallChunkedCodeArgument {
        mode: ManagementInstallMode::Upgrade,
        target_canister: canister,
        store_canister: None,
        chunk_hashes_list,
        wasm_module_hash: Sha256::digest(wasm).to_vec(),
        arg: Vec::new(),
        sender_canister_version: None,
    })
    .map_err(|error| error.to_string())?;
    let submission: ProductionUpgradeSubmission =
        serde_json::from_slice(submission_bytes).map_err(|error| error.to_string())?;
    if submission.schema_version != 2
        || submission.install_method != "install_chunked_code"
        || submission.ic_host != host
        || submission.effective_canister_id != canister.to_text()
        || submission.sender_principal != sender.to_text()
        || !submission.wasm_sha256.eq_ignore_ascii_case(&wasm_sha256)
        || submission.chunk_size_bytes != PRODUCTION_UPGRADE_CHUNK_SIZE as u64
        || submission.chunks.len() != chunks.len()
        || !submission
            .argument_hex
            .eq_ignore_ascii_case(&hex(&argument))
        || !hex_sha256_matches(&submission.argument_hex, &submission.argument_sha256)
    {
        return Err("production upgrade submission does not match the exact request".into());
    }
    let stored_chunks_argument = Encode!(&ManagementStoredChunksArgument {
        canister_id: canister,
    })
    .map_err(|error| error.to_string())?;
    if !submission
        .stored_chunks
        .argument_hex
        .eq_ignore_ascii_case(&hex(&stored_chunks_argument))
        || !hex_sha256_matches(
            &submission.stored_chunks.argument_hex,
            &submission.stored_chunks.argument_sha256,
        )
    {
        return Err("production upgrade stored-chunks request is invalid".into());
    }
    validate_production_upgrade_signed_update(
        sender,
        "stored_chunks",
        &stored_chunks_argument,
        submission.stored_chunks.ingress_expiry,
        &submission.stored_chunks.request_id,
        &submission.stored_chunks.signed_update_hex,
        &submission.stored_chunks.signed_update_sha256,
    )?;
    validate_production_upgrade_signed_update(
        sender,
        "install_chunked_code",
        &argument,
        submission.ingress_expiry,
        &submission.request_id,
        &submission.signed_update_hex,
        &submission.signed_update_sha256,
    )?;
    for (index, (chunk, recorded)) in chunks.iter().zip(&submission.chunks).enumerate() {
        let chunk_sha256 = hex(&Sha256::digest(chunk));
        let chunk_argument = Encode!(&ManagementUploadChunkArgument {
            canister_id: canister,
            chunk: chunk.to_vec(),
        })
        .map_err(|error| error.to_string())?;
        if recorded.index != u32::try_from(index).map_err(|error| error.to_string())?
            || recorded.offset
                != u64::try_from(
                    index
                        .checked_mul(PRODUCTION_UPGRADE_CHUNK_SIZE)
                        .ok_or("production upgrade chunk offset overflow")?,
                )
                .map_err(|error| error.to_string())?
            || recorded.size_bytes
                != u64::try_from(chunk.len()).map_err(|error| error.to_string())?
            || !recorded.sha256.eq_ignore_ascii_case(&chunk_sha256)
            || !recorded
                .argument_hex
                .eq_ignore_ascii_case(&hex(&chunk_argument))
            || !hex_sha256_matches(&recorded.argument_hex, &recorded.argument_sha256)
        {
            return Err("production upgrade chunk submission does not match the exact Wasm".into());
        }
        validate_production_upgrade_signed_update(
            sender,
            "upload_chunk",
            &chunk_argument,
            recorded.ingress_expiry,
            &recorded.request_id,
            &recorded.signed_update_hex,
            &recorded.signed_update_sha256,
        )?;
    }
    Ok(submission)
}

fn send_production_upgrade_signed_update(
    agent: &Agent,
    effective_canister_id: Principal,
    request_id_hex: &str,
    signed_update_hex: &str,
) -> Result<Vec<u8>, String> {
    let signed_update = decode_hex(signed_update_hex)?;
    async_runtime()?.block_on(async {
        match agent
            .update_signed(effective_canister_id, signed_update)
            .await
            .map_err(|error| error.to_string())?
        {
            CallResponse::Response(response) => Ok(response),
            CallResponse::Poll(observed_request_id) => {
                if !hex(observed_request_id.as_slice()).eq_ignore_ascii_case(request_id_hex) {
                    return Err("IC returned a request ID different from the signed update".into());
                }
                agent
                    .wait(&observed_request_id, effective_canister_id)
                    .await
                    .map(|(response, _)| response)
                    .map_err(|error| error.to_string())
            }
        }
    })
}

fn build_production_upgrade_submission(
    agent: &Agent,
    host: &str,
    canister: Principal,
    sender: Principal,
    wasm: &[u8],
) -> Result<ProductionUpgradeSubmission, String> {
    if wasm.is_empty() {
        return Err("production upgrade Wasm must not be empty".into());
    }
    let management = Principal::management_canister();
    let stored_chunks_argument = Encode!(&ManagementStoredChunksArgument {
        canister_id: canister,
    })
    .map_err(|error| error.to_string())?;
    let stored_chunks_signed = agent
        .update(&management, "stored_chunks")
        .with_effective_canister_id(canister)
        .with_arg(stored_chunks_argument.clone())
        .sign()
        .map_err(|error| error.to_string())?;
    let stored_chunks = ProductionUpgradeSignedUpdate {
        argument_hex: hex(&stored_chunks_argument),
        argument_sha256: hex(&Sha256::digest(&stored_chunks_argument)),
        ingress_expiry: stored_chunks_signed.ingress_expiry,
        request_id: hex(stored_chunks_signed.request_id.as_slice()),
        signed_update_hex: hex(&stored_chunks_signed.signed_update),
        signed_update_sha256: hex(&Sha256::digest(&stored_chunks_signed.signed_update)),
    };
    let mut chunks = Vec::new();
    let mut chunk_hashes_list = Vec::new();
    for (index, chunk) in wasm.chunks(PRODUCTION_UPGRADE_CHUNK_SIZE).enumerate() {
        let chunk_sha256 = Sha256::digest(chunk).to_vec();
        let argument = Encode!(&ManagementUploadChunkArgument {
            canister_id: canister,
            chunk: chunk.to_vec(),
        })
        .map_err(|error| error.to_string())?;
        let signed = agent
            .update(&management, "upload_chunk")
            .with_effective_canister_id(canister)
            .with_arg(argument.clone())
            .sign()
            .map_err(|error| error.to_string())?;
        chunks.push(ProductionUpgradeChunkSubmission {
            index: u32::try_from(index).map_err(|error| error.to_string())?,
            offset: u64::try_from(
                index
                    .checked_mul(PRODUCTION_UPGRADE_CHUNK_SIZE)
                    .ok_or("production upgrade chunk offset overflow")?,
            )
            .map_err(|error| error.to_string())?,
            size_bytes: u64::try_from(chunk.len()).map_err(|error| error.to_string())?,
            sha256: hex(&chunk_sha256),
            argument_hex: hex(&argument),
            argument_sha256: hex(&Sha256::digest(&argument)),
            ingress_expiry: signed.ingress_expiry,
            request_id: hex(signed.request_id.as_slice()),
            signed_update_hex: hex(&signed.signed_update),
            signed_update_sha256: hex(&Sha256::digest(&signed.signed_update)),
        });
        chunk_hashes_list.push(ManagementChunkHash { hash: chunk_sha256 });
    }
    let argument = Encode!(&ManagementInstallChunkedCodeArgument {
        mode: ManagementInstallMode::Upgrade,
        target_canister: canister,
        store_canister: None,
        chunk_hashes_list,
        wasm_module_hash: Sha256::digest(wasm).to_vec(),
        arg: Vec::new(),
        sender_canister_version: None,
    })
    .map_err(|error| error.to_string())?;
    let signed = agent
        .update(&management, "install_chunked_code")
        .with_effective_canister_id(canister)
        .with_arg(argument.clone())
        .sign()
        .map_err(|error| error.to_string())?;
    Ok(ProductionUpgradeSubmission {
        schema_version: 2,
        install_method: "install_chunked_code".into(),
        ic_host: host.to_string(),
        effective_canister_id: canister.to_text(),
        sender_principal: sender.to_text(),
        wasm_sha256: hex(&Sha256::digest(wasm)),
        chunk_size_bytes: PRODUCTION_UPGRADE_CHUNK_SIZE as u64,
        stored_chunks,
        chunks,
        argument_hex: hex(&argument),
        argument_sha256: hex(&Sha256::digest(&argument)),
        ingress_expiry: signed.ingress_expiry,
        request_id: hex(signed.request_id.as_slice()),
        signed_update_hex: hex(&signed.signed_update),
        signed_update_sha256: hex(&Sha256::digest(&signed.signed_update)),
    })
}

fn execute_production_canister_upgrade(
    host: &str,
    canister_text: &str,
    expected_principal_text: &str,
    pem_path: &Path,
    wasm_path: &Path,
    profile_path: &Path,
    expected_current_module_sha256: &str,
) -> Result<(), String> {
    let canister = Principal::from_text(canister_text).map_err(|error| error.to_string())?;
    let expected_principal =
        Principal::from_text(expected_principal_text).map_err(|error| error.to_string())?;
    let wasm = fs::read(wasm_path).map_err(|error| error.to_string())?;
    let (agent, sender) = production_upgrade_agent(host, expected_principal, pem_path)?;
    verify_production_current_state(
        profile_path,
        expected_principal_text,
        expected_current_module_sha256,
        "sole",
    )?;
    let before = production_upgrade_current_state_snapshot(&agent, canister)?;
    let submission = build_production_upgrade_submission(&agent, host, canister, sender, &wasm)?;
    let submission_bytes = serde_json::to_vec(&submission).map_err(|error| error.to_string())?;
    validate_production_upgrade_submission_bytes(host, canister, sender, &wasm, &submission_bytes)?;

    production_upgrade_request_has_time(submission.stored_chunks.ingress_expiry)?;
    let stored_raw = send_production_upgrade_signed_update(
        &agent,
        canister,
        &submission.stored_chunks.request_id,
        &submission.stored_chunks.signed_update_hex,
    )?;
    let stored =
        Decode!(&stored_raw, Vec<ManagementChunkHash>).map_err(|error| error.to_string())?;
    let expected = submission
        .chunks
        .iter()
        .map(|chunk| chunk.sha256.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if stored
        .iter()
        .any(|chunk| !expected.contains(&hex(&chunk.hash)))
    {
        return Err("production chunk store contains an unexpected chunk".into());
    }
    for chunk in &submission.chunks {
        production_upgrade_request_has_time(chunk.ingress_expiry)?;
        let raw = send_production_upgrade_signed_update(
            &agent,
            canister,
            &chunk.request_id,
            &chunk.signed_update_hex,
        )?;
        let observed = Decode!(&raw, ManagementChunkHash).map_err(|error| error.to_string())?;
        if !hex(&observed.hash).eq_ignore_ascii_case(&chunk.sha256) {
            return Err("production chunk response hash differs".into());
        }
    }
    let pre_send_module = async_runtime()?.block_on(async {
        agent
            .read_state_canister_module_hash(canister)
            .await
            .map_err(|error| error.to_string())
    })?;
    if !hex(&pre_send_module).eq_ignore_ascii_case(expected_current_module_sha256) {
        return Err("certified production module changed immediately before upgrade send".into());
    }
    let pre_send = production_upgrade_current_state_snapshot(&agent, canister)?;
    if before != pre_send {
        return Err(
            "authenticated production state changed immediately before upgrade send".into(),
        );
    }
    production_upgrade_request_has_time(submission.ingress_expiry)?;
    println!("request_id={}", submission.request_id);
    std::io::stdout()
        .flush()
        .map_err(|error| error.to_string())?;
    let response = match send_production_upgrade_signed_update(
        &agent,
        canister,
        &submission.request_id,
        &submission.signed_update_hex,
    ) {
        Ok(response) => response,
        Err(send_error) => {
            let after = production_upgrade_current_state_snapshot(&agent, canister).map_err(
                |observe_error| {
                    format!(
                        "upgrade outcome is unresolved; do not retry for 6 minutes, then rerun check against certified state: send={send_error}; observation={observe_error}"
                    )
                },
            )?;
            match classify_production_upgrade_observation(
                &before,
                &after,
                expected_current_module_sha256,
                &submission.wasm_sha256,
            )? {
                ProductionUpgradeObservedOutcome::CandidatePreserved => {
                    verify_production_current_state(
                        profile_path,
                        expected_principal_text,
                        &submission.wasm_sha256,
                        "sole",
                    )?;
                    println!(
                        "production_upgrade=ambiguous-response-verified request_id={}",
                        submission.request_id
                    );
                    return Ok(());
                }
                ProductionUpgradeObservedOutcome::CurrentUnresolved => return Err(format!(
                    "upgrade outcome is unresolved; do not retry for 6 minutes, then rerun check against certified state: {send_error}"
                )),
            }
        }
    };
    println!("response_hex={}", hex(&response));
    println!("sender_principal={sender}");
    println!("wasm_sha256={}", submission.wasm_sha256);
    let after = production_upgrade_current_state_snapshot(&agent, canister)?;
    if classify_production_upgrade_observation(
        &before,
        &after,
        expected_current_module_sha256,
        &submission.wasm_sha256,
    )? != ProductionUpgradeObservedOutcome::CandidatePreserved
    {
        return Err("certified production module does not match the submitted Wasm".into());
    }
    verify_production_current_state(
        profile_path,
        expected_principal_text,
        &submission.wasm_sha256,
        "sole",
    )?;
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
struct ProductionUpgradeCurrentStateSnapshot {
    lifecycle: Vec<u8>,
    activation_attestation: Vec<u8>,
    activation_status: Vec<u8>,
    runtime_binding: Vec<u8>,
    bridge_status: BridgeStatusLiveView,
    pending_governance: Vec<u8>,
    withdrawal_index_probe: Vec<u8>,
    operational_config: Vec<u8>,
    control_plane_addresses: Vec<u8>,
    storage_integrity: Vec<u8>,
    controllers: BTreeSet<Principal>,
    module_hash: Vec<u8>,
}

impl ProductionUpgradeCurrentStateSnapshot {
    fn preserved_across_upgrade(&self, after: &Self) -> bool {
        let mut expected = self.clone();
        expected.module_hash = after.module_hash.clone();
        expected == *after
    }

    fn preserved_across_root_addition(&self, after: &Self) -> bool {
        let mut expected = self.clone();
        expected.controllers = after.controllers.clone();
        expected == *after
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ProductionUpgradeObservedOutcome {
    CandidatePreserved,
    CurrentUnresolved,
}

fn classify_production_upgrade_observation(
    before: &ProductionUpgradeCurrentStateSnapshot,
    after: &ProductionUpgradeCurrentStateSnapshot,
    expected_current_module_sha256: &str,
    candidate_module_sha256: &str,
) -> Result<ProductionUpgradeObservedOutcome, String> {
    let observed = hex(&after.module_hash);
    if observed.eq_ignore_ascii_case(candidate_module_sha256) {
        if !before.preserved_across_upgrade(after) {
            return Err("authenticated production state was not preserved across upgrade".into());
        }
        return Ok(ProductionUpgradeObservedOutcome::CandidatePreserved);
    }
    if observed.eq_ignore_ascii_case(expected_current_module_sha256) {
        return Ok(ProductionUpgradeObservedOutcome::CurrentUnresolved);
    }
    Err("certified production module changed to an unexpected hash".into())
}

fn validate_production_root_addition_observation(
    before: &ProductionUpgradeCurrentStateSnapshot,
    after: &ProductionUpgradeCurrentStateSnapshot,
    expected_controller: Principal,
    root: Principal,
    expected_module_sha256: &str,
) -> Result<(), String> {
    if before.controllers != BTreeSet::from([expected_controller]) {
        return Err("production identity was not the sole controller before Root addition".into());
    }
    if after.controllers != BTreeSet::from([expected_controller, root]) {
        return Err(
            "certified controller set is not exactly production identity plus SNS Root".into(),
        );
    }
    if !hex(&before.module_hash).eq_ignore_ascii_case(expected_module_sha256)
        || !hex(&after.module_hash).eq_ignore_ascii_case(expected_module_sha256)
    {
        return Err("certified production module changed during Root addition".into());
    }
    if !before.preserved_across_root_addition(after) {
        return Err("authenticated production state changed during Root addition".into());
    }
    Ok(())
}

fn production_upgrade_current_state_snapshot(
    agent: &Agent,
    canister: Principal,
) -> Result<ProductionUpgradeCurrentStateSnapshot, String> {
    let (
        lifecycle,
        activation_attestation,
        activation_status,
        runtime_binding,
        status_raw,
        pending_governance,
        withdrawal_index_probe,
        operational_config,
        control_plane_addresses,
        storage_integrity,
        controllers,
        module_hash,
    ) = async_runtime()?.block_on(async {
        let query = |method: &'static str, argument: Vec<u8>| async move {
            agent
                .query(&canister, method)
                .with_arg(argument)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())
        };
        let empty = Encode!().map_err(|error| error.to_string())?;
        let lifecycle = query("get_production_lifecycle", empty.clone()).await?;
        let activation_attestation = query("get_activation_attestation", empty.clone()).await?;
        let activation_status = query("get_activation_status", empty.clone()).await?;
        let runtime_binding = query("get_runtime_binding", empty.clone()).await?;
        let status = query("get_bridge_status", empty.clone()).await?;
        let pending_governance =
            query("get_pending_base_governance_transaction", empty.clone()).await?;
        let withdrawal_index_probe = query(
            "list_withdrawals",
            Encode!(&ProductionUiHistoryProbe {
                requester: vec![0; 20],
                before_cursor: None,
                limit: 1,
            })
            .map_err(|error| error.to_string())?,
        )
        .await?;
        let operational_config = query("get_release_operational_config", empty.clone()).await?;
        let control_plane_addresses = query("get_control_plane_addresses", empty.clone()).await?;
        let storage_integrity = query("get_release_storage_integrity", empty).await?;
        let controllers = agent
            .read_state_canister_controllers(canister)
            .await
            .map_err(|error| error.to_string())?;
        let module_hash = agent
            .read_state_canister_module_hash(canister)
            .await
            .map_err(|error| error.to_string())?;
        Ok::<_, String>((
            lifecycle,
            activation_attestation,
            activation_status,
            runtime_binding,
            status,
            pending_governance,
            withdrawal_index_probe,
            operational_config,
            control_plane_addresses,
            storage_integrity,
            controllers,
            module_hash,
        ))
    })?;
    let bridge_status =
        Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    Ok(ProductionUpgradeCurrentStateSnapshot {
        lifecycle,
        activation_attestation,
        activation_status,
        runtime_binding,
        bridge_status,
        pending_governance,
        withdrawal_index_probe,
        operational_config,
        control_plane_addresses,
        storage_integrity,
        controllers: controllers.into_iter().collect(),
        module_hash,
    })
}

fn execute_production_root_addition(
    profile_path: &Path,
    expected_controller_text: &str,
    expected_module_sha256: &str,
    identity: &str,
) -> Result<(), String> {
    if identity != "production" || !valid_sha256(expected_module_sha256) {
        return Err(
            "production Root addition requires the fixed identity and module SHA-256".into(),
        );
    }
    let profile: Profile = read_json(profile_path)?;
    if profile.bridge_canister_id != PRODUCTION_BRIDGE_CANISTER
        || profile.root_canister_id != KINIC_ROOT
        || profile.pause_principal != expected_controller_text
        || profile.ic_host != "https://icp-api.io"
    {
        return Err("production Root addition profile differs from the fixed domain".into());
    }
    let canister =
        Principal::from_text(&profile.bridge_canister_id).map_err(|error| error.to_string())?;
    let expected_controller =
        Principal::from_text(expected_controller_text).map_err(|error| error.to_string())?;
    let root =
        Principal::from_text(&profile.root_canister_id).map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&profile.ic_host, false)?;

    verify_production_current_state(
        profile_path,
        expected_controller_text,
        expected_module_sha256,
        "sole",
    )?;
    let before = production_upgrade_current_state_snapshot(&agent, canister)?;
    if before.controllers != BTreeSet::from([expected_controller])
        || !hex(&before.module_hash).eq_ignore_ascii_case(expected_module_sha256)
    {
        return Err(
            "certified production controller set or module changed before Root addition".into(),
        );
    }

    let output = Command::new("icp")
        .args([
            "canister",
            "settings",
            "update",
            profile.bridge_canister_id.as_str(),
            "-n",
            "ic",
            "--add-controller",
            profile.root_canister_id.as_str(),
            "--force",
            "--identity",
            identity,
            "--debug",
        ])
        .output()
        .map_err(|error| format!("failed to submit production Root addition: {error}"))?;
    std::io::stdout()
        .write_all(&output.stdout)
        .map_err(|error| error.to_string())?;
    std::io::stderr()
        .write_all(&output.stderr)
        .map_err(|error| error.to_string())?;

    let postcondition = (|| -> Result<(), String> {
        let after = production_upgrade_current_state_snapshot(&agent, canister)?;
        validate_production_root_addition_observation(
            &before,
            &after,
            expected_controller,
            root,
            expected_module_sha256,
        )?;
        verify_production_current_state(
            profile_path,
            expected_controller_text,
            expected_module_sha256,
            "joint",
        )
    })();

    match (output.status.success(), postcondition) {
        (_, Ok(())) => {
            if !output.status.success() {
                eprintln!("controller update returned an ambiguous response; exact joint state was verified");
            }
            println!(
                "production_handover=co-controller-ready module_sha256={}",
                expected_module_sha256.to_ascii_lowercase()
            );
            Ok(())
        }
        (false, Err(error)) => Err(format!(
            "controller update outcome is unresolved; do not retry for 6 minutes, then rerun authenticated validation: {error}"
        )),
        (true, Err(error)) => Err(format!(
            "controller update returned success but the exact joint-control postcondition is absent: {error}"
        )),
    }
}

fn production_upgrade_request_has_time(ingress_expiry: u64) -> Result<(), String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let minimum_remaining = 15_u128 * 1_000_000_000;
    if u128::from(ingress_expiry) <= now.saturating_add(minimum_remaining) {
        return Err(
            "production upgrade signed request has expired or is too close to expiry".into(),
        );
    }
    Ok(())
}

fn production_upgrade_agent(
    host: &str,
    expected_principal: Principal,
    pem_path: &Path,
) -> Result<(Agent, Principal), String> {
    let identity = production_upgrade_identity(pem_path)?;
    let sender = identity.sender().map_err(|error| error.to_string())?;
    if sender != expected_principal {
        return Err(
            "production controller PEM principal does not match the expected installer".into(),
        );
    }
    let agent = Agent::builder()
        .with_url(host)
        .with_boxed_identity(identity)
        .with_verify_query_signatures(true)
        .build()
        .map_err(|error| error.to_string())?;
    Ok((agent, sender))
}

#[allow(dead_code)]
#[derive(CandidType, Deserialize)]
struct ReleaseUpgradeObservationView {
    completed_at_ns: u64,
    upgrader: Principal,
}

#[allow(dead_code)]
fn validate_sns_upgrade_completion(
    observation: &ReleaseUpgradeObservationView,
    decided_at: u64,
    handover_at: u64,
    now: u64,
) -> Result<(), String> {
    if !bridge_core::kernel::sns_upgrade_completion_allowed(
        observation.upgrader.to_text() == KINIC_ROOT,
        observation.completed_at_ns / 1_000_000_000,
        decided_at,
        handover_at,
        now,
    ) {
        return Err("SNS Root acknowledged the proposal, but no matching successful post_upgrade is observed".into());
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("verify-production-current-state") if args.len() == 6 => {
            verify_production_current_state(
                Path::new(&args[2]),
                &args[3],
                &args[4],
                &args[5],
            )?;
        }
        Some("render-production-current-ui-runtime") if args.len() == 6 => {
            render_production_current_ui_runtime(
                Path::new(&args[2]),
                &args[3],
                Path::new(&args[4]),
                Path::new(&args[5]),
            )?;
        }
        Some("verify-production-current-ui-live") if args.len() == 7 => {
            verify_production_current_ui_live(
                Path::new(&args[2]),
                &args[3],
                Path::new(&args[4]),
                Path::new(&args[5]),
                &args[6],
            )?;
        }
        Some("derive") if args.len() == 3 => {
            let evidence: Evidence = read_json(Path::new(&args[2]))?;
            println!("{}", serde_json::to_string_pretty(&derive(&evidence)?).map_err(|e| e.to_string())?);
        }
        Some("validate") | Some("validate-test") if args.len() == 3 => {
            let profile: Profile = read_json(Path::new(&args[2]))?;
            validate_profile(&profile, args[1] == "validate")?;
            println!("{}", hex(&canonical_sha256(&profile)?));
        }
        Some("write-provider-independence-receipt") if args.len() == 7 => {
            write_provider_independence_receipt(
                Path::new(&args[2]),
                &args[3],
                &args[4],
                &args[5],
                Path::new(&args[6]),
            )?;
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
        Some("verify-production-canister-predeploy") if args.len() == 4 => {
            verify_production_canister_predeploy(Path::new(&args[2]), Path::new(&args[3]))?;
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
                "gate_b=pre_seal-pass authorizing=seal manifest_sha256={}",
                bundle.manifest_sha256
            );
        }
        Some("verify-live") if args.len() == 4 => {
            let phase = &args[2];
            if phase != "schedule" && phase != "execute" {
                return Err("live Gate B phase must be schedule or execute".into());
            }
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only { return Err("Gate B rejects test-only bundles".into()); }
            verify_live(&bundle, true)?;
            println!(
                "gate_b=live-pass authorizing={} manifest_sha256={}",
                phase, bundle.manifest_sha256
            );
        }
        Some("reserve-operational-config-seal") if args.len() == 5 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only {
                return Err("operational config seal reservation rejects test-only bundles".into());
            }
            let created = operational_config_seal_reservation(
                &bundle,
                &args[3],
                Path::new(&args[4]),
            )?;
            println!("{}", if created { "created" } else { "existing" });
        }
        Some("write-operational-config-seal-receipt") if args.len() == 6 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only {
                return Err("operational config seal receipt rejects test-only bundles".into());
            }
            let attempt = if args[4] == "-" {
                None
            } else {
                Some(Path::new(&args[4]))
            };
            write_operational_config_seal_receipt(
                &bundle,
                Path::new(&args[3]),
                attempt,
                Path::new(&args[5]),
            )?;
        }
        Some("authorize-controller-activation") if args.len() == 7 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err("controller activation authorization rejects test-only bundles".into());
            }
            controller_activation_authorization(
                &args[2],
                &bundle,
                &args[4],
                Path::new(&args[5]),
                Some(Path::new(&args[6])),
                None,
            )?;
        }
        Some("verify-controller-activation-authorization") if args.len() == 7 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err("controller activation authorization rejects test-only bundles".into());
            }
            controller_activation_authorization(
                &args[2],
                &bundle,
                &args[4],
                Path::new(&args[5]),
                None,
                Some(Path::new(&args[6])),
            )?;
        }
        Some("verify-controller-activation-authorization-fresh") if args.len() == 7 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err("controller activation authorization rejects test-only bundles".into());
            }
            verify_controller_activation_authorization_fresh(
                &args[2],
                &bundle,
                &args[4],
                Path::new(&args[5]),
                Path::new(&args[6]),
            )?;
        }
        Some("decode-handover-query") if args.len() == 4 => {
            println!("{}", decode_handover_query(&args[2], Path::new(&args[3]))?);
        }
        Some("sns-activation-payload") if args.len() == 3 => {
            println!("{}", hex(&sns_activation_payload(args[2].parse::<u64>().map_err(|_| "invalid previous operation ID")?)?));
        }
        Some("verify-activation") if args.len() == 7 => {
            let bundle = dao_activation_bundle(Path::new(&args[3]))?;
            if bundle.manifest.test_only {
                return Err("activation verification rejects test-only bundles".into());
            }
            match args[2].as_str() {
                "schedule" => verify_dao_live_inputs(&bundle, true)?,
                "execute" => verify_dao_live_inputs(&bundle, false)?,
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
        Some("verify-controller-activation-artifact") if args.len() == 9 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err("controller activation artifact verification rejects test-only bundles".into());
            }
            let prior = if args[8] == "-" {
                None
            } else {
                Some(Path::new(&args[8]))
            };
            verify_controller_activation_artifact_binding(
                &args[2],
                &bundle,
                Path::new(&args[4]),
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
                prior,
                SealReceiptLiveContext::PendingResume,
            )?;
            println!("controller_activation_artifact=verified phase={}", args[2]);
        }
        Some("verify-controller-activation-confirm-inputs") if args.len() == 9 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err(
                    "controller activation confirmation input verification rejects test-only bundles"
                        .into(),
                );
            }
            let prior = if args[8] == "-" {
                None
            } else {
                Some(Path::new(&args[8]))
            };
            verify_controller_activation_artifact_binding(
                &args[2],
                &bundle,
                Path::new(&args[4]),
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
                prior,
                SealReceiptLiveContext::ConfirmationInput,
            )?;
            println!("controller_activation_confirm_inputs=verified phase={}", args[2]);
        }
        Some("verify-controller-activation") if args.len() == 11 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err("controller activation verification rejects test-only bundles".into());
            }
            let prior = if args[9] == "-" {
                None
            } else {
                Some(Path::new(&args[9]))
            };
            verify_controller_activation(
                &args[2],
                &bundle,
                Path::new(&args[4]),
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
                Path::new(&args[8]),
                prior,
                Path::new(&args[10]),
            )?;
            println!(
                "controller_activation=verified phase={} receipt={}",
                args[2], args[10]
            );
        }
        Some("verify-controller-schedule-receipt-live") if args.len() == 5 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only {
                return Err("controller schedule receipt verification rejects test-only bundles".into());
            }
            verify_live(&bundle, true)?;
            verify_controller_schedule_receipt_live(
                &bundle,
                Path::new(&args[3]),
                Path::new(&args[4]),
            )?;
            println!(
                "controller_schedule_receipt=verified manifest_sha256={} receipt={}",
                bundle.manifest_sha256, args[4]
            );
        }
        Some("verify-schedule-receipt-live") if args.len() == 4 => {
            let bundle = dao_activation_bundle(Path::new(&args[2]))?;
            if bundle.manifest.test_only {
                return Err("schedule receipt verification rejects test-only bundles".into());
            }
            verify_dao_live_inputs(&bundle, true)?;
            verify_schedule_receipt_live(&bundle, Path::new(&args[3]))?;
            println!(
                "schedule_receipt=verified manifest_sha256={} receipt={}",
                bundle.manifest_sha256, args[3]
            );
        }
        Some("execute-production-canister-upgrade") if args.len() == 9 => {
            execute_production_canister_upgrade(
                &args[2],
                &args[3],
                &args[4],
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
                &args[8],
            )?;
        }
        Some("execute-production-root-addition") if args.len() == 6 => {
            execute_production_root_addition(
                Path::new(&args[2]),
                &args[3],
                &args[4],
                &args[5],
            )?;
        }
        Some("validate-operational-epoch-snapshot") if args.len() == 6 => {
            let evidence = OperationalEpochEvidence::from_response(&args[2])?;
            let runtime = decode_candid_hex::<RuntimeBindingView>(&args[3])?;
            let status = decode_candid_hex::<BridgeStatusLiveView>(&args[4])?;
            let ledger_fee = args[5].parse::<u128>().map_err(|_| "invalid ledger fee")?;
            validate_operational_epoch_snapshot(&evidence, &status, &live_runtime_binding_from_view(&runtime), ledger_fee)?;
            println!("operational_epoch_snapshot=verified");
        }
        Some("operational-epoch-digests") if args.len() == 6 => {
            let response = fs::read_to_string(&args[2]).map_err(|error| error.to_string())?;
            let ledger_fee = args[3].parse::<u128>().map_err(|_| "invalid ledger fee")?;
            for epoch in [&args[4], &args[5]] {
                let epoch = epoch.parse::<u64>().map_err(|_| "invalid epoch")?;
                println!("epoch={} operational_config_sha256={}", epoch, operational_epoch_digest(response.trim(), ledger_fee, epoch)?);
            }
        }
        _ => return Err("usage: bridge-profile <command> <arguments>; production commands: verify-production-current-state, execute-production-canister-upgrade, execute-production-root-addition".into()),
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
    use k256::ecdsa::SigningKey;

    #[test]
    fn sns_upgrade_completion_rejects_personal_and_stale_hooks() {
        let root = Principal::from_text(KINIC_ROOT).unwrap();
        let valid = ReleaseUpgradeObservationView {
            completed_at_ns: 100_000_000_000,
            upgrader: root,
        };
        assert!(validate_sns_upgrade_completion(&valid, 99, 98, 101).is_ok());
        let personal = ReleaseUpgradeObservationView {
            completed_at_ns: 100_000_000_000,
            upgrader: Principal::anonymous(),
        };
        assert!(validate_sns_upgrade_completion(&personal, 99, 98, 101).is_err());
        assert!(validate_sns_upgrade_completion(&valid, 101, 98, 102).is_err());
        assert!(validate_sns_upgrade_completion(&valid, 99, 101, 102).is_err());
        assert!(validate_sns_upgrade_completion(&valid, 99, 98, 99).is_err());
        assert!(validate_sns_upgrade_completion(&valid, 0, 98, 101).is_err());
    }

    #[test]
    fn production_upgrade_submission_rejects_target_sender_wasm_and_signature_drift() {
        let identity = BasicIdentity::from_raw_key(&[0x42; 32]);
        let sender = identity.sender().unwrap();
        let canister = Principal::self_authenticating([0x24; 32]);
        let host = "https://icp-api.io";
        let wasm = b"\0asm\x01\0\0\0candidate";
        let agent = Agent::builder()
            .with_url(host)
            .with_identity(identity)
            .build()
            .unwrap();
        let submission =
            build_production_upgrade_submission(&agent, host, canister, sender, wasm).unwrap();
        let bytes = serde_json::to_vec(&submission).unwrap();
        assert!(
            validate_production_upgrade_submission_bytes(host, canister, sender, wasm, &bytes,)
                .is_ok()
        );

        let wrong_canister = Principal::self_authenticating([0x25; 32]);
        assert!(validate_production_upgrade_submission_bytes(
            host,
            wrong_canister,
            sender,
            wasm,
            &bytes,
        )
        .is_err());
        assert!(validate_production_upgrade_submission_bytes(
            host,
            canister,
            Principal::anonymous(),
            wasm,
            &bytes,
        )
        .is_err());
        assert!(validate_production_upgrade_submission_bytes(
            host,
            canister,
            sender,
            b"\0asm\x01\0\0\0other",
            &bytes,
        )
        .is_err());

        let mut forged: Value = serde_json::from_slice(&bytes).unwrap();
        let mut signed = decode_hex(forged["signed_update_hex"].as_str().unwrap()).unwrap();
        let last = signed.last_mut().unwrap();
        *last ^= 1;
        forged["signed_update_hex"] = Value::String(hex(&signed));
        forged["signed_update_sha256"] = Value::String(hex(&Sha256::digest(&signed)));
        assert!(validate_production_upgrade_submission_bytes(
            host,
            canister,
            sender,
            wasm,
            &serde_json::to_vec(&forged).unwrap(),
        )
        .is_err());
    }

    fn trim_leading_zeroes(value: &[u8]) -> &[u8] {
        let first = value
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(value.len());
        &value[first..]
    }

    fn signed_activation_artifact(
        profile: &Profile,
        salt: [u8; 32],
        value: u128,
        nonempty_access_list: bool,
    ) -> DirectActivationArtifact {
        let calldata = initial_activation_calldata(
            "schedule_activation",
            decode_address(&profile.bridge_contract).unwrap(),
            salt,
            profile.timelock.minimum_delay_seconds,
        )
        .unwrap();
        let target = decode_hex(&profile.timelock.address).unwrap();
        let access_list = if nonempty_access_list {
            let mut storage_keys = Vec::new();
            storage_keys.extend_from_slice(&encode_rlp_bytes(&[0x55; 32]));
            let mut entry = Vec::new();
            entry.extend_from_slice(&encode_rlp_bytes(&[0x44; 20]));
            entry.extend_from_slice(&encode_rlp_list_payload(&storage_keys));
            encode_rlp_list_payload(&encode_rlp_list_payload(&entry))
        } else {
            encode_rlp_list_payload(&[])
        };
        let unsigned_fields = vec![
            encode_rlp_uint(profile.chain_id.into()),
            encode_rlp_uint(7),
            encode_rlp_uint(2),
            encode_rlp_uint(20),
            encode_rlp_uint(100_000),
            encode_rlp_bytes(&target),
            encode_rlp_uint(value),
            encode_rlp_bytes(&decode_hex(&calldata).unwrap()),
            access_list,
        ];
        let unsigned_payload = unsigned_fields.concat();
        let mut unsigned = vec![0x02];
        unsigned.extend_from_slice(&encode_rlp_list_payload(&unsigned_payload));
        let signing_key = SigningKey::from_bytes((&[0x11; 32]).into()).unwrap();
        let (signature, recovery) = signing_key
            .sign_prehash_recoverable(&keccak256(&unsigned))
            .unwrap();
        let signature_bytes = signature.to_bytes();
        let mut signed_fields = unsigned_fields;
        signed_fields.push(encode_rlp_uint(u128::from(recovery.is_y_odd())));
        signed_fields.push(encode_rlp_bytes(trim_leading_zeroes(
            &signature_bytes[..32],
        )));
        signed_fields.push(encode_rlp_bytes(trim_leading_zeroes(
            &signature_bytes[32..],
        )));
        let mut raw = vec![0x02];
        raw.extend_from_slice(&encode_rlp_list_payload(&signed_fields.concat()));
        let public_key = signing_key.verifying_key().to_encoded_point(false);
        let sender_hash = keccak256(&public_key.as_bytes()[1..]);
        DirectActivationArtifact {
            operation_id: "7".into(),
            kind: DirectActivationKind::ScheduleActivation(DirectActivationOperation {
                operation_id: format!("0x{}", "11".repeat(32)),
                salt: format!("0x{}", hex(&salt)),
            }),
            chain_id: profile.chain_id.to_string(),
            sender: format!("0x{}", hex(&sender_hash[12..])),
            nonce: "7".into(),
            target: profile.timelock.address.clone(),
            calldata,
            gas_limit: "100000".into(),
            max_fee_per_gas: "20".into(),
            max_priority_fee_per_gas: "2".into(),
            raw_transaction: format!("0x{}", hex(&raw)),
            transaction_hash: format!("0x{}", hex(&keccak256(&raw))),
            generation: 0,
            signed_at_ns: "1".into(),
        }
    }

    #[test]
    fn controller_activation_raw_evidence_rejects_digest_drift() {
        let raw = "000102";
        let digest = hex(&Sha256::digest(decode_hex(raw).unwrap()));
        assert!(activation_raw_digest_matches(raw, &digest).unwrap());
        assert!(!activation_raw_digest_matches(raw, &"ff".repeat(32)).unwrap());
        assert!(activation_raw_digest_matches("not-hex", &digest).is_err());
    }

    #[test]
    fn controller_activation_authorization_rejects_field_and_time_drift() {
        let mut receipt = ControllerActivationAuthorizationReceipt {
            schema_version: 1,
            phase: "schedule".into(),
            release_id: "release".into(),
            source_revision: "revision".into(),
            source_tree_sha256: "11".repeat(32),
            gate_b_manifest_sha256: "22".repeat(32),
            operational_config_seal_receipt_sha256: "44".repeat(32),
            controller_principal: "aaaaa-aa".into(),
            certified_controller_set: vec!["aaaaa-aa".into()],
            certified_module_sha256: "33".repeat(32),
            authorized_at_unix: 110,
        };
        let matches = |value: &ControllerActivationAuthorizationReceipt| {
            controller_activation_authorization_fields_match(
                value,
                "schedule",
                "release",
                "revision",
                &"11".repeat(32),
                &"22".repeat(32),
                &"44".repeat(32),
                "aaaaa-aa",
                &"33".repeat(32),
            )
        };
        assert!(matches(&receipt));
        receipt.certified_controller_set.push("2vxsx-fae".into());
        assert!(!matches(&receipt));
        receipt.certified_controller_set.pop();
        receipt.certified_module_sha256 = "44".repeat(32);
        assert!(!matches(&receipt));
        receipt.certified_module_sha256 = "33".repeat(32);
        receipt.phase = "execute".into();
        assert!(!matches(&receipt));

        assert!(validate_controller_activation_timeline(100, 110, 120, 125, 130, 130).is_ok());
        assert!(validate_controller_activation_timeline(100, 99, 120, 125, 130, 130).is_err());
        assert!(validate_controller_activation_timeline(100, 121, 120, 125, 130, 130).is_err());
        assert!(validate_controller_activation_timeline(100, 110, 126, 125, 130, 130).is_err());
        assert!(validate_controller_activation_timeline(100, 110, 120, 131, 130, 130).is_err());
        assert!(validate_controller_activation_timeline(100, 110, 120, 125, 131, 130).is_err());
        assert!(controller_activation_authorization_is_fresh(100, 400));
        assert!(!controller_activation_authorization_is_fresh(100, 401));
        assert!(!controller_activation_authorization_is_fresh(101, 100));

        let mut binding = ControllerActivationPrepareReceipt {
            schema_version: 1,
            phase: "schedule".into(),
            gate_b_manifest_sha256: "22".repeat(32),
            artifact_sha256: "55".repeat(32),
            authorization_receipt_sha256: "66".repeat(32),
            bound_at_unix: 120,
        };
        let binding_matches = |value: &ControllerActivationPrepareReceipt| {
            controller_activation_prepare_fields_match(
                value,
                "schedule",
                &"22".repeat(32),
                &"55".repeat(32),
                &"66".repeat(32),
            )
        };
        assert!(binding_matches(&binding));
        binding.artifact_sha256 = "77".repeat(32);
        assert!(!binding_matches(&binding));
        binding.artifact_sha256 = "55".repeat(32);
        binding.authorization_receipt_sha256 = "88".repeat(32);
        assert!(!binding_matches(&binding));
    }

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
        let expires = created + 100;
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
        let historical_now = now + MAX_EVIDENCE_AGE_SECS + 1;
        assert!(validate_controller_activation_receipt_timeline(
            ActivationReceiptFreshness::Historical,
            (created, expires),
            created,
            created + 1,
            created + 2,
            created + 3,
            historical_now,
        )
        .is_ok());
        assert!(validate_controller_activation_receipt_timeline(
            ActivationReceiptFreshness::Current,
            (created, expires),
            created,
            created + 1,
            created + 2,
            created + 3,
            historical_now,
        )
        .is_err());
    }

    #[test]
    fn historical_activation_evidence_must_stay_within_the_original_gate_b_window() {
        let created = 1_000_000;
        let expires = created + 100;
        let historical_now = expires + MAX_EVIDENCE_AGE_SECS;
        let validate = |authorized, bound, confirmed, verified| {
            validate_controller_activation_receipt_timeline(
                ActivationReceiptFreshness::Historical,
                (created, expires),
                authorized,
                bound,
                confirmed,
                verified,
                historical_now,
            )
        };

        assert!(validate(created, created + 1, created + 2, expires).is_ok());
        assert!(validate(expires + 1, expires + 1, expires + 1, expires + 1).is_err());
        assert!(validate(created, expires + 1, expires + 1, expires + 1).is_err());
        assert!(validate(created, created + 1, expires + 1, expires + 1).is_err());
        assert!(validate(created, created + 1, created + 2, expires + 1).is_err());
    }

    #[test]
    fn historical_seal_evidence_must_stay_within_the_original_gate_b_window() {
        assert_eq!(
            live_activation_pause_requirement(SealReceiptLiveContext::HistoricalEvidence),
            None
        );
        let created = 1_000_000;
        let expires = created + 100;

        assert!(validate_historical_evidence_window(created, expires, &[created, expires]).is_ok());
        assert!(
            validate_historical_evidence_window(created, expires, &[created - 1, created]).is_err()
        );
        assert!(
            validate_historical_evidence_window(created, expires, &[created, expires + 1]).is_err()
        );
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

    pub(super) fn valid_profile() -> Profile {
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

    #[test]
    fn historical_profile_schema_policy_is_bounded_to_v35_and_v36() {
        let mut profile = valid_profile();
        assert!(validate_profile(&profile, true).is_ok());
        assert!(validate_profile_with_schema_policy(
            &profile,
            true,
            ProfileSchemaPolicy::Historical,
        )
        .is_ok());

        profile.canister_schema_version = PREVIOUS_STABLE_SCHEMA_VERSION;
        assert!(validate_profile(&profile, true).is_err());
        assert!(validate_profile_with_schema_policy(
            &profile,
            true,
            ProfileSchemaPolicy::Historical,
        )
        .is_ok());

        for unknown in [
            PREVIOUS_STABLE_SCHEMA_VERSION - 1,
            CURRENT_STABLE_SCHEMA_VERSION + 1,
        ] {
            profile.canister_schema_version = unknown;
            assert!(validate_profile_with_schema_policy(
                &profile,
                true,
                ProfileSchemaPolicy::Historical,
            )
            .is_err());
        }
    }

    #[test]
    fn historical_gate_b_rejects_mixed_profile_schema_versions() {
        let mut profile = valid_profile();
        let mut gate_a_profile = profile.clone();
        assert!(validate_gate_b_profile_schema_convergence(&profile, &gate_a_profile).is_ok());

        profile.canister_schema_version = PREVIOUS_STABLE_SCHEMA_VERSION;
        assert!(validate_gate_b_profile_schema_convergence(&profile, &gate_a_profile).is_err());
        gate_a_profile.canister_schema_version = PREVIOUS_STABLE_SCHEMA_VERSION;
        assert!(validate_gate_b_profile_schema_convergence(&profile, &gate_a_profile).is_ok());
    }

    #[test]
    fn production_ui_runtime_profile_rendering_is_byte_stable() {
        let profile = valid_profile();
        let profile_bytes = canonical_bytes(&profile).unwrap();
        let manifest_sha256 = "a".repeat(64);
        let first = canonical_bytes(
            &ui_runtime_profile(&profile, &profile_bytes, true, Some(&manifest_sha256)).unwrap(),
        )
        .unwrap();
        let second = canonical_bytes(
            &ui_runtime_profile(&profile, &profile_bytes, true, Some(&manifest_sha256)).unwrap(),
        )
        .unwrap();
        assert_eq!(first, second);
        assert!(String::from_utf8(first)
            .unwrap()
            .contains(&format!("\"gateBManifestSha256\":\"{manifest_sha256}\"")));
    }

    #[test]
    fn production_ui_runtime_profile_binds_v36_current_module_and_reviewed_rpc() {
        let root = env::temp_dir().join(format!("bridge-ui-runtime-{}", process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut profile = valid_profile();
        profile.canister_schema_version = CURRENT_STABLE_SCHEMA_VERSION;
        let profile_bytes = canonical_bytes(&profile).unwrap();
        let manifest = "a".repeat(64);
        let module = "b".repeat(64);
        let rpc = br#"{"schema_version":1,"base_rpc_url":"https://base-mainnet.g.alchemy.com/v2/test-key"}"#;
        let expected = canonical_bytes(
            &production_current_ui_runtime_profile_value(
                &profile,
                &profile_bytes,
                &manifest,
                &module,
                rpc,
            )
            .unwrap(),
        )
        .unwrap();
        let parsed: Value = serde_json::from_slice(&expected).unwrap();
        assert_eq!(parsed["canisterSchemaVersion"], 36);
        assert_eq!(parsed["canisterModuleSha256"], module);
        assert!(parsed.get("postActivationUpgradeSha256").is_none());
        assert_eq!(
            parsed["profileFileSha256"],
            hex(&Sha256::digest(&profile_bytes))
        );
        assert_eq!(parsed["uiRpcConfigSha256"], hex(&Sha256::digest(rpc)));
        for schema in [34, 35, 37] {
            profile.canister_schema_version = schema;
            assert!(production_current_ui_runtime_profile_value(
                &profile,
                &profile_bytes,
                &manifest,
                &module,
                rpc
            )
            .is_err());
        }
        profile.canister_schema_version = 36;
        for invalid_rpc in [
            br#"{"schema_version":1,"base_rpc_url":"https://mainnet.base.org"}"#.as_slice(),
            br#"{"schema_version":1,"base_rpc_url":"https://base-mainnet.g.alchemy.com/v2/key?secret=1"}"#.as_slice(),
            br#"{"schema_version":2,"base_rpc_url":"https://base-mainnet.g.alchemy.com/v2/key"}"#.as_slice(),
            br#"{"schema_version":1,"base_rpc_url":"https://base-mainnet.g.alchemy.com/v2/key","override":true}"#.as_slice(),
        ] {
            assert!(production_current_ui_runtime_profile_value(
                &profile,
                &profile_bytes,
                &manifest,
                &module,
                invalid_rpc
            )
            .is_err());
        }
        fs::remove_dir_all(root).unwrap();
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

    fn matching_handover_status() -> BridgeStatusLiveView {
        BridgeStatusLiveView {
            reserve: ReserveStatusView { sufficient: true },
            deposits_paused: false,
            mint_authorization_ttl_seconds: 900,
            mint_authorization_epoch: 7,
            counts: ProductionStatusCountsView {
                deposits: 2,
                withdrawals: 3,
                reconciliation_holds: 1,
                pending_ledger_operations: 2,
                reserved_deposit_mint_amount: 50,
                reserved_deposit_mint_operations: 1,
                retained_audit_events: 8,
                pruned_audit_events: 5,
                retained_deposit_index_entries: 2,
            },
        }
    }

    #[test]
    fn production_storage_integrity_requires_the_gate_b_installer_identity_and_ok_response() {
        let installer = Principal::from_text("aaaaa-aa").unwrap();
        validate_production_installer_principal(installer, b"aaaaa-aa\n").unwrap();
        assert!(
            validate_production_installer_principal(installer, b"2vxsx-fae\n")
                .unwrap_err()
                .contains("differs from the Gate B sole controller")
        );
        assert!(validate_production_installer_principal(installer, &[0xff]).is_err());

        let ok = Encode!(&StorageIntegrityResultView::Ok("ok".into())).unwrap();
        assert!(matches!(
            decode_production_storage_integrity(hex(&ok).as_bytes()).unwrap(),
            StorageIntegrityResultView::Ok(value) if value == "ok"
        ));
        let unauthorized = Encode!(&StorageIntegrityResultView::Err(Reserved)).unwrap();
        assert!(matches!(
            decode_production_storage_integrity(hex(&unauthorized).as_bytes()).unwrap(),
            StorageIntegrityResultView::Err(_)
        ));
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
    fn provider_independence_binds_the_immutable_base_mainnet_service() {
        let profile = valid_profile();
        let release_id = "release-123";
        let source_revision = "a".repeat(40);
        let source_tree = "b".repeat(64);
        let profile_sha = "c".repeat(64);
        let manifest = ReleaseManifest {
            schema_version: 5,
            release_id: release_id.into(),
            test_only: false,
            source_revision: source_revision.clone(),
            source_tree_sha256: source_tree.clone(),
            created_at_unix: 2,
            expires_at_unix: 3,
            parent_gate_a_manifest_sha256: Some("d".repeat(64)),
            artifacts: vec![
                ArtifactDigest {
                    path: "profile.json".into(),
                    sha256: profile_sha.clone(),
                },
                ArtifactDigest {
                    path: "bridge-canister.wasm".into(),
                    sha256: profile.bridge_canister_wasm_sha256.clone(),
                },
            ],
        };
        let receipt = provider_independence_receipt(
            &profile,
            1,
            release_id,
            &source_revision,
            &source_tree,
            &profile_sha,
        )
        .unwrap();
        assert!(validate_provider_independence_binding(&receipt, &manifest, &profile).is_ok());

        let mut wrong_service = receipt.clone();
        wrong_service.rpc_service = "EthMainnet".into();
        assert!(
            validate_provider_independence_binding(&wrong_service, &manifest, &profile).is_err()
        );

        let mut wrong_threshold = receipt.clone();
        wrong_threshold.consensus_strategy.min = 1;
        assert!(
            validate_provider_independence_binding(&wrong_threshold, &manifest, &profile).is_err()
        );

        let mut wrong_source = receipt.clone();
        wrong_source.source_revision = "e".repeat(40);
        assert!(
            validate_provider_independence_binding(&wrong_source, &manifest, &profile).is_err()
        );

        for invalid in [
            {
                let mut value = receipt.clone();
                value.release_id = "release-999".into();
                value
            },
            {
                let mut value = receipt.clone();
                value.source_tree_sha256 = "e".repeat(64);
                value
            },
            {
                let mut value = receipt.clone();
                value.profile_sha256 = "e".repeat(64);
                value
            },
            {
                let mut value = receipt.clone();
                value.bridge_canister_wasm_sha256 = "e".repeat(64);
                value
            },
            {
                let mut value = receipt.clone();
                value.evm_rpc_canister_id = test_principal(30);
                value
            },
            {
                let mut value = receipt.clone();
                value.base_chain_id = 1;
                value
            },
            {
                let mut value = receipt.clone();
                value.provider_selection = "all".into();
                value
            },
            {
                let mut value = receipt.clone();
                value.custom_evm_rpc_urls_sha256 = "e".repeat(64);
                value
            },
            {
                let mut value = receipt.clone();
                value.consensus_strategy.total = 2;
                value
            },
            {
                let mut value = receipt.clone();
                value.guarantee_boundary = "none".into();
                value
            },
        ] {
            assert!(validate_provider_independence_binding(&invalid, &manifest, &profile).is_err());
        }

        let mut legacy = serde_json::to_value(&receipt).unwrap();
        legacy["schema_version"] = Value::from(1);
        let legacy: ProviderIndependenceReceipt = serde_json::from_value(legacy).unwrap();
        assert!(validate_provider_independence_binding(&legacy, &manifest, &profile).is_err());

        let mut custom = valid_profile();
        custom.base_rpc_url = Some("https://rpc.example".into());
        assert!(validate_production_rpc_profile(&custom).is_err());
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
    fn controller_activation_artifact_binds_exact_transaction_fields() {
        let mut profile = valid_profile();
        let salt = [0x5a; 32];
        let mut artifact = signed_activation_artifact(&profile, salt, 0, false);
        profile.governance_operator = artifact.sender.clone();
        let salt_hex = format!("0x{}", hex(&salt));
        assert!(validate_direct_activation_transaction_fields(
            "schedule", &profile, &salt_hex, &artifact,
        )
        .is_ok());

        let expected_target = artifact.target.clone();
        artifact.target = profile.bridge_contract.clone();
        assert!(validate_direct_activation_transaction_fields(
            "schedule", &profile, &salt_hex, &artifact,
        )
        .is_err());
        artifact.target = expected_target;

        artifact.calldata = format!("0x{}", "00".repeat(32));
        assert!(validate_direct_activation_transaction_fields(
            "schedule", &profile, &salt_hex, &artifact,
        )
        .is_err());
        artifact.calldata = signed_activation_artifact(&profile, salt, 0, false).calldata;

        for drift in [
            ("sender", "0x".to_owned() + &"33".repeat(20)),
            ("nonce", "8".into()),
            ("gas_limit", "100001".into()),
            ("max_fee_per_gas", "21".into()),
            ("max_priority_fee_per_gas", "3".into()),
            ("transaction_hash", "0x".to_owned() + &"00".repeat(32)),
        ] {
            let mut drifted = signed_activation_artifact(&profile, salt, 0, false);
            match drift.0 {
                "sender" => drifted.sender = drift.1,
                "nonce" => drifted.nonce = drift.1,
                "gas_limit" => drifted.gas_limit = drift.1,
                "max_fee_per_gas" => drifted.max_fee_per_gas = drift.1,
                "max_priority_fee_per_gas" => drifted.max_priority_fee_per_gas = drift.1,
                "transaction_hash" => drifted.transaction_hash = drift.1,
                _ => unreachable!(),
            }
            assert!(
                validate_signed_eip1559_artifact(&drifted).is_err(),
                "{}",
                drift.0
            );
        }

        assert!(
            validate_signed_eip1559_artifact(
                &signed_activation_artifact(&profile, salt, 1, false,)
            )
            .is_err()
        );
        assert!(
            validate_signed_eip1559_artifact(&signed_activation_artifact(&profile, salt, 0, true,))
                .is_err()
        );
        let mut wrong_type = signed_activation_artifact(&profile, salt, 0, false);
        wrong_type.raw_transaction.replace_range(2..4, "01");
        assert!(validate_signed_eip1559_artifact(&wrong_type).is_err());
        let mut trailing = signed_activation_artifact(&profile, salt, 0, false);
        trailing.raw_transaction.push_str("00");
        assert!(validate_signed_eip1559_artifact(&trailing).is_err());
        let mut noncanonical = signed_activation_artifact(&profile, salt, 0, false);
        noncanonical.raw_transaction = "0x02b80100".into();
        noncanonical.transaction_hash = format!(
            "0x{}",
            hex(&keccak256(
                &decode_hex(&noncanonical.raw_transaction).unwrap()
            ))
        );
        assert!(validate_signed_eip1559_artifact(&noncanonical).is_err());

        let strict = signed_activation_artifact(&profile, salt, 0, false);
        let mut dual_kind = serde_json::to_value(&strict).unwrap();
        dual_kind["kind"]["ExecuteActivation"] = dual_kind["kind"]["ScheduleActivation"].clone();
        assert!(serde_json::from_value::<DirectActivationArtifact>(dual_kind).is_err());
        let mut extra_kind_field = serde_json::to_value(&strict).unwrap();
        extra_kind_field["kind"]["ScheduleActivation"]["extra"] = Value::Bool(true);
        assert!(serde_json::from_value::<DirectActivationArtifact>(extra_kind_field).is_err());
    }

    #[test]
    fn confirmation_input_does_not_revalidate_phase_attestation() {
        assert_eq!(
            live_activation_pause_requirement(SealReceiptLiveContext::ConfirmationInput),
            None
        );
        assert_eq!(
            live_activation_pause_requirement(SealReceiptLiveContext::PendingResume),
            Some(true)
        );
        assert_eq!(
            live_activation_pause_requirement(SealReceiptLiveContext::ScheduleFinalization),
            Some(true)
        );
        assert_eq!(
            live_activation_pause_requirement(SealReceiptLiveContext::ExecuteFinalization),
            Some(false)
        );
    }

    #[test]
    fn controller_activation_confirmation_authenticates_generation_and_signing_time() {
        let profile = valid_profile();
        let salt = [0x5a; 32];
        let mut artifact = signed_activation_artifact(&profile, salt, 0, false);
        artifact.generation = 3;
        artifact.signed_at_ns = "42".into();
        let mut confirmation = ActivationConfirmationStatusView {
            phase: "schedule".into(),
            governance_operation_id: 7,
            timelock_operation_id: vec![0x11; 32],
            transaction_hash: decode_hex(&artifact.transaction_hash).unwrap(),
            receipt_block_number: 10,
            generation: 3,
            signed_at_ns: 42,
        };
        assert!(
            activation_confirmation_artifact_metadata_matches(&confirmation, &artifact).unwrap()
        );
        confirmation.generation = 4;
        assert!(
            !activation_confirmation_artifact_metadata_matches(&confirmation, &artifact).unwrap()
        );
        confirmation.generation = 3;
        confirmation.signed_at_ns = 43;
        assert!(
            !activation_confirmation_artifact_metadata_matches(&confirmation, &artifact).unwrap()
        );
    }

    #[test]
    fn gate_b_bundle_excludes_historical_upgrade_artifacts() {
        assert_eq!(GATE_B_ARTIFACTS.len(), 11);
        assert!(!GATE_B_ARTIFACTS.contains(&"production-canister-upgrade-receipt.json"));
        assert!(!GATE_B_ARTIFACTS.contains(&"post-gate-a-policy-transition.json"));
        assert!(GATE_B_ARTIFACTS.contains(&"gate-a-receipt.json"));
        assert!(GATE_B_ARTIFACTS.contains(&"gate-a-profile.json"));
    }

    #[test]
    fn current_state_gate_rejects_each_operational_drift() {
        let controller = Principal::from_text(test_principal(70)).unwrap();
        let expected_controllers = BTreeSet::from([controller]);
        let module = vec![0x55; 32];
        let module_sha256 = hex(&module);
        let activation = |paused| ActivationStatusView {
            deposits_paused: paused,
            pending_timelock_operation: None,
            last_confirmed_activation: Some(ActivationConfirmationStatusView {
                phase: "execute".into(),
                governance_operation_id: 2,
                timelock_operation_id: vec![1; 32],
                transaction_hash: vec![2; 32],
                receipt_block_number: 3,
                generation: 1,
                signed_at_ns: 4,
            }),
        };
        let runtime = |schema_version| RuntimeBindingView {
            base_chain_id: 8453,
            bridge_contract: vec![0; 20],
            expected_bridge_runtime_sha256: vec![0; 32],
            timelock_contract: vec![0; 20],
            deployment_instance_id: vec![0; 32],
            minimum_withdrawal_id: vec![0; 32],
            ledger_canister_id: Principal::anonymous(),
            index_canister_id: Principal::anonymous(),
            schema_version,
            expected_bridge_signer: vec![0; 20],
            evm_rpc_canister_id: Principal::anonymous(),
            rpc_provider_urls_sha256: vec![0; 32],
            operational_config_sha256: vec![0; 32],
        };
        let status = |paused| {
            let mut value = matching_handover_status();
            value.deposits_paused = paused;
            value
        };
        let validate = |controllers: &[Principal],
                        module_hash: &[u8],
                        lifecycle,
                        activation: &ActivationStatusView,
                        runtime: &RuntimeBindingView,
                        status: &BridgeStatusLiveView,
                        pending: &PendingGovernanceTransactionsView,
                        history_ready,
                        registered,
                        storage_ok| {
            validate_production_current_state_core(
                controllers,
                &expected_controllers,
                module_hash,
                &module_sha256,
                lifecycle,
                activation,
                runtime,
                status,
                pending,
                history_ready,
                registered,
                storage_ok,
            )
        };
        let ok_activation = activation(false);
        let ok_runtime = runtime(36);
        let ok_status = status(false);
        let ok_pending = PendingGovernanceTransactionsView::Ok(Vec::new());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &ok_pending,
            true,
            false,
            true,
        )
        .is_ok());
        assert!(validate(
            &[controller, Principal::anonymous()],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &ok_pending,
            true,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &[0; 32],
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &ok_pending,
            true,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Bootstrap,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &ok_pending,
            true,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &activation(true),
            &ok_runtime,
            &ok_status,
            &ok_pending,
            true,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &runtime(35),
            &ok_status,
            &ok_pending,
            true,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &status(true),
            &ok_pending,
            true,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &PendingGovernanceTransactionsView::Err(Reserved),
            true,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &ok_pending,
            false,
            false,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &ok_pending,
            true,
            true,
            true
        )
        .is_err());
        assert!(validate(
            &[controller],
            &module,
            ProductionLifecycleView::Activated,
            &ok_activation,
            &ok_runtime,
            &ok_status,
            &ok_pending,
            true,
            false,
            false
        )
        .is_err());

        let snapshot = ProductionUpgradeCurrentStateSnapshot {
            lifecycle: vec![1],
            activation_attestation: vec![2],
            activation_status: vec![3],
            runtime_binding: vec![4],
            bridge_status: ok_status.clone(),
            pending_governance: vec![5],
            withdrawal_index_probe: vec![6],
            operational_config: vec![7],
            control_plane_addresses: vec![8],
            storage_integrity: vec![9],
            controllers: expected_controllers.clone(),
            module_hash: module.clone(),
        };
        let mut upgraded = snapshot.clone();
        upgraded.module_hash = vec![0x66; 32];
        assert!(snapshot.preserved_across_upgrade(&upgraded));
        assert!(!snapshot.preserved_across_root_addition(&upgraded));
        assert_eq!(
            classify_production_upgrade_observation(
                &snapshot,
                &upgraded,
                &module_sha256,
                &hex(&upgraded.module_hash),
            )
            .unwrap(),
            ProductionUpgradeObservedOutcome::CandidatePreserved
        );
        assert_eq!(
            classify_production_upgrade_observation(
                &snapshot,
                &snapshot,
                &module_sha256,
                &hex(&upgraded.module_hash),
            )
            .unwrap(),
            ProductionUpgradeObservedOutcome::CurrentUnresolved
        );
        let mut unexpected_module = snapshot.clone();
        unexpected_module.module_hash = vec![0x77; 32];
        assert!(classify_production_upgrade_observation(
            &snapshot,
            &unexpected_module,
            &module_sha256,
            &hex(&upgraded.module_hash),
        )
        .is_err());

        let mut joint = snapshot.clone();
        let root = Principal::from_text(KINIC_ROOT).unwrap();
        joint.controllers.insert(root);
        assert!(snapshot.preserved_across_root_addition(&joint));
        assert!(!snapshot.preserved_across_upgrade(&joint));
        assert!(validate_production_root_addition_observation(
            &snapshot,
            &joint,
            controller,
            root,
            &module_sha256,
        )
        .is_ok());
        let mut third_controller = joint.clone();
        third_controller.controllers.insert(Principal::anonymous());
        assert!(validate_production_root_addition_observation(
            &snapshot,
            &third_controller,
            controller,
            root,
            &module_sha256,
        )
        .is_err());

        type SnapshotDrift = Box<dyn Fn(&mut ProductionUpgradeCurrentStateSnapshot)>;
        let drifts: Vec<SnapshotDrift> = vec![
            Box::new(|value| value.lifecycle.push(0)),
            Box::new(|value| value.activation_attestation.push(0)),
            Box::new(|value| value.activation_status.push(0)),
            Box::new(|value| value.runtime_binding.push(0)),
            Box::new(|value| value.bridge_status.mint_authorization_epoch += 1),
            Box::new(|value| value.pending_governance.push(0)),
            Box::new(|value| value.withdrawal_index_probe.push(0)),
            Box::new(|value| value.operational_config.push(0)),
            Box::new(|value| value.control_plane_addresses.push(0)),
            Box::new(|value| value.storage_integrity.push(0)),
        ];
        for drift in drifts {
            let mut after_upgrade = upgraded.clone();
            drift(&mut after_upgrade);
            assert!(!snapshot.preserved_across_upgrade(&after_upgrade));
            assert!(classify_production_upgrade_observation(
                &snapshot,
                &after_upgrade,
                &module_sha256,
                &hex(&upgraded.module_hash),
            )
            .is_err());

            let mut after_handover = joint.clone();
            drift(&mut after_handover);
            assert!(!snapshot.preserved_across_root_addition(&after_handover));
            assert!(validate_production_root_addition_observation(
                &snapshot,
                &after_handover,
                controller,
                root,
                &module_sha256,
            )
            .is_err());
        }
    }
}
