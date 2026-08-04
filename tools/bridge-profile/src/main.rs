use candid::{CandidType, Decode, Encode, Principal, Reserved};
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
const CURRENT_STABLE_SCHEMA_VERSION: u16 = 31;
const GATE_A_ARTIFACTS: [&str; 4] = [
    "profile.json",
    "monitor-drill.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
];
const GATE_B_ARTIFACTS: [&str; 9] = [
    "profile.json",
    "signer-snapshot.json",
    "rpc-e2e.json",
    "controller-handover.json",
    "sns-upgrade.json",
    "monitor-drill.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
    "gate-a-receipt.json",
];

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct Profile {
    environment: String,
    test_assets_only: bool,
    chain_id: u64,
    evm_rpc_canister_id: String,
    ledger_canister_id: String,
    index_canister_id: String,
    root_canister_id: String,
    governance_principal: String,
    decimals: u8,
    bridge_canister_id: String,
    canister_schema_version: u16,
    ic_host: String,
    base_rpc_url: String,
    bridge_contract: String,
    bsns_contract: String,
    deployment_instance_id: String,
    deployment_block: u64,
    expected_bridge_signer: String,
    bridge_canister_wasm_sha256: String,
    bridge_runtime_bytecode_sha256: String,
    bsns_runtime_bytecode_sha256: String,
    ecdsa_key_name: String,
    ecdsa_derivation_path: Vec<String>,
    governance_ecdsa_derivation_path: Vec<String>,
    governance_operator: String,
    timelock: Timelock,
    pause_principal: String,
    fee_recipient: String,
    rpc_providers: Vec<RpcProvider>,
    monitoring: Monitoring,
    parameters: Parameters,
    rate_limits: RateLimits,
    governance_replacement: GovernanceReplacementPolicy,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
struct RpcProvider {
    url: String,
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
    governance_eth_floor_wei: u128,
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

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct GovernanceReplacementPolicy {
    max_replacements: u8,
    fee_bump_bps: u16,
}

#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq)]
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
    total_governance_fee_wei: Vec<u128>,
    governance_transactions_per_reserve_window: u128,
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
    governance_eth_floor_wei: u128,
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
struct GateAReceipt {
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
struct SignerSnapshot {
    schema_version: u8,
    observed_at_unix: u64,
    chain_id: u64,
    evm_rpc_canister_id: String,
    finalized_head_block_number: u64,
    finalized_head_block_hash: String,
    canonical: bool,
    agreeing_providers: u8,
    total_providers: u8,
    rpc_provider_urls_sha256: String,
    base_deposit_mints_paused: bool,
    base_withdrawals_paused: bool,
    canister_deposits_paused: bool,
    base_bridge_signer: String,
    canister_bridge_signer: String,
    base_runtime_administrator: String,
    bridge_runtime_bytecode_sha256: String,
    expected_bridge_runtime_bytecode_sha256: String,
    bridge_canister_wasm_sha256: String,
    bridge_canister_id: String,
    timelock_address: String,
    timelock_runtime_code_hash: String,
    bridge_approved_timelock_runtime_code_hash: String,
    timelock_minimum_delay_seconds: u64,
    timelock_self_admin: bool,
    timelock_proposer: String,
    timelock_executor: String,
    timelock_canceller: String,
    timelock_proposer_authorized: bool,
    timelock_executor_authorized: bool,
    timelock_canceller_authorized: bool,
    timelock_open_proposer: bool,
    timelock_open_executor: bool,
    timelock_open_canceller: bool,
    timelock_external_admins_absent: bool,
    timelock_roles_exact: bool,
    bridge_deployment_transaction_hash: String,
    bridge_deployment_block_number: u64,
    bridge_deployment_block_hash: String,
    timelock_deployment_transaction_hash: String,
    timelock_deployment_block_number: u64,
    timelock_deployment_block_hash: String,
    bsns_address: String,
    bsns_runtime_bytecode_sha256: String,
    bsns_name: String,
    bsns_symbol: String,
    bsns_decimals: u8,
    bsns_bridge: String,
    ic_controller: String,
    expected_ic_controller: String,
    settlement_reserve_sufficient: bool,
    ledger_fee: u128,
    base_service_fee: u128,
    public_config: LivePublicConfig,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LivePublicConfig {
    base_chain_id: u64,
    bridge_contract: String,
    timelock_contract: String,
    deployment_instance_id: String,
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
    governance_eth_floor_wei: u128,
    #[serde(with = "u128_string")]
    cycles_floor: u128,
    #[serde(with = "u128_string")]
    settlement_cycle_ceiling: u128,
    governance_principal: String,
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
    base_postcondition_sha256: String,
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
}

#[derive(CandidType, Deserialize)]
enum ActivationStatusResultView {
    Ok(ActivationStatusView),
    Err(Reserved),
}

#[derive(CandidType, Deserialize)]
struct EmergencyPauseReceiptView {
    caller: Principal,
    local_deposits_paused: bool,
    local_pause_audit_sequence: u64,
    local_pause_audit_sha256: Vec<u8>,
    base_governance: BaseGovernanceReceiptView,
}

#[derive(CandidType, Deserialize)]
struct BaseGovernanceReceiptView {
    operation_id: u64,
    nonce: u64,
    transaction_hash: Option<Vec<u8>>,
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
        || evidence.total_governance_fee_wei.len() < 10
    {
        return Err(
            "governance gas, fee, total-fee, and cycle evidence must contain at least 10 samples each".into(),
        );
    }
    if evidence.ledger_fee == 0
        || evidence.baseline_cycles_per_day == 0
        || evidence.expected_daily_settlements == 0
        || evidence.governance_transactions_per_reserve_window == 0
        || evidence.governance_gas_used.contains(&0)
        || evidence.settlement_cycles.contains(&0)
        || evidence.base_fee_per_gas.is_empty()
        || evidence.base_fee_per_gas.len() != evidence.priority_fee_per_gas.len()
        || evidence.base_fee_per_gas.len() != evidence.l1_fee_upper_bound_wei.len()
        || evidence.base_fee_per_gas.len() != evidence.total_governance_fee_wei.len()
        || evidence.base_fee_per_gas.contains(&0)
        || evidence.priority_fee_per_gas.contains(&0)
        || evidence.l1_fee_upper_bound_wei.contains(&0)
        || evidence.total_governance_fee_wei.contains(&0)
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
    let floor_transactions = if evidence.environment == "base-sepolia" {
        10
    } else {
        evidence.governance_transactions_per_reserve_window
    };
    let governance_eth_floor_wei = percentile(&evidence.total_governance_fee_wei, 99, 100)?
        .checked_mul(floor_transactions)
        .and_then(|value| value.checked_mul(2))
        .ok_or("ETH floor overflow")?;
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
        governance_eth_floor_wei,
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
    if !principal(&profile.bridge_canister_id)
        || !credential_free_https(&profile.ic_host)
        || !credential_free_https(&profile.base_rpc_url)
        || !profile
            .rpc_providers
            .iter()
            .any(|provider| provider.url.eq_ignore_ascii_case(&profile.base_rpc_url))
    {
        return Err("invalid release endpoint".into());
    }
    if profile.evm_rpc_canister_id != OFFICIAL_EVM_RPC_CANISTER {
        return Err("profile must bind the official EVM RPC canister ID".into());
    }
    if !valid_nonzero_hash32(&profile.deployment_instance_id)
        || !valid_sha256(&profile.bridge_canister_wasm_sha256)
        || !valid_sha256(&profile.bridge_runtime_bytecode_sha256)
        || !valid_sha256(&profile.bsns_runtime_bytecode_sha256)
    {
        return Err("profile must bind a deployment instance ID and Bridge artifact hashes".into());
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
    let principals = [
        &profile.governance_principal,
        &profile.pause_principal,
        &profile.fee_recipient,
    ];
    let mut unique_principals = BTreeSet::new();
    for value in principals {
        if !principal(value) || !unique_principals.insert(value) {
            return Err("invalid or overlapping IC operational principal".into());
        }
    }
    if profile.rpc_providers.len() != 3 {
        return Err("exactly three RPC providers are required".into());
    }
    let urls = profile
        .rpc_providers
        .iter()
        .map(|p| p.url.trim().to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    if urls.len() != 3
        || profile
            .rpc_providers
            .iter()
            .any(|p| !credential_free_https(p.url.trim()))
    {
        return Err("RPC providers must be three distinct credential-free HTTPS URLs".into());
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
        || p.governance_eth_floor_wei == 0
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
        "governance_eth_floor_wei": profile.parameters.governance_eth_floor_wei.to_string(),
        "cycles_floor": profile.parameters.cycles_floor.to_string(),
        "settlement_cycle_ceiling": profile.parameters.settlement_cycle_ceiling.to_string(),
        "governance_principal": profile.governance_principal,
        "pause_principal": profile.pause_principal,
        "fee_recipient": { "owner": profile.fee_recipient, "subaccount_hex": "" }
    });
    let constructors = serde_json::json!({
        "bridge": [
            "KINIC", "KINIC", profile.decimals.to_string(), profile.expected_bridge_signer,
            profile.governance_operator, profile.timelock.address, profile.timelock.runtime_code_hash,
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
        "initial_pause_required": true
    });
    let ui = serde_json::json!({
        "environment": profile.environment,
        "label": if profile.test_assets_only { "Base Sepolia" } else { "Base" },
        "testOnly": profile.test_assets_only,
        "environmentMode": null,
        "activationTimelockDelaySeconds": profile.timelock.minimum_delay_seconds,
        "gateBManifestSha256": gate_b_manifest_sha256,
        "profileFileSha256": profile_file_sha256,
        "profileCanonicalSha256": profile_canonical_sha256,
        "icHost": profile.ic_host,
        "baseRpcUrl": profile.base_rpc_url,
        "chainId": profile.chain_id,
        "bridgeCanisterId": profile.bridge_canister_id,
        "deploymentInstanceId": profile.deployment_instance_id,
        "ledgerCanisterId": profile.ledger_canister_id,
        "indexCanisterId": profile.index_canister_id,
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

fn validate_bundle(root: &Path, gate_b: bool) -> Result<ValidatedBundle, String> {
    if root.join("proof-attestation.json").exists() {
        return Err(
            "obsolete self-asserted proof attestation is forbidden; release drivers rerun proofs"
                .into(),
        );
    }
    let manifest: ReleaseManifest = read_json(&root.join("release-manifest.json"))?;
    if manifest.schema_version != 2
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
    {
        return Err("release artifacts do not match profile code hashes".into());
    }
    if gate_b {
        let receipt: GateAReceipt = read_json(&root.join("gate-a-receipt.json"))?;
        let profile_hash = artifacts["profile.json"].sha256.as_str();
        let mut gate_a_profile = profile.clone();
        gate_a_profile.deployment_block = 0;
        let expected_gate_a_profile_hash = hex(&canonical_sha256(&gate_a_profile)?);
        if !receipt.gate_a_manifest_sha256.eq_ignore_ascii_case(
            manifest
                .parent_gate_a_manifest_sha256
                .as_deref()
                .unwrap_or_default(),
        ) || receipt.release_id != manifest.release_id
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
    let drill: MonitorDrill = read_json(&root.join("monitor-drill.json"))?;
    validate_monitor_drill(&drill, &manifest, &profile, now)?;
    if gate_b {
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
        || observed.governance_eth_floor_wei != p.governance_eth_floor_wei
        || observed.cycles_floor != p.cycles_floor
        || observed.settlement_cycle_ceiling != p.settlement_cycle_ceiling
        || observed.governance_principal != profile.governance_principal
        || observed.pause_principal != profile.pause_principal
        || observed.fee_recipient.owner != profile.fee_recipient
        || !observed.fee_recipient.subaccount_hex.is_empty()
    {
        return Err("live Canister PublicConfig does not exactly match the profile".into());
    }
    Ok(())
}

fn verify_live_inputs(bundle: &ValidatedBundle) -> Result<(), String> {
    let snapshot: SignerSnapshot = read_json(&bundle.root.join("signer-snapshot.json"))?;
    let receipt: GateAReceipt = read_json(&bundle.root.join("gate-a-receipt.json"))?;
    let now = now_unix()?;
    validate_evidence_time(
        snapshot.observed_at_unix,
        bundle.manifest.created_at_unix,
        now,
    )?;
    let rpc_url_hash = hex(&canonical_sha256(
        &bundle
            .profile
            .rpc_providers
            .iter()
            .map(|provider| provider.url.clone())
            .collect::<Vec<_>>(),
    )?);
    validate_live_public_config(&snapshot.public_config, &bundle.profile, &rpc_url_hash)?;
    if snapshot.schema_version != 2
        || now
            .checked_sub(snapshot.observed_at_unix)
            .is_none_or(|age| age > 5 * 60)
        || snapshot.bridge_canister_id != bundle.profile.bridge_canister_id
        || snapshot.chain_id != bundle.profile.chain_id
        || snapshot.evm_rpc_canister_id != bundle.profile.evm_rpc_canister_id
        || snapshot.finalized_head_block_number == 0
        || !valid_hash32(&snapshot.finalized_head_block_hash)
        || !snapshot.canonical
        || snapshot.total_providers != 3
        || snapshot.agreeing_providers < 2
        || !snapshot
            .rpc_provider_urls_sha256
            .eq_ignore_ascii_case(&rpc_url_hash)
        || !snapshot.base_deposit_mints_paused
        || !snapshot.base_withdrawals_paused
        || !snapshot.canister_deposits_paused
        || !snapshot
            .base_bridge_signer
            .eq_ignore_ascii_case(&bundle.profile.expected_bridge_signer)
        || !snapshot
            .canister_bridge_signer
            .eq_ignore_ascii_case(&bundle.profile.expected_bridge_signer)
        || !snapshot
            .base_runtime_administrator
            .eq_ignore_ascii_case(&bundle.profile.governance_operator)
        || !valid_sha256(&snapshot.bridge_runtime_bytecode_sha256)
        || !snapshot
            .bridge_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&snapshot.expected_bridge_runtime_bytecode_sha256)
        || !snapshot
            .bridge_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&bundle.profile.bridge_runtime_bytecode_sha256)
        || !snapshot
            .bridge_canister_wasm_sha256
            .eq_ignore_ascii_case(&bundle.profile.bridge_canister_wasm_sha256)
        || snapshot.bridge_canister_id != bundle.profile.bridge_canister_id
        || !snapshot
            .timelock_address
            .eq_ignore_ascii_case(&bundle.profile.timelock.address)
        || !valid_hash32(&snapshot.timelock_runtime_code_hash)
        || !snapshot
            .timelock_runtime_code_hash
            .eq_ignore_ascii_case(&bundle.profile.timelock.runtime_code_hash)
        || !snapshot
            .bridge_approved_timelock_runtime_code_hash
            .eq_ignore_ascii_case(&bundle.profile.timelock.runtime_code_hash)
        || snapshot.timelock_minimum_delay_seconds != bundle.profile.timelock.minimum_delay_seconds
        || !snapshot.timelock_self_admin
        || !snapshot
            .timelock_proposer
            .eq_ignore_ascii_case(&bundle.profile.timelock.proposer)
        || !snapshot
            .timelock_executor
            .eq_ignore_ascii_case(&bundle.profile.timelock.executor)
        || !snapshot
            .timelock_canceller
            .eq_ignore_ascii_case(&bundle.profile.timelock.canceller)
        || !snapshot.timelock_proposer_authorized
        || !snapshot.timelock_executor_authorized
        || !snapshot.timelock_canceller_authorized
        || snapshot.timelock_open_proposer
        || snapshot.timelock_open_executor
        || snapshot.timelock_open_canceller
        || !snapshot.timelock_external_admins_absent
        || !snapshot.timelock_roles_exact
        || !snapshot
            .bridge_deployment_transaction_hash
            .eq_ignore_ascii_case(&receipt.bridge_deployment_transaction_hash)
        || snapshot.bridge_deployment_block_number != receipt.bridge_deployment_block_number
        || !snapshot
            .bridge_deployment_block_hash
            .eq_ignore_ascii_case(&receipt.bridge_deployment_block_hash)
        || !snapshot
            .timelock_deployment_transaction_hash
            .eq_ignore_ascii_case(&receipt.timelock_deployment_transaction_hash)
        || snapshot.timelock_deployment_block_number != receipt.timelock_deployment_block_number
        || !snapshot
            .timelock_deployment_block_hash
            .eq_ignore_ascii_case(&receipt.timelock_deployment_block_hash)
        || !snapshot
            .bsns_address
            .eq_ignore_ascii_case(&bundle.profile.bsns_contract)
        || !snapshot
            .bsns_runtime_bytecode_sha256
            .eq_ignore_ascii_case(&bundle.profile.bsns_runtime_bytecode_sha256)
        || snapshot.bsns_name != "KINIC"
        || snapshot.bsns_symbol != "KINIC"
        || snapshot.bsns_decimals != bundle.profile.decimals
        || !snapshot
            .bsns_bridge
            .eq_ignore_ascii_case(&bundle.profile.bridge_contract)
        || snapshot.ic_controller != snapshot.expected_ic_controller
        || snapshot.ic_controller != bundle.profile.root_canister_id
        || !principal(&snapshot.ic_controller)
        || !snapshot.settlement_reserve_sufficient
        || snapshot.ledger_fee != bundle.profile.parameters.ledger_fee
        || snapshot.base_service_fee != bundle.profile.parameters.service_fee
        || snapshot.ledger_fee > snapshot.base_service_fee
    {
        return Err(
            "live snapshot does not match the approved profile or safety requirements".into(),
        );
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
    let first_pause = drill
        .base_actions
        .iter()
        .find(|action| action.kind == "PauseDepositMints")
        .ok_or("monitor evidence is missing PauseDepositMints")?;
    let expected_transaction = decode_hex(&first_pause.transaction_hash)?;
    if receipt.caller != pause_principal
        || drill.ic_pause.pause_principal != bundle.profile.pause_principal
        || !receipt.local_deposits_paused
        || receipt.local_pause_audit_sequence != drill.ic_pause.audit_sequence
        || receipt.local_pause_audit_sha256 != audit_sha
        || receipt.base_governance.operation_id == 0
        || receipt.base_governance.transaction_hash.as_deref()
            != Some(expected_transaction.as_slice())
    {
        return Err("certified emergency_pause receipt is not bound to the drill evidence".into());
    }
    let _ = receipt.base_governance.nonce;
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

fn verify_live(bundle: &ValidatedBundle) -> Result<(), String> {
    verify_live_inputs(bundle)?;
    verify_sns_upgrade_authenticity(bundle)
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
    if receipt.schema_version != 3
        || receipt.phase != "schedule"
        || receipt.release_id != bundle.manifest.release_id
        || receipt.source_revision != bundle.manifest.source_revision
        || !receipt
            .source_tree_sha256
            .eq_ignore_ascii_case(&bundle.manifest.source_tree_sha256)
        || !valid_sha256(&receipt.gate_b_manifest_sha256)
        || receipt
            .gate_b_manifest_sha256
            .eq_ignore_ascii_case(&bundle.manifest_sha256)
        || receipt.proposal_id == 0
        || receipt.function_id == 0
        || receipt.target_method_name != "schedule_activation"
        || !receipt.payload_sha256.eq_ignore_ascii_case(&payload_sha256)
        || receipt.executed_at_unix == 0
        || receipt.verified_at_unix < receipt.executed_at_unix
        || receipt.verified_at_unix > bundle.manifest.created_at_unix
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
        || !valid_sha256(&receipt.base_postcondition_sha256)
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
        ("execute", Some(path)) => Some((read_json::<ActivationReceipt>(path)?, path)),
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

    let verifier =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/production-live-preflight.sh");
    let base_output = Command::new(verifier)
        .arg("verify-activation")
        .arg(phase)
        .arg(&bundle.root)
        .arg(&operation_id)
        .output()
        .map_err(|error| format!("failed to execute activation Base verifier: {error}"))?;
    if !base_output.status.success() {
        return Err("activation Base postcondition verifier rejected the live state".into());
    }
    let prior_schedule_receipt_sha256 = prior
        .as_ref()
        .map(|(_, path)| fs::read(path).map(|bytes| hex(&Sha256::digest(bytes))))
        .transpose()
        .map_err(|error| error.to_string())?;
    let receipt = ActivationReceipt {
        schema_version: 3,
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
        base_postcondition_sha256: hex(&Sha256::digest(&base_output.stdout)),
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
        .ok_or("schedule receipt operation is no longer pending in the Canister")?;
    if !activation.deposits_paused
        || format!("0x{}", hex(&pending.operation_id)) != receipt.operation_id.to_lowercase()
        || format!("0x{}", hex(&pending.salt)) != receipt.operation_salt.to_lowercase()
    {
        return Err("live Canister activation state does not match the schedule receipt".into());
    }

    let verifier =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/production-live-preflight.sh");
    let status = Command::new(verifier)
        .arg("verify-activation")
        .arg("schedule")
        .arg(&bundle.root)
        .arg(&receipt.operation_id)
        .status()
        .map_err(|error| format!("failed to execute schedule Base verifier: {error}"))?;
    if !status.success() {
        return Err("prior schedule receipt no longer matches the live Base state".into());
    }
    Ok(())
}

fn verify_gate_a_authenticity(bundle: &ValidatedBundle) -> Result<(), String> {
    verify_monitor_ic_certificate(bundle)?;
    let verifier =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/production-live-preflight.sh");
    let status = Command::new(verifier)
        .arg("verify-gate-a")
        .arg(&bundle.root)
        .status()
        .map_err(|error| format!("failed to execute Gate A Base verifier: {error}"))?;
    if !status.success() {
        return Err("Gate A Base receipt/log verifier rejected the monitor drill".into());
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
                "gate_a=structural-pass authorizing=false manifest_sha256={}",
                bundle.manifest_sha256
            );
        }
        Some("verify-gate-a-live") if args.len() == 3 => {
            let bundle = validate_bundle(Path::new(&args[2]), false)?;
            if bundle.manifest.test_only {
                return Err("Gate A rejects test-only bundles".into());
            }
            verify_gate_a_authenticity(&bundle)?;
            println!("gate_a=pass manifest_sha256={}", bundle.manifest_sha256);
        }
        Some("verify-live") if args.len() == 3 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only { return Err("Gate B rejects test-only bundles".into()); }
            verify_live(&bundle)?;
            println!("gate_b=pass manifest_sha256={}", bundle.manifest_sha256);
        }
        Some("verify-activation") if args.len() == 7 => {
            let bundle = validate_bundle(Path::new(&args[3]), true)?;
            if bundle.manifest.test_only {
                return Err("activation verification rejects test-only bundles".into());
            }
            verify_live(&bundle)?;
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
            verify_live(&bundle)?;
            verify_schedule_receipt_live(&bundle, Path::new(&args[3]))?;
            println!(
                "schedule_receipt=verified manifest_sha256={} receipt={}",
                bundle.manifest_sha256, args[3]
            );
        }
        _ => return Err("usage: bridge-profile <derive|validate|validate-test> <json-file> | render-release-inputs <profile.json> <output-dir> | render-test-inputs <profile.json> <output-dir> | render-bundle-inputs <bundle-dir> <output-dir> | validate-bundle --offline <bundle-dir> | verify-gate-a-live <bundle-dir> | verify-live <bundle-dir> | verify-schedule-receipt-live <bundle-dir> <schedule-receipt.json> | verify-activation <schedule|execute> <bundle-dir> <submission.json> <prior-schedule-receipt.json|-> <receipt.json>".into()),
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

    fn test_principal(seed: u8) -> String {
        Principal::self_authenticating([seed; 32]).to_text()
    }
    fn address(seed: u8) -> String {
        format!("0x{seed:040x}")
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
    fn deployment_instance_id_must_be_nonzero() {
        assert!(valid_nonzero_hash32(&format!("0x{}", "11".repeat(32))));
        assert!(!valid_nonzero_hash32(&format!("0x{}", "00".repeat(32))));
    }

    fn valid_profile() -> Profile {
        Profile {
            environment: "mainnet-candidate".into(),
            test_assets_only: false,
            chain_id: 8453,
            evm_rpc_canister_id: OFFICIAL_EVM_RPC_CANISTER.into(),
            ledger_canister_id: KINIC_LEDGER.into(),
            index_canister_id: KINIC_INDEX.into(),
            root_canister_id: KINIC_ROOT.into(),
            governance_principal: KINIC_GOVERNANCE.into(),
            decimals: 8,
            bridge_canister_id: test_principal(9),
            canister_schema_version: CURRENT_STABLE_SCHEMA_VERSION,
            ic_host: "https://icp-api.io".into(),
            base_rpc_url: "https://prod-one.example/base-mainnet".into(),
            bridge_contract: address(1),
            bsns_contract: address(8),
            deployment_instance_id: format!("0x{}", "11".repeat(32)),
            deployment_block: 1,
            expected_bridge_signer: address(2),
            bridge_canister_wasm_sha256: "3".repeat(64),
            bridge_runtime_bytecode_sha256: "4".repeat(64),
            bsns_runtime_bytecode_sha256: "5".repeat(64),
            ecdsa_key_name: "key_1".into(),
            ecdsa_derivation_path: vec!["KINIC-BASE-BRIDGE".into()],
            governance_ecdsa_derivation_path: vec!["KINIC-BASE-GOVERNANCE".into()],
            governance_operator: address(3),
            timelock: Timelock {
                address: address(5),
                runtime_code_hash: format!("0x{}", "ab".repeat(32)),
                minimum_delay_seconds: 86_400,
                proposer: address(3),
                canceller: address(3),
                executor: address(3),
                external_admins: 0,
            },
            pause_principal: test_principal(2),
            fee_recipient: test_principal(4),
            rpc_providers: vec![
                RpcProvider {
                    url: "https://prod-one.example/base-mainnet".into(),
                },
                RpcProvider {
                    url: "https://prod-two.example/base-mainnet".into(),
                },
                RpcProvider {
                    url: "https://prod-three.example/base-mainnet".into(),
                },
            ],
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
                governance_eth_floor_wei: 100_000_000,
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
            governance_eth_floor_wei: profile.parameters.governance_eth_floor_wei,
            cycles_floor: profile.parameters.cycles_floor,
            settlement_cycle_ceiling: profile.parameters.settlement_cycle_ceiling,
            governance_principal: profile.governance_principal.clone(),
            pause_principal: profile.pause_principal.clone(),
            fee_recipient: LiveFeeRecipient {
                owner: profile.fee_recipient.clone(),
                subaccount_hex: String::new(),
            },
        }
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
            total_governance_fee_wei: vec![100; 10],
            governance_transactions_per_reserve_window: 4,
            settlement_cycles: vec![1_000; 10],
            baseline_cycles_per_day: 10_000,
            expected_daily_settlements: 4,
        };
        let result = derive(&evidence).unwrap();
        assert_eq!(result.gas_limit_ceiling, 40_000);
        assert_eq!(result.max_fee_per_gas_ceiling, 200);
        assert_eq!(result.max_priority_fee_per_gas_ceiling, 8);
        assert_eq!(result.l1_fee_per_transaction_ceiling_wei, 50);
        assert_eq!(result.governance_eth_floor_wei, 800);
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
            total_governance_fee_wei: vec![100; 10],
            governance_transactions_per_reserve_window: 4,
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
    fn profile_has_no_self_asserted_status_and_does_not_require_provider_independence() {
        let mut profile = valid_profile();
        profile.rpc_providers[1].url = "https://another.example".into();
        assert!(validate_profile(&profile, true).is_ok());
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
        profile.rpc_providers[1].url = profile.rpc_providers[0].url.clone();
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.rpc_providers[1].url = "https://user:secret@rpc.example".into();
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.rpc_providers[1].url = "https://rpc.example/rpc?token=secret".into();
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.rpc_providers[1].url = "https://rpc.example/abcdefghijklmnop".into();
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.rpc_providers[1].url = "https://127.0.0.1/rpc".into();
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
        let profile = serde_json::to_value(valid_profile()).unwrap();
        assert!(profile["parameters"]["governance_eth_floor_wei"].is_string());
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
            "governance_eth_floor_wei",
            "cycles_floor",
            "settlement_cycle_ceiling",
            "governance_principal",
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
            constructors["bridge"][6],
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
        assert_eq!(
            ui["rpcProviderUrlsSha256"],
            format!("0x{}", hex(&Sha256::digest(b"[]")))
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundle_gate_validates_hashes_slo_and_live_snapshot() {
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
        let controller = profile.root_canister_id.clone();
        let mut snapshot = SignerSnapshot {
            schema_version: 2,
            observed_at_unix: now,
            chain_id: profile.chain_id,
            evm_rpc_canister_id: profile.evm_rpc_canister_id.clone(),
            finalized_head_block_number: 1,
            finalized_head_block_hash: format!("0x{}", "ab".repeat(32)),
            canonical: true,
            agreeing_providers: 2,
            total_providers: 3,
            rpc_provider_urls_sha256: hex(&canonical_sha256(
                &profile
                    .rpc_providers
                    .iter()
                    .map(|provider| provider.url.clone())
                    .collect::<Vec<_>>(),
            )
            .unwrap()),
            base_deposit_mints_paused: true,
            base_withdrawals_paused: true,
            canister_deposits_paused: true,
            base_bridge_signer: profile.expected_bridge_signer.clone(),
            canister_bridge_signer: profile.expected_bridge_signer.clone(),
            base_runtime_administrator: profile.governance_operator.clone(),
            bridge_runtime_bytecode_sha256: "1".repeat(64),
            expected_bridge_runtime_bytecode_sha256: "1".repeat(64),
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            timelock_address: profile.timelock.address.clone(),
            timelock_runtime_code_hash: profile.timelock.runtime_code_hash.clone(),
            bridge_approved_timelock_runtime_code_hash: profile.timelock.runtime_code_hash.clone(),
            timelock_minimum_delay_seconds: 86_400,
            timelock_self_admin: true,
            timelock_proposer: profile.timelock.proposer.clone(),
            timelock_executor: profile.timelock.executor.clone(),
            timelock_canceller: profile.timelock.canceller.clone(),
            timelock_proposer_authorized: true,
            timelock_executor_authorized: true,
            timelock_canceller_authorized: true,
            timelock_open_proposer: false,
            timelock_open_executor: false,
            timelock_open_canceller: false,
            timelock_external_admins_absent: true,
            timelock_roles_exact: true,
            bridge_deployment_transaction_hash: format!("0x{}", "aa".repeat(32)),
            bridge_deployment_block_number: 1,
            bridge_deployment_block_hash: format!("0x{}", "cc".repeat(32)),
            timelock_deployment_transaction_hash: format!("0x{}", "bb".repeat(32)),
            timelock_deployment_block_number: 1,
            timelock_deployment_block_hash: format!("0x{}", "dd".repeat(32)),
            bsns_address: profile.bsns_contract.clone(),
            bsns_runtime_bytecode_sha256: profile.bsns_runtime_bytecode_sha256.clone(),
            bsns_name: "KINIC".into(),
            bsns_symbol: "KINIC".into(),
            bsns_decimals: profile.decimals,
            bsns_bridge: profile.bridge_contract.clone(),
            ic_controller: controller.clone(),
            expected_ic_controller: controller,
            settlement_reserve_sufficient: true,
            ledger_fee: profile.parameters.ledger_fee,
            base_service_fee: profile.parameters.service_fee,
            public_config: live_public_config(&profile),
        };
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
  m.rehearsal.capture_artifact(value,m.config(),scenario,kind,output,command,0 if kind=='base' else None); reference['sha256']=m.rehearsal.hashlib.sha256(output.read_bytes()).hexdigest()
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
            proposal_target_canister_id: profile.bridge_canister_id.clone(),
            proposal_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            governance_query_response_hex: hex(b"governance raw"),
            governance_query_response_sha256: hex(&Sha256::digest(b"governance raw")),
        };
        let mut docs = vec![
            ("profile.json", serde_json::to_vec(&profile).unwrap()),
            (
                "signer-snapshot.json",
                serde_json::to_vec(&snapshot).unwrap(),
            ),
            ("rpc-e2e.json", fs::read(root.join("rpc-e2e.json")).unwrap()),
            (
                "controller-handover.json",
                serde_json::to_vec(&handover).unwrap(),
            ),
            ("sns-upgrade.json", serde_json::to_vec(&upgrade).unwrap()),
            ("monitor-drill.json", serde_json::to_vec(&drill).unwrap()),
            ("bridge-canister.wasm", b"wasm".to_vec()),
            ("bridge-runtime.bin", b"runtime".to_vec()),
        ];
        snapshot.bridge_canister_wasm_sha256 = profile.bridge_canister_wasm_sha256.clone();
        snapshot.bridge_runtime_bytecode_sha256 = profile.bridge_runtime_bytecode_sha256.clone();
        snapshot.expected_bridge_runtime_bytecode_sha256 =
            profile.bridge_runtime_bytecode_sha256.clone();
        docs[0].1 = serde_json::to_vec(&profile).unwrap();
        docs[1].1 = serde_json::to_vec(&snapshot).unwrap();
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
            schema_version: 2,
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
        let gate_a_authenticity_error = verify_gate_a_authenticity(&gate_a).unwrap_err();
        assert!(gate_a_authenticity_error.contains("invalid IC certificate CBOR"));
        let gate_a_profile_sha256 = hex(&canonical_sha256(&profile).unwrap());
        profile.deployment_block = snapshot.bridge_deployment_block_number;
        let post_deploy_profile = serde_json::to_vec(&profile).unwrap();
        fs::write(root.join("profile.json"), &post_deploy_profile).unwrap();
        let post_deploy_profile_sha256 = hex(&Sha256::digest(&post_deploy_profile));
        artifacts
            .iter_mut()
            .find(|a| a.path == "profile.json")
            .unwrap()
            .sha256 = post_deploy_profile_sha256.clone();
        let receipt = GateAReceipt {
            gate_a_manifest_sha256: gate_a.manifest_sha256.clone(),
            release_id: "release-1".into(),
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            gate_a_profile_sha256,
            post_deploy_profile_sha256,
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            bridge_runtime_bytecode_sha256: profile.bridge_runtime_bytecode_sha256.clone(),
            bridge_deployment_transaction_hash: snapshot.bridge_deployment_transaction_hash.clone(),
            bridge_deployment_block_number: snapshot.bridge_deployment_block_number,
            bridge_deployment_block_hash: snapshot.bridge_deployment_block_hash.clone(),
            timelock_deployment_transaction_hash: snapshot
                .timelock_deployment_transaction_hash
                .clone(),
            timelock_deployment_block_number: snapshot.timelock_deployment_block_number,
            timelock_deployment_block_hash: snapshot.timelock_deployment_block_hash.clone(),
        };
        let receipt_bytes = serde_json::to_vec(&receipt).unwrap();
        fs::write(root.join("gate-a-receipt.json"), &receipt_bytes).unwrap();
        artifacts.push(ArtifactDigest {
            path: "gate-a-receipt.json".into(),
            sha256: hex(&Sha256::digest(receipt_bytes)),
        });
        let manifest = ReleaseManifest {
            schema_version: 2,
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
        // Cryptographic SNS authenticity is verified against the live network by
        // `verify-live`; this fixture exercises only deterministic bundle inputs.
        verify_live_inputs(&bundle).unwrap();

        let payload_sha256 = hex(&Sha256::digest([0x44, 0x49, 0x44, 0x4c, 0x00, 0x00]));
        let mut schedule_receipt = ActivationReceipt {
            schema_version: 3,
            phase: "schedule".into(),
            release_id: bundle.manifest.release_id.clone(),
            source_revision: bundle.manifest.source_revision.clone(),
            source_tree_sha256: bundle.manifest.source_tree_sha256.clone(),
            gate_b_manifest_sha256: "9".repeat(64),
            proposal_id: 1,
            function_id: 1,
            target_method_name: "schedule_activation".into(),
            payload_sha256,
            executed_at_unix: manifest_created - 2,
            verified_at_unix: manifest_created - 1,
            governance_query_response_hex: hex(b"proposal"),
            governance_query_response_sha256: hex(&Sha256::digest(b"proposal")),
            function_registry_response_hex: hex(b"registry"),
            function_registry_response_sha256: hex(&Sha256::digest(b"registry")),
            activation_status_response_hex: hex(b"activation"),
            activation_status_response_sha256: hex(&Sha256::digest(b"activation")),
            operation_id: format!("0x{}", "1".repeat(64)),
            operation_salt: format!("0x{}", "2".repeat(64)),
            base_postcondition_sha256: "3".repeat(64),
            prior_schedule_receipt_sha256: None,
        };
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_ok());
        schedule_receipt.gate_b_manifest_sha256 = bundle.manifest_sha256.clone();
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());
        schedule_receipt.gate_b_manifest_sha256 = "9".repeat(64);
        schedule_receipt.activation_status_response_sha256 = "4".repeat(64);
        assert!(validate_schedule_receipt_binding(&schedule_receipt, &bundle).is_err());

        let valid_snapshot_bytes = fs::read(root.join("signer-snapshot.json")).unwrap();
        let mut legacy_snapshot: Value = serde_json::from_slice(&valid_snapshot_bytes).unwrap();
        legacy_snapshot["chain_key_eip191_signature"] = Value::String("00".repeat(65));
        fs::write(
            root.join("signer-snapshot.json"),
            serde_json::to_vec(&legacy_snapshot).unwrap(),
        )
        .unwrap();
        assert!(read_json::<SignerSnapshot>(&root.join("signer-snapshot.json")).is_err());

        let mut signer_drift: Value = serde_json::from_slice(&valid_snapshot_bytes).unwrap();
        signer_drift["canister_bridge_signer"] =
            Value::String("0x9999999999999999999999999999999999999999".into());
        fs::write(
            root.join("signer-snapshot.json"),
            serde_json::to_vec(&signer_drift).unwrap(),
        )
        .unwrap();
        assert!(verify_live_inputs(&bundle).is_err());

        let mut base_drift: Value = serde_json::from_slice(&valid_snapshot_bytes).unwrap();
        base_drift["base_bridge_signer"] =
            Value::String("0x8888888888888888888888888888888888888888".into());
        fs::write(
            root.join("signer-snapshot.json"),
            serde_json::to_vec(&base_drift).unwrap(),
        )
        .unwrap();
        assert!(verify_live_inputs(&bundle).is_err());
        let mut notification_limit_drift: Value =
            serde_json::from_slice(&valid_snapshot_bytes).unwrap();
        notification_limit_drift["public_config"]["notification_rate_limit_global"] =
            Value::from(59);
        fs::write(
            root.join("signer-snapshot.json"),
            serde_json::to_vec(&notification_limit_drift).unwrap(),
        )
        .unwrap();
        assert!(verify_live_inputs(&bundle).is_err());
        fs::write(root.join("signer-snapshot.json"), valid_snapshot_bytes).unwrap();

        let valid_snapshot_bytes = fs::read(root.join("signer-snapshot.json")).unwrap();
        let mut delay_drift: Value = serde_json::from_slice(&valid_snapshot_bytes).unwrap();
        delay_drift["timelock_minimum_delay_seconds"] = Value::from(86_401);
        fs::write(
            root.join("signer-snapshot.json"),
            serde_json::to_vec(&delay_drift).unwrap(),
        )
        .unwrap();
        assert!(verify_live_inputs(&bundle).is_err());
        let mut public_config_drift: Value = serde_json::from_slice(&valid_snapshot_bytes).unwrap();
        public_config_drift["public_config"]["governance_evm_fee"]["max_fee_per_gas_ceiling"] =
            Value::String("11".into());
        fs::write(
            root.join("signer-snapshot.json"),
            serde_json::to_vec(&public_config_drift).unwrap(),
        )
        .unwrap();
        assert!(verify_live_inputs(&bundle).is_err());
        fs::write(root.join("signer-snapshot.json"), valid_snapshot_bytes).unwrap();

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
