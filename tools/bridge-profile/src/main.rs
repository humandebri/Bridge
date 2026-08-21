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
const GATE_A_ARTIFACTS: [&str; 6] = [
    "profile.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "bsns-creation.bin",
    "bsns-runtime.bin",
    "bsns-runtime-layout.json",
];
const GATE_B_ARTIFACTS: [&str; 16] = [
    "profile.json",
    "rpc-e2e.json",
    "controller-handover.json",
    "sns-upgrade.json",
    "monitor-drill.json",
    "keeper-drill.json",
    "monitoring-receipt.json",
    "fee-cycles-measurements.json",
    "provider-independence.json",
    "ui-assets.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "bsns-creation.bin",
    "bsns-runtime.bin",
    "bsns-runtime-layout.json",
    "gate-a-receipt.json",
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeeCyclesMeasurementsReceipt {
    schema_version: u16,
    sample_count: u16,
    observation_days: u16,
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
struct Evidence {
    schema_version: u8,
    environment: String,
    ledger_fee: u128,
    governance_gas_used: Vec<u128>,
    fee_observation_start_unix: u64,
    fee_observation_end_unix: u64,
    base_fee_per_gas: Vec<u128>,
    priority_fee_per_gas: Vec<u128>,
    l1_fee_upper_bound_wei: Vec<u128>,
    settlement_cycles: Vec<u128>,
    baseline_cycles_per_day: u128,
    expected_daily_settlements: u128,
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

#[derive(Deserialize, Serialize)]
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
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LivePublicConfig {
    base_chain_id: u64,
    bridge_contract: String,
    timelock_contract: String,
    deployment_instance_id: String,
    minimum_withdrawal_id: String,
    ledger_canister_id: String,
    index_canister_id: String,
    schema_version: u16,
    expected_bridge_signer: String,
    governance_operator: String,
    evm_rpc_canister_id: String,
    rpc_provider_urls_sha256: String,
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
    confirmation_relayer_principal: String,
    pause_principal: String,
    fee_recipient: LiveFeeRecipient,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LiveFeeRecipient {
    owner: String,
    subaccount_hex: String,
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
struct PublicFeeRecipientView {
    owner: Principal,
    subaccount: Vec<u8>,
}

#[derive(CandidType, Deserialize)]
struct PublicConfigView {
    base_chain_id: u64,
    bridge_contract: Vec<u8>,
    expected_bridge_runtime_sha256: Vec<u8>,
    timelock_contract: Vec<u8>,
    deployment_instance_id: Vec<u8>,
    minimum_withdrawal_id: Vec<u8>,
    ledger_canister_id: Principal,
    ledger_fee: u128,
    index_canister_id: Principal,
    schema_version: u16,
    expected_bridge_signer: Vec<u8>,
    governance_operator: Vec<u8>,
    evm_rpc_canister_id: Principal,
    rpc_provider_urls_sha256: Vec<u8>,
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
    confirmation_relayer_principal: Principal,
    pause_principal: Principal,
    fee_recipient: PublicFeeRecipientView,
}

#[derive(CandidType, Deserialize)]
struct ReserveStatusView {
    sufficient: bool,
}

#[derive(CandidType, Deserialize)]
struct BridgeStatusLiveView {
    reserve: ReserveStatusView,
    deposits_paused: bool,
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
    if evidence.schema_version != 2 {
        return Err("measurement evidence must use schema v2".into());
    }
    if evidence.governance_gas_used.len() < 10
        || evidence.settlement_cycles.len() < 10
        || evidence.base_fee_per_gas.len() < 10
        || evidence.priority_fee_per_gas.len() < 10
        || evidence.l1_fee_upper_bound_wei.len() < 10
    {
        return Err(
            "governance gas, fee, and cycle evidence must contain at least 10 samples each".into(),
        );
    }
    if evidence.ledger_fee == 0
        || evidence.baseline_cycles_per_day == 0
        || evidence.expected_daily_settlements == 0
        || evidence.governance_gas_used.contains(&0)
        || evidence.settlement_cycles.contains(&0)
        || evidence.base_fee_per_gas.is_empty()
        || evidence.base_fee_per_gas.len() != evidence.priority_fee_per_gas.len()
        || evidence.base_fee_per_gas.len() != evidence.l1_fee_upper_bound_wei.len()
        || evidence.base_fee_per_gas.contains(&0)
        || evidence.priority_fee_per_gas.contains(&0)
        || evidence.l1_fee_upper_bound_wei.contains(&0)
    {
        return Err("measurement evidence values must be positive and fee samples aligned".into());
    }
    let minimum_days = match evidence.environment.as_str() {
        "base-sepolia" | "mainnet-candidate" => 7,
        _ => return Err("unsupported evidence environment".into()),
    };
    if evidence
        .fee_observation_end_unix
        .checked_sub(evidence.fee_observation_start_unix)
        .is_none_or(|duration| duration < minimum_days * 24 * 60 * 60)
    {
        return Err(format!(
            "Base fee evidence must cover at least {minimum_days} days"
        ));
    }
    let gas_max = *evidence
        .governance_gas_used
        .iter()
        .max()
        .ok_or("missing gas samples")?;
    let gas_limit_ceiling = checked_ratio_ceil(gas_max, 130, 100)?
        .checked_add(999)
        .map(|value| value / 1_000 * 1_000)
        .ok_or("gas limit overflow")?;
    let max_priority_fee_per_gas_ceiling = percentile(&evidence.priority_fee_per_gas, 95, 100)?
        .checked_mul(4)
        .ok_or("priority fee cap overflow")?;
    let max_fee_per_gas_ceiling = percentile(&evidence.base_fee_per_gas, 99, 100)?
        .checked_mul(20)
        .ok_or("max fee cap overflow")?;
    let l1_fee_per_transaction_ceiling_wei = percentile(&evidence.l1_fee_upper_bound_wei, 99, 100)?
        .checked_mul(10)
        .ok_or("L1 fee cap overflow")?;
    let settlement_cycles_max = *evidence
        .settlement_cycles
        .iter()
        .max()
        .ok_or("missing cycle samples")?;
    let settlement_cycle_ceiling = checked_ratio_ceil(settlement_cycles_max, 150, 100)?;
    let cycles_floor = evidence
        .expected_daily_settlements
        .checked_mul(settlement_cycles_max)
        .and_then(|settlement_daily| settlement_daily.checked_add(evidence.baseline_cycles_per_day))
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
    let mut hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(signature.as_bytes());
    keccak.finalize(&mut hash);
    format!("0x{}", hex(&hash[..4]))
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
        || drill.source_revision != manifest.source_revision
        || !drill
            .source_tree_sha256
            .eq_ignore_ascii_case(&manifest.source_tree_sha256)
        || drill.ic_network != "ic"
        || drill.base_chain_id != 84_532
        || !principal(&drill.bridge_canister_id)
        || !evm_address(&drill.bridge_contract)
        || !evm_address(&drill.timelock_contract)
        || !drill
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&profile.bridge_canister_wasm_sha256)
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
    if profile.schema_version != 4 {
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
            .eq_ignore_ascii_case(&profile.governance_operator)
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
            profile.expected_bridge_signer, profile.governance_operator, profile.timelock.address,
            profile.timelock.runtime_code_hash,
            profile.parameters.per_deposit_limit.to_string(),
            profile.parameters.mint_throughput_limit.to_string(),
            profile.parameters.mint_window_duration_seconds.to_string(),
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

fn validate_bundle(root: &Path, gate_b: bool) -> Result<ValidatedBundle, String> {
    if root.join("proof-attestation.json").exists() {
        return Err(
            "obsolete self-asserted proof attestation is forbidden; release drivers rerun proofs"
                .into(),
        );
    }
    let manifest: ReleaseManifest = read_json(&root.join("release-manifest.json"))?;
    if manifest.schema_version != 3
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
        let measurements: FeeCyclesMeasurementsReceipt =
            read_json(&root.join("fee-cycles-measurements.json"))?;
        if measurements.schema_version != 2
            || measurements.sample_count < 10
            || measurements.observation_days < 7
        {
            return Err("Gate B requires at least 7 days and 10 fee/cycles samples".into());
        }
        if profile.deployment_block == 0 {
            return Err("Gate B profile must bind the actual Bridge deployment block".into());
        }
    } else if profile.deployment_block != 0 {
        return Err("Gate A profile must leave deployment_block unbound until deployment".into());
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
        let profile_hash = artifacts["profile.json"].sha256.as_str();
        let mut gate_a_profile = profile.clone();
        gate_a_profile.deployment_block = 0;
        let expected_gate_a_profile_hash = hex(&canonical_sha256(&gate_a_profile)?);
        if receipt.schema_version != 1
            || !receipt.gate_a_manifest_sha256.eq_ignore_ascii_case(
                manifest
                    .parent_gate_a_manifest_sha256
                    .as_deref()
                    .unwrap_or_default(),
            )
            || receipt.release_id != manifest.release_id
            || receipt.source_revision != manifest.source_revision
            || !receipt
                .source_tree_sha256
                .eq_ignore_ascii_case(&manifest.source_tree_sha256)
            || !receipt
                .post_deploy_profile_sha256
                .eq_ignore_ascii_case(profile_hash)
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
    }
    if profile.test_assets_only != manifest.test_only {
        return Err("manifest/profile test-only mismatch".into());
    }
    if gate_b {
        let drill: MonitorDrill = read_json(&root.join("monitor-drill.json"))?;
        validate_monitor_drill(&drill, &manifest, &profile, now)?;
        validate_keeper_drill(root, &manifest, &profile, now)?;
        validate_provider_independence_receipt(root, &manifest, &profile, now)?;
        validate_ui_assets_receipt(root, &manifest)?;
        validate_plan006_evidence(root, &manifest, &profile, now)?;
    }
    let hash = unsigned_manifest_hash(&manifest)?;
    Ok(ValidatedBundle {
        root: root.to_path_buf(),
        manifest,
        profile,
        manifest_sha256: hex(&hash),
    })
}

fn validate_live_public_config(
    observed: &LivePublicConfig,
    profile: &Profile,
    rpc_url_hash: &str,
) -> Result<(), String> {
    let p = &profile.parameters;
    let r = &profile.rate_limits;
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
        || !observed
            .governance_operator
            .eq_ignore_ascii_case(&profile.governance_operator)
        || observed.evm_rpc_canister_id != profile.evm_rpc_canister_id
        || !observed
            .rpc_provider_urls_sha256
            .eq_ignore_ascii_case(rpc_url_hash)
        || observed.deposit_rate_limit_window_seconds != r.deposit_window_seconds
        || observed.deposit_rate_limit_global != r.deposit_global
        || observed.deposit_rate_limit_per_principal != r.deposit_per_principal
        || observed.notification_rate_limit_window_seconds != r.notification_window_seconds
        || observed.notification_rate_limit_global != r.notification_global
        || observed.notification_ingestion_rate_limit_global != r.notification_ingestion_global
        || observed.settlement_rate_limit_window_seconds != r.settlement_window_seconds
        || observed.settlement_rate_limit_global != r.settlement_global
        || observed.settlement_rate_limit_per_principal != r.settlement_per_principal
        || observed.settlement_rate_limit_per_record != r.settlement_per_record
        || observed.settlement_retry_interval_seconds != r.settlement_retry_interval_seconds
        || observed.governance_evm_fee != p.governance_evm_fee()
        || observed.governance_replacement != profile.governance_replacement
        || observed.cycles_floor != p.cycles_floor
        || observed.settlement_cycle_ceiling != p.settlement_cycle_ceiling
        || observed.governance_principal != profile.governance_principal
        || observed.confirmation_relayer_principal != profile.confirmation_relayer_principal
        || observed.pause_principal != profile.pause_principal
        || observed.fee_recipient.owner != profile.fee_recipient
        || !observed.fee_recipient.subaccount_hex.is_empty()
    {
        return Err("live Canister PublicConfig does not exactly match the profile".into());
    }
    Ok(())
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
            .query(&bridge, "get_public_config")
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
    let public = Decode!(&public_raw, PublicConfigView).map_err(|error| error.to_string())?;
    let status = Decode!(&status_raw, BridgeStatusLiveView).map_err(|error| error.to_string())?;
    let observed = LivePublicConfig {
        base_chain_id: public.base_chain_id,
        bridge_contract: format!("0x{}", hex(&public.bridge_contract)),
        timelock_contract: format!("0x{}", hex(&public.timelock_contract)),
        deployment_instance_id: format!("0x{}", hex(&public.deployment_instance_id)),
        minimum_withdrawal_id: format!("0x{}", hex(&public.minimum_withdrawal_id)),
        ledger_canister_id: public.ledger_canister_id.to_text(),
        index_canister_id: public.index_canister_id.to_text(),
        schema_version: public.schema_version,
        expected_bridge_signer: format!("0x{}", hex(&public.expected_bridge_signer)),
        governance_operator: format!("0x{}", hex(&public.governance_operator)),
        evm_rpc_canister_id: public.evm_rpc_canister_id.to_text(),
        rpc_provider_urls_sha256: hex(&public.rpc_provider_urls_sha256),
        deposit_rate_limit_window_seconds: public.deposit_rate_limit_window_seconds,
        deposit_rate_limit_global: public.deposit_rate_limit_global,
        deposit_rate_limit_per_principal: public.deposit_rate_limit_per_principal,
        notification_rate_limit_window_seconds: public.notification_rate_limit_window_seconds,
        notification_rate_limit_global: public.notification_rate_limit_global,
        notification_ingestion_rate_limit_global: public.notification_ingestion_rate_limit_global,
        settlement_rate_limit_window_seconds: public.settlement_rate_limit_window_seconds,
        settlement_rate_limit_global: public.settlement_rate_limit_global,
        settlement_rate_limit_per_principal: public.settlement_rate_limit_per_principal,
        settlement_rate_limit_per_record: public.settlement_rate_limit_per_record,
        settlement_retry_interval_seconds: public.settlement_retry_interval_seconds,
        governance_evm_fee: public.governance_evm_fee,
        governance_replacement: public.governance_replacement,
        cycles_floor: public.cycles_floor,
        settlement_cycle_ceiling: public.settlement_cycle_ceiling,
        governance_principal: public.governance_principal.to_text(),
        confirmation_relayer_principal: public.confirmation_relayer_principal.to_text(),
        pause_principal: public.pause_principal.to_text(),
        fee_recipient: LiveFeeRecipient {
            owner: public.fee_recipient.owner.to_text(),
            subaccount_hex: hex(&public.fee_recipient.subaccount),
        },
    };
    validate_live_public_config(&observed, &bundle.profile, &rpc_url_hash)?;
    if public.expected_bridge_runtime_sha256
        != decode_hex(&bundle.profile.bridge_runtime_bytecode_sha256)?
        || public.ledger_fee != bundle.profile.parameters.ledger_fee
        || public.ledger_fee > bundle.profile.parameters.service_fee
        || status.deposits_paused != expected_deposits_paused
        || !status.reserve.sufficient
    {
        return Err("authenticated live Canister state does not satisfy Gate B".into());
    }
    validate_rpc_rehearsal(bundle)?;
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
    let now = now_unix()?;
    validate_activation_attestation_time(
        attestation.observed_at_ns,
        bundle.manifest.created_at_unix,
        now,
    )?;
    let expected_signer = decode_address(&bundle.profile.expected_bridge_signer)?;
    let expected_runtime = decode_hex(&bundle.profile.bridge_runtime_bytecode_sha256)?;
    let expected_timelock = decode_address(&bundle.profile.timelock.address)?;
    let expected_operator = decode_address(&bundle.profile.governance_operator)?;
    if attestation.chain_id != bundle.profile.chain_id
        || attestation.finalized_block_number == 0
        || attestation.finalized_block_hash.len() != 32
        || attestation.bridge_signer != expected_signer
        || attestation.bridge_runtime_sha256 != expected_runtime
        || !attestation.deposits_paused
        || !attestation.withdrawals_paused
        || attestation.bridge_timelock != expected_timelock
        || attestation.runtime_administrator != expected_operator
        || attestation.timelock_admin != expected_timelock
        || attestation.timelock_proposer != expected_operator
        || attestation.timelock_canceller != expected_operator
        || attestation.timelock_executor != expected_operator
        || attestation.timelock_runtime_code_hash
            != decode_hex(&bundle.profile.timelock.runtime_code_hash)?
        || attestation.bridge_approved_timelock_runtime_code_hash
            != decode_hex(&bundle.profile.timelock.runtime_code_hash)?
        || attestation.timelock_minimum_delay_seconds
            != bundle.profile.timelock.minimum_delay_seconds
        || attestation.bsns_address != decode_address(&bundle.profile.bsns_contract)?
        || attestation.bsns_runtime_sha256
            != decode_hex(&bundle.profile.bsns_runtime_bytecode_sha256)?
        || attestation.bsns_name != "KINIC"
        || attestation.bsns_symbol != "KINIC"
        || attestation.bsns_decimals != bundle.profile.decimals
        || attestation.bsns_bridge != decode_address(&bundle.profile.bridge_contract)?
        || attestation.base_service_fee != bundle.profile.parameters.service_fee
    {
        return Err("authenticated activation attestation does not match the release".into());
    }
    Ok(())
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

fn verify_live(bundle: &ValidatedBundle, expected_deposits_paused: bool) -> Result<(), String> {
    verify_live_inputs(bundle, expected_deposits_paused)?;
    verify_activation_attestation_authenticity(bundle)?;
    verify_monitor_drill_authenticity(bundle)?;
    verify_keeper_authenticity(bundle)?;
    verify_sns_upgrade_authenticity(bundle)?;
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
        || receipt.prior_schedule_receipt_sha256.is_some()
    {
        return Err("prior schedule receipt is malformed or not bound to this release".into());
    }
    Ok(())
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

    let governance = Principal::from_text(KINIC_GOVERNANCE).map_err(|error| error.to_string())?;
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let proposal_arg = Encode!(&GetProposalRequest {
        proposal_id: Some(ProposalId {
            id: submission.proposal_id,
        }),
    })
    .map_err(|error| error.to_string())?;
    let empty_arg = canonical_payload.to_vec();
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (proposal_raw, registry_raw, activation_raw, controllers, module_hash) =
        async_runtime()?.block_on(async {
            let proposal = agent
                .query(&governance, "get_proposal")
                .with_arg(proposal_arg)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let registry = agent
                .query(&governance, "list_nervous_system_functions")
                .with_arg(empty_arg.clone())
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let activation = agent
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
            Ok::<_, String>((proposal, registry, activation, controllers, module_hash))
        })?;

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
        || controllers != [Principal::from_text(KINIC_ROOT).map_err(|error| error.to_string())?]
        || !hex(&module_hash).eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
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
        || confirmation.governance_operation_id == 0
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

    let governance = Principal::from_text(KINIC_GOVERNANCE).map_err(|error| error.to_string())?;
    let bridge = Principal::from_text(&bundle.profile.bridge_canister_id)
        .map_err(|error| error.to_string())?;
    let proposal_arg = Encode!(&GetProposalRequest {
        proposal_id: Some(ProposalId {
            id: receipt.proposal_id,
        }),
    })
    .map_err(|error| error.to_string())?;
    let agent = mainnet_agent(&bundle.profile.ic_host, false)?;
    let (proposal_raw, registry_raw, activation_raw, controllers, module_hash) =
        async_runtime()?.block_on(async {
            let proposal = agent
                .query(&governance, "get_proposal")
                .with_arg(proposal_arg)
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let registry = agent
                .query(&governance, "list_nervous_system_functions")
                .with_arg(canonical_payload.to_vec())
                .call_with_verification()
                .await
                .map_err(|error| error.to_string())?;
            let activation = agent
                .query(&bridge, "get_activation_status")
                .with_arg(canonical_payload.to_vec())
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
            Ok::<_, String>((proposal, registry, activation, controllers, module_hash))
        })?;

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
        || controllers != [Principal::from_text(KINIC_ROOT).map_err(|error| error.to_string())?]
        || !hex(&module_hash).eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
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
        || confirmation.governance_operation_id == 0
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
        _ => return Err("usage: bridge-profile <derive|validate|validate-test> <json-file> | render-release-inputs <profile.json> <output-dir> | render-test-inputs <profile.json> <output-dir> | render-bundle-inputs <bundle-dir> <output-dir> | validate-bundle --offline <bundle-dir> | validate-bundle --offline --gate-b <bundle-dir> | verify-live <bundle-dir> | verify-schedule-receipt-live <bundle-dir> <schedule-receipt.json> | verify-activation <schedule|execute> <bundle-dir> <submission.json> <prior-schedule-receipt.json|-> <receipt.json>".into()),
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
            schema_version: 4,
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
                canceller: address(3),
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

    fn live_public_config(profile: &Profile) -> LivePublicConfig {
        let rpc_url_hash = hex(&canonical_sha256(
            &profile
                .rpc_providers
                .iter()
                .map(|provider| provider.url.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap());
        LivePublicConfig {
            base_chain_id: profile.chain_id,
            bridge_contract: profile.bridge_contract.clone(),
            timelock_contract: profile.timelock.address.clone(),
            deployment_instance_id: profile.deployment_instance_id.clone(),
            minimum_withdrawal_id: profile.minimum_withdrawal_id.clone(),
            ledger_canister_id: profile.ledger_canister_id.clone(),
            index_canister_id: profile.index_canister_id.clone(),
            schema_version: profile.canister_schema_version,
            expected_bridge_signer: profile.expected_bridge_signer.clone(),
            governance_operator: profile.governance_operator.clone(),
            evm_rpc_canister_id: profile.evm_rpc_canister_id.clone(),
            rpc_provider_urls_sha256: rpc_url_hash,
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
            governance_principal: profile.governance_principal.clone(),
            confirmation_relayer_principal: profile.confirmation_relayer_principal.clone(),
            pause_principal: profile.pause_principal.clone(),
            fee_recipient: LiveFeeRecipient {
                owner: profile.fee_recipient.clone(),
                subaccount_hex: String::new(),
            },
        }
    }

    #[test]
    fn live_public_config_must_exactly_match_the_profile() {
        let profile = valid_profile();
        let rpc_url_hash = hex(&canonical_sha256(
            &profile
                .rpc_providers
                .iter()
                .map(|provider| provider.url.clone())
                .collect::<Vec<_>>(),
        )
        .unwrap());
        let mut observed = live_public_config(&profile);
        assert!(validate_live_public_config(&observed, &profile, &rpc_url_hash).is_ok());
        observed.notification_rate_limit_global -= 1;
        assert!(validate_live_public_config(&observed, &profile, &rpc_url_hash).is_err());
    }

    #[test]
    fn conservative_derivation_uses_exact_boundaries() {
        let evidence = Evidence {
            schema_version: 2,
            environment: "mainnet-candidate".into(),
            ledger_fee: 100_000,
            governance_gas_used: vec![30_001; 10],
            fee_observation_start_unix: 1_700_000_000,
            fee_observation_end_unix: 1_700_000_000 + 7 * 24 * 60 * 60,
            base_fee_per_gas: vec![10; 10],
            priority_fee_per_gas: vec![2; 10],
            l1_fee_upper_bound_wei: vec![5; 10],
            settlement_cycles: vec![1_000; 10],
            baseline_cycles_per_day: 10_000,
            expected_daily_settlements: 4,
        };
        let result = derive(&evidence).unwrap();
        assert_eq!(result.gas_limit_ceiling, 40_000);
        assert_eq!(result.max_fee_per_gas_ceiling, 200);
        assert_eq!(result.max_priority_fee_per_gas_ceiling, 8);
        assert_eq!(result.l1_fee_per_transaction_ceiling_wei, 50);
        assert_eq!(result.settlement_cycle_ceiling, 1_500);
        assert_eq!(result.cycles_floor, 840_000);
    }

    #[test]
    fn derivation_rejects_incomplete_stale_and_obsolete_measurement_shapes() {
        let mut evidence = Evidence {
            schema_version: 2,
            environment: "mainnet-candidate".into(),
            ledger_fee: 100_000,
            governance_gas_used: vec![10_000; 10],
            fee_observation_start_unix: 1_700_000_000,
            fee_observation_end_unix: 1_700_000_000 + 7 * 24 * 60 * 60,
            base_fee_per_gas: vec![10; 10],
            priority_fee_per_gas: vec![2; 10],
            l1_fee_upper_bound_wei: vec![5; 10],
            settlement_cycles: vec![1_001; 10],
            baseline_cycles_per_day: 10_000,
            expected_daily_settlements: 4,
        };
        let mut value = serde_json::to_value(&evidence).unwrap();
        value["observed_daily_cycles"] = Value::from(10_000);
        assert!(serde_json::from_value::<Evidence>(value).is_err());

        let mut short = serde_json::to_value(&evidence).unwrap();
        short["governance_gas_used"] = serde_json::to_value(vec![10_000u128; 9]).unwrap();
        assert!(derive(&serde_json::from_value(short).unwrap()).is_err());

        let mut obsolete_schema = serde_json::to_value(&evidence).unwrap();
        obsolete_schema["schema_version"] = Value::from(1);
        assert!(derive(&serde_json::from_value(obsolete_schema).unwrap()).is_err());

        let mut obsolete_field = serde_json::to_value(&evidence).unwrap();
        obsolete_field["base_fee_per_gas_30d"] = obsolete_field["base_fee_per_gas"].clone();
        assert!(serde_json::from_value::<Evidence>(obsolete_field).is_err());

        let mut short_period = serde_json::to_value(&evidence).unwrap();
        short_period["fee_observation_end_unix"] = Value::from(1_700_000_001u64);
        assert!(derive(&serde_json::from_value(short_period).unwrap()).is_err());

        let mut zero = serde_json::to_value(&evidence).unwrap();
        zero["baseline_cycles_per_day"] = Value::from(0);
        assert!(derive(&serde_json::from_value(zero).unwrap()).is_err());

        evidence.expected_daily_settlements = u128::MAX;
        assert!(derive(&evidence).is_err());
    }

    #[test]
    fn profile_has_no_self_asserted_status_and_requires_provider_independence() {
        let mut profile = valid_profile();
        profile.schema_version = 3;
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
        profile.timelock.canceller = profile.governance_operator.clone();
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
        profile.bsns_runtime_template_sha256 = hex(&Sha256::digest(b"bsns-runtime"));
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
                br#"{"schema_version":2,"sample_count":10,"observation_days":7}"#.to_vec(),
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
        fs::write(root.join("profile.json"), planned_profile).unwrap();
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
        let post_deploy_profile = serde_json::to_vec(&profile).unwrap();
        fs::write(root.join("profile.json"), &post_deploy_profile).unwrap();
        let post_deploy_profile_sha256 = hex(&Sha256::digest(&post_deploy_profile));
        artifacts
            .iter_mut()
            .find(|a| a.path == "profile.json")
            .unwrap()
            .sha256 = post_deploy_profile_sha256.clone();
        let receipt = GateAReceipt {
            schema_version: 1,
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
        };
        let receipt_bytes = serde_json::to_vec(&receipt).unwrap();
        fs::write(root.join("gate-a-receipt.json"), &receipt_bytes).unwrap();
        artifacts.push(ArtifactDigest {
            path: "gate-a-receipt.json".into(),
            sha256: hex(&Sha256::digest(receipt_bytes)),
        });
        let manifest = ReleaseManifest {
            schema_version: 3,
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
        let bundle = validate_bundle(&root, true).unwrap();
        // Cryptographic live inputs are verified against the network by `verify-live`;
        // this fixture exercises only deterministic bundle inputs.

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
            operation_id: format!("0x{}", "1".repeat(64)),
            operation_salt: format!("0x{}", "2".repeat(64)),
            prior_schedule_receipt_sha256: None,
        };
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_ok());
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
            drift_error.contains("Gate B evidence is not bound"),
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
