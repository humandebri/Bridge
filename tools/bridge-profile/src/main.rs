#![recursion_limit = "256"]

use candid::{CandidType, Decode, Encode, Nat, Principal, Reserved};
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
const PRODUCTION_PAUSE_PRINCIPAL: &str =
    "lqfvd-m7ihy-e5dvc-gngvr-blzbt-pupeq-6t7ua-r7v4p-bvqjw-ea7gl-4qe";
const OFFICIAL_EVM_RPC_CANISTER: &str = "7hfb6-caaaa-aaaar-qadga-cai";
const MAX_EVIDENCE_AGE_SECS: u64 = 90 * 24 * 60 * 60;
const MAX_ACTIVATION_ATTESTATION_AGE_SECS: u64 = 5 * 60;
const CURRENT_STABLE_SCHEMA_VERSION: u16 = 35;
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
const GATE_B_ARTIFACTS: [&str; 13] = [
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
    "production-canister-upgrade-receipt.json",
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
    upgrade_source_revision: String,
    upgrade_source_tree_sha256: String,
    to_source_revision: String,
    to_source_tree_sha256: String,
    bridge_canister_id: String,
    bridge_contract: String,
    bsns_contract: String,
    timelock_contract: String,
    from_bridge_canister_wasm_sha256: String,
    to_bridge_canister_wasm_sha256: String,
    production_canister_upgrade_receipt_sha256: String,
    bridge_runtime_bytecode_sha256: String,
    bsns_runtime_bytecode_sha256: String,
    bsns_runtime_template_sha256: String,
    bridge_deployment_transaction_hash: String,
    timelock_deployment_transaction_hash: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionCanisterUpgradeReceipt {
    schema_version: u8,
    kind: String,
    source_revision: String,
    source_tree_sha256: String,
    bridge_canister_id: String,
    install_mode: String,
    executing_principal: String,
    executed_at_unix: u64,
    verified_at_unix: u64,
    recovered: bool,
    recovered_at_unix: Option<u64>,
    before_controllers: Vec<String>,
    after_controllers: Vec<String>,
    before_module_sha256: String,
    after_module_sha256: String,
    wasm_sha256: String,
    before_schema_version: u16,
    after_schema_version: u16,
    before_lifecycle: String,
    after_lifecycle: String,
    before_deposits_paused: bool,
    after_deposits_paused: bool,
    before_storage_validation_complete: bool,
    after_storage_validation_complete: bool,
    before_management_status_json_hex: String,
    before_management_status_json_sha256: String,
    after_management_status_json_hex: String,
    after_management_status_json_sha256: String,
    before_bridge_status_response_hex: String,
    before_bridge_status_response_sha256: String,
    after_bridge_status_response_hex: String,
    after_bridge_status_response_sha256: String,
    before_lifecycle_response_hex: String,
    before_lifecycle_response_sha256: String,
    after_lifecycle_response_hex: String,
    after_lifecycle_response_sha256: String,
    before_runtime_binding_response_hex: String,
    before_runtime_binding_response_sha256: String,
    after_runtime_binding_response_hex: String,
    after_runtime_binding_response_sha256: String,
    before_storage_integrity_response_hex: String,
    before_storage_integrity_response_sha256: String,
    after_storage_integrity_response_hex: String,
    after_storage_integrity_response_sha256: String,
    before_public_state_sha256: String,
    after_public_state_sha256: String,
    command_argv: Vec<String>,
    chunk_upload_evidence_json_hex: String,
    chunk_upload_evidence_json_sha256: String,
    submission_json_hex: String,
    submission_json_sha256: String,
    request_id: String,
    response_stdout_hex: String,
    response_stdout_sha256: String,
    response_stderr_hex: String,
    response_stderr_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionCanisterUpgradeChain {
    schema_version: u8,
    kind: String,
    entries: Vec<ProductionCanisterUpgradeChainEntry>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionCanisterUpgradeChainEntry {
    sequence: u8,
    previous_receipt_sha256: Option<String>,
    receipt_sha256: String,
    receipt_json_hex: String,
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
struct ProductionUpgradeChunkResponse {
    schema_version: u8,
    index: u32,
    request_id: String,
    response_hex: String,
    response_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionUpgradeStoredChunksResponse {
    schema_version: u8,
    request_id: String,
    response_hex: String,
    response_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProductionUpgradeUploadEvidence {
    schema_version: u8,
    stored_chunks_request_id: String,
    stored_chunks_response_hex: String,
    stored_chunks_response_sha256: String,
    chunks: Vec<ProductionUpgradeChunkResponse>,
}

#[derive(Serialize)]
struct ProductionUpgradeSendError {
    schema_version: u8,
    request_kind: String,
    request_id: String,
    observed_at_ns: u64,
    error: String,
    error_sha256: String,
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
struct MonitoringPaidObservation {
    observed_at_unix: u64,
    state: String,
    response_hex: String,
    response_sha256: String,
    authenticated_query: bool,
}

#[derive(CandidType, Deserialize, Serialize, Debug, Eq, PartialEq)]
#[allow(dead_code)]
enum WithdrawalPhaseView {
    Paid,
    ReleasePending,
    ReconciliationHold,
    Observed,
}

#[derive(CandidType, Deserialize, Serialize, Debug, Eq, PartialEq)]
#[allow(dead_code)]
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

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct ControllerHandover {
    schema_version: u8,
    stage: String,
    observed_at_unix: u64,
    source_revision: String,
    source_tree_sha256: String,
    gate_b_manifest_sha256: String,
    operational_config_seal_receipt_sha256: String,
    controller_schedule_receipt_sha256: String,
    controller_execute_receipt_sha256: String,
    bridge_canister_id: String,
    sns_root_canister_id: String,
    executing_principal: String,
    command_argv: Vec<String>,
    request_id: String,
    response_exit_code: i32,
    response_stdout_hex: String,
    response_stderr_hex: String,
    response_sha256: String,
    before_controllers: Vec<String>,
    before_module_sha256: String,
    pre_send_controllers: Vec<String>,
    pre_send_module_sha256: String,
    final_controllers: Vec<String>,
    after_module_sha256: String,
    before_management_status_response_json_hex: String,
    before_management_status_response_sha256: String,
    pre_send_management_status_response_json_hex: String,
    pre_send_management_status_response_sha256: String,
    pre_send_bridge_status_response_json_hex: String,
    pre_send_bridge_status_response_sha256: String,
    pre_send_lifecycle_response_json_hex: String,
    pre_send_lifecycle_response_sha256: String,
    pre_send_runtime_binding_response_json_hex: String,
    pre_send_runtime_binding_response_sha256: String,
    pre_send_storage_integrity_response_json_hex: String,
    pre_send_storage_integrity_response_sha256: String,
    pre_send_activation_status_response_json_hex: String,
    pre_send_activation_status_response_sha256: String,
    pre_send_activation_attestation_response_json_hex: String,
    pre_send_activation_attestation_response_sha256: String,
    after_management_status_response_json_hex: String,
    after_management_status_response_sha256: String,
    before_bridge_status_response_json_hex: String,
    before_bridge_status_response_sha256: String,
    after_bridge_status_response_json_hex: String,
    after_bridge_status_response_sha256: String,
    before_lifecycle_response_json_hex: String,
    before_lifecycle_response_sha256: String,
    after_lifecycle_response_json_hex: String,
    after_lifecycle_response_sha256: String,
    before_runtime_binding_response_json_hex: String,
    before_runtime_binding_response_sha256: String,
    after_runtime_binding_response_json_hex: String,
    after_runtime_binding_response_sha256: String,
    before_storage_integrity_response_json_hex: String,
    before_storage_integrity_response_sha256: String,
    after_storage_integrity_response_json_hex: String,
    after_storage_integrity_response_sha256: String,
    before_activation_status_response_json_hex: String,
    before_activation_status_response_sha256: String,
    after_activation_status_response_json_hex: String,
    after_activation_status_response_sha256: String,
    before_activation_attestation_response_json_hex: String,
    before_activation_attestation_response_sha256: String,
    after_activation_attestation_response_json_hex: String,
    after_activation_attestation_response_sha256: String,
    cycles_balance: u128,
    freezing_threshold_seconds: u64,
    idle_cycles_burned_per_day: u128,
    required_freezing_cycles: u128,
    pre_send_cycles_balance: u128,
    pre_send_required_freezing_cycles: u128,
    #[serde(default)]
    pre_send_checkpoint_json_hex: String,
    #[serde(default)]
    pre_send_checkpoint_sha256: String,
    #[serde(default)]
    recovered_without_request_id: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
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
    generation: u8,
    signed_at_ns: u64,
}

#[derive(CandidType, Deserialize)]
enum ActivationStatusResultView {
    Ok(ActivationStatusView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
enum PendingGovernanceTransactionsView {
    Ok(Vec<Reserved>),
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

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq)]
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

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq)]
struct ReserveStatusView {
    sufficient: bool,
}

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq)]
struct BridgeStatusLiveView {
    reserve: ReserveStatusView,
    deposits_paused: bool,
    mint_authorization_ttl_seconds: u64,
    mint_authorization_epoch: u64,
    counts: ProductionStatusCountsView,
}

#[derive(CandidType, Deserialize, Clone, PartialEq, Eq)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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

#[allow(dead_code)]
fn evm_topic(signature: &str) -> String {
    let mut hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(signature.as_bytes());
    keccak.finalize(&mut hash);
    format!("0x{}", hex(&hash))
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

fn validate_production_upgrade_gate_a_binding(
    profile: &Profile,
    profile_source: &[u8],
    receipt: &GateAReceipt,
) -> Result<(), String> {
    validate_profile(profile, true)?;
    validate_production_canister_receipt(profile, &receipt.canister_install)?;
    let expected_post_deploy_profile_sha256 =
        post_deploy_profile_sha256(profile_source, receipt.bridge_deployment_block_number)?;
    if receipt.schema_version != 2
        || profile.deployment_block != 0
        || !receipt
            .gate_a_profile_sha256
            .eq_ignore_ascii_case(&hex(&canonical_sha256(profile)?))
        || !receipt
            .post_deploy_profile_sha256
            .eq_ignore_ascii_case(&expected_post_deploy_profile_sha256)
        || !receipt
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
    {
        return Err("Gate A profile and receipt are not an immutable install binding".into());
    }
    Ok(())
}

fn validate_production_upgrade_gate_a_binding_files(
    profile_path: &Path,
    receipt_path: &Path,
) -> Result<String, String> {
    let profile_source = fs::read(profile_path).map_err(|error| error.to_string())?;
    let profile: Profile =
        serde_json::from_slice(&profile_source).map_err(|error| error.to_string())?;
    let receipt: GateAReceipt = read_json(receipt_path)?;
    validate_production_upgrade_gate_a_binding(&profile, &profile_source, &receipt)?;
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
    let profile_source = fs::read(bundle.root.join("profile.json")).map_err(|e| e.to_string())?;
    let post_deploy_profile_sha256 =
        post_deploy_profile_sha256(&profile_source, receipt.bridge_deployment_block_number)?;
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
    seal_receipt_path: &Path,
    schedule_receipt_path: &Path,
    execute_receipt_path: &Path,
) -> Result<(ValidatedBundle, GateAReceipt, ControllerActivationReceipt), String> {
    validate_production_handover_evidence_files(
        bundle_path,
        seal_receipt_path,
        schedule_receipt_path,
        execute_receipt_path,
        SealReceiptLiveContext::HandoverPreTransfer,
    )
}

fn validate_production_handover_evidence_files(
    bundle_path: &Path,
    seal_receipt_path: &Path,
    schedule_receipt_path: &Path,
    execute_receipt_path: &Path,
    live_context: SealReceiptLiveContext,
) -> Result<(ValidatedBundle, GateAReceipt, ControllerActivationReceipt), String> {
    let bundle = validate_historical_gate_b_bundle(bundle_path)?;
    if bundle.manifest.schema_version != 4 {
        return Err("controller handover requires the current Gate B bundle".into());
    }
    let seal_receipt_sha256 =
        validate_operational_config_seal_receipt(&bundle, seal_receipt_path, live_context)?;
    let schedule_receipt: ControllerActivationReceipt = read_json(schedule_receipt_path)?;
    validate_controller_schedule_receipt(
        &bundle,
        &schedule_receipt,
        &seal_receipt_sha256,
        ActivationReceiptFreshness::Historical,
    )?;
    let execute_receipt: ControllerActivationReceipt = read_json(execute_receipt_path)?;
    validate_controller_execute_receipt(
        &bundle,
        &execute_receipt,
        &seal_receipt_sha256,
        schedule_receipt_path,
        ActivationReceiptFreshness::Historical,
    )?;
    let gate_a_receipt: GateAReceipt = read_json(&bundle.root.join("gate-a-receipt.json"))?;
    Ok((bundle, gate_a_receipt, execute_receipt))
}

fn validate_controller_handover_completion_files(
    bundle_path: &Path,
    seal_receipt_path: &Path,
    schedule_receipt_path: &Path,
    execute_receipt_path: &Path,
    handover_path: &Path,
) -> Result<(), String> {
    let (bundle, gate_a_receipt, _) = validate_production_handover_evidence_files(
        bundle_path,
        seal_receipt_path,
        schedule_receipt_path,
        execute_receipt_path,
        SealReceiptLiveContext::HandoverPostTransfer,
    )?;
    let handover: ControllerHandover = read_json(handover_path)?;
    validate_controller_handover_lineage(
        &handover,
        &bundle,
        seal_receipt_path,
        schedule_receipt_path,
        execute_receipt_path,
    )?;
    validate_controller_handover_completion(
        &handover,
        &bundle.profile,
        &gate_a_receipt.canister_install.installer_principal,
        bundle.manifest.created_at_unix,
        now_unix()?,
    )
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

#[cfg(test)]
const REHEARSAL_VALIDATOR: &str = include_str!("../../../scripts/evm-rpc-rehearsal/rehearsal.py");

#[cfg(test)]
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

#[cfg(test)]
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

fn validate_handover_completion_time(
    observed_at: u64,
    manifest_created: u64,
    now: u64,
) -> Result<(), String> {
    if observed_at < manifest_created
        || observed_at > now
        || now - observed_at > MAX_EVIDENCE_AGE_SECS
    {
        return Err(
            "handover completion timestamp predates Gate B, is future-dated, or is too old".into(),
        );
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

fn handover_json_evidence(raw_hex: &str, expected_sha256: &str) -> Result<Value, String> {
    let raw = decode_hex(raw_hex)?;
    if !valid_sha256(expected_sha256)
        || !hex(&Sha256::digest(&raw)).eq_ignore_ascii_case(expected_sha256)
    {
        return Err("controller handover snapshot digest mismatch".into());
    }
    serde_json::from_slice(&raw).map_err(|error| error.to_string())
}

fn single_json_key<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    let mut found = Vec::new();
    collect_json_key(value, key, &mut found);
    if found.len() != 1 {
        return Err(format!(
            "controller handover snapshot must contain one {key}"
        ));
    }
    Ok(found[0])
}

fn json_u128(value: &Value, key: &str) -> Result<u128, String> {
    let value = single_json_key(value, key)?;
    value
        .as_u64()
        .map(u128::from)
        .or_else(|| {
            value
                .as_str()
                .and_then(|value| value.replace('_', "").parse().ok())
        })
        .ok_or_else(|| format!("controller handover {key} is not an integer"))
}

fn handover_json_text(value: &Value, key: &str) -> Result<String, String> {
    single_json_key(value, key)?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("controller handover {key} is not text"))
}

fn handover_json_blob(value: &Value, key: &str) -> Result<Vec<u8>, String> {
    let value = single_json_key(value, key)?;
    if let Some(text) = value.as_str() {
        return decode_hex(text);
    }
    value
        .as_array()
        .ok_or_else(|| format!("controller handover {key} is not a blob"))?
        .iter()
        .map(|byte| {
            byte.as_u64()
                .and_then(|byte| u8::try_from(byte).ok())
                .ok_or_else(|| format!("controller handover {key} has an invalid byte"))
        })
        .collect()
}

fn handover_runtime_binding(value: &Value) -> Result<(LiveRuntimeBinding, Vec<u8>), String> {
    let principal = |key| -> Result<String, String> {
        Principal::from_text(handover_json_text(value, key)?)
            .map(|principal| principal.to_text())
            .map_err(|error| error.to_string())
    };
    let blob_hex = |key| handover_json_blob(value, key).map(|bytes| hex(&bytes));
    Ok((
        LiveRuntimeBinding {
            base_chain_id: u64::try_from(json_u128(value, "base_chain_id")?)
                .map_err(|_| "controller handover base_chain_id exceeds nat64")?,
            bridge_contract: format!("0x{}", blob_hex("bridge_contract")?),
            timelock_contract: format!("0x{}", blob_hex("timelock_contract")?),
            deployment_instance_id: format!("0x{}", blob_hex("deployment_instance_id")?),
            minimum_withdrawal_id: format!("0x{}", blob_hex("minimum_withdrawal_id")?),
            ledger_canister_id: principal("ledger_canister_id")?,
            index_canister_id: principal("index_canister_id")?,
            schema_version: u16::try_from(json_u128(value, "schema_version")?)
                .map_err(|_| "controller handover schema_version exceeds nat16")?,
            expected_bridge_signer: format!("0x{}", blob_hex("expected_bridge_signer")?),
            evm_rpc_canister_id: principal("evm_rpc_canister_id")?,
            rpc_provider_urls_sha256: blob_hex("rpc_provider_urls_sha256")?,
            operational_config_sha256: blob_hex("operational_config_sha256")?,
        },
        handover_json_blob(value, "expected_bridge_runtime_sha256")?,
    ))
}

fn handover_request_ids(response: &str) -> Result<BTreeSet<String>, String> {
    let response = response.to_ascii_lowercase();
    let mut ids = BTreeSet::new();
    for label in ["request_id", "request-id", "request id"] {
        let mut rest = response.as_str();
        while let Some(index) = rest.find(label) {
            let tail = rest[index + label.len()..]
                .trim_start_matches([' ', '\t', '\r', '\n', '=', ':', '"', '\'']);
            let tail = tail.strip_prefix("0x").unwrap_or(tail);
            let candidate = tail.get(..64).ok_or("handover request ID is truncated")?;
            if !candidate.bytes().all(|byte| byte.is_ascii_hexdigit())
                || tail.as_bytes().get(64).is_some_and(u8::is_ascii_hexdigit)
            {
                return Err("handover request ID is malformed".into());
            }
            ids.insert(candidate.to_owned());
            rest = &tail[64..];
        }
    }
    Ok(ids)
}

fn validate_controller_handover_lineage(
    handover: &ControllerHandover,
    bundle: &ValidatedBundle,
    seal_receipt_path: &Path,
    schedule_receipt_path: &Path,
    execute_receipt_path: &Path,
) -> Result<(), String> {
    let file_sha256 = |path: &Path| -> Result<String, String> {
        Ok(hex(&Sha256::digest(
            fs::read(path).map_err(|error| error.to_string())?,
        )))
    };
    if !controller_handover_lineage_fields_match(
        handover,
        &bundle.manifest.source_revision,
        &bundle.manifest.source_tree_sha256,
        &bundle.manifest_sha256,
        &file_sha256(seal_receipt_path)?,
        &file_sha256(schedule_receipt_path)?,
        &file_sha256(execute_receipt_path)?,
    ) {
        return Err("controller handover completion lineage is invalid".into());
    }
    Ok(())
}

fn validate_controller_handover_recovery_files(
    bundle_path: &Path,
    seal_receipt_path: &Path,
    schedule_receipt_path: &Path,
    execute_receipt_path: &Path,
    checkpoint_path: &Path,
) -> Result<(), String> {
    let (bundle, gate_a_receipt, _) = validate_production_handover_evidence_files(
        bundle_path,
        seal_receipt_path,
        schedule_receipt_path,
        execute_receipt_path,
        SealReceiptLiveContext::HandoverPostTransfer,
    )?;
    let checkpoint: Value = read_json(checkpoint_path)?;
    let object = checkpoint
        .as_object()
        .ok_or("controller handover recovery checkpoint is not an object")?;
    let text = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("controller handover recovery checkpoint lacks {key}"))
    };
    let strings = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_array)
            .ok_or_else(|| format!("controller handover recovery checkpoint lacks {key}"))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("controller handover recovery {key} is malformed"))
            })
            .collect::<Result<Vec<_>, _>>()
    };
    let digest = |path: &Path| -> Result<String, String> {
        Ok(hex(&Sha256::digest(
            fs::read(path).map_err(|error| error.to_string())?,
        )))
    };
    let installer = &gate_a_receipt.canister_install.installer_principal;
    let stage = text("stage")?;
    if object.get("schema_version").and_then(Value::as_u64) != Some(4)
        || ![
            "pre_send_checkpoint",
            "controller_update_uncertain",
            "controller_update_submitted",
        ]
        .contains(&stage)
        || text("source_revision")? != bundle.manifest.source_revision
        || !text("source_tree_sha256")?.eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || !text("gate_b_manifest_sha256")?.eq_ignore_ascii_case(&bundle.manifest_sha256)
        || !text("operational_config_seal_receipt_sha256")?
            .eq_ignore_ascii_case(&digest(seal_receipt_path)?)
        || !text("controller_schedule_receipt_sha256")?
            .eq_ignore_ascii_case(&digest(schedule_receipt_path)?)
        || !text("controller_execute_receipt_sha256")?
            .eq_ignore_ascii_case(&digest(execute_receipt_path)?)
        || text("bridge_canister_id")? != bundle.profile.bridge_canister_id
        || text("sns_root_canister_id")? != KINIC_ROOT
        || text("executing_principal")? != installer
        || strings("pre_send_controllers")? != [installer.clone()]
        || !text("pre_send_module_sha256")?
            .eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
    {
        return Err("controller handover recovery checkpoint lineage is invalid".into());
    }
    for prefix in [
        "pre_send_management_status",
        "pre_send_bridge_status",
        "pre_send_lifecycle",
        "pre_send_runtime_binding",
        "pre_send_storage_integrity",
        "pre_send_activation_status",
        "pre_send_activation_attestation",
    ] {
        let raw = text(&format!("{prefix}_response_json_hex"))?;
        let digest = text(&format!("{prefix}_response_sha256"))?;
        handover_json_evidence(raw, digest)?;
    }
    let mut response = decode_hex(text("response_stdout_hex")?)?;
    response.extend_from_slice(&decode_hex(text("response_stderr_hex")?)?);
    if !valid_sha256(text("response_sha256")?)
        || !hex(&Sha256::digest(&response)).eq_ignore_ascii_case(text("response_sha256")?)
    {
        return Err("controller handover recovery response digest is invalid".into());
    }
    let request_id = text("request_id")?.trim_start_matches("0x");
    if stage == "controller_update_submitted"
        && (!valid_sha256(request_id)
            || handover_request_ids(&String::from_utf8_lossy(&response))?
                != BTreeSet::from([request_id.to_ascii_lowercase()]))
    {
        return Err("controller handover submitted checkpoint request ID is invalid".into());
    }
    let command = strings("command_argv")?;
    if command
        != [
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
            "--debug",
        ]
    {
        return Err("controller handover recovery command is not the fixed transfer".into());
    }
    Ok(())
}

fn controller_handover_lineage_fields_match(
    handover: &ControllerHandover,
    source_revision: &str,
    source_tree_sha256: &str,
    gate_b_manifest_sha256: &str,
    seal_receipt_sha256: &str,
    schedule_receipt_sha256: &str,
    execute_receipt_sha256: &str,
) -> bool {
    handover.source_revision == source_revision
        && handover
            .source_tree_sha256
            .eq_ignore_ascii_case(source_tree_sha256)
        && handover
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(gate_b_manifest_sha256)
        && handover
            .operational_config_seal_receipt_sha256
            .eq_ignore_ascii_case(seal_receipt_sha256)
        && handover
            .controller_schedule_receipt_sha256
            .eq_ignore_ascii_case(schedule_receipt_sha256)
        && handover
            .controller_execute_receipt_sha256
            .eq_ignore_ascii_case(execute_receipt_sha256)
}

fn controller_handover_checkpoint_matches(
    handover: &ControllerHandover,
    checkpoint: &Value,
) -> bool {
    let Some(checkpoint) = checkpoint.as_object() else {
        return false;
    };
    let string_matches =
        |key: &str, expected: &str| checkpoint.get(key).and_then(Value::as_str) == Some(expected);
    let strings_match = |key: &str, expected: &[String]| {
        checkpoint
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|values| {
                values
                    .iter()
                    .map(Value::as_str)
                    .eq(expected.iter().map(|value| Some(value.as_str())))
            })
    };
    checkpoint.get("schema_version").and_then(Value::as_u64) == Some(4)
        && string_matches("stage", "pre_send_checkpoint")
        && string_matches("source_revision", &handover.source_revision)
        && string_matches("source_tree_sha256", &handover.source_tree_sha256)
        && string_matches("gate_b_manifest_sha256", &handover.gate_b_manifest_sha256)
        && string_matches(
            "operational_config_seal_receipt_sha256",
            &handover.operational_config_seal_receipt_sha256,
        )
        && string_matches(
            "controller_schedule_receipt_sha256",
            &handover.controller_schedule_receipt_sha256,
        )
        && string_matches(
            "controller_execute_receipt_sha256",
            &handover.controller_execute_receipt_sha256,
        )
        && string_matches("bridge_canister_id", &handover.bridge_canister_id)
        && string_matches("sns_root_canister_id", &handover.sns_root_canister_id)
        && string_matches("executing_principal", &handover.executing_principal)
        && strings_match("command_argv", &handover.command_argv)
        && strings_match("pre_send_controllers", &handover.pre_send_controllers)
        && string_matches("pre_send_module_sha256", &handover.pre_send_module_sha256)
        && string_matches(
            "pre_send_management_status_response_sha256",
            &handover.pre_send_management_status_response_sha256,
        )
        && string_matches(
            "pre_send_bridge_status_response_sha256",
            &handover.pre_send_bridge_status_response_sha256,
        )
        && string_matches(
            "pre_send_lifecycle_response_sha256",
            &handover.pre_send_lifecycle_response_sha256,
        )
        && string_matches(
            "pre_send_runtime_binding_response_sha256",
            &handover.pre_send_runtime_binding_response_sha256,
        )
        && string_matches(
            "pre_send_storage_integrity_response_sha256",
            &handover.pre_send_storage_integrity_response_sha256,
        )
        && string_matches(
            "pre_send_activation_status_response_sha256",
            &handover.pre_send_activation_status_response_sha256,
        )
        && string_matches(
            "pre_send_activation_attestation_response_sha256",
            &handover.pre_send_activation_attestation_response_sha256,
        )
}

fn validate_controller_handover_continuity(
    handover: &ControllerHandover,
    profile: &Profile,
    installer: &str,
) -> Result<(), String> {
    let before_management = handover_json_evidence(
        &handover.before_management_status_response_json_hex,
        &handover.before_management_status_response_sha256,
    )?;
    let pre_send_management = handover_json_evidence(
        &handover.pre_send_management_status_response_json_hex,
        &handover.pre_send_management_status_response_sha256,
    )?;
    let after_management = handover_json_evidence(
        &handover.after_management_status_response_json_hex,
        &handover.after_management_status_response_sha256,
    )?;
    let management = |value: &Value| -> Result<(Vec<String>, String), String> {
        let mut controllers = Vec::new();
        collect_json_key(value, "controllers", &mut controllers);
        let controllers = controllers
            .first()
            .and_then(|value| value.as_array())
            .ok_or("controller handover management controllers are malformed")?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "controller handover controller is malformed".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut modules = Vec::new();
        collect_json_key(value, "module_hash", &mut modules);
        if modules.is_empty() {
            collect_json_key(value, "module", &mut modules);
        }
        let module = modules
            .first()
            .and_then(|value| management_module_sha256(value))
            .ok_or("controller handover management module is malformed")?;
        Ok((controllers, module))
    };
    let (before_controllers, before_module) = management(&before_management)?;
    let (pre_send_controllers, pre_send_module) = management(&pre_send_management)?;
    let (after_controllers, after_module) = management(&after_management)?;
    if before_controllers != [installer]
        || pre_send_controllers != [installer]
        || after_controllers != [KINIC_ROOT]
        || handover.executing_principal != installer
        || handover.before_controllers != before_controllers
        || handover.pre_send_controllers != pre_send_controllers
        || handover.final_controllers != after_controllers
        || !before_module.eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || pre_send_module != before_module
        || after_module != before_module
        || !handover
            .before_module_sha256
            .eq_ignore_ascii_case(&before_module)
        || !handover
            .pre_send_module_sha256
            .eq_ignore_ascii_case(&pre_send_module)
        || !handover
            .after_module_sha256
            .eq_ignore_ascii_case(&after_module)
    {
        return Err("controller handover management continuity is invalid".into());
    }

    let before_bridge = handover_json_evidence(
        &handover.before_bridge_status_response_json_hex,
        &handover.before_bridge_status_response_sha256,
    )?;
    let pre_send_bridge = handover_json_evidence(
        &handover.pre_send_bridge_status_response_json_hex,
        &handover.pre_send_bridge_status_response_sha256,
    )?;
    let after_bridge = handover_json_evidence(
        &handover.after_bridge_status_response_json_hex,
        &handover.after_bridge_status_response_sha256,
    )?;
    let before_lifecycle = handover_json_evidence(
        &handover.before_lifecycle_response_json_hex,
        &handover.before_lifecycle_response_sha256,
    )?;
    let pre_send_lifecycle = handover_json_evidence(
        &handover.pre_send_lifecycle_response_json_hex,
        &handover.pre_send_lifecycle_response_sha256,
    )?;
    let after_lifecycle = handover_json_evidence(
        &handover.after_lifecycle_response_json_hex,
        &handover.after_lifecycle_response_sha256,
    )?;
    let before_runtime = handover_json_evidence(
        &handover.before_runtime_binding_response_json_hex,
        &handover.before_runtime_binding_response_sha256,
    )?;
    let pre_send_runtime = handover_json_evidence(
        &handover.pre_send_runtime_binding_response_json_hex,
        &handover.pre_send_runtime_binding_response_sha256,
    )?;
    let after_runtime = handover_json_evidence(
        &handover.after_runtime_binding_response_json_hex,
        &handover.after_runtime_binding_response_sha256,
    )?;
    let before_integrity = handover_json_evidence(
        &handover.before_storage_integrity_response_json_hex,
        &handover.before_storage_integrity_response_sha256,
    )?;
    let pre_send_integrity = handover_json_evidence(
        &handover.pre_send_storage_integrity_response_json_hex,
        &handover.pre_send_storage_integrity_response_sha256,
    )?;
    let after_integrity = handover_json_evidence(
        &handover.after_storage_integrity_response_json_hex,
        &handover.after_storage_integrity_response_sha256,
    )?;
    let before_activation = handover_json_evidence(
        &handover.before_activation_status_response_json_hex,
        &handover.before_activation_status_response_sha256,
    )?;
    let pre_send_activation = handover_json_evidence(
        &handover.pre_send_activation_status_response_json_hex,
        &handover.pre_send_activation_status_response_sha256,
    )?;
    let after_activation = handover_json_evidence(
        &handover.after_activation_status_response_json_hex,
        &handover.after_activation_status_response_sha256,
    )?;
    let before_attestation = handover_json_evidence(
        &handover.before_activation_attestation_response_json_hex,
        &handover.before_activation_attestation_response_sha256,
    )?;
    let pre_send_attestation = handover_json_evidence(
        &handover.pre_send_activation_attestation_response_json_hex,
        &handover.pre_send_activation_attestation_response_sha256,
    )?;
    let after_attestation = handover_json_evidence(
        &handover.after_activation_attestation_response_json_hex,
        &handover.after_activation_attestation_response_sha256,
    )?;
    let expected_operational_config_sha256 = expected_operational_config_sha256(
        profile,
        u64::try_from(json_u128(&before_bridge, "mint_authorization_ttl_seconds")?)
            .map_err(|_| "controller handover mint authorization TTL exceeds nat64")?,
        u64::try_from(json_u128(&before_bridge, "mint_authorization_epoch")?)
            .map_err(|_| "controller handover mint authorization epoch exceeds nat64")?,
    )?;
    let rpc_provider_urls_sha256 = hex(&canonical_sha256(&Vec::<String>::new())?);
    let expected_bridge_runtime_sha256 = decode_hex(&profile.bridge_runtime_bytecode_sha256)?;
    for runtime in [&before_runtime, &pre_send_runtime, &after_runtime] {
        let (runtime, bridge_runtime_sha256) = handover_runtime_binding(runtime)?;
        validate_live_runtime_binding(
            &runtime,
            profile,
            &rpc_provider_urls_sha256,
            &expected_operational_config_sha256,
        )?;
        if bridge_runtime_sha256 != expected_bridge_runtime_sha256 {
            return Err("controller handover runtime code binding differs from the profile".into());
        }
    }
    if before_runtime != pre_send_runtime
        || before_runtime != after_runtime
        || before_lifecycle != pre_send_lifecycle
        || before_lifecycle != after_lifecycle
        || before_activation != pre_send_activation
        || before_activation != after_activation
        || before_attestation != pre_send_attestation
        || before_lifecycle != serde_json::json!({"Ok":{"Activated":null}})
        || single_json_key(&before_integrity, "Ok")? != &Value::String("ok".into())
        || single_json_key(&pre_send_integrity, "Ok")? != &Value::String("ok".into())
        || single_json_key(&after_integrity, "Ok")? != &Value::String("ok".into())
        || single_json_key(&before_bridge, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&pre_send_bridge, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&after_bridge, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&before_activation, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&pre_send_activation, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&after_activation, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&before_attestation, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&before_attestation, "withdrawals_paused")? != &Value::Bool(false)
        || single_json_key(&pre_send_attestation, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&pre_send_attestation, "withdrawals_paused")? != &Value::Bool(false)
        || single_json_key(&after_attestation, "deposits_paused")? != &Value::Bool(false)
        || single_json_key(&after_attestation, "withdrawals_paused")? != &Value::Bool(false)
        || single_json_key(&before_bridge, "sufficient")? != &Value::Bool(true)
        || single_json_key(&pre_send_bridge, "sufficient")? != &Value::Bool(true)
        || single_json_key(&after_bridge, "sufficient")? != &Value::Bool(true)
        || json_u128(&before_bridge, "mint_authorization_ttl_seconds")?
            != json_u128(&pre_send_bridge, "mint_authorization_ttl_seconds")?
        || json_u128(&before_bridge, "mint_authorization_ttl_seconds")?
            != json_u128(&after_bridge, "mint_authorization_ttl_seconds")?
        || json_u128(&before_bridge, "mint_authorization_epoch")?
            != json_u128(&pre_send_bridge, "mint_authorization_epoch")?
        || json_u128(&before_bridge, "mint_authorization_epoch")?
            != json_u128(&after_bridge, "mint_authorization_epoch")?
        || json_u128(&after_bridge, "deposits")? < json_u128(&before_bridge, "deposits")?
        || json_u128(&pre_send_bridge, "deposits")? < json_u128(&before_bridge, "deposits")?
        || json_u128(&after_bridge, "withdrawals")? < json_u128(&before_bridge, "withdrawals")?
        || json_u128(&pre_send_bridge, "withdrawals")? < json_u128(&before_bridge, "withdrawals")?
        || json_u128(&after_bridge, "retained_audit_events")?
            .checked_add(json_u128(&after_bridge, "pruned_audit_events")?)
            .ok_or("controller handover audit sequence overflow")?
            < json_u128(&before_bridge, "retained_audit_events")?
                .checked_add(json_u128(&before_bridge, "pruned_audit_events")?)
                .ok_or("controller handover audit sequence overflow")?
    {
        return Err("controller handover operational continuity is invalid".into());
    }
    Ok(())
}

fn validate_controller_handover_completion(
    handover: &ControllerHandover,
    profile: &Profile,
    installer: &str,
    manifest_created_at_unix: u64,
    now: u64,
) -> Result<(), String> {
    validate_handover_completion_time(handover.observed_at_unix, manifest_created_at_unix, now)?;
    validate_controller_handover_continuity(handover, profile, installer)?;
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
    let response_request_ids = handover_request_ids(&response_text)?;
    let schema_is_supported = handover.schema_version == 3 || handover.schema_version == 4;
    let checkpoint_is_valid = if handover.schema_version == 4 {
        let checkpoint = decode_hex(&handover.pre_send_checkpoint_json_hex)?;
        valid_sha256(&handover.pre_send_checkpoint_sha256)
            && hex(&Sha256::digest(&checkpoint))
                .eq_ignore_ascii_case(&handover.pre_send_checkpoint_sha256)
            && serde_json::from_slice::<Value>(&checkpoint)
                .ok()
                .is_some_and(|value| controller_handover_checkpoint_matches(handover, &value))
    } else {
        handover.pre_send_checkpoint_json_hex.is_empty()
            && handover.pre_send_checkpoint_sha256.is_empty()
            && !handover.recovered_without_request_id
    };
    let request_binding_is_valid = if handover.request_id.is_empty() {
        handover.schema_version == 4
            && handover.recovered_without_request_id
            && response_request_ids.is_empty()
    } else {
        (valid_sha256(&handover.request_id) || valid_hash32(&handover.request_id))
            && response_request_ids == BTreeSet::from([request_id_text])
    };
    if !schema_is_supported
        || !checkpoint_is_valid
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
        || !request_binding_is_valid
        || handover.response_exit_code != 0
        || !response_digest.eq_ignore_ascii_case(&handover.response_sha256)
        || !valid_sha256(&handover.response_sha256)
        || handover.final_controllers != [KINIC_ROOT.to_string()]
        || handover.freezing_threshold_seconds == 0
        || handover.idle_cycles_burned_per_day == 0
        || handover.required_freezing_cycles != expected_freezing_cycles
        || handover.cycles_balance < profile.parameters.cycles_floor
        || handover.cycles_balance < handover.required_freezing_cycles
        || handover.pre_send_cycles_balance < profile.parameters.cycles_floor
        || handover.pre_send_cycles_balance < handover.pre_send_required_freezing_cycles
    {
        return Err("controller handover evidence is not an atomic SNS Root-only transfer".into());
    }
    Ok(())
}

#[allow(dead_code)]
fn validate_plan006_evidence(
    root: &Path,
    manifest: &ReleaseManifest,
    profile: &Profile,
    now: u64,
) -> Result<(), String> {
    let handover: ControllerHandover = read_json(&root.join("controller-handover.json"))?;
    let gate_a_receipt: GateAReceipt = read_json(&root.join("gate-a-receipt.json"))?;
    validate_controller_handover_completion(
        &handover,
        profile,
        &gate_a_receipt.canister_install.installer_principal,
        manifest.created_at_unix,
        now,
    )?;
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

fn production_upgrade_public_state_sha256(
    status: &BridgeStatusLiveView,
    values: &[&str],
) -> Result<String, String> {
    let mut digest = Sha256::new();
    digest.update(b"KINIC_PRODUCTION_UPGRADE_PUBLIC_STATE_V1\0");
    digest.update([u8::from(status.deposits_paused)]);
    digest.update(status.mint_authorization_ttl_seconds.to_be_bytes());
    digest.update(status.mint_authorization_epoch.to_be_bytes());
    digest.update(status.counts.deposits.to_be_bytes());
    digest.update(status.counts.withdrawals.to_be_bytes());
    digest.update(status.counts.reconciliation_holds.to_be_bytes());
    digest.update(status.counts.pending_ledger_operations.to_be_bytes());
    digest.update(status.counts.reserved_deposit_mint_amount.to_be_bytes());
    digest.update(status.counts.reserved_deposit_mint_operations.to_be_bytes());
    digest.update(status.counts.retained_audit_events.to_be_bytes());
    digest.update(status.counts.pruned_audit_events.to_be_bytes());
    digest.update(status.counts.retained_deposit_index_entries.to_be_bytes());
    for value in values {
        let raw = decode_hex(value)?;
        digest.update((raw.len() as u64).to_be_bytes());
        digest.update(raw);
    }
    Ok(hex(&digest.finalize()))
}

fn production_upgrade_query_state(
    status_hex: &str,
    lifecycle_hex: &str,
    runtime_hex: &str,
    integrity_hex: &str,
) -> Result<(BridgeStatusLiveView, RuntimeBindingView, String), String> {
    let status = decode_candid_hex::<BridgeStatusLiveView>(status_hex)?;
    if !status.reserve.sufficient {
        return Err("production upgrade requires a sufficient cycles reserve".into());
    }
    if !matches!(
        decode_candid_hex::<ProductionLifecycleResultView>(lifecycle_hex)?,
        ProductionLifecycleResultView::Ok(ProductionLifecycleView::Bootstrap)
    ) {
        return Err("production upgrade requires Bootstrap lifecycle".into());
    }
    let runtime = decode_candid_hex::<RuntimeBindingView>(runtime_hex)?;
    match decode_candid_hex::<StorageIntegrityResultView>(integrity_hex)? {
        StorageIntegrityResultView::Ok(value) if value == "ok" => {}
        _ => return Err("production upgrade storage integrity response is not ok".into()),
    }
    let public_state_sha256 = production_upgrade_public_state_sha256(
        &status,
        &[lifecycle_hex, runtime_hex, integrity_hex],
    )?;
    Ok((status, runtime, public_state_sha256))
}

fn production_upgrade_status_preserved(
    before: &BridgeStatusLiveView,
    after: &BridgeStatusLiveView,
) -> bool {
    before.reserve.sufficient
        && after.reserve.sufficient
        && before.deposits_paused == after.deposits_paused
        && before.mint_authorization_ttl_seconds == after.mint_authorization_ttl_seconds
        && before.mint_authorization_epoch == after.mint_authorization_epoch
        && before.counts.deposits == after.counts.deposits
        && before.counts.withdrawals == after.counts.withdrawals
        && before.counts.reconciliation_holds == after.counts.reconciliation_holds
        && before.counts.pending_ledger_operations == after.counts.pending_ledger_operations
        && before.counts.reserved_deposit_mint_amount == after.counts.reserved_deposit_mint_amount
        && before.counts.reserved_deposit_mint_operations
            == after.counts.reserved_deposit_mint_operations
        && before.counts.retained_audit_events == after.counts.retained_audit_events
        && before.counts.pruned_audit_events == after.counts.pruned_audit_events
        && before.counts.retained_deposit_index_entries
            == after.counts.retained_deposit_index_entries
}

fn production_upgrade_status_matches_pause_migration(
    before: &BridgeStatusLiveView,
    after: &BridgeStatusLiveView,
) -> bool {
    let Some(expected_audit_events) = before.counts.retained_audit_events.checked_add(1) else {
        return false;
    };
    let mut expected = before.clone();
    expected.counts.retained_audit_events = expected_audit_events;
    expected == *after
}

fn production_upgrade_pause_migration_matches(
    gate_a_profile: &Profile,
    gate_a_runtime: &LiveRuntimeBinding,
    before_status: &BridgeStatusLiveView,
    after_status: &BridgeStatusLiveView,
    before_runtime: &RuntimeBindingView,
    after_runtime: &RuntimeBindingView,
) -> Result<bool, String> {
    if gate_a_profile.pause_principal != KINIC_ROOT
        || live_runtime_binding_from_view(before_runtime) != *gate_a_runtime
    {
        return Ok(false);
    }
    let mut migrated_profile = gate_a_profile.clone();
    migrated_profile.pause_principal = PRODUCTION_PAUSE_PRINCIPAL.into();
    let mut expected_after_runtime = gate_a_runtime.clone();
    expected_after_runtime.operational_config_sha256 = hex(&expected_operational_config_sha256(
        &migrated_profile,
        after_status.mint_authorization_ttl_seconds,
        after_status.mint_authorization_epoch,
    )?);
    Ok(
        live_runtime_binding_from_view(after_runtime) == expected_after_runtime
            && production_upgrade_status_matches_pause_migration(before_status, after_status),
    )
}

fn collect_json_key<'a>(value: &'a Value, key: &str, output: &mut Vec<&'a Value>) {
    match value {
        Value::Object(values) => {
            for (name, child) in values {
                if name == key {
                    output.push(child);
                }
                collect_json_key(child, key, output);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_json_key(child, key, output);
            }
        }
        _ => {}
    }
}

fn management_module_sha256(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => {
            let value = value.strip_prefix("0x").unwrap_or(value);
            valid_sha256(value).then(|| value.to_ascii_lowercase())
        }
        Value::Array(values)
            if values.len() == 32
                && values
                    .iter()
                    .all(|value| value.as_u64().is_some_and(|value| value <= 255)) =>
        {
            Some(hex(&values
                .iter()
                .map(|value| value.as_u64().unwrap() as u8)
                .collect::<Vec<_>>()))
        }
        Value::Object(values) if values.len() == 1 => {
            management_module_sha256(values.values().next().unwrap())
        }
        _ => None,
    }
}

fn production_upgrade_management_state(raw_hex: &str) -> Result<(Vec<String>, String), String> {
    let raw = decode_hex(raw_hex)?;
    let value: Value = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
    let mut controllers = Vec::new();
    let mut modules = Vec::new();
    collect_json_key(&value, "controllers", &mut controllers);
    collect_json_key(&value, "module_hash", &mut modules);
    if controllers.len() != 1 || modules.len() != 1 {
        return Err("production upgrade management status is ambiguous".into());
    }
    let mut controllers = controllers[0]
        .as_array()
        .ok_or("production upgrade controllers are malformed")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|value| principal(value))
                .map(str::to_string)
                .ok_or_else(|| "production upgrade controller is malformed".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    controllers.sort();
    controllers.dedup();
    if controllers.is_empty() {
        return Err("production upgrade controller set is empty".into());
    }
    let module = management_module_sha256(modules[0])
        .ok_or("production upgrade module hash is malformed")?;
    Ok((controllers, module))
}

const MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES: usize = 128 * 1024 * 1024;
const MAX_PRODUCTION_UPGRADE_CHAIN_BYTES: usize = 256 * 1024 * 1024;

fn validate_production_upgrade_receipt_size(size: usize) -> Result<(), String> {
    if size > MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES {
        return Err("production upgrade receipt is too large".into());
    }
    Ok(())
}

fn production_upgrade_chain_receipts(
    bytes: &[u8],
) -> Result<Vec<(ProductionCanisterUpgradeReceipt, Vec<u8>)>, String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    if value.get("kind").and_then(Value::as_str) == Some("production-controller-bootstrap-upgrade")
    {
        validate_production_upgrade_receipt_size(bytes.len())?;
        let receipt = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
        return Ok(vec![(receipt, bytes.to_vec())]);
    }
    let chain: ProductionCanisterUpgradeChain =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    if chain.schema_version != 1
        || chain.kind != "production-controller-bootstrap-upgrade-chain"
        || chain.entries.is_empty()
        || chain.entries.len() > 16
    {
        return Err("production upgrade chain envelope is invalid".into());
    }
    let mut previous = None;
    let mut total = 0usize;
    let mut receipts = Vec::with_capacity(chain.entries.len());
    for (index, entry) in chain.entries.into_iter().enumerate() {
        let raw = decode_hex(&entry.receipt_json_hex)?;
        validate_production_upgrade_receipt_size(raw.len())?;
        total = total
            .checked_add(raw.len())
            .filter(|total| *total <= MAX_PRODUCTION_UPGRADE_CHAIN_BYTES)
            .ok_or("production upgrade chain is too large")?;
        let digest = hex(&Sha256::digest(&raw));
        if usize::from(entry.sequence) != index
            || entry.previous_receipt_sha256 != previous
            || !entry.receipt_sha256.eq_ignore_ascii_case(&digest)
        {
            return Err("production upgrade chain linkage is invalid".into());
        }
        let receipt = serde_json::from_slice(&raw).map_err(|error| error.to_string())?;
        previous = Some(digest);
        receipts.push((receipt, raw));
    }
    Ok(receipts)
}

fn production_upgrade_wasm(receipt: &ProductionCanisterUpgradeReceipt) -> Result<Vec<u8>, String> {
    let submission: ProductionUpgradeSubmission =
        serde_json::from_slice(&decode_hex(&receipt.submission_json_hex)?)
            .map_err(|error| error.to_string())?;
    let mut wasm = Vec::new();
    for (index, chunk) in submission.chunks.iter().enumerate() {
        if usize::try_from(chunk.index).ok() != Some(index) {
            return Err("production upgrade chunk sequence is invalid".into());
        }
        let argument = Decode!(
            &decode_hex(&chunk.argument_hex)?,
            ManagementUploadChunkArgument
        )
        .map_err(|error| error.to_string())?;
        wasm.extend_from_slice(&argument.chunk);
        if wasm.len() > 128 * 1024 * 1024 {
            return Err("production upgrade Wasm chain entry is too large".into());
        }
    }
    if wasm.is_empty() {
        return Err("production upgrade chain entry has no Wasm".into());
    }
    Ok(wasm)
}

fn append_production_upgrade_receipt(
    prior: Option<&Path>,
    receipt_path: &Path,
    output: &Path,
) -> Result<(), String> {
    let receipt = fs::read(receipt_path).map_err(|error| error.to_string())?;
    validate_production_upgrade_receipt_size(receipt.len())?;
    let _: ProductionCanisterUpgradeReceipt =
        serde_json::from_slice(&receipt).map_err(|error| error.to_string())?;
    let mut raw_receipts = if let Some(prior) = prior {
        production_upgrade_chain_receipts(&fs::read(prior).map_err(|error| error.to_string())?)?
            .into_iter()
            .map(|(_, raw)| raw)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    raw_receipts.push(receipt);
    if raw_receipts.len() > 16
        || raw_receipts
            .iter()
            .try_fold(0usize, |total, raw| {
                total
                    .checked_add(raw.len())
                    .filter(|total| *total <= MAX_PRODUCTION_UPGRADE_CHAIN_BYTES)
            })
            .is_none()
    {
        return Err("production upgrade chain is too large".into());
    }
    let mut previous = None;
    let entries = raw_receipts
        .into_iter()
        .enumerate()
        .map(|(index, raw)| {
            let digest = hex(&Sha256::digest(&raw));
            let entry = ProductionCanisterUpgradeChainEntry {
                sequence: u8::try_from(index).expect("bounded chain"),
                previous_receipt_sha256: previous.clone(),
                receipt_sha256: digest.clone(),
                receipt_json_hex: hex(&raw),
            };
            previous = Some(digest);
            entry
        })
        .collect();
    write_json_new(
        output,
        &ProductionCanisterUpgradeChain {
            schema_version: 1,
            kind: "production-controller-bootstrap-upgrade-chain".into(),
            entries,
        },
    )
}

fn validate_post_gate_a_policy_transition(
    root: &Path,
    manifest: &ReleaseManifest,
    profile: &Profile,
    gate_a_profile: &Profile,
    receipt: &GateAReceipt,
    now: u64,
) -> Result<(), String> {
    let transition: PostGateAPolicyTransition =
        read_json(&root.join("post-gate-a-policy-transition.json"))?;
    let upgrade_path = root.join("production-canister-upgrade-receipt.json");
    let upgrade_bytes = fs::read(&upgrade_path).map_err(|e| e.to_string())?;
    let upgrades = production_upgrade_chain_receipts(&upgrade_bytes)?;
    let upgrade = &upgrades
        .last()
        .ok_or("production upgrade chain is empty")?
        .0;
    validate_evidence_time(transition.observed_at_unix, manifest.created_at_unix, now)?;
    validate_evidence_time(upgrade.executed_at_unix, manifest.created_at_unix, now)?;
    validate_evidence_time(upgrade.verified_at_unix, manifest.created_at_unix, now)?;
    let receipt_bytes = fs::read(root.join("gate-a-receipt.json")).map_err(|e| e.to_string())?;
    let installer = &receipt.canister_install.installer_principal;
    let expected_controllers = vec![installer.clone()];
    let canister =
        Principal::from_text(&profile.bridge_canister_id).map_err(|error| error.to_string())?;
    let sender = Principal::from_text(installer).map_err(|error| error.to_string())?;
    let mut expected_before_module = gate_a_profile.bridge_canister_wasm_sha256.clone();
    let migration_required = gate_a_profile.pause_principal == KINIC_ROOT
        && profile.pause_principal == PRODUCTION_PAUSE_PRINCIPAL;
    let mut migration_seen = false;
    let mut expected_runtime = receipt.canister_install.runtime_binding.clone();
    for (entry, _) in &upgrades {
        validate_evidence_time(entry.executed_at_unix, manifest.created_at_unix, now)?;
        validate_evidence_time(entry.verified_at_unix, manifest.created_at_unix, now)?;
        let (entry_before_controllers, entry_before_module) =
            production_upgrade_management_state(&entry.before_management_status_json_hex)?;
        let (entry_after_controllers, entry_after_module) =
            production_upgrade_management_state(&entry.after_management_status_json_hex)?;
        let (entry_before_status, entry_before_runtime, entry_before_public_state) =
            production_upgrade_query_state(
                &entry.before_bridge_status_response_hex,
                &entry.before_lifecycle_response_hex,
                &entry.before_runtime_binding_response_hex,
                &entry.before_storage_integrity_response_hex,
            )?;
        let (entry_after_status, entry_after_runtime, entry_after_public_state) =
            production_upgrade_query_state(
                &entry.after_bridge_status_response_hex,
                &entry.after_lifecycle_response_hex,
                &entry.after_runtime_binding_response_hex,
                &entry.after_storage_integrity_response_hex,
            )?;
        let wasm = production_upgrade_wasm(entry)?;
        let submission_bytes = decode_hex(&entry.submission_json_hex)?;
        let submission = validate_production_upgrade_submission_bytes(
            &gate_a_profile.ic_host,
            canister,
            sender,
            &wasm,
            &submission_bytes,
        )?;
        validate_production_upgrade_upload_evidence(
            &submission,
            &decode_hex(&entry.chunk_upload_evidence_json_hex)?,
        )?;
        let wasm_sha256 = hex(&Sha256::digest(&wasm));
        let entry_before_binding = live_runtime_binding_from_view(&entry_before_runtime);
        let entry_after_binding = live_runtime_binding_from_view(&entry_after_runtime);
        let unchanged_runtime = entry_before_binding == expected_runtime
            && entry_after_binding == expected_runtime
            && production_upgrade_status_preserved(&entry_before_status, &entry_after_status)
            && entry.before_runtime_binding_response_hex
                == entry.after_runtime_binding_response_hex
            && entry_before_public_state.eq_ignore_ascii_case(&entry_after_public_state);
        let pause_migration = migration_required
            && !migration_seen
            && entry_before_binding == expected_runtime
            && expected_runtime == receipt.canister_install.runtime_binding
            && production_upgrade_pause_migration_matches(
                gate_a_profile,
                &receipt.canister_install.runtime_binding,
                &entry_before_status,
                &entry_after_status,
                &entry_before_runtime,
                &entry_after_runtime,
            )?;
        if entry.install_mode != "upgrade" {
            return Err("production upgrade management metadata is incomplete".into());
        }
        if entry.schema_version != 1
            || entry.kind != "production-controller-bootstrap-upgrade"
            || entry.bridge_canister_id != profile.bridge_canister_id
            || entry.executing_principal != *installer
            || entry.executed_at_unix > entry.verified_at_unix
            || entry.verified_at_unix > transition.observed_at_unix
            || entry.recovered != entry.recovered_at_unix.is_some()
            || entry.recovered_at_unix.is_some_and(|value| {
                value < entry.executed_at_unix || value > entry.verified_at_unix
            })
            || entry_before_controllers != expected_controllers
            || entry_after_controllers != expected_controllers
            || !entry_before_module.eq_ignore_ascii_case(&expected_before_module)
            || !entry
                .before_module_sha256
                .eq_ignore_ascii_case(&entry_before_module)
            || !entry
                .after_module_sha256
                .eq_ignore_ascii_case(&entry_after_module)
            || !entry_after_module.eq_ignore_ascii_case(&wasm_sha256)
            || !entry.wasm_sha256.eq_ignore_ascii_case(&wasm_sha256)
            || entry.before_schema_version != CURRENT_STABLE_SCHEMA_VERSION
            || entry.after_schema_version != CURRENT_STABLE_SCHEMA_VERSION
            || entry_before_runtime.schema_version != entry.before_schema_version
            || entry_after_runtime.schema_version != entry.after_schema_version
            || entry.before_lifecycle != "Bootstrap"
            || entry.after_lifecycle != "Bootstrap"
            || !entry.before_deposits_paused
            || !entry.after_deposits_paused
            || !entry_before_status.deposits_paused
            || !entry_after_status.deposits_paused
            || !entry.before_storage_validation_complete
            || !entry.after_storage_validation_complete
            || (!unchanged_runtime && !pause_migration)
            || entry.before_lifecycle_response_hex != entry.after_lifecycle_response_hex
            || entry.before_storage_integrity_response_hex
                != entry.after_storage_integrity_response_hex
            || !entry
                .before_public_state_sha256
                .eq_ignore_ascii_case(&entry_before_public_state)
            || !entry
                .after_public_state_sha256
                .eq_ignore_ascii_case(&entry_after_public_state)
            || !hex_sha256_matches(&entry.submission_json_hex, &entry.submission_json_sha256)
            || !hex_sha256_matches(
                &entry.chunk_upload_evidence_json_hex,
                &entry.chunk_upload_evidence_json_sha256,
            )
            || submission.request_id != entry.request_id
            || !production_upgrade_ingress_window_valid(
                entry.executed_at_unix,
                submission.ingress_expiry,
            )
        {
            return Err("production upgrade chain entry is incomplete".into());
        }
        migration_seen |= pause_migration;
        expected_runtime = entry_after_binding;
        expected_before_module = entry_after_module;
    }
    if migration_seen != migration_required
        || !expected_before_module.eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
    {
        return Err("production upgrade chain does not reach the current profile".into());
    }
    let (before_controllers, before_module) =
        production_upgrade_management_state(&upgrade.before_management_status_json_hex)?;
    let (after_controllers, after_module) =
        production_upgrade_management_state(&upgrade.after_management_status_json_hex)?;
    let (before_status, before_runtime, before_public_state_sha256) =
        production_upgrade_query_state(
            &upgrade.before_bridge_status_response_hex,
            &upgrade.before_lifecycle_response_hex,
            &upgrade.before_runtime_binding_response_hex,
            &upgrade.before_storage_integrity_response_hex,
        )?;
    let (after_status, after_runtime, after_public_state_sha256) = production_upgrade_query_state(
        &upgrade.after_bridge_status_response_hex,
        &upgrade.after_lifecycle_response_hex,
        &upgrade.after_runtime_binding_response_hex,
        &upgrade.after_storage_integrity_response_hex,
    )?;
    let last_before_binding = live_runtime_binding_from_view(&before_runtime);
    let last_after_binding = live_runtime_binding_from_view(&after_runtime);
    let last_unchanged_transition = last_before_binding == expected_runtime
        && last_after_binding == expected_runtime
        && production_upgrade_status_preserved(&before_status, &after_status)
        && upgrade.before_runtime_binding_response_hex
            == upgrade.after_runtime_binding_response_hex
        && before_public_state_sha256.eq_ignore_ascii_case(&after_public_state_sha256);
    let last_pause_migration_transition = migration_required
        && last_before_binding == receipt.canister_install.runtime_binding
        && last_after_binding == expected_runtime
        && production_upgrade_pause_migration_matches(
            gate_a_profile,
            &receipt.canister_install.runtime_binding,
            &before_status,
            &after_status,
            &before_runtime,
            &after_runtime,
        )?;
    let last_runtime_transition_valid =
        last_unchanged_transition || last_pause_migration_transition;
    let submission_bytes = decode_hex(&upgrade.submission_json_hex)?;
    let current_wasm =
        fs::read(root.join("bridge-canister.wasm")).map_err(|error| error.to_string())?;
    let validated_submission = validate_production_upgrade_submission_bytes(
        &gate_a_profile.ic_host,
        Principal::from_text(&profile.bridge_canister_id).map_err(|error| error.to_string())?,
        Principal::from_text(installer).map_err(|error| error.to_string())?,
        &current_wasm,
        &submission_bytes,
    )?;
    let upload_evidence_bytes = decode_hex(&upgrade.chunk_upload_evidence_json_hex)?;
    validate_production_upgrade_upload_evidence(&validated_submission, &upload_evidence_bytes)?;
    let expected_last_before_module = upgrades
        .iter()
        .rev()
        .nth(1)
        .map(|(receipt, _)| receipt.after_module_sha256.as_str())
        .unwrap_or(&gate_a_profile.bridge_canister_wasm_sha256);
    if !before_module.eq_ignore_ascii_case(expected_last_before_module)
        || !after_module.eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !hex(&Sha256::digest(&current_wasm))
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
    {
        return Err("post-Gate-A Wasm upgrade chain does not reach the current profile".into());
    }
    if transition.schema_version != 3
        || transition.reason != "activate-before-production-measurements"
        || !transition
            .gate_a_manifest_sha256
            .eq_ignore_ascii_case(&receipt.gate_a_manifest_sha256)
        || !transition
            .gate_a_receipt_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(&receipt_bytes)))
        || transition.from_source_revision != receipt.source_revision
        || !transition
            .from_source_tree_sha256
            .eq_ignore_ascii_case(&receipt.source_tree_sha256)
        || transition.upgrade_source_revision != upgrade.source_revision
        || !transition
            .upgrade_source_tree_sha256
            .eq_ignore_ascii_case(&upgrade.source_tree_sha256)
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
            .production_canister_upgrade_receipt_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(&upgrade_bytes)))
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
        return Err("post-Gate-A policy transition identity binding is incomplete".into());
    }
    if upgrade.schema_version != 1
        || upgrade.kind != "production-controller-bootstrap-upgrade"
        || upgrade.bridge_canister_id != profile.bridge_canister_id
        || upgrade.install_mode != "upgrade"
        || upgrade.executing_principal != *installer
        || upgrade.executed_at_unix > upgrade.verified_at_unix
        || upgrade.recovered != upgrade.recovered_at_unix.is_some()
        || upgrade.recovered_at_unix.is_some_and(|value| {
            value < upgrade.executed_at_unix || value > upgrade.verified_at_unix
        })
        || upgrade.verified_at_unix > transition.observed_at_unix
        || upgrade.before_controllers != before_controllers
        || upgrade.after_controllers != after_controllers
        || before_controllers != expected_controllers
        || after_controllers != expected_controllers
        || !upgrade
            .before_module_sha256
            .eq_ignore_ascii_case(&before_module)
        || !upgrade
            .after_module_sha256
            .eq_ignore_ascii_case(&after_module)
    {
        return Err("production upgrade management metadata is incomplete".into());
    }
    if upgrade.before_schema_version != CURRENT_STABLE_SCHEMA_VERSION
        || upgrade.after_schema_version != CURRENT_STABLE_SCHEMA_VERSION
        || before_runtime.schema_version != upgrade.before_schema_version
        || after_runtime.schema_version != upgrade.after_schema_version
        || !last_runtime_transition_valid
    {
        return Err("production upgrade schema or RuntimeBinding continuity is incomplete".into());
    }
    if upgrade.before_lifecycle != "Bootstrap"
        || upgrade.after_lifecycle != "Bootstrap"
        || !upgrade.before_deposits_paused
        || !upgrade.after_deposits_paused
        || !before_status.deposits_paused
        || !after_status.deposits_paused
        || !upgrade.before_storage_validation_complete
        || !upgrade.after_storage_validation_complete
    {
        return Err("production upgrade lifecycle or pause continuity is incomplete".into());
    }
    if transition.schema_version != 3
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
        || transition.upgrade_source_revision != upgrade.source_revision
        || !transition
            .upgrade_source_tree_sha256
            .eq_ignore_ascii_case(&upgrade.source_tree_sha256)
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
            .from_bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&receipt.bridge_canister_wasm_sha256)
        || !transition
            .from_bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&gate_a_profile.bridge_canister_wasm_sha256)
        || !transition
            .to_bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !transition
            .production_canister_upgrade_receipt_sha256
            .eq_ignore_ascii_case(&hex(&Sha256::digest(&upgrade_bytes)))
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
        || upgrade.schema_version != 1
        || upgrade.kind != "production-controller-bootstrap-upgrade"
        || upgrade.bridge_canister_id != profile.bridge_canister_id
        || upgrade.install_mode != "upgrade"
        || upgrade.executing_principal != *installer
        || upgrade.executed_at_unix > upgrade.verified_at_unix
        || upgrade.recovered != upgrade.recovered_at_unix.is_some()
        || upgrade.recovered_at_unix.is_some_and(|value| {
            value < upgrade.executed_at_unix || value > upgrade.verified_at_unix
        })
        || upgrade.verified_at_unix > transition.observed_at_unix
        || upgrade.before_controllers != before_controllers
        || upgrade.after_controllers != after_controllers
        || before_controllers != expected_controllers
        || after_controllers != expected_controllers
        || !upgrade
            .before_module_sha256
            .eq_ignore_ascii_case(&before_module)
        || !before_module.eq_ignore_ascii_case(expected_last_before_module)
        || !upgrade
            .after_module_sha256
            .eq_ignore_ascii_case(&after_module)
        || !after_module.eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || !upgrade
            .wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
        || upgrade.before_schema_version != CURRENT_STABLE_SCHEMA_VERSION
        || upgrade.after_schema_version != CURRENT_STABLE_SCHEMA_VERSION
        || before_runtime.schema_version != upgrade.before_schema_version
        || after_runtime.schema_version != upgrade.after_schema_version
        || !last_runtime_transition_valid
        || upgrade.before_lifecycle != "Bootstrap"
        || upgrade.after_lifecycle != "Bootstrap"
        || !upgrade.before_deposits_paused
        || !upgrade.after_deposits_paused
        || !before_status.deposits_paused
        || !after_status.deposits_paused
        || !upgrade.before_storage_validation_complete
        || !upgrade.after_storage_validation_complete
        || !hex_sha256_matches(
            &upgrade.before_management_status_json_hex,
            &upgrade.before_management_status_json_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.after_management_status_json_hex,
            &upgrade.after_management_status_json_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.before_bridge_status_response_hex,
            &upgrade.before_bridge_status_response_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.after_bridge_status_response_hex,
            &upgrade.after_bridge_status_response_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.before_lifecycle_response_hex,
            &upgrade.before_lifecycle_response_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.after_lifecycle_response_hex,
            &upgrade.after_lifecycle_response_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.before_runtime_binding_response_hex,
            &upgrade.before_runtime_binding_response_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.after_runtime_binding_response_hex,
            &upgrade.after_runtime_binding_response_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.before_storage_integrity_response_hex,
            &upgrade.before_storage_integrity_response_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.after_storage_integrity_response_hex,
            &upgrade.after_storage_integrity_response_sha256,
        )
        || upgrade.before_lifecycle_response_hex != upgrade.after_lifecycle_response_hex
        || upgrade.before_storage_integrity_response_hex
            != upgrade.after_storage_integrity_response_hex
        || !upgrade
            .before_public_state_sha256
            .eq_ignore_ascii_case(&before_public_state_sha256)
        || !upgrade
            .after_public_state_sha256
            .eq_ignore_ascii_case(&after_public_state_sha256)
        || upgrade.command_argv
            != [
                "bridge-profile",
                "submit-production-canister-upgrade",
                gate_a_profile.ic_host.as_str(),
                profile.bridge_canister_id.as_str(),
                installer.as_str(),
                "<production-controller-pem>",
                "<verified-release-artifact>",
                "<durable-submission-artifact>",
                "<durable-chunk-upload-evidence>",
                "<durable-response-artifact>",
            ]
            .map(str::to_string)
        || !valid_sha256(&upgrade.response_stdout_sha256)
        || !hex_sha256_matches(
            &upgrade.response_stdout_hex,
            &upgrade.response_stdout_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.response_stderr_hex,
            &upgrade.response_stderr_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.submission_json_hex,
            &upgrade.submission_json_sha256,
        )
        || !hex_sha256_matches(
            &upgrade.chunk_upload_evidence_json_hex,
            &upgrade.chunk_upload_evidence_json_sha256,
        )
        || !valid_sha256(&upgrade.request_id)
        || validated_submission.request_id != upgrade.request_id
        || !production_upgrade_ingress_window_valid(
            upgrade.executed_at_unix,
            validated_submission.ingress_expiry,
        )
    {
        return Err(
            "post-Gate-A policy transition or production upgrade evidence is incomplete".into(),
        );
    }
    Ok(())
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
    validate_profile(&profile, !manifest.test_only)?;
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
        validate_profile(&gate_a_profile, !manifest.test_only)?;
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
        validate_post_gate_a_policy_transition(
            root,
            &manifest,
            &profile,
            &gate_a_profile,
            &receipt,
            now,
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
        &rpc_url_hash,
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

struct ProductionHandoverCanisterObservation<'a> {
    lifecycle: &'a ProductionLifecycleView,
    attestation: Option<&'a ActivationAttestationView>,
    activation_status: &'a ActivationStatusView,
    runtime: &'a RuntimeBindingView,
    status: &'a BridgeStatusLiveView,
    storage_integrity: &'a StorageIntegrityResultView,
    controllers: &'a [Principal],
    module_hash: &'a [u8],
}

struct ProductionHandoverActivationBinding<'a> {
    governance_operation_id: u64,
    finalized_block_number: u64,
    timelock_operation_id: &'a str,
    transaction_hash: &'a str,
    confirmed_generation: u8,
    confirmed_signed_at_ns: &'a str,
}

fn validate_production_handover_canister_state(
    profile: &Profile,
    installer: Principal,
    gate_a_receipt: &GateAReceipt,
    activation: &ProductionHandoverActivationBinding<'_>,
    observation: &ProductionHandoverCanisterObservation<'_>,
    manifest_created_at_unix: u64,
    now: u64,
) -> Result<(), String> {
    if !matches!(observation.lifecycle, ProductionLifecycleView::Activated) {
        return Err("production Canister must be Activated before handover".into());
    }
    validate_current_profile_management_snapshot(
        profile,
        installer,
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
    if observation.status.deposits_paused || !observation.status.reserve.sufficient {
        return Err("production Canister must be active and reserved for handover".into());
    }
    if !matches!(
        observation.storage_integrity,
        StorageIntegrityResultView::Ok(value) if value == "ok"
    ) {
        return Err("production Canister storage integrity is not ok for handover".into());
    }
    let last = observation
        .activation_status
        .last_confirmed_activation
        .as_ref()
        .ok_or("active handover has no confirmed execute activation")?;
    let confirmed_signed_at_ns = u64::try_from(parse_decimal_u128(
        activation.confirmed_signed_at_ns,
        "confirmed signed timestamp",
    )?)
    .map_err(|_| "confirmed signed timestamp exceeds nat64")?;
    if observation.activation_status.deposits_paused
        || observation
            .activation_status
            .pending_timelock_operation
            .is_some()
        || last.phase != "execute"
        || last.governance_operation_id != activation.governance_operation_id
        || last.receipt_block_number != activation.finalized_block_number
        || last.generation != activation.confirmed_generation
        || last.signed_at_ns != confirmed_signed_at_ns
        || !format!("0x{}", hex(&last.timelock_operation_id))
            .eq_ignore_ascii_case(activation.timelock_operation_id)
        || !format!("0x{}", hex(&last.transaction_hash))
            .eq_ignore_ascii_case(activation.transaction_hash)
    {
        return Err("live activation state differs from the execute receipt".into());
    }
    let attestation = observation
        .attestation
        .ok_or("authenticated activation attestation is unavailable for active handover")?;
    validate_activation_attestation_with_pause(
        profile,
        attestation,
        manifest_created_at_unix,
        gate_a_receipt
            .bridge_deployment_block_number
            .max(gate_a_receipt.timelock_deployment_block_number),
        now,
        Some(false),
    )
}

fn verify_production_canister_handover(
    bundle_path: &Path,
    seal_receipt_path: &Path,
    schedule_receipt_path: &Path,
    execute_receipt_path: &Path,
) -> Result<(), String> {
    let (bundle, gate_a_receipt, execute_receipt) = validate_production_handover_candidate_files(
        bundle_path,
        seal_receipt_path,
        schedule_receipt_path,
        execute_receipt_path,
    )?;
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (
        lifecycle_raw,
        attestation_raw,
        activation_status_raw,
        runtime_raw,
        status_raw,
        storage_integrity_raw,
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
            .with_arg(empty)
            .call_with_verification()
            .await
            .map_err(|error| error.to_string())?;
        let activation_status = agent
            .query(&bridge, "get_activation_status")
            .with_arg(Encode!().map_err(|error| error.to_string())?)
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
        let storage_integrity = agent
            .query(&bridge, "storage_integrity_check")
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
            activation_status,
            runtime,
            status,
            storage_integrity,
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
    let activation_status = match Decode!(&activation_status_raw, ActivationStatusResultView)
        .map_err(|error| error.to_string())?
    {
        ActivationStatusResultView::Ok(value) => value,
        ActivationStatusResultView::Err(_) => {
            return Err("authenticated activation status is unavailable".into())
        }
    };
    let runtime = Decode!(&runtime_raw, RuntimeBindingView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let storage_integrity = Decode!(&storage_integrity_raw, StorageIntegrityResultView)
        .map_err(|error| error.to_string())?;
    let observation = ProductionHandoverCanisterObservation {
        lifecycle: &lifecycle,
        attestation: attestation.as_deref(),
        activation_status: &activation_status,
        runtime: &runtime,
        status: &status,
        storage_integrity: &storage_integrity,
        controllers: &controllers,
        module_hash: &module_hash,
    };
    let activation = ProductionHandoverActivationBinding {
        governance_operation_id: execute_receipt
            .governance_operation_id
            .parse::<u64>()
            .map_err(|_| "invalid execute governance operation ID")?,
        finalized_block_number: execute_receipt
            .finalized_block_number
            .parse::<u64>()
            .map_err(|_| "invalid execute Finalized block")?,
        timelock_operation_id: &execute_receipt.timelock_operation_id,
        transaction_hash: &execute_receipt.transaction_hash,
        confirmed_generation: execute_receipt.confirmed_generation,
        confirmed_signed_at_ns: &execute_receipt.confirmed_signed_at_ns,
    };
    let installer = gate_b_controller(&bundle)?;
    validate_production_handover_canister_state(
        &bundle.profile,
        installer,
        &gate_a_receipt,
        &activation,
        &observation,
        bundle.manifest.created_at_unix,
        now_unix()?,
    )?;
    println!(
        "production_canister_handover=verified canister={}",
        bundle.profile.bridge_canister_id
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

fn validate_post_handover_management_snapshot(
    profile: &Profile,
    controllers: &[Principal],
    module_hash: &[u8],
) -> Result<(), String> {
    let root = Principal::from_text(KINIC_ROOT).map_err(|error| error.to_string())?;
    if controllers != [root]
        || !hex(module_hash).eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
    {
        return Err("completed handover requires KINIC SNS Root as sole controller".into());
    }
    Ok(())
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
enum SealReceiptLiveContext {
    PrePrepare,
    PendingResume,
    ConfirmationInput,
    ScheduleFinalization,
    ExecuteFinalization,
    HandoverPreTransfer,
    HandoverPostTransfer,
}

fn live_activation_pause_requirement(context: SealReceiptLiveContext) -> Option<bool> {
    match context {
        SealReceiptLiveContext::PrePrepare | SealReceiptLiveContext::ConfirmationInput => None,
        SealReceiptLiveContext::PendingResume | SealReceiptLiveContext::ScheduleFinalization => {
            Some(true)
        }
        SealReceiptLiveContext::ExecuteFinalization
        | SealReceiptLiveContext::HandoverPreTransfer
        | SealReceiptLiveContext::HandoverPostTransfer => Some(false),
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
    if matches!(
        live_context,
        SealReceiptLiveContext::HandoverPreTransfer | SealReceiptLiveContext::HandoverPostTransfer
    ) {
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
    if matches!(
        live_context,
        SealReceiptLiveContext::HandoverPreTransfer | SealReceiptLiveContext::HandoverPostTransfer
    ) {
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
            SealReceiptLiveContext::ExecuteFinalization
            | SealReceiptLiveContext::HandoverPreTransfer
            | SealReceiptLiveContext::HandoverPostTransfer => {
                matches!(
                    live_lifecycle,
                    ProductionLifecycleResultView::Ok(ProductionLifecycleView::Activated)
                ) && !live_status.deposits_paused
            }
            SealReceiptLiveContext::PrePrepare | SealReceiptLiveContext::ConfirmationInput => {
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
    let (controllers, module_hash) =
        if matches!(live_context, SealReceiptLiveContext::HandoverPostTransfer) {
            live_management_snapshot(bundle)?
        } else {
            gate_b_management_snapshot(bundle)?
        };
    if matches!(live_context, SealReceiptLiveContext::HandoverPostTransfer) {
        validate_post_handover_management_snapshot(&bundle.profile, &controllers, &module_hash)?;
    } else if controllers
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

fn validate_controller_execute_receipt(
    bundle: &ValidatedBundle,
    receipt: &ControllerActivationReceipt,
    expected_seal_receipt_sha256: &str,
    schedule_receipt_path: &Path,
    freshness: ActivationReceiptFreshness,
) -> Result<(u64, u64), String> {
    let installer = gate_b_controller(bundle)?;
    let schedule_bytes = fs::read(schedule_receipt_path).map_err(|error| error.to_string())?;
    let schedule_receipt_sha256 = hex(&Sha256::digest(&schedule_bytes));
    let schedule: ControllerActivationReceipt =
        serde_json::from_slice(&schedule_bytes).map_err(|error| error.to_string())?;
    let (schedule_governance_operation_id, _) = validate_controller_schedule_receipt(
        bundle,
        &schedule,
        expected_seal_receipt_sha256,
        freshness,
    )?;
    let authorization_bytes = decode_hex(&receipt.authorization_receipt_hex)?;
    let authorization: ControllerActivationAuthorizationReceipt =
        serde_json::from_slice(&authorization_bytes).map_err(|error| error.to_string())?;
    let prepare_receipt_bytes = decode_hex(&receipt.prepare_receipt_hex)?;
    let prepare_receipt: ControllerActivationPrepareReceipt =
        serde_json::from_slice(&prepare_receipt_bytes).map_err(|error| error.to_string())?;
    let governance_operation_id = receipt
        .governance_operation_id
        .parse::<u64>()
        .map_err(|_| "invalid execute governance operation ID")?;
    let finalized_block = receipt
        .finalized_block_number
        .parse::<u64>()
        .map_err(|_| "invalid execute Finalized block")?;
    let now = now_unix()?;
    if receipt.schema_version != 1
        || receipt.phase != "execute"
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
        || receipt.controller_principal != installer.to_text()
        || receipt.certified_controller_set != [installer.to_text()]
        || receipt.deposits_paused
        || receipt.prior_schedule_receipt_sha256.as_deref()
            != Some(schedule_receipt_sha256.as_str())
        || governance_operation_id <= schedule_governance_operation_id
        || finalized_block == 0
        || !receipt
            .timelock_operation_id
            .eq_ignore_ascii_case(&schedule.timelock_operation_id)
        || !receipt
            .operation_salt
            .eq_ignore_ascii_case(&schedule.operation_salt)
        || !valid_hash32(&receipt.transaction_hash)
        || !valid_sha256(&receipt.artifact_sha256)
        || !valid_sha256(&receipt.authorization_receipt_sha256)
        || !hex(&Sha256::digest(&authorization_bytes))
            .eq_ignore_ascii_case(&receipt.authorization_receipt_sha256)
        || validate_controller_activation_authorization(
            "execute",
            bundle,
            &bundle.manifest_sha256,
            expected_seal_receipt_sha256,
            &authorization,
            freshness,
        )
        .is_err()
        || !valid_sha256(&receipt.prepare_receipt_sha256)
        || !hex(&Sha256::digest(&prepare_receipt_bytes))
            .eq_ignore_ascii_case(&receipt.prepare_receipt_sha256)
        || !controller_activation_prepare_fields_match(
            &prepare_receipt,
            "execute",
            &bundle.manifest_sha256,
            &receipt.artifact_sha256,
            &receipt.authorization_receipt_sha256,
        )
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
        || !activation_raw_digest_matches(
            &receipt.activation_status_response_hex,
            &receipt.activation_status_response_sha256,
        )?
    {
        return Err("controller execute receipt is not bound to this Gate B release".into());
    }
    let raw = decode_hex(&receipt.activation_status_response_hex)?;
    let ActivationStatusResultView::Ok(status) =
        Decode!(&raw, ActivationStatusResultView).map_err(|error| error.to_string())?
    else {
        return Err("controller execute receipt contains an error response".into());
    };
    let last = status
        .last_confirmed_activation
        .as_ref()
        .ok_or("controller execute receipt has no confirmation")?;
    if status.deposits_paused
        || status.pending_timelock_operation.is_some()
        || last.phase != "execute"
        || last.governance_operation_id != governance_operation_id
        || last.receipt_block_number != finalized_block
        || !controller_activation_confirmation_fields_match(
            receipt.confirmed_generation,
            &receipt.confirmed_signed_at_ns,
            last,
        )
        || !format!("0x{}", hex(&last.timelock_operation_id))
            .eq_ignore_ascii_case(&receipt.timelock_operation_id)
        || !format!("0x{}", hex(&last.transaction_hash))
            .eq_ignore_ascii_case(&receipt.transaction_hash)
    {
        return Err("controller execute receipt response disagrees with its fields".into());
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

fn validate_schedule_receipt_binding(
    receipt: &ActivationReceipt,
    bundle: &ValidatedBundle,
) -> Result<(), String> {
    let canonical_payload = [0x44, 0x49, 0x44, 0x4c, 0x00, 0x00];
    let payload_sha256 = hex(&Sha256::digest(canonical_payload));
    let now = now_unix()?;
    let (expected_governance_operation_id, expected_operation_id, expected_salt) =
        gate_b_initial_activation_binding(bundle, ActivationReceiptFreshness::Current)?;
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
            gate_b_initial_activation_binding(bundle, ActivationReceiptFreshness::Current)?;
        if confirmation_governance_operation_id != expected_governance_operation_id
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
        if confirmation_governance_operation_id <= prior_governance_operation_id {
            return Err("SNS execute governance operation ID does not follow schedule".into());
        }
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

#[allow(dead_code)]
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

fn validate_production_upgrade_submission(
    host: &str,
    canister: Principal,
    sender: Principal,
    wasm: &[u8],
    submission_path: &Path,
) -> Result<ProductionUpgradeSubmission, String> {
    let bytes = fs::read(submission_path).map_err(|error| error.to_string())?;
    validate_production_upgrade_submission_bytes(host, canister, sender, wasm, &bytes)
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

fn production_upgrade_ingress_window_valid(executed_at_unix: u64, ingress_expiry: u64) -> bool {
    executed_at_unix
        .checked_mul(1_000_000_000)
        .and_then(|executed_at_ns| {
            executed_at_ns
                .checked_add(5 * 60 * 1_000_000_000)
                .map(|latest| ingress_expiry > executed_at_ns && ingress_expiry <= latest)
        })
        .unwrap_or(false)
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

fn validate_production_upgrade_upload_evidence(
    submission: &ProductionUpgradeSubmission,
    evidence_bytes: &[u8],
) -> Result<ProductionUpgradeUploadEvidence, String> {
    let evidence: ProductionUpgradeUploadEvidence =
        serde_json::from_slice(evidence_bytes).map_err(|error| error.to_string())?;
    if evidence.schema_version != 1
        || evidence.stored_chunks_request_id != submission.stored_chunks.request_id
        || !hex_sha256_matches(
            &evidence.stored_chunks_response_hex,
            &evidence.stored_chunks_response_sha256,
        )
        || evidence.chunks.len() != submission.chunks.len()
    {
        return Err("production upgrade upload evidence metadata is invalid".into());
    }
    let stored_response = decode_hex(&evidence.stored_chunks_response_hex)?;
    let stored =
        Decode!(&stored_response, Vec<ManagementChunkHash>).map_err(|error| error.to_string())?;
    let expected = submission
        .chunks
        .iter()
        .map(|chunk| chunk.sha256.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if stored
        .iter()
        .any(|chunk| !expected.contains(&hex(&chunk.hash)))
    {
        return Err("production upgrade chunk store contains an unexpected chunk".into());
    }
    for (index, (recorded, chunk)) in evidence.chunks.iter().zip(&submission.chunks).enumerate() {
        if recorded.schema_version != 1
            || recorded.index != u32::try_from(index).map_err(|error| error.to_string())?
            || recorded.request_id != chunk.request_id
            || !hex_sha256_matches(&recorded.response_hex, &recorded.response_sha256)
        {
            return Err("production upgrade chunk response metadata is invalid".into());
        }
        let response = decode_hex(&recorded.response_hex)?;
        let observed =
            Decode!(&response, ManagementChunkHash).map_err(|error| error.to_string())?;
        if !hex(&observed.hash).eq_ignore_ascii_case(&chunk.sha256) {
            return Err("production upgrade chunk response hash is invalid".into());
        }
    }
    Ok(evidence)
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

fn prepare_production_canister_upgrade(
    host: &str,
    canister_text: &str,
    expected_principal_text: &str,
    pem_path: &Path,
    wasm_path: &Path,
    submission_path: &Path,
) -> Result<(), String> {
    let canister = Principal::from_text(canister_text).map_err(|error| error.to_string())?;
    let expected_principal =
        Principal::from_text(expected_principal_text).map_err(|error| error.to_string())?;
    let identity = production_upgrade_identity(pem_path)?;
    let sender = identity.sender().map_err(|error| error.to_string())?;
    if sender != expected_principal {
        return Err(
            "production controller PEM principal does not match the expected installer".into(),
        );
    }
    let wasm = fs::read(wasm_path).map_err(|error| error.to_string())?;
    let wasm_sha256 = hex(&Sha256::digest(&wasm));
    let agent = Agent::builder()
        .with_url(host)
        .with_boxed_identity(identity)
        .with_verify_query_signatures(true)
        .build()
        .map_err(|error| error.to_string())?;
    let management = Principal::management_canister();
    if submission_path.exists() {
        return Err("production upgrade submission artifact already exists".into());
    }
    let submission = {
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
            wasm_module_hash: Sha256::digest(&wasm).to_vec(),
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
        let submission = ProductionUpgradeSubmission {
            schema_version: 2,
            install_method: "install_chunked_code".into(),
            ic_host: host.to_string(),
            effective_canister_id: canister_text.to_string(),
            sender_principal: sender.to_text(),
            wasm_sha256: wasm_sha256.clone(),
            chunk_size_bytes: PRODUCTION_UPGRADE_CHUNK_SIZE as u64,
            stored_chunks,
            chunks,
            argument_hex: hex(&argument),
            argument_sha256: hex(&Sha256::digest(&argument)),
            ingress_expiry: signed.ingress_expiry,
            request_id: hex(signed.request_id.as_slice()),
            signed_update_hex: hex(&signed.signed_update),
            signed_update_sha256: hex(&Sha256::digest(&signed.signed_update)),
        };
        submission
    };
    write_json_new(submission_path, &submission)?;
    println!("request_id={}", submission.request_id);
    Ok(())
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

fn record_production_upgrade_send_error(
    evidence_dir: &Path,
    request_kind: &str,
    request_id: &str,
    error: &str,
) -> Result<(), String> {
    let observed_at_ns = u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|value| value.to_string())?
            .as_nanos(),
    )
    .map_err(|value| value.to_string())?;
    let value = ProductionUpgradeSendError {
        schema_version: 1,
        request_kind: request_kind.into(),
        request_id: request_id.into(),
        observed_at_ns,
        error: error.into(),
        error_sha256: hex(&Sha256::digest(error.as_bytes())),
    };
    write_json_new(
        &evidence_dir.join(format!("error-{request_kind}-{observed_at_ns}.json")),
        &value,
    )
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

fn validate_stored_chunks_response(
    submission: &ProductionUpgradeSubmission,
    response: &ProductionUpgradeStoredChunksResponse,
) -> Result<(), String> {
    if response.schema_version != 1
        || response.request_id != submission.stored_chunks.request_id
        || !hex_sha256_matches(&response.response_hex, &response.response_sha256)
    {
        return Err("production upgrade stored-chunks response metadata is invalid".into());
    }
    let raw = decode_hex(&response.response_hex)?;
    let stored = Decode!(&raw, Vec<ManagementChunkHash>).map_err(|error| error.to_string())?;
    let expected = submission
        .chunks
        .iter()
        .map(|chunk| chunk.sha256.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if stored
        .iter()
        .any(|chunk| !expected.contains(&hex(&chunk.hash)))
    {
        return Err("production upgrade chunk store contains an unexpected chunk".into());
    }
    Ok(())
}

fn validate_chunk_response(
    chunk: &ProductionUpgradeChunkSubmission,
    response: &ProductionUpgradeChunkResponse,
) -> Result<(), String> {
    if response.schema_version != 1
        || response.index != chunk.index
        || response.request_id != chunk.request_id
        || !hex_sha256_matches(&response.response_hex, &response.response_sha256)
    {
        return Err("production upgrade chunk response metadata is invalid".into());
    }
    let raw = decode_hex(&response.response_hex)?;
    let observed = Decode!(&raw, ManagementChunkHash).map_err(|error| error.to_string())?;
    if !hex(&observed.hash).eq_ignore_ascii_case(&chunk.sha256) {
        return Err("production upgrade chunk response hash is invalid".into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn upload_production_canister_upgrade_chunks(
    host: &str,
    canister_text: &str,
    expected_principal_text: &str,
    pem_path: &Path,
    wasm_path: &Path,
    submission_path: &Path,
    evidence_dir: &Path,
) -> Result<(), String> {
    let canister = Principal::from_text(canister_text).map_err(|error| error.to_string())?;
    let expected_principal =
        Principal::from_text(expected_principal_text).map_err(|error| error.to_string())?;
    let wasm = fs::read(wasm_path).map_err(|error| error.to_string())?;
    let (agent, sender) = production_upgrade_agent(host, expected_principal, pem_path)?;
    let submission =
        validate_production_upgrade_submission(host, canister, sender, &wasm, submission_path)?;
    if evidence_dir.exists() {
        let metadata = fs::symlink_metadata(evidence_dir).map_err(|error| error.to_string())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("production upgrade upload evidence path is unsafe".into());
        }
    } else {
        fs::create_dir(evidence_dir).map_err(|error| error.to_string())?;
    }
    let complete_path = evidence_dir.join("complete.json");
    if complete_path.exists() {
        let bytes = fs::read(&complete_path).map_err(|error| error.to_string())?;
        validate_production_upgrade_upload_evidence(&submission, &bytes)?;
        println!("chunk_upload_evidence={}", complete_path.display());
        return Ok(());
    }
    let stored_path = evidence_dir.join("stored-chunks.json");
    let stored_response = if stored_path.exists() {
        read_json::<ProductionUpgradeStoredChunksResponse>(&stored_path)?
    } else {
        production_upgrade_request_has_time(submission.stored_chunks.ingress_expiry)?;
        let raw = match send_production_upgrade_signed_update(
            &agent,
            canister,
            &submission.stored_chunks.request_id,
            &submission.stored_chunks.signed_update_hex,
        ) {
            Ok(raw) => raw,
            Err(error) => {
                record_production_upgrade_send_error(
                    evidence_dir,
                    "stored-chunks",
                    &submission.stored_chunks.request_id,
                    &error,
                )?;
                return Err(error);
            }
        };
        let response = ProductionUpgradeStoredChunksResponse {
            schema_version: 1,
            request_id: submission.stored_chunks.request_id.clone(),
            response_hex: hex(&raw),
            response_sha256: hex(&Sha256::digest(&raw)),
        };
        validate_stored_chunks_response(&submission, &response)?;
        write_json_new(&stored_path, &response)?;
        response
    };
    validate_stored_chunks_response(&submission, &stored_response)?;
    let mut responses = Vec::with_capacity(submission.chunks.len());
    for chunk in &submission.chunks {
        let response_path = evidence_dir.join(format!("chunk-{:04}.json", chunk.index));
        let response = if response_path.exists() {
            read_json::<ProductionUpgradeChunkResponse>(&response_path)?
        } else {
            production_upgrade_request_has_time(chunk.ingress_expiry)?;
            let raw = match send_production_upgrade_signed_update(
                &agent,
                canister,
                &chunk.request_id,
                &chunk.signed_update_hex,
            ) {
                Ok(raw) => raw,
                Err(error) => {
                    record_production_upgrade_send_error(
                        evidence_dir,
                        &format!("chunk-{:04}", chunk.index),
                        &chunk.request_id,
                        &error,
                    )?;
                    return Err(error);
                }
            };
            let response = ProductionUpgradeChunkResponse {
                schema_version: 1,
                index: chunk.index,
                request_id: chunk.request_id.clone(),
                response_hex: hex(&raw),
                response_sha256: hex(&Sha256::digest(&raw)),
            };
            validate_chunk_response(chunk, &response)?;
            write_json_new(&response_path, &response)?;
            response
        };
        validate_chunk_response(chunk, &response)?;
        responses.push(response);
    }
    let complete = ProductionUpgradeUploadEvidence {
        schema_version: 1,
        stored_chunks_request_id: stored_response.request_id,
        stored_chunks_response_hex: stored_response.response_hex,
        stored_chunks_response_sha256: stored_response.response_sha256,
        chunks: responses,
    };
    let complete_bytes = serde_json::to_vec(&complete).map_err(|error| error.to_string())?;
    validate_production_upgrade_upload_evidence(&submission, &complete_bytes)?;
    write_json_new(&complete_path, &complete)?;
    println!("chunk_upload_evidence={}", complete_path.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn submit_production_canister_upgrade(
    host: &str,
    canister_text: &str,
    expected_principal_text: &str,
    pem_path: &Path,
    wasm_path: &Path,
    submission_path: &Path,
    upload_evidence_path: &Path,
    response_path: &Path,
) -> Result<(), String> {
    let canister = Principal::from_text(canister_text).map_err(|error| error.to_string())?;
    let expected_principal =
        Principal::from_text(expected_principal_text).map_err(|error| error.to_string())?;
    let wasm = fs::read(wasm_path).map_err(|error| error.to_string())?;
    let wasm_sha256 = hex(&Sha256::digest(&wasm));
    let (agent, sender) = production_upgrade_agent(host, expected_principal, pem_path)?;
    let submission =
        validate_production_upgrade_submission(host, canister, sender, &wasm, submission_path)?;
    let upload_evidence = fs::read(upload_evidence_path).map_err(|error| error.to_string())?;
    validate_production_upgrade_upload_evidence(&submission, &upload_evidence)?;
    production_upgrade_request_has_time(submission.ingress_expiry)?;
    let request_id = submission.request_id.clone();
    let mut durable_response = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(response_path)
        .map_err(|error| format!("{}: {error}", response_path.display()))?;
    writeln!(durable_response, "request_id={request_id}").map_err(|error| error.to_string())?;
    durable_response
        .sync_all()
        .map_err(|error| error.to_string())?;
    fs::File::open(response_path.parent().unwrap_or_else(|| Path::new(".")))
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())?;
    println!("request_id={request_id}");
    std::io::stdout()
        .flush()
        .map_err(|error| error.to_string())?;
    let response = send_production_upgrade_signed_update(
        &agent,
        canister,
        &request_id,
        &submission.signed_update_hex,
    )?;
    writeln!(durable_response, "response_hex={}", hex(&response))
        .map_err(|error| error.to_string())?;
    writeln!(durable_response, "sender_principal={sender}").map_err(|error| error.to_string())?;
    writeln!(durable_response, "wasm_sha256={wasm_sha256}").map_err(|error| error.to_string())?;
    durable_response
        .sync_all()
        .map_err(|error| error.to_string())?;
    println!("response_hex={}", hex(&response));
    println!("sender_principal={sender}");
    println!("wasm_sha256={wasm_sha256}");
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
        Some("validate-production-upgrade-gate-a-binding") if args.len() == 4 => {
            println!(
                "{}",
                validate_production_upgrade_gate_a_binding_files(
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
        Some("append-production-upgrade-receipt") if args.len() == 5 => {
            let prior = (args[2] != "-").then(|| Path::new(&args[2]));
            append_production_upgrade_receipt(prior, Path::new(&args[3]), Path::new(&args[4]))?;
        }
        Some("verify-production-canister-predeploy") if args.len() == 4 => {
            verify_production_canister_predeploy(Path::new(&args[2]), Path::new(&args[3]))?;
        }
        Some("validate-production-handover-candidate") if args.len() == 6 => {
            let (bundle, _, _) = validate_production_handover_candidate_files(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
                Path::new(&args[5]),
            )?;
            println!(
                "production_handover_candidate=pass manifest_sha256={}",
                bundle.manifest_sha256
            );
        }
        Some("validate-controller-handover-completion") if args.len() == 7 => {
            validate_controller_handover_completion_files(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
                Path::new(&args[5]),
                Path::new(&args[6]),
            )?;
        }
        Some("validate-controller-handover-recovery") if args.len() == 7 => {
            validate_controller_handover_recovery_files(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
                Path::new(&args[5]),
                Path::new(&args[6]),
            )?;
        }
        Some("verify-production-canister-handover") if args.len() == 6 => {
            verify_production_canister_handover(
                Path::new(&args[2]),
                Path::new(&args[3]),
                Path::new(&args[4]),
                Path::new(&args[5]),
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
        Some("prepare-production-canister-upgrade") if args.len() == 8 => {
            prepare_production_canister_upgrade(
                &args[2],
                &args[3],
                &args[4],
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
            )?;
        }
        Some("upload-production-canister-upgrade-chunks") if args.len() == 9 => {
            upload_production_canister_upgrade_chunks(
                &args[2],
                &args[3],
                &args[4],
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
                Path::new(&args[8]),
            )?;
        }
        Some("validate-production-upgrade-submission") if args.len() == 7 => {
            let canister = Principal::from_text(&args[3]).map_err(|error| error.to_string())?;
            let sender = Principal::from_text(&args[4]).map_err(|error| error.to_string())?;
            let wasm = fs::read(&args[5]).map_err(|error| error.to_string())?;
            let submission = validate_production_upgrade_submission(
                &args[2],
                canister,
                sender,
                &wasm,
                Path::new(&args[6]),
            )?;
            println!("{}", submission.request_id);
        }
        Some("submit-production-canister-upgrade") if args.len() == 10 => {
            submit_production_canister_upgrade(
                &args[2],
                &args[3],
                &args[4],
                Path::new(&args[5]),
                Path::new(&args[6]),
                Path::new(&args[7]),
                Path::new(&args[8]),
                Path::new(&args[9]),
            )?;
        }
        Some("production-upgrade-public-state-sha256") if args.len() == 6 => {
            let (_, _, digest) = production_upgrade_query_state(
                &args[2], &args[3], &args[4], &args[5],
            )?;
            println!("{digest}");
        }
        Some("verify-production-upgrade-state-preserved") if args.len() == 12 => {
            let (before, before_runtime, before_digest) = production_upgrade_query_state(
                &args[2], &args[3], &args[4], &args[5],
            )?;
            let (after, after_runtime, after_digest) = production_upgrade_query_state(
                &args[6], &args[7], &args[8], &args[9],
            )?;
            let gate_a_profile_source =
                fs::read(&args[10]).map_err(|error| error.to_string())?;
            let gate_a_profile: Profile = serde_json::from_slice(&gate_a_profile_source)
                .map_err(|error| error.to_string())?;
            let gate_a_receipt: GateAReceipt = read_json(Path::new(&args[11]))?;
            validate_production_upgrade_gate_a_binding(
                &gate_a_profile,
                &gate_a_profile_source,
                &gate_a_receipt,
            )?;
            let unchanged = production_upgrade_status_preserved(&before, &after)
                && args[3] == args[7]
                && args[4] == args[8]
                && args[5] == args[9]
                && before_digest.eq_ignore_ascii_case(&after_digest);
            let pause_migration = gate_a_receipt.canister_install.runtime_binding
                == live_runtime_binding_from_view(&before_runtime)
                && production_upgrade_pause_migration_matches(
                    &gate_a_profile,
                    &gate_a_receipt.canister_install.runtime_binding,
                    &before,
                    &after,
                    &before_runtime,
                    &after_runtime,
                )?
                && args[3] == args[7]
                && args[5] == args[9];
            if !unchanged && !pause_migration {
                return Err("production public state was not preserved across upgrade".into());
            }
            println!("{after_digest}");
        }
        Some("verify-production-upgrade-submission") if args.len() == 8 => {
            let canister = Principal::from_text(&args[3]).map_err(|error| error.to_string())?;
            let sender = Principal::from_text(&args[4]).map_err(|error| error.to_string())?;
            let wasm = fs::read(&args[5]).map_err(|error| error.to_string())?;
            let submission = validate_production_upgrade_submission(
                &args[2], canister, sender, &wasm, Path::new(&args[6]),
            )?;
            let evidence = fs::read(&args[7]).map_err(|error| error.to_string())?;
            validate_production_upgrade_upload_evidence(&submission, &evidence)?;
            println!("{}", submission.request_id);
        }
        _ => return Err("usage: bridge-profile <command> <arguments>; production upgrade commands: validate-production-upgrade-gate-a-binding, prepare-production-canister-upgrade, validate-production-upgrade-submission, upload-production-canister-upgrade-chunks, submit-production-canister-upgrade, verify-production-upgrade-submission, append-production-upgrade-receipt".into()),
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
    fn production_upgrade_query_state_rejects_insufficient_cycles_reserve() {
        let profile = valid_profile();
        let mut status = matching_handover_status();
        status.reserve.sufficient = false;
        let runtime = matching_handover_runtime(&profile, &status);
        let error = production_upgrade_query_state(
            &hex(&Encode!(&status).unwrap()),
            &hex(&Encode!(&ProductionLifecycleResultView::Ok(
                ProductionLifecycleView::Bootstrap
            ))
            .unwrap()),
            &hex(&Encode!(&runtime).unwrap()),
            &hex(&Encode!(&StorageIntegrityResultView::Ok("ok".into())).unwrap()),
        )
        .err()
        .expect("insufficient reserve must fail closed");
        assert!(error.contains("sufficient cycles reserve"));
    }

    #[test]
    fn production_upgrade_pause_migration_accepts_only_the_exact_runtime_and_audit_delta() {
        let mut gate_a_profile = valid_profile();
        gate_a_profile.pause_principal = KINIC_ROOT.into();
        let mut before_status = matching_handover_status();
        before_status.deposits_paused = true;
        let before_runtime = matching_handover_runtime(&gate_a_profile, &before_status);
        let gate_a_runtime = live_runtime_binding_from_view(&before_runtime);

        let mut migrated_profile = gate_a_profile.clone();
        migrated_profile.pause_principal = PRODUCTION_PAUSE_PRINCIPAL.into();
        let mut after_status = before_status.clone();
        after_status.counts.retained_audit_events += 1;
        let after_runtime = matching_handover_runtime(&migrated_profile, &after_status);
        assert!(production_upgrade_pause_migration_matches(
            &gate_a_profile,
            &gate_a_runtime,
            &before_status,
            &after_status,
            &before_runtime,
            &after_runtime,
        )
        .unwrap());

        let mut runtime_drift = after_runtime.clone();
        runtime_drift.schema_version += 1;
        assert!(!production_upgrade_pause_migration_matches(
            &gate_a_profile,
            &gate_a_runtime,
            &before_status,
            &after_status,
            &before_runtime,
            &runtime_drift,
        )
        .unwrap());

        let mut status_drift = after_status.clone();
        status_drift.counts.pending_ledger_operations += 1;
        assert!(!production_upgrade_pause_migration_matches(
            &gate_a_profile,
            &gate_a_runtime,
            &before_status,
            &status_drift,
            &before_runtime,
            &after_runtime,
        )
        .unwrap());
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
    fn production_handover_requires_active_integral_state_and_fresh_attestation() {
        let profile = valid_profile();
        assert!(profile.rpc_providers.is_empty());
        let install_receipt = production_canister_receipt(&profile);
        let mut gate_a_receipt = handover_gate_a_receipt(&profile, install_receipt.clone());
        gate_a_receipt.bridge_canister_wasm_sha256 = "6".repeat(64);
        gate_a_receipt.canister_install.module_sha256 = "6".repeat(64);
        let installer = Principal::from_text(&install_receipt.installer_principal).unwrap();
        let controllers = [installer];
        let module_hash = decode_hex(&profile.bridge_canister_wasm_sha256).unwrap();
        let created = 1_000_000;
        let now = created + 600;
        let mut attestation = matching_activation_attestation(&profile, now, 101);
        attestation.deposits_paused = false;
        attestation.withdrawals_paused = false;
        let status = matching_handover_status();
        let runtime = matching_handover_runtime(&profile, &status);
        let storage_integrity = StorageIntegrityResultView::Ok("ok".into());
        let activation_status = ActivationStatusView {
            deposits_paused: false,
            pending_timelock_operation: None,
            last_confirmed_activation: Some(ActivationConfirmationStatusView {
                phase: "execute".into(),
                governance_operation_id: 8,
                timelock_operation_id: vec![0x51; 32],
                transaction_hash: vec![0x52; 32],
                receipt_block_number: 102,
                generation: 0,
                signed_at_ns: 123,
            }),
        };
        let timelock_operation_id = format!("0x{}", "51".repeat(32));
        let transaction_hash = format!("0x{}", "52".repeat(32));
        let activation = ProductionHandoverActivationBinding {
            governance_operation_id: 8,
            finalized_block_number: 102,
            timelock_operation_id: &timelock_operation_id,
            transaction_hash: &transaction_hash,
            confirmed_generation: 0,
            confirmed_signed_at_ns: "123",
        };
        let validate = |lifecycle: &ProductionLifecycleView,
                        attestation: Option<&ActivationAttestationView>,
                        controllers: &[Principal],
                        module_hash: &[u8]| {
            let observation = ProductionHandoverCanisterObservation {
                lifecycle,
                attestation,
                activation_status: &activation_status,
                runtime: &runtime,
                status: &status,
                storage_integrity: &storage_integrity,
                controllers,
                module_hash,
            };
            validate_production_handover_canister_state(
                &profile,
                installer,
                &gate_a_receipt,
                &activation,
                &observation,
                created,
                now,
            )
        };
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&attestation),
            &controllers,
            &module_hash,
        )
        .is_ok());
        let confirmed = activation_status
            .last_confirmed_activation
            .as_ref()
            .unwrap();
        assert!(controller_activation_confirmation_fields_match(
            0, "123", confirmed,
        ));
        for (generation, signed_at_ns) in [
            (1, "123"),
            (0, "124"),
            (0, "0123"),
            (0, "+123"),
            (0, "18446744073709551616"),
        ] {
            assert!(!controller_activation_confirmation_fields_match(
                generation,
                signed_at_ns,
                confirmed,
            ));
        }
        for (generation, signed_at_ns) in [
            (1, "123"),
            (0, "124"),
            (0, "0123"),
            (0, "+123"),
            (0, "18446744073709551616"),
        ] {
            let drifted_activation = ProductionHandoverActivationBinding {
                governance_operation_id: 8,
                finalized_block_number: 102,
                timelock_operation_id: &timelock_operation_id,
                transaction_hash: &transaction_hash,
                confirmed_generation: generation,
                confirmed_signed_at_ns: signed_at_ns,
            };
            let observation = ProductionHandoverCanisterObservation {
                lifecycle: &ProductionLifecycleView::Activated,
                attestation: Some(&attestation),
                activation_status: &activation_status,
                runtime: &runtime,
                status: &status,
                storage_integrity: &storage_integrity,
                controllers: &controllers,
                module_hash: &module_hash,
            };
            assert!(validate_production_handover_canister_state(
                &profile,
                installer,
                &gate_a_receipt,
                &drifted_activation,
                &observation,
                created,
                now,
            )
            .is_err());
        }
        assert!(validate(
            &ProductionLifecycleView::Bootstrap,
            Some(&attestation),
            &controllers,
            &module_hash,
        )
        .is_err());
        assert!(validate(
            &ProductionLifecycleView::OperationalConfigSealed,
            Some(&attestation),
            &controllers,
            &module_hash,
        )
        .is_err());
        assert!(validate(
            &ProductionLifecycleView::Activated,
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
            &ProductionLifecycleView::Activated,
            Some(&stale),
            &controllers,
            &module_hash,
        )
        .is_err());
        stale.observed_at_ns = now * 1_000_000_000;
        stale.finalized_block_number = 100;
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&stale),
            &controllers,
            &module_hash,
        )
        .is_err());

        let mut profile_drift = attestation;
        profile_drift.chain_id = 84532;
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&profile_drift),
            &controllers,
            &module_hash,
        )
        .is_err());
        let initial_install_module =
            decode_hex(&gate_a_receipt.bridge_canister_wasm_sha256).unwrap();
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&matching_activation_attestation(&profile, now, 101)),
            &controllers,
            &initial_install_module,
        )
        .is_err());
        let mut base_deposit_paused = matching_activation_attestation(&profile, now, 101);
        base_deposit_paused.deposits_paused = true;
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&base_deposit_paused),
            &controllers,
            &module_hash,
        )
        .is_err());
        let mut base_withdrawal_paused = matching_activation_attestation(&profile, now, 101);
        base_withdrawal_paused.withdrawals_paused = true;
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&base_withdrawal_paused),
            &controllers,
            &module_hash,
        )
        .is_err());
        let extra_controllers = [controllers[0], Principal::anonymous()];
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&matching_activation_attestation(&profile, now, 101)),
            &extra_controllers,
            &module_hash,
        )
        .is_err());
        let mut drifted_module = module_hash.clone();
        drifted_module[0] ^= 1;
        assert!(validate(
            &ProductionLifecycleView::Activated,
            Some(&matching_activation_attestation(&profile, now, 101)),
            &controllers,
            &drifted_module,
        )
        .is_err());

        let active_lifecycle = ProductionLifecycleView::Activated;
        let validate_runtime =
            |candidate_profile: &Profile,
             candidate_runtime: &RuntimeBindingView,
             candidate_status: &BridgeStatusLiveView| {
                let candidate_attestation =
                    matching_activation_attestation(candidate_profile, now, 101);
                let observation = ProductionHandoverCanisterObservation {
                    lifecycle: &active_lifecycle,
                    attestation: Some(&candidate_attestation),
                    activation_status: &activation_status,
                    runtime: candidate_runtime,
                    status: candidate_status,
                    storage_integrity: &storage_integrity,
                    controllers: &controllers,
                    module_hash: &module_hash,
                };
                validate_production_handover_canister_state(
                    candidate_profile,
                    installer,
                    &gate_a_receipt,
                    &activation,
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
        let mut schema_drift = matching_handover_runtime(&profile, &status);
        schema_drift.schema_version -= 1;
        assert!(validate_runtime(&profile, &schema_drift, &status).is_err());
        let mut runtime_code_drift = matching_handover_runtime(&profile, &status);
        runtime_code_drift.expected_bridge_runtime_sha256[0] ^= 1;
        assert!(validate_runtime(&profile, &runtime_code_drift, &status).is_err());
        let mut insufficient_reserve = matching_handover_status();
        insufficient_reserve.reserve.sufficient = false;
        assert!(validate_runtime(&profile, &runtime, &insufficient_reserve).is_err());
        let mut paused = matching_handover_status();
        paused.deposits_paused = true;
        assert!(validate_runtime(&profile, &runtime, &paused).is_err());

        let failed_integrity = StorageIntegrityResultView::Err(Reserved);
        let observation = ProductionHandoverCanisterObservation {
            lifecycle: &active_lifecycle,
            attestation: Some(&matching_activation_attestation(&profile, now, 101)),
            activation_status: &activation_status,
            runtime: &runtime,
            status: &status,
            storage_integrity: &failed_integrity,
            controllers: &controllers,
            module_hash: &module_hash,
        };
        assert!(validate_production_handover_canister_state(
            &profile,
            installer,
            &gate_a_receipt,
            &activation,
            &observation,
            created,
            now,
        )
        .is_err());
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
            schema_version: 4,
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
        profile.pause_principal = KINIC_ROOT.into();
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
            .arg(&test_helper)
            .arg(root.join("rpc-e2e.json"))
            .arg(&profile.expected_bridge_signer)
            .arg(&profile.bridge_canister_wasm_sha256)
            .arg(&profile.bridge_runtime_bytecode_sha256)
            .status()
            .unwrap();
        assert!(generated.success());
        let mut drill = MonitorDrill {
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
        let installer = test_principal(31);
        let json_bytes = |value: Value| serde_json::to_vec(&value).unwrap();
        let before_management = json_bytes(serde_json::json!({
            "controllers": [installer],
            "module_hash": profile.bridge_canister_wasm_sha256.clone(),
        }));
        let after_management = json_bytes(serde_json::json!({
            "controllers": [KINIC_ROOT],
            "module_hash": profile.bridge_canister_wasm_sha256.clone(),
        }));
        let bridge_status = json_bytes(serde_json::json!({
            "reserve": {"sufficient": true},
            "deposits_paused": false,
            "mint_authorization_ttl_seconds": 900,
            "mint_authorization_epoch": 7,
            "counts": {"deposits": 2,"withdrawals": 3,"retained_audit_events": 8,"pruned_audit_events": 5}
        }));
        let lifecycle = json_bytes(serde_json::json!({"Ok":{"Activated":null}}));
        let operational_config_sha256 = hex(&expected_operational_config_sha256(&profile, 900, 7)
            .expect("derive operational config binding"));
        let runtime = json_bytes(serde_json::json!({
            "base_chain_id": profile.chain_id,
            "bridge_contract": profile.bridge_contract,
            "expected_bridge_runtime_sha256": profile.bridge_runtime_bytecode_sha256,
            "timelock_contract": profile.timelock.address,
            "deployment_instance_id": profile.deployment_instance_id,
            "minimum_withdrawal_id": profile.minimum_withdrawal_id,
            "ledger_canister_id": profile.ledger_canister_id,
            "index_canister_id": profile.index_canister_id,
            "schema_version": profile.canister_schema_version,
            "expected_bridge_signer": profile.expected_bridge_signer,
            "evm_rpc_canister_id": profile.evm_rpc_canister_id,
            "rpc_provider_urls_sha256": hex(&canonical_sha256(&Vec::<String>::new()).unwrap()),
            "operational_config_sha256": operational_config_sha256,
        }));
        let integrity = json_bytes(serde_json::json!({"Ok":"ok"}));
        let activation_status = json_bytes(serde_json::json!({
            "Ok":{"deposits_paused":false,"pending_timelock_operation":[],"last_confirmed_activation":[{"phase":"execute"}]}
        }));
        let attestation = json_bytes(serde_json::json!({
            "Ok":{"deposits_paused":false,"withdrawals_paused":false}
        }));
        let handover = ControllerHandover {
            schema_version: 3,
            stage: "complete".into(),
            observed_at_unix: now - 95,
            source_revision: "1".repeat(40),
            source_tree_sha256: "1".repeat(64),
            gate_b_manifest_sha256: "2".repeat(64),
            operational_config_seal_receipt_sha256: "3".repeat(64),
            controller_schedule_receipt_sha256: "4".repeat(64),
            controller_execute_receipt_sha256: "5".repeat(64),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            sns_root_canister_id: profile.root_canister_id.clone(),
            executing_principal: installer.clone(),
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
            before_controllers: vec![installer.clone()],
            before_module_sha256: profile.bridge_canister_wasm_sha256.clone(),
            pre_send_controllers: vec![installer.clone()],
            pre_send_module_sha256: profile.bridge_canister_wasm_sha256.clone(),
            final_controllers: vec![profile.root_canister_id.clone()],
            after_module_sha256: profile.bridge_canister_wasm_sha256.clone(),
            before_management_status_response_json_hex: hex(&before_management),
            before_management_status_response_sha256: hex(&Sha256::digest(&before_management)),
            pre_send_management_status_response_json_hex: hex(&before_management),
            pre_send_management_status_response_sha256: hex(&Sha256::digest(&before_management)),
            pre_send_bridge_status_response_json_hex: hex(&bridge_status),
            pre_send_bridge_status_response_sha256: hex(&Sha256::digest(&bridge_status)),
            pre_send_lifecycle_response_json_hex: hex(&lifecycle),
            pre_send_lifecycle_response_sha256: hex(&Sha256::digest(&lifecycle)),
            pre_send_runtime_binding_response_json_hex: hex(&runtime),
            pre_send_runtime_binding_response_sha256: hex(&Sha256::digest(&runtime)),
            pre_send_storage_integrity_response_json_hex: hex(&integrity),
            pre_send_storage_integrity_response_sha256: hex(&Sha256::digest(&integrity)),
            pre_send_activation_status_response_json_hex: hex(&activation_status),
            pre_send_activation_status_response_sha256: hex(&Sha256::digest(&activation_status)),
            pre_send_activation_attestation_response_json_hex: hex(&attestation),
            pre_send_activation_attestation_response_sha256: hex(&Sha256::digest(&attestation)),
            after_management_status_response_json_hex: hex(&after_management),
            after_management_status_response_sha256: hex(&Sha256::digest(&after_management)),
            before_bridge_status_response_json_hex: hex(&bridge_status),
            before_bridge_status_response_sha256: hex(&Sha256::digest(&bridge_status)),
            after_bridge_status_response_json_hex: hex(&bridge_status),
            after_bridge_status_response_sha256: hex(&Sha256::digest(&bridge_status)),
            before_lifecycle_response_json_hex: hex(&lifecycle),
            before_lifecycle_response_sha256: hex(&Sha256::digest(&lifecycle)),
            after_lifecycle_response_json_hex: hex(&lifecycle),
            after_lifecycle_response_sha256: hex(&Sha256::digest(&lifecycle)),
            before_runtime_binding_response_json_hex: hex(&runtime),
            before_runtime_binding_response_sha256: hex(&Sha256::digest(&runtime)),
            after_runtime_binding_response_json_hex: hex(&runtime),
            after_runtime_binding_response_sha256: hex(&Sha256::digest(&runtime)),
            before_storage_integrity_response_json_hex: hex(&integrity),
            before_storage_integrity_response_sha256: hex(&Sha256::digest(&integrity)),
            after_storage_integrity_response_json_hex: hex(&integrity),
            after_storage_integrity_response_sha256: hex(&Sha256::digest(&integrity)),
            before_activation_status_response_json_hex: hex(&activation_status),
            before_activation_status_response_sha256: hex(&Sha256::digest(&activation_status)),
            after_activation_status_response_json_hex: hex(&activation_status),
            after_activation_status_response_sha256: hex(&Sha256::digest(&activation_status)),
            before_activation_attestation_response_json_hex: hex(&attestation),
            before_activation_attestation_response_sha256: hex(&Sha256::digest(&attestation)),
            after_activation_attestation_response_json_hex: hex(&attestation),
            after_activation_attestation_response_sha256: hex(&Sha256::digest(&attestation)),
            cycles_balance: 10_000_000,
            freezing_threshold_seconds: 86_400,
            idle_cycles_burned_per_day: 1_000,
            required_freezing_cycles: 1_000,
            pre_send_cycles_balance: 10_000_000,
            pre_send_required_freezing_cycles: 1_000,
            pre_send_checkpoint_json_hex: String::new(),
            pre_send_checkpoint_sha256: String::new(),
            recovered_without_request_id: false,
        };
        assert!(validate_controller_handover_continuity(&handover, &profile, &installer).is_ok());
        assert!(validate_controller_handover_completion(
            &handover,
            &profile,
            &installer,
            now - 100,
            now,
        )
        .is_ok());
        let mut schema4 = handover.clone();
        schema4.schema_version = 4;
        let mut checkpoint = serde_json::to_value(&handover).unwrap();
        checkpoint["schema_version"] = Value::from(4);
        checkpoint["stage"] = Value::from("pre_send_checkpoint");
        let checkpoint = serde_json::to_vec(&checkpoint).unwrap();
        schema4.pre_send_checkpoint_json_hex = hex(&checkpoint);
        schema4.pre_send_checkpoint_sha256 = hex(&Sha256::digest(&checkpoint));
        assert!(validate_controller_handover_completion(
            &schema4,
            &profile,
            &installer,
            now - 100,
            now,
        )
        .is_ok());
        let mut recovered_without_request = schema4.clone();
        recovered_without_request.request_id.clear();
        recovered_without_request.response_stdout_hex.clear();
        recovered_without_request.response_stderr_hex.clear();
        recovered_without_request.response_sha256 = hex(&Sha256::digest([]));
        recovered_without_request.recovered_without_request_id = true;
        assert!(validate_controller_handover_completion(
            &recovered_without_request,
            &profile,
            &installer,
            now - 100,
            now,
        )
        .is_ok());
        let mut checkpoint_drift = schema4.clone();
        checkpoint_drift.pre_send_checkpoint_json_hex = hex(b"{}");
        checkpoint_drift.pre_send_checkpoint_sha256 = hex(&Sha256::digest(b"{}"));
        assert!(validate_controller_handover_completion(
            &checkpoint_drift,
            &profile,
            &installer,
            now - 100,
            now,
        )
        .is_err());
        let mut pre_manifest = handover.clone();
        pre_manifest.observed_at_unix = now - 101;
        assert!(validate_controller_handover_completion(
            &pre_manifest,
            &profile,
            &installer,
            now - 100,
            now,
        )
        .is_err());
        let mut future = handover.clone();
        future.observed_at_unix = now + 1;
        assert!(validate_controller_handover_completion(
            &future,
            &profile,
            &installer,
            now - 100,
            now,
        )
        .is_err());
        let mut stale = handover.clone();
        stale.observed_at_unix = now - MAX_EVIDENCE_AGE_SECS - 1;
        assert!(validate_controller_handover_completion(
            &stale,
            &profile,
            &installer,
            stale.observed_at_unix - 1,
            now,
        )
        .is_err());
        let lineage_matches = |value: &ControllerHandover| {
            controller_handover_lineage_fields_match(
                value,
                &"1".repeat(40),
                &"1".repeat(64),
                &"2".repeat(64),
                &"3".repeat(64),
                &"4".repeat(64),
                &"5".repeat(64),
            )
        };
        assert!(lineage_matches(&handover));
        for mutate in [
            |value: &mut ControllerHandover| value.source_revision = "9".repeat(40),
            |value: &mut ControllerHandover| value.source_tree_sha256 = "9".repeat(64),
            |value: &mut ControllerHandover| value.gate_b_manifest_sha256 = "9".repeat(64),
            |value: &mut ControllerHandover| {
                value.operational_config_seal_receipt_sha256 = "9".repeat(64)
            },
            |value: &mut ControllerHandover| {
                value.controller_schedule_receipt_sha256 = "9".repeat(64)
            },
            |value: &mut ControllerHandover| {
                value.controller_execute_receipt_sha256 = "9".repeat(64)
            },
        ] {
            let mut drift = handover.clone();
            mutate(&mut drift);
            assert!(!lineage_matches(&drift));
        }
        let mut runtime_drift = handover.clone();
        let after_runtime = json_bytes(serde_json::json!({
            "schema_version": profile.canister_schema_version,
            "operational_config_sha256": "8".repeat(64)
        }));
        runtime_drift.after_runtime_binding_response_json_hex = hex(&after_runtime);
        runtime_drift.after_runtime_binding_response_sha256 = hex(&Sha256::digest(&after_runtime));
        assert!(
            validate_controller_handover_continuity(&runtime_drift, &profile, &installer).is_err()
        );
        let mut wrong_runtime = handover.clone();
        let wrong_runtime_bytes = json_bytes(serde_json::json!({
            "base_chain_id": profile.chain_id,
            "bridge_contract": profile.bridge_contract,
            "expected_bridge_runtime_sha256": profile.bridge_runtime_bytecode_sha256,
            "timelock_contract": profile.timelock.address,
            "deployment_instance_id": profile.deployment_instance_id,
            "minimum_withdrawal_id": profile.minimum_withdrawal_id,
            "ledger_canister_id": profile.ledger_canister_id,
            "index_canister_id": profile.index_canister_id,
            "schema_version": profile.canister_schema_version,
            "expected_bridge_signer": profile.expected_bridge_signer,
            "evm_rpc_canister_id": profile.evm_rpc_canister_id,
            "rpc_provider_urls_sha256": hex(&canonical_sha256(&Vec::<String>::new()).unwrap()),
            "operational_config_sha256": "8".repeat(64),
        }));
        wrong_runtime.before_runtime_binding_response_json_hex = hex(&wrong_runtime_bytes);
        wrong_runtime.before_runtime_binding_response_sha256 =
            hex(&Sha256::digest(&wrong_runtime_bytes));
        wrong_runtime.after_runtime_binding_response_json_hex = hex(&wrong_runtime_bytes);
        wrong_runtime.after_runtime_binding_response_sha256 =
            hex(&Sha256::digest(&wrong_runtime_bytes));
        assert!(
            validate_controller_handover_continuity(&wrong_runtime, &profile, &installer).is_err()
        );
        let mut wrong_lifecycle = handover.clone();
        let not_activated = json_bytes(serde_json::json!({"Err":"not activated"}));
        wrong_lifecycle.before_lifecycle_response_json_hex = hex(&not_activated);
        wrong_lifecycle.before_lifecycle_response_sha256 = hex(&Sha256::digest(&not_activated));
        wrong_lifecycle.after_lifecycle_response_json_hex = hex(&not_activated);
        wrong_lifecycle.after_lifecycle_response_sha256 = hex(&Sha256::digest(&not_activated));
        assert!(
            validate_controller_handover_continuity(&wrong_lifecycle, &profile, &installer)
                .is_err()
        );
        let mut base_pause_drift = handover.clone();
        let paused_attestation = json_bytes(serde_json::json!({
            "Ok":{"deposits_paused":false,"withdrawals_paused":true}
        }));
        base_pause_drift.after_activation_attestation_response_json_hex = hex(&paused_attestation);
        base_pause_drift.after_activation_attestation_response_sha256 =
            hex(&Sha256::digest(&paused_attestation));
        assert!(
            validate_controller_handover_continuity(&base_pause_drift, &profile, &installer)
                .is_err()
        );
        let mut controller_race = handover.clone();
        controller_race
            .pre_send_controllers
            .push(test_principal(32));
        assert!(
            validate_controller_handover_continuity(&controller_race, &profile, &installer)
                .is_err()
        );
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
        let provider_independence = provider_independence_receipt(
            &profile,
            now - 30,
            "release-1",
            &"a".repeat(40),
            &"2".repeat(64),
            &hex(&Sha256::digest(serde_json::to_vec(&profile).unwrap())),
        )
        .unwrap();
        let ui_files = vec![UiAssetDigest {
            path: "assets/index.js".into(),
            sha256: hex(&Sha256::digest(b"ui")),
        }];
        let ui_assets = UiAssetsReceipt {
            schema_version: 2,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            walletconnect_project_id: "3".repeat(32),
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
        wrong_operation_id.governance_operation_id = 8;
        wrong_operation_id.operation_salt = format!(
            "0x{}",
            hex(&initial_activation_salt(deployment_instance_id, 8))
        );
        assert!(derive_initial_operational_parameters(&wrong_operation_id).is_err());
        let mut maximum_operation_id: InitialOperationalParameters =
            serde_json::from_value(serde_json::to_value(&initial_parameters).unwrap()).unwrap();
        maximum_operation_id.governance_operation_id = u64::MAX;
        let maximum_salt = initial_activation_salt(deployment_instance_id, u64::MAX);
        maximum_operation_id.operation_salt = format!("0x{}", hex(&maximum_salt));
        for estimate in &mut maximum_operation_id.gas_estimates {
            estimate.calldata_hex = initial_activation_calldata(
                &estimate.action,
                activation_bridge,
                maximum_salt,
                profile.timelock.minimum_delay_seconds,
            )
            .unwrap();
        }
        assert!(derive_initial_operational_parameters(&maximum_operation_id).is_err());
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
        let upgrade_identity =
            Secp256k1Identity::from_private_key(k256::SecretKey::from_slice(&[7u8; 32]).unwrap());
        let upgrade_sender = upgrade_identity.sender().unwrap();
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
                installer_principal: upgrade_sender.to_text(),
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
        let gate_a_profile: Profile = serde_json::from_slice(&planned_profile).unwrap();
        assert!(validate_production_upgrade_gate_a_binding(
            &gate_a_profile,
            &planned_profile,
            &receipt,
        )
        .is_ok());
        let mut independently_installed_receipt = receipt.clone();
        independently_installed_receipt
            .canister_install
            .source_revision = "b".repeat(40);
        independently_installed_receipt
            .canister_install
            .source_tree_sha256 = "3".repeat(64);
        independently_installed_receipt
            .canister_install
            .plan
            .source_revision = "b".repeat(40);
        independently_installed_receipt
            .canister_install
            .plan
            .source_tree_sha256 = "3".repeat(64);
        independently_installed_receipt.canister_install.plan_sha256 =
            hex(&canonical_sha256(&independently_installed_receipt.canister_install.plan).unwrap());
        assert!(validate_production_upgrade_gate_a_binding(
            &gate_a_profile,
            &planned_profile,
            &independently_installed_receipt,
        )
        .is_ok());
        assert!(validate_completed_gate_a_receipt(
            &gate_a,
            &independently_installed_receipt,
            &independently_installed_receipt.canister_install,
            &deployment_binding,
        )
        .is_ok());
        let mut forged_gate_a_receipt = receipt.clone();
        forged_gate_a_receipt
            .canister_install
            .runtime_binding
            .operational_config_sha256 = "9".repeat(64);
        assert!(validate_production_upgrade_gate_a_binding(
            &gate_a_profile,
            &planned_profile,
            &forged_gate_a_receipt,
        )
        .is_err());
        profile.pause_principal = PRODUCTION_PAUSE_PRINCIPAL.into();
        let mut upgraded_wasm = vec![0x61; PRODUCTION_UPGRADE_CHUNK_SIZE + 1];
        upgraded_wasm[..4].copy_from_slice(b"\0asm");
        profile.bridge_canister_wasm_sha256 = hex(&Sha256::digest(&upgraded_wasm));
        fs::write(root.join("bridge-canister.wasm"), &upgraded_wasm).unwrap();
        artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "bridge-canister.wasm")
            .unwrap()
            .sha256 = profile.bridge_canister_wasm_sha256.clone();
        drill.bridge_canister_wasm_sha256 = profile.bridge_canister_wasm_sha256.clone();
        let drill_bytes = serde_json::to_vec(&drill).unwrap();
        fs::write(root.join("monitor-drill.json"), &drill_bytes).unwrap();
        artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "monitor-drill.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&drill_bytes));
        let regenerated = Command::new("python3")
            .arg("-c")
            .arg(python)
            .arg(&test_helper)
            .arg(root.join("rpc-e2e.json"))
            .arg(&profile.expected_bridge_signer)
            .arg(&profile.bridge_canister_wasm_sha256)
            .arg(&profile.bridge_runtime_bytecode_sha256)
            .status()
            .unwrap();
        assert!(regenerated.success());
        let rpc_bytes = fs::read(root.join("rpc-e2e.json")).unwrap();
        artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "rpc-e2e.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&rpc_bytes));
        profile.parameters = final_operational_parameters;
        let final_profile_bytes = serde_json::to_vec(&profile).unwrap();
        fs::write(root.join("profile.json"), &final_profile_bytes).unwrap();
        artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "profile.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&final_profile_bytes));
        let provider_independence = provider_independence_receipt(
            &profile,
            now - 30,
            "release-1",
            &"a".repeat(40),
            &"2".repeat(64),
            &hex(&Sha256::digest(&final_profile_bytes)),
        )
        .unwrap();
        let provider_independence_bytes = serde_json::to_vec(&provider_independence).unwrap();
        fs::write(
            root.join("provider-independence.json"),
            &provider_independence_bytes,
        )
        .unwrap();
        artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "provider-independence.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&provider_independence_bytes));
        let receipt_bytes = serde_json::to_vec(&receipt).unwrap();
        fs::write(root.join("gate-a-receipt.json"), &receipt_bytes).unwrap();
        let gate_a_profile_bytes = canonical_bytes(&gate_a_profile).unwrap();
        fs::write(root.join("gate-a-profile.json"), &gate_a_profile_bytes).unwrap();
        initial_parameters.profile_sha256 = hex(&canonical_sha256(&profile).unwrap());
        let initial_parameters_bytes = serde_json::to_vec(&initial_parameters).unwrap();
        fs::write(
            root.join("initial-operational-parameters.json"),
            &initial_parameters_bytes,
        )
        .unwrap();
        let production_upgrade = ProductionCanisterUpgradeReceipt {
            // These raw responses are produced by the tracked production upgrade
            // driver; the fixture uses the same Candid and management-status shapes.
            schema_version: 1,
            kind: "production-controller-bootstrap-upgrade".into(),
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            install_mode: "upgrade".into(),
            executing_principal: receipt.canister_install.installer_principal.clone(),
            executed_at_unix: manifest_created,
            verified_at_unix: manifest_created,
            recovered: false,
            recovered_at_unix: None,
            before_controllers: vec![receipt.canister_install.installer_principal.clone()],
            after_controllers: vec![receipt.canister_install.installer_principal.clone()],
            before_module_sha256: gate_a_profile.bridge_canister_wasm_sha256.clone(),
            after_module_sha256: profile.bridge_canister_wasm_sha256.clone(),
            wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            before_schema_version: CURRENT_STABLE_SCHEMA_VERSION,
            after_schema_version: CURRENT_STABLE_SCHEMA_VERSION,
            before_lifecycle: "Bootstrap".into(),
            after_lifecycle: "Bootstrap".into(),
            before_deposits_paused: true,
            after_deposits_paused: true,
            before_storage_validation_complete: true,
            after_storage_validation_complete: true,
            before_management_status_json_hex: String::new(),
            before_management_status_json_sha256: String::new(),
            after_management_status_json_hex: String::new(),
            after_management_status_json_sha256: String::new(),
            before_bridge_status_response_hex: String::new(),
            before_bridge_status_response_sha256: String::new(),
            after_bridge_status_response_hex: String::new(),
            after_bridge_status_response_sha256: String::new(),
            before_lifecycle_response_hex: String::new(),
            before_lifecycle_response_sha256: String::new(),
            after_lifecycle_response_hex: String::new(),
            after_lifecycle_response_sha256: String::new(),
            before_runtime_binding_response_hex: String::new(),
            before_runtime_binding_response_sha256: String::new(),
            after_runtime_binding_response_hex: String::new(),
            after_runtime_binding_response_sha256: String::new(),
            before_storage_integrity_response_hex: String::new(),
            before_storage_integrity_response_sha256: String::new(),
            after_storage_integrity_response_hex: String::new(),
            after_storage_integrity_response_sha256: String::new(),
            before_public_state_sha256: String::new(),
            after_public_state_sha256: String::new(),
            command_argv: [
                "bridge-profile",
                "submit-production-canister-upgrade",
                gate_a_profile.ic_host.as_str(),
                profile.bridge_canister_id.as_str(),
                receipt.canister_install.installer_principal.as_str(),
                "<production-controller-pem>",
                "<verified-release-artifact>",
                "<durable-submission-artifact>",
                "<durable-chunk-upload-evidence>",
                "<durable-response-artifact>",
            ]
            .map(str::to_string)
            .to_vec(),
            chunk_upload_evidence_json_hex: String::new(),
            chunk_upload_evidence_json_sha256: String::new(),
            submission_json_hex: String::new(),
            submission_json_sha256: String::new(),
            request_id: String::new(),
            response_stdout_hex: String::new(),
            response_stdout_sha256: String::new(),
            response_stderr_hex: String::new(),
            response_stderr_sha256: hex(&Sha256::digest([])),
        };
        let mut upgrade_status = matching_handover_status();
        upgrade_status.deposits_paused = true;
        let status_raw = Encode!(&upgrade_status).unwrap();
        let lifecycle_raw = Encode!(&ProductionLifecycleResultView::Ok(
            ProductionLifecycleView::Bootstrap
        ))
        .unwrap();
        let runtime_raw = Encode!(&matching_handover_runtime(
            &gate_a_profile,
            &matching_handover_status(),
        ))
        .unwrap();
        let integrity_raw = Encode!(&StorageIntegrityResultView::Ok("ok".into())).unwrap();
        let before_management_json = serde_json::to_vec(&serde_json::json!({
            "status": {
                "settings": {"controllers": [receipt.canister_install.installer_principal.clone()]},
                "module_hash": gate_a_profile.bridge_canister_wasm_sha256.clone(),
            }
        }))
        .unwrap();
        let after_management_json = serde_json::to_vec(&serde_json::json!({
            "status": {
                "settings": {"controllers": [receipt.canister_install.installer_principal.clone()]},
                "module_hash": profile.bridge_canister_wasm_sha256.clone(),
            }
        }))
        .unwrap();
        let mut production_upgrade = production_upgrade;
        production_upgrade.before_management_status_json_hex = hex(&before_management_json);
        production_upgrade.before_management_status_json_sha256 =
            hex(&Sha256::digest(&before_management_json));
        production_upgrade.after_management_status_json_hex = hex(&after_management_json);
        production_upgrade.after_management_status_json_sha256 =
            hex(&Sha256::digest(&after_management_json));
        for (before, before_digest, after, after_digest, raw) in [
            (
                &mut production_upgrade.before_bridge_status_response_hex,
                &mut production_upgrade.before_bridge_status_response_sha256,
                &mut production_upgrade.after_bridge_status_response_hex,
                &mut production_upgrade.after_bridge_status_response_sha256,
                &status_raw,
            ),
            (
                &mut production_upgrade.before_lifecycle_response_hex,
                &mut production_upgrade.before_lifecycle_response_sha256,
                &mut production_upgrade.after_lifecycle_response_hex,
                &mut production_upgrade.after_lifecycle_response_sha256,
                &lifecycle_raw,
            ),
            (
                &mut production_upgrade.before_runtime_binding_response_hex,
                &mut production_upgrade.before_runtime_binding_response_sha256,
                &mut production_upgrade.after_runtime_binding_response_hex,
                &mut production_upgrade.after_runtime_binding_response_sha256,
                &runtime_raw,
            ),
            (
                &mut production_upgrade.before_storage_integrity_response_hex,
                &mut production_upgrade.before_storage_integrity_response_sha256,
                &mut production_upgrade.after_storage_integrity_response_hex,
                &mut production_upgrade.after_storage_integrity_response_sha256,
                &integrity_raw,
            ),
        ] {
            *before = hex(raw);
            *before_digest = hex(&Sha256::digest(raw));
            *after = hex(raw);
            *after_digest = hex(&Sha256::digest(raw));
        }
        let mut migrated_upgrade_status = upgrade_status.clone();
        migrated_upgrade_status.counts.retained_audit_events += 1;
        let migrated_status_raw = Encode!(&migrated_upgrade_status).unwrap();
        production_upgrade.after_bridge_status_response_hex = hex(&migrated_status_raw);
        production_upgrade.after_bridge_status_response_sha256 =
            hex(&Sha256::digest(&migrated_status_raw));
        let mut migrated_gate_a_profile = gate_a_profile.clone();
        migrated_gate_a_profile.pause_principal = PRODUCTION_PAUSE_PRINCIPAL.into();
        let migrated_runtime_raw = Encode!(&matching_handover_runtime(
            &migrated_gate_a_profile,
            &migrated_upgrade_status,
        ))
        .unwrap();
        production_upgrade.after_runtime_binding_response_hex = hex(&migrated_runtime_raw);
        production_upgrade.after_runtime_binding_response_sha256 =
            hex(&Sha256::digest(&migrated_runtime_raw));
        let before_runtime_view = decode_candid_hex::<RuntimeBindingView>(
            &production_upgrade.before_runtime_binding_response_hex,
        )
        .unwrap();
        let after_runtime_view = decode_candid_hex::<RuntimeBindingView>(
            &production_upgrade.after_runtime_binding_response_hex,
        )
        .unwrap();
        assert!(
            live_runtime_binding_from_view(&before_runtime_view)
                == receipt.canister_install.runtime_binding
        );
        assert!(production_upgrade_pause_migration_matches(
            &gate_a_profile,
            &receipt.canister_install.runtime_binding,
            &upgrade_status,
            &migrated_upgrade_status,
            &before_runtime_view,
            &after_runtime_view,
        )
        .unwrap());
        production_upgrade.before_public_state_sha256 = production_upgrade_public_state_sha256(
            &upgrade_status,
            &[
                &production_upgrade.before_lifecycle_response_hex,
                &production_upgrade.before_runtime_binding_response_hex,
                &production_upgrade.before_storage_integrity_response_hex,
            ],
        )
        .unwrap();
        production_upgrade.after_public_state_sha256 = production_upgrade_public_state_sha256(
            &migrated_upgrade_status,
            &[
                &production_upgrade.after_lifecycle_response_hex,
                &production_upgrade.after_runtime_binding_response_hex,
                &production_upgrade.after_storage_integrity_response_hex,
            ],
        )
        .unwrap();
        let canister = Principal::from_text(&profile.bridge_canister_id).unwrap();
        let wasm = fs::read(root.join("bridge-canister.wasm")).unwrap();
        let signing_agent = Agent::builder()
            .with_url(&profile.ic_host)
            .with_identity(upgrade_identity)
            .build()
            .unwrap();
        let stored_chunks_argument = Encode!(&ManagementStoredChunksArgument {
            canister_id: canister,
        })
        .unwrap();
        let stored_chunks_signed = signing_agent
            .update(&Principal::management_canister(), "stored_chunks")
            .with_effective_canister_id(canister)
            .with_arg(stored_chunks_argument.clone())
            .sign()
            .unwrap();
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
            let chunk_argument = Encode!(&ManagementUploadChunkArgument {
                canister_id: canister,
                chunk: chunk.to_vec(),
            })
            .unwrap();
            let chunk_signed = signing_agent
                .update(&Principal::management_canister(), "upload_chunk")
                .with_effective_canister_id(canister)
                .with_arg(chunk_argument.clone())
                .sign()
                .unwrap();
            chunks.push(ProductionUpgradeChunkSubmission {
                index: index as u32,
                offset: (index * PRODUCTION_UPGRADE_CHUNK_SIZE) as u64,
                size_bytes: chunk.len() as u64,
                sha256: hex(&chunk_sha256),
                argument_hex: hex(&chunk_argument),
                argument_sha256: hex(&Sha256::digest(&chunk_argument)),
                ingress_expiry: chunk_signed.ingress_expiry,
                request_id: hex(chunk_signed.request_id.as_slice()),
                signed_update_hex: hex(&chunk_signed.signed_update),
                signed_update_sha256: hex(&Sha256::digest(&chunk_signed.signed_update)),
            });
            chunk_hashes_list.push(ManagementChunkHash { hash: chunk_sha256 });
        }
        let argument = Encode!(&ManagementInstallChunkedCodeArgument {
            mode: ManagementInstallMode::Upgrade,
            target_canister: canister,
            store_canister: None,
            chunk_hashes_list,
            wasm_module_hash: Sha256::digest(&wasm).to_vec(),
            arg: Vec::new(),
            sender_canister_version: None,
        })
        .unwrap();
        let signed = signing_agent
            .update(&Principal::management_canister(), "install_chunked_code")
            .with_effective_canister_id(canister)
            .with_arg(argument.clone())
            .sign()
            .unwrap();
        let submission = ProductionUpgradeSubmission {
            schema_version: 2,
            install_method: "install_chunked_code".into(),
            ic_host: profile.ic_host.clone(),
            effective_canister_id: profile.bridge_canister_id.clone(),
            sender_principal: upgrade_sender.to_text(),
            wasm_sha256: hex(&Sha256::digest(&wasm)),
            chunk_size_bytes: PRODUCTION_UPGRADE_CHUNK_SIZE as u64,
            stored_chunks,
            chunks,
            argument_hex: hex(&argument),
            argument_sha256: hex(&Sha256::digest(&argument)),
            ingress_expiry: signed.ingress_expiry,
            request_id: hex(signed.request_id.as_slice()),
            signed_update_hex: hex(&signed.signed_update),
            signed_update_sha256: hex(&Sha256::digest(&signed.signed_update)),
        };
        let submission_bytes = serde_json::to_vec(&submission).unwrap();
        assert_eq!(submission.chunks.len(), 2);
        assert_eq!(
            submission.chunks[0].size_bytes,
            PRODUCTION_UPGRADE_CHUNK_SIZE as u64
        );
        assert_eq!(submission.chunks[1].size_bytes, 1);
        let mut forged_submission: Value = serde_json::from_slice(&submission_bytes).unwrap();
        forged_submission["request_id"] = Value::String("9".repeat(64));
        assert!(validate_production_upgrade_submission_bytes(
            &profile.ic_host,
            canister,
            upgrade_sender,
            &wasm,
            &serde_json::to_vec(&forged_submission).unwrap(),
        )
        .is_err());
        let mut forged_install_argument: Value = serde_json::from_slice(&submission_bytes).unwrap();
        let mut altered_argument =
            decode_hex(forged_install_argument["argument_hex"].as_str().unwrap()).unwrap();
        *altered_argument.last_mut().unwrap() ^= 1;
        forged_install_argument["argument_hex"] = Value::String(hex(&altered_argument));
        forged_install_argument["argument_sha256"] =
            Value::String(hex(&Sha256::digest(&altered_argument)));
        assert!(validate_production_upgrade_submission_bytes(
            &profile.ic_host,
            canister,
            upgrade_sender,
            &wasm,
            &serde_json::to_vec(&forged_install_argument).unwrap(),
        )
        .is_err());
        let mut forged_chunk_submission: Value = serde_json::from_slice(&submission_bytes).unwrap();
        forged_chunk_submission["chunks"][0]["sha256"] = Value::String("9".repeat(64));
        assert!(validate_production_upgrade_submission_bytes(
            &profile.ic_host,
            canister,
            upgrade_sender,
            &wasm,
            &serde_json::to_vec(&forged_chunk_submission).unwrap(),
        )
        .is_err());
        let mut invalid_signature_envelope: Envelope<'_> =
            serde_cbor::from_slice(&signed.signed_update).unwrap();
        invalid_signature_envelope.sender_sig.as_mut().unwrap()[0] ^= 1;
        let invalid_signed_update = serde_cbor::to_vec(&invalid_signature_envelope).unwrap();
        let mut invalid_signature_submission: Value =
            serde_json::from_slice(&submission_bytes).unwrap();
        invalid_signature_submission["signed_update_hex"] =
            Value::String(hex(&invalid_signed_update));
        invalid_signature_submission["signed_update_sha256"] =
            Value::String(hex(&Sha256::digest(&invalid_signed_update)));
        assert!(validate_production_upgrade_submission_bytes(
            &profile.ic_host,
            canister,
            upgrade_sender,
            &wasm,
            &serde_json::to_vec(&invalid_signature_submission).unwrap(),
        )
        .is_err());
        let stored_response = Encode!(&Vec::<ManagementChunkHash>::new()).unwrap();
        let chunk_responses = submission
            .chunks
            .iter()
            .map(|chunk| {
                let response = Encode!(&ManagementChunkHash {
                    hash: decode_hex(&chunk.sha256).unwrap(),
                })
                .unwrap();
                ProductionUpgradeChunkResponse {
                    schema_version: 1,
                    index: chunk.index,
                    request_id: chunk.request_id.clone(),
                    response_hex: hex(&response),
                    response_sha256: hex(&Sha256::digest(&response)),
                }
            })
            .collect::<Vec<_>>();
        let upload_evidence = ProductionUpgradeUploadEvidence {
            schema_version: 1,
            stored_chunks_request_id: submission.stored_chunks.request_id.clone(),
            stored_chunks_response_hex: hex(&stored_response),
            stored_chunks_response_sha256: hex(&Sha256::digest(&stored_response)),
            chunks: chunk_responses,
        };
        let upload_evidence_bytes = serde_json::to_vec(&upload_evidence).unwrap();
        assert!(
            validate_production_upgrade_upload_evidence(&submission, &upload_evidence_bytes)
                .is_ok()
        );
        let unexpected_stored_response = Encode!(&vec![ManagementChunkHash {
            hash: vec![0x99; 32],
        }])
        .unwrap();
        let mut unexpected_store: Value = serde_json::from_slice(&upload_evidence_bytes).unwrap();
        unexpected_store["stored_chunks_response_hex"] =
            Value::String(hex(&unexpected_stored_response));
        unexpected_store["stored_chunks_response_sha256"] =
            Value::String(hex(&Sha256::digest(&unexpected_stored_response)));
        assert!(validate_production_upgrade_upload_evidence(
            &submission,
            &serde_json::to_vec(&unexpected_store).unwrap()
        )
        .is_err());
        let mut forged_upload_response: Value =
            serde_json::from_slice(&upload_evidence_bytes).unwrap();
        let forged_response = Encode!(&ManagementChunkHash {
            hash: vec![0x42; 32],
        })
        .unwrap();
        forged_upload_response["chunks"][0]["response_hex"] = Value::String(hex(&forged_response));
        forged_upload_response["chunks"][0]["response_sha256"] =
            Value::String(hex(&Sha256::digest(&forged_response)));
        assert!(validate_production_upgrade_upload_evidence(
            &submission,
            &serde_json::to_vec(&forged_upload_response).unwrap()
        )
        .is_err());
        let now_ns = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        )
        .unwrap();
        assert!(production_upgrade_request_has_time(now_ns + 16 * 1_000_000_000).is_ok());
        assert!(production_upgrade_request_has_time(now_ns + 14 * 1_000_000_000).is_err());
        let executed_at = production_upgrade.executed_at_unix;
        let executed_at_ns = executed_at * 1_000_000_000;
        assert!(!production_upgrade_ingress_window_valid(
            executed_at,
            executed_at_ns
        ));
        assert!(production_upgrade_ingress_window_valid(
            executed_at,
            executed_at_ns + 1
        ));
        assert!(production_upgrade_ingress_window_valid(
            executed_at,
            executed_at_ns + 5 * 60 * 1_000_000_000
        ));
        assert!(!production_upgrade_ingress_window_valid(
            executed_at,
            executed_at_ns + 5 * 60 * 1_000_000_000 + 1
        ));
        assert!(!production_upgrade_ingress_window_valid(u64::MAX, u64::MAX));
        production_upgrade.submission_json_hex = hex(&submission_bytes);
        production_upgrade.submission_json_sha256 = hex(&Sha256::digest(&submission_bytes));
        production_upgrade.chunk_upload_evidence_json_hex = hex(&upload_evidence_bytes);
        production_upgrade.chunk_upload_evidence_json_sha256 =
            hex(&Sha256::digest(&upload_evidence_bytes));
        production_upgrade.request_id = submission.request_id.clone();
        let response_stdout = format!(
            "request_id={}\nresponse_hex=\nsender_principal={}\nwasm_sha256={}\n",
            submission.request_id, submission.sender_principal, submission.wasm_sha256
        );
        production_upgrade.response_stdout_hex = hex(response_stdout.as_bytes());
        production_upgrade.response_stdout_sha256 =
            hex(&Sha256::digest(response_stdout.as_bytes()));
        let production_upgrade_bytes = serde_json::to_vec(&production_upgrade).unwrap();
        fs::write(
            root.join("production-canister-upgrade-receipt.json"),
            &production_upgrade_bytes,
        )
        .unwrap();
        let transition = PostGateAPolicyTransition {
            schema_version: 3,
            reason: "activate-before-production-measurements".into(),
            observed_at_unix: manifest_created,
            gate_a_manifest_sha256: receipt.gate_a_manifest_sha256.clone(),
            gate_a_receipt_sha256: hex(&Sha256::digest(&receipt_bytes)),
            from_source_revision: receipt.source_revision.clone(),
            from_source_tree_sha256: receipt.source_tree_sha256.clone(),
            upgrade_source_revision: "a".repeat(40),
            upgrade_source_tree_sha256: "2".repeat(64),
            to_source_revision: "a".repeat(40),
            to_source_tree_sha256: "2".repeat(64),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            bridge_contract: profile.bridge_contract.clone(),
            bsns_contract: profile.bsns_contract.clone(),
            timelock_contract: profile.timelock.address.clone(),
            from_bridge_canister_wasm_sha256: gate_a_profile.bridge_canister_wasm_sha256.clone(),
            to_bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            production_canister_upgrade_receipt_sha256: hex(&Sha256::digest(
                &production_upgrade_bytes,
            )),
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
            path: "gate-a-profile.json".into(),
            sha256: hex(&Sha256::digest(gate_a_profile_bytes)),
        });
        artifacts.push(ArtifactDigest {
            path: "production-canister-upgrade-receipt.json".into(),
            sha256: hex(&Sha256::digest(&production_upgrade_bytes)),
        });
        artifacts.push(ArtifactDigest {
            path: "initial-operational-parameters.json".into(),
            sha256: hex(&Sha256::digest(&initial_parameters_bytes)),
        });
        artifacts.push(ArtifactDigest {
            path: "post-gate-a-policy-transition.json".into(),
            sha256: hex(&Sha256::digest(&transition_bytes)),
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
        assert_eq!(
            receipt.gate_a_profile_sha256,
            hex(&canonical_sha256(&gate_a_profile).unwrap())
        );
        let mut expected_post_deploy_profile = gate_a_profile.clone();
        expected_post_deploy_profile.deployment_block = profile.deployment_block;
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
        let baseline_manifest_bytes = fs::read(root.join("release-manifest.json")).unwrap();
        let chain_bytes = serde_json::to_vec(&ProductionCanisterUpgradeChain {
            schema_version: 1,
            kind: "production-controller-bootstrap-upgrade-chain".into(),
            entries: vec![ProductionCanisterUpgradeChainEntry {
                sequence: 0,
                previous_receipt_sha256: None,
                receipt_sha256: hex(&Sha256::digest(&production_upgrade_bytes)),
                receipt_json_hex: hex(&production_upgrade_bytes),
            }],
        })
        .unwrap();
        fs::write(
            root.join("production-canister-upgrade-receipt.json"),
            &chain_bytes,
        )
        .unwrap();
        let mut chain_transition: PostGateAPolicyTransition =
            serde_json::from_slice(&transition_bytes).unwrap();
        chain_transition.production_canister_upgrade_receipt_sha256 =
            hex(&Sha256::digest(&chain_bytes));
        let chain_transition_bytes = serde_json::to_vec(&chain_transition).unwrap();
        fs::write(
            root.join("post-gate-a-policy-transition.json"),
            &chain_transition_bytes,
        )
        .unwrap();
        let mut chain_manifest: ReleaseManifest =
            serde_json::from_slice(&baseline_manifest_bytes).unwrap();
        for artifact in &mut chain_manifest.artifacts {
            if artifact.path == "production-canister-upgrade-receipt.json" {
                artifact.sha256 = hex(&Sha256::digest(&chain_bytes));
            } else if artifact.path == "post-gate-a-policy-transition.json" {
                artifact.sha256 = hex(&Sha256::digest(&chain_transition_bytes));
            }
        }
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&chain_manifest).unwrap(),
        )
        .unwrap();
        assert!(validate_bundle(&root, true).is_ok());
        let mut broken_chain: ProductionCanisterUpgradeChain =
            serde_json::from_slice(&chain_bytes).unwrap();
        broken_chain.entries[0].previous_receipt_sha256 = Some("9".repeat(64));
        assert!(
            production_upgrade_chain_receipts(&serde_json::to_vec(&broken_chain).unwrap()).is_err()
        );
        assert!(
            validate_production_upgrade_receipt_size(MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES).is_ok()
        );
        assert!(
            validate_production_upgrade_receipt_size(MAX_PRODUCTION_UPGRADE_RECEIPT_BYTES + 1)
                .is_err()
        );
        fs::write(
            root.join("production-canister-upgrade-receipt.json"),
            &production_upgrade_bytes,
        )
        .unwrap();
        fs::write(
            root.join("post-gate-a-policy-transition.json"),
            &transition_bytes,
        )
        .unwrap();
        fs::write(root.join("release-manifest.json"), &baseline_manifest_bytes).unwrap();
        for field in [
            "deposit_and_throughput_limits",
            "mint_throughput_limit",
            "mint_window_duration_seconds",
        ] {
            let mut drifted_profile: Profile =
                serde_json::from_slice(&final_profile_bytes).unwrap();
            match field {
                "deposit_and_throughput_limits" => {
                    drifted_profile.parameters.per_deposit_limit += 1;
                    drifted_profile.parameters.mint_throughput_limit += 1;
                }
                "mint_throughput_limit" => drifted_profile.parameters.mint_throughput_limit += 1,
                "mint_window_duration_seconds" => {
                    drifted_profile.parameters.mint_window_duration_seconds += 1
                }
                _ => unreachable!(),
            }
            let drifted_profile_bytes = serde_json::to_vec(&drifted_profile).unwrap();
            fs::write(root.join("profile.json"), &drifted_profile_bytes).unwrap();
            let mut drifted_initial: InitialOperationalParameters =
                serde_json::from_slice(&initial_parameters_bytes).unwrap();
            drifted_initial.profile_sha256 = hex(&canonical_sha256(&drifted_profile).unwrap());
            let drifted_initial_bytes = serde_json::to_vec(&drifted_initial).unwrap();
            fs::write(
                root.join("initial-operational-parameters.json"),
                &drifted_initial_bytes,
            )
            .unwrap();
            let mut drifted_manifest: ReleaseManifest =
                serde_json::from_slice(&baseline_manifest_bytes).unwrap();
            for artifact in &mut drifted_manifest.artifacts {
                if artifact.path == "profile.json" {
                    artifact.sha256 = hex(&Sha256::digest(&drifted_profile_bytes));
                } else if artifact.path == "initial-operational-parameters.json" {
                    artifact.sha256 = hex(&Sha256::digest(&drifted_initial_bytes));
                }
            }
            fs::write(
                root.join("release-manifest.json"),
                serde_json::to_vec(&drifted_manifest).unwrap(),
            )
            .unwrap();
            let error = match validate_bundle(&root, true) {
                Ok(_) => panic!("Gate B accepted drift of fixed parameter {field}"),
                Err(error) => error,
            };
            assert!(
                error.contains("fields outside the reviewed operational config"),
                "unexpected {field} drift error: {error}"
            );
        }
        fs::write(root.join("profile.json"), &final_profile_bytes).unwrap();
        fs::write(
            root.join("initial-operational-parameters.json"),
            &initial_parameters_bytes,
        )
        .unwrap();
        fs::write(root.join("release-manifest.json"), &baseline_manifest_bytes).unwrap();
        let mut drifted_transition: PostGateAPolicyTransition =
            serde_json::from_slice(&transition_bytes).unwrap();
        drifted_transition.upgrade_source_revision = "f".repeat(40);
        let drifted_transition_bytes = serde_json::to_vec(&drifted_transition).unwrap();
        fs::write(
            root.join("post-gate-a-policy-transition.json"),
            &drifted_transition_bytes,
        )
        .unwrap();
        let mut drifted_manifest: ReleaseManifest =
            serde_json::from_slice(&baseline_manifest_bytes).unwrap();
        drifted_manifest
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "post-gate-a-policy-transition.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&drifted_transition_bytes));
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&drifted_manifest).unwrap(),
        )
        .unwrap();
        let transition_error = match validate_bundle(&root, true) {
            Ok(_) => panic!("Gate B accepted an upgrade source outside its receipt"),
            Err(error) => error,
        };
        assert!(
            transition_error.contains("policy transition identity binding"),
            "unexpected upgrade-source drift error: {transition_error}"
        );
        fs::write(
            root.join("post-gate-a-policy-transition.json"),
            &transition_bytes,
        )
        .unwrap();
        fs::write(root.join("release-manifest.json"), &baseline_manifest_bytes).unwrap();
        let after_gate_b_freshness = now + MAX_EVIDENCE_AGE_SECS + 1;
        assert!(
            validate_bundle_with_freshness_at(&root, true, true, after_gate_b_freshness,).is_err()
        );
        assert!(
            validate_bundle_with_freshness_at(&root, true, false, after_gate_b_freshness,).is_ok()
        );
        let valid_manifest_bytes = fs::read(root.join("release-manifest.json")).unwrap();
        let valid_production_upgrade_bytes =
            fs::read(root.join("production-canister-upgrade-receipt.json")).unwrap();
        let mut reinstall_upgrade: Value =
            serde_json::from_slice(&valid_production_upgrade_bytes).unwrap();
        reinstall_upgrade["install_mode"] = Value::String("reinstall".into());
        let reinstall_upgrade_bytes = serde_json::to_vec(&reinstall_upgrade).unwrap();
        fs::write(
            root.join("production-canister-upgrade-receipt.json"),
            &reinstall_upgrade_bytes,
        )
        .unwrap();
        let mut rehashed_manifest: ReleaseManifest =
            serde_json::from_slice(&valid_manifest_bytes).unwrap();
        rehashed_manifest
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "production-canister-upgrade-receipt.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&reinstall_upgrade_bytes));
        let mut reinstall_transition: PostGateAPolicyTransition =
            serde_json::from_slice(&transition_bytes).unwrap();
        reinstall_transition.production_canister_upgrade_receipt_sha256 =
            hex(&Sha256::digest(&reinstall_upgrade_bytes));
        let reinstall_transition_bytes = serde_json::to_vec(&reinstall_transition).unwrap();
        fs::write(
            root.join("post-gate-a-policy-transition.json"),
            &reinstall_transition_bytes,
        )
        .unwrap();
        rehashed_manifest
            .artifacts
            .iter_mut()
            .find(|artifact| artifact.path == "post-gate-a-policy-transition.json")
            .unwrap()
            .sha256 = hex(&Sha256::digest(&reinstall_transition_bytes));
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&rehashed_manifest).unwrap(),
        )
        .unwrap();
        let reinstall_error = match validate_bundle(&root, true) {
            Ok(_) => panic!("Gate B accepted reinstall evidence"),
            Err(error) => error,
        };
        assert!(
            reinstall_error.contains("production upgrade management metadata is incomplete"),
            "{reinstall_error}"
        );
        fs::write(
            root.join("production-canister-upgrade-receipt.json"),
            valid_production_upgrade_bytes,
        )
        .unwrap();
        fs::write(
            root.join("post-gate-a-policy-transition.json"),
            transition_bytes,
        )
        .unwrap();
        fs::write(root.join("release-manifest.json"), valid_manifest_bytes).unwrap();
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
        let root_controller = Principal::from_text(KINIC_ROOT).unwrap();
        assert!(validate_post_handover_management_snapshot(
            &bundle.profile,
            &[root_controller],
            &module_hash,
        )
        .is_ok());
        assert!(validate_post_handover_management_snapshot(
            &bundle.profile,
            &[installer],
            &module_hash,
        )
        .is_err());
        assert!(validate_post_handover_management_snapshot(
            &bundle.profile,
            &[root_controller],
            &[0; 32],
        )
        .is_err());

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
            governance_operation_id: governance_operation_id.to_string(),
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
        let mut legacy_manifest: ReleaseManifest =
            serde_json::from_slice(&baseline_manifest_bytes).unwrap();
        assert_eq!(legacy_manifest.artifacts.len(), 13);
        for legacy_path in ["rpc-e2e.json", "monitor-drill.json"] {
            let legacy_bytes = fs::read(root.join(legacy_path)).unwrap();
            legacy_manifest.artifacts.push(ArtifactDigest {
                path: legacy_path.into(),
                sha256: hex(&Sha256::digest(legacy_bytes)),
            });
        }
        assert_eq!(legacy_manifest.artifacts.len(), 15);
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&legacy_manifest).unwrap(),
        )
        .unwrap();
        assert!(validate_bundle(&root, true)
            .err()
            .unwrap()
            .contains("manifest must contain each required evidence artifact exactly once"));
        fs::remove_dir_all(root).unwrap();
    }
}
