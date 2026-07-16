use candid::Principal;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    env, fs,
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
const GATE_A_ARTIFACTS: [&str; 5] = [
    "profile.json",
    "ceremony.json",
    "monitor-drill.json",
    "bridge-canister.wasm",
    "bridge-runtime.bin",
];
const GATE_B_ARTIFACTS: [&str; 8] = [
    "profile.json",
    "signer-snapshot.json",
    "ceremony.json",
    "rpc-e2e.json",
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
    ic_host: String,
    base_rpc_url: String,
    bridge_contract: String,
    bsns_contract: String,
    deployment_block: u64,
    expected_bridge_signer: String,
    bridge_canister_wasm_sha256: String,
    bridge_runtime_bytecode_sha256: String,
    bsns_runtime_bytecode_sha256: String,
    release_approver: String,
    ecdsa_key_name: String,
    ecdsa_derivation_path: Vec<String>,
    runtime_administrator: String,
    base_admin_wallet: String,
    timelock: Timelock,
    finance_administrator: String,
    pause_principals: Vec<String>,
    fee_recipient: String,
    rpc_providers: Vec<RpcProvider>,
    monitoring: Monitoring,
    parameters: Parameters,
    rate_limits: RateLimits,
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
    transaction_gas_limit: u128,
    #[serde(with = "u128_string")]
    max_fee_per_gas: u128,
    #[serde(with = "u128_string")]
    max_priority_fee_per_gas: u128,
    #[serde(with = "u128_string")]
    eth_floor_wei: u128,
    #[serde(with = "u128_string")]
    cycles_floor: u128,
    #[serde(with = "u128_string")]
    settlement_cycle_ceiling: u128,
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
    settlement_window_seconds: u64,
    settlement_global: u16,
    settlement_per_principal: u16,
    settlement_per_record: u16,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    ledger_fee: u128,
    mint_gas_used: Vec<u128>,
    acknowledgement_gas_used: Vec<u128>,
    refund_gas_used: Vec<u128>,
    base_fee_per_gas_30d: Vec<u128>,
    priority_fee_per_gas_30d: Vec<u128>,
    settlement_cycles: Vec<u128>,
    observed_daily_cycles: u128,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
struct DerivedParameters {
    ledger_fee: u128,
    max_service_fee: u128,
    service_fee: u128,
    transaction_gas_limit: u128,
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    eth_floor_wei: u128,
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
    approval: Option<Approval>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ArtifactDigest {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Approval {
    signer: String,
    eip191_signature: String,
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
    chain_key_eip191_signature: String,
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
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Ceremony {
    release_approver: String,
    base_admin: String,
    timelock_canceller: String,
    runtime_administrator: String,
    backup_restore_verified: bool,
    contains_secret_material: bool,
    role_challenges: Vec<RoleChallenge>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RoleChallenge {
    role: String,
    address: String,
    custodian_id: String,
    device_class: String,
    device_failure_domain: String,
    eip191_signature: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MonitorDrill {
    routing_sha256: String,
    fault_started_at_unix: u64,
    detected_at_unix: u64,
    acknowledged_at_unix: u64,
    base_paused_at_unix: u64,
    ic_paused_at_unix: u64,
    base_pause_reference: String,
    ic_pause_reference: String,
}

struct ValidatedBundle {
    root: PathBuf,
    manifest: ReleaseManifest,
    profile: Profile,
    manifest_sha256: String,
}

fn checked_percent(value: u128, numerator: u128, denominator: u128) -> Result<u128, String> {
    value
        .checked_mul(numerator)
        .map(|product| product / denominator)
        .ok_or_else(|| "percentage overflow".into())
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
    if evidence.mint_gas_used.len() != 100
        || evidence.acknowledgement_gas_used.len() != 100
        || evidence.refund_gas_used.len() != 100
        || evidence.settlement_cycles.len() != 100
    {
        return Err(
            "gas and settlement cycle evidence must contain exactly 100 samples per operation"
                .into(),
        );
    }
    let gas_max = evidence
        .mint_gas_used
        .iter()
        .chain(&evidence.acknowledgement_gas_used)
        .chain(&evidence.refund_gas_used)
        .copied()
        .max()
        .ok_or("missing gas samples")?;
    let transaction_gas_limit = checked_percent(gas_max, 130, 100)?
        .checked_add(999)
        .map(|value| value / 1_000 * 1_000)
        .ok_or("gas limit overflow")?;
    let max_priority_fee_per_gas = percentile(&evidence.priority_fee_per_gas_30d, 95, 100)?;
    let max_fee_per_gas = percentile(&evidence.base_fee_per_gas_30d, 99, 100)?
        .checked_mul(2)
        .and_then(|value| value.checked_add(max_priority_fee_per_gas))
        .ok_or("fee cap overflow")?;
    let eth_floor_wei = transaction_gas_limit
        .checked_mul(max_fee_per_gas)
        .and_then(|value| value.checked_mul(100))
        .ok_or("ETH floor overflow")?;
    let settlement_cycle_ceiling = checked_percent(
        *evidence.settlement_cycles.iter().max().unwrap_or(&0),
        150,
        100,
    )?;
    let cycles_floor = evidence
        .observed_daily_cycles
        .checked_mul(30)
        .ok_or("cycles floor overflow")?;
    Ok(DerivedParameters {
        ledger_fee: evidence.ledger_fee,
        max_service_fee: evidence
            .ledger_fee
            .checked_mul(100)
            .ok_or("maximum service fee overflow")?,
        service_fee: evidence
            .ledger_fee
            .checked_mul(10)
            .ok_or("service fee overflow")?,
        transaction_gas_limit,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        eth_floor_wei,
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
    if !valid_sha256(&profile.bridge_canister_wasm_sha256)
        || !valid_sha256(&profile.bridge_runtime_bytecode_sha256)
        || !valid_sha256(&profile.bsns_runtime_bytecode_sha256)
    {
        return Err("profile must bind Bridge Wasm and runtime bytecode hashes".into());
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
        &profile.release_approver,
        &profile.runtime_administrator,
        &profile.base_admin_wallet,
        &profile.timelock.address,
        &profile.timelock.canceller,
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
        || profile.timelock.minimum_delay_seconds < 72 * 60 * 60
        || profile.timelock.external_admins != 0
        || !profile
            .timelock
            .proposer
            .eq_ignore_ascii_case(&profile.base_admin_wallet)
        || !profile
            .timelock
            .executor
            .eq_ignore_ascii_case(&profile.base_admin_wallet)
    {
        return Err("unsafe Timelock configuration".into());
    }
    let principals = profile
        .pause_principals
        .iter()
        .chain([&profile.finance_administrator, &profile.fee_recipient]);
    let mut unique_principals = BTreeSet::new();
    for value in principals {
        if !principal(value) || !unique_principals.insert(value) {
            return Err("invalid or overlapping IC operational principal".into());
        }
    }
    if profile.pause_principals.len() != 2 || profile.rpc_providers.len() != 3 {
        return Err("exactly two pause principals and three RPC providers are required".into());
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
        || !(60..=3_600).contains(&r.settlement_window_seconds)
        || r.settlement_per_record == 0
        || r.settlement_per_record > r.settlement_per_principal
        || r.settlement_per_principal > r.settlement_global
    {
        return Err("unsafe rate-limit configuration".into());
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
        || p.mint_window_duration_seconds == 0
        || p.max_service_fee != p.ledger_fee.saturating_mul(100)
        || p.service_fee != p.ledger_fee.saturating_mul(10)
        || p.service_fee > p.max_service_fee
        || p.transaction_gas_limit == 0
        || p.max_priority_fee_per_gas > p.max_fee_per_gas
        || p.eth_floor_wei
            != p.transaction_gas_limit
                .checked_mul(p.max_fee_per_gas)
                .and_then(|v| v.checked_mul(100))
                .ok_or("reserve overflow")?
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
    let rpc_url_value = Value::Array(
        profile
            .rpc_providers
            .iter()
            .map(|provider| Value::String(provider.url.trim().to_string()))
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
        "custom_evm_rpc_urls": profile.rpc_providers.iter().map(|p| p.url.clone()).collect::<Vec<_>>(),
        "base_chain_id": profile.chain_id,
        "bridge_contract_hex": contract_hex,
        "ecdsa_key_name": profile.ecdsa_key_name,
        "ecdsa_derivation_path_utf8": profile.ecdsa_derivation_path,
        "deposit_rate_limit_window_seconds": profile.rate_limits.deposit_window_seconds,
        "deposit_rate_limit_global": profile.rate_limits.deposit_global,
        "deposit_rate_limit_per_principal": profile.rate_limits.deposit_per_principal,
        "settlement_rate_limit_window_seconds": profile.rate_limits.settlement_window_seconds,
        "settlement_rate_limit_global": profile.rate_limits.settlement_global,
        "settlement_rate_limit_per_principal": profile.rate_limits.settlement_per_principal,
        "settlement_rate_limit_per_record": profile.rate_limits.settlement_per_record,
        "transaction_gas_limit": profile.parameters.transaction_gas_limit.to_string(),
        "max_fee_per_gas": profile.parameters.max_fee_per_gas.to_string(),
        "max_priority_fee_per_gas": profile.parameters.max_priority_fee_per_gas.to_string(),
        "eth_floor_wei": profile.parameters.eth_floor_wei.to_string(),
        "cycles_floor": profile.parameters.cycles_floor.to_string(),
        "settlement_cycle_ceiling": profile.parameters.settlement_cycle_ceiling.to_string(),
        "governance_principal": profile.governance_principal,
        "pause_principals": profile.pause_principals,
        "finance_administrator": profile.finance_administrator,
        "fee_recipient": { "owner": profile.fee_recipient, "subaccount_hex": "" },
        "install_paused": true
    });
    let constructors = serde_json::json!({
        "bridge": [
            "KINIC", "KINIC", profile.decimals.to_string(), profile.expected_bridge_signer,
            profile.runtime_administrator, profile.timelock.address, profile.timelock.runtime_code_hash,
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
        "gateBManifestSha256": gate_b_manifest_sha256,
        "profileFileSha256": profile_file_sha256,
        "profileCanonicalSha256": profile_canonical_sha256,
        "icHost": profile.ic_host,
        "baseRpcUrl": profile.base_rpc_url,
        "chainId": profile.chain_id,
        "bridgeCanisterId": profile.bridge_canister_id,
        "ledgerCanisterId": profile.ledger_canister_id,
        "indexCanisterId": profile.index_canister_id,
        "icToken": { "name": "KINIC", "symbol": "KINIC", "decimals": profile.decimals },
        "baseToken": { "symbol": "KINIC", "decimals": profile.decimals },
        "bridgeAddress": profile.bridge_contract,
        "bsnsAddress": profile.bsns_contract,
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
        "schema_version": 1,
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
    let mut value = serde_json::to_value(manifest).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .ok_or("manifest must be an object")?
        .remove("approval");
    let mut bytes = Vec::new();
    canonical_json(&value, &mut bytes)?;
    Ok(Sha256::digest(bytes).into())
}

fn verify_eip191(hash: [u8; 32], approval: &Approval, expected: &str) -> Result<(), String> {
    if !evm_address(&approval.signer) || !approval.signer.eq_ignore_ascii_case(expected) {
        return Err("approval signer does not match release approver".into());
    }
    let bytes = decode_hex(&approval.eip191_signature)?;
    if bytes.len() != 65 {
        return Err("EIP-191 signature must be 65 bytes".into());
    }
    let signature = Signature::from_slice(&bytes[..64]).map_err(|_| "invalid ECDSA signature")?;
    let recovery = RecoveryId::try_from(match bytes[64] {
        27 | 28 => bytes[64] - 27,
        0 | 1 => bytes[64],
        _ => return Err("invalid recovery id".into()),
    })
    .map_err(|_| "invalid recovery id")?;
    let mut keccak = Keccak::v256();
    keccak.update(b"\x19Ethereum Signed Message:\n32");
    keccak.update(&hash);
    let mut digest = [0u8; 32];
    keccak.finalize(&mut digest);
    let key = VerifyingKey::recover_from_prehash(&digest, &signature, recovery)
        .map_err(|_| "signature recovery failed")?;
    let encoded = key.to_encoded_point(false);
    let mut address_hash = [0u8; 32];
    let mut address_keccak = Keccak::v256();
    address_keccak.update(&encoded.as_bytes()[1..]);
    address_keccak.finalize(&mut address_hash);
    let recovered = format!("0x{}", hex(&address_hash[12..]));
    if !recovered.eq_ignore_ascii_case(expected) {
        return Err("EIP-191 signature recovered an unexpected signer".into());
    }
    Ok(())
}

fn role_challenge_hash(release_id: &str, role: &str, address: &str) -> [u8; 32] {
    Sha256::digest(
        format!(
            "KINIC Bridge role control v1\nrelease_id={release_id}\nrole={role}\naddress={}",
            address.to_ascii_lowercase()
        )
        .as_bytes(),
    )
    .into()
}

fn validate_role_challenges(
    ceremony: &Ceremony,
    profile: &Profile,
    release_id: &str,
) -> Result<(), String> {
    let expected = [
        ("release_approver", profile.release_approver.as_str()),
        ("base_admin", profile.base_admin_wallet.as_str()),
        ("timelock_canceller", profile.timelock.canceller.as_str()),
        (
            "runtime_administrator",
            profile.runtime_administrator.as_str(),
        ),
    ];
    if ceremony.role_challenges.len() != expected.len() {
        return Err("key ceremony must contain one challenge per EVM role".into());
    }
    for (role, address) in expected {
        let challenge = ceremony
            .role_challenges
            .iter()
            .find(|value| value.role == role)
            .ok_or_else(|| format!("missing role challenge: {role}"))?;
        if !challenge.address.eq_ignore_ascii_case(address)
            || challenge.custodian_id.trim().is_empty()
            || challenge.device_class.trim().is_empty()
            || challenge.device_failure_domain.trim().is_empty()
        {
            return Err(format!("invalid role challenge metadata: {role}"));
        }
        let approval = Approval {
            signer: challenge.address.clone(),
            eip191_signature: challenge.eip191_signature.clone(),
        };
        verify_eip191(
            role_challenge_hash(release_id, role, address),
            &approval,
            address,
        )?;
    }
    let canceller = ceremony
        .role_challenges
        .iter()
        .find(|value| value.role == "timelock_canceller")
        .ok_or("missing Timelock canceller challenge")?;
    if ceremony.role_challenges.iter().any(|value| {
        value.role != "timelock_canceller"
            && (value
                .custodian_id
                .eq_ignore_ascii_case(&canceller.custodian_id)
                || value
                    .device_failure_domain
                    .eq_ignore_ascii_case(&canceller.device_failure_domain))
    }) {
        return Err(
            "Timelock canceller must use an independent custodian and device failure domain".into(),
        );
    }
    Ok(())
}

fn chain_key_challenge_hash(release_id: &str, address: &str) -> [u8; 32] {
    Sha256::digest(
        format!(
            "KINIC Bridge chain-key control v1\nrelease_id={release_id}\naddress={}",
            address.to_ascii_lowercase()
        )
        .as_bytes(),
    )
    .into()
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
    if value.pointer("/complete") != Some(&Value::Bool(true))
        || string("/state")? != "COMPLETE"
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

fn validate_bundle(root: &Path, require_approval: bool) -> Result<ValidatedBundle, String> {
    let manifest: ReleaseManifest = read_json(&root.join("release-manifest.json"))?;
    if manifest.schema_version != 1
        || !valid_release_id(&manifest.release_id)
        || manifest.source_revision.trim().is_empty()
        || !valid_sha256(&manifest.source_tree_sha256)
    {
        return Err("invalid release manifest identity".into());
    }
    if require_approval {
        if !manifest
            .parent_gate_a_manifest_sha256
            .as_deref()
            .is_some_and(valid_sha256)
        {
            return Err("Gate B must bind a Gate A manifest hash".into());
        }
    } else if manifest.parent_gate_a_manifest_sha256.is_some() || manifest.approval.is_some() {
        return Err("Gate A manifest must not contain approval or a parent Gate A hash".into());
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
    let required = if require_approval {
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
    if require_approval {
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
    if require_approval {
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
    let ceremony: Ceremony = read_json(&root.join("ceremony.json"))?;
    if ceremony.contains_secret_material
        || !ceremony.backup_restore_verified
        || !ceremony
            .release_approver
            .eq_ignore_ascii_case(&profile.release_approver)
        || !ceremony
            .base_admin
            .eq_ignore_ascii_case(&profile.base_admin_wallet)
        || !ceremony
            .timelock_canceller
            .eq_ignore_ascii_case(&profile.timelock.canceller)
        || !ceremony
            .runtime_administrator
            .eq_ignore_ascii_case(&profile.runtime_administrator)
    {
        return Err("key ceremony evidence is incomplete or inconsistent".into());
    }
    validate_role_challenges(&ceremony, &profile, &manifest.release_id)?;
    let drill: MonitorDrill = read_json(&root.join("monitor-drill.json"))?;
    for at in [
        drill.fault_started_at_unix,
        drill.detected_at_unix,
        drill.acknowledged_at_unix,
        drill.base_paused_at_unix,
        drill.ic_paused_at_unix,
    ] {
        validate_evidence_time(at, manifest.created_at_unix, now)?;
    }
    let elapsed = |at: u64| {
        at.checked_sub(drill.fault_started_at_unix)
            .ok_or("monitor timestamp precedes fault")
    };
    if elapsed(drill.detected_at_unix)? > 5 * 60
        || elapsed(drill.acknowledged_at_unix)? > 15 * 60
        || elapsed(drill.base_paused_at_unix)? > 60 * 60
        || elapsed(drill.ic_paused_at_unix)? > 60 * 60
        || drill.acknowledged_at_unix < drill.detected_at_unix
        || drill.base_paused_at_unix < drill.acknowledged_at_unix
        || drill.ic_paused_at_unix < drill.acknowledged_at_unix
        || drill.base_pause_reference.trim().is_empty()
        || drill.ic_pause_reference.trim().is_empty()
        || !drill
            .routing_sha256
            .eq_ignore_ascii_case(&profile.monitoring.routing_sha256)
    {
        return Err("monitor drill does not satisfy the 5/15/60 SLO".into());
    }
    let hash = unsigned_manifest_hash(&manifest)?;
    if require_approval || manifest.approval.is_some() {
        verify_eip191(
            hash,
            manifest
                .approval
                .as_ref()
                .ok_or("Gate B requires release approval")?,
            &profile.release_approver,
        )?;
    }
    Ok(ValidatedBundle {
        root: root.to_path_buf(),
        manifest,
        profile,
        manifest_sha256: hex(&hash),
    })
}

fn verify_live(bundle: &ValidatedBundle) -> Result<(), String> {
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
    if now
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
            .eq_ignore_ascii_case(&bundle.profile.runtime_administrator)
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
        || snapshot.timelock_minimum_delay_seconds < 72 * 60 * 60
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
    {
        return Err(
            "live snapshot does not match the approved profile or safety requirements".into(),
        );
    }
    verify_eip191(
        chain_key_challenge_hash(
            &bundle.manifest.release_id,
            &bundle.profile.expected_bridge_signer,
        ),
        &Approval {
            signer: snapshot.canister_bridge_signer.clone(),
            eip191_signature: snapshot.chain_key_eip191_signature.clone(),
        },
        &bundle.profile.expected_bridge_signer,
    )?;
    validate_rpc_rehearsal(bundle)?;
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
            verify_live(&bundle)?;
            render_release_inputs(
                &Path::new(&args[2]).join("profile.json"),
                Path::new(&args[3]),
                true,
                Some(&bundle.manifest_sha256),
            )?;
        }
        Some("validate-bundle") if args.len() == 4 && args[2] == "--offline" => {
            let bundle = validate_bundle(Path::new(&args[3]), false)?;
            println!("gate_a=pass manifest_sha256={}", bundle.manifest_sha256);
        }
        Some("verify-live") if args.len() == 3 => {
            let bundle = validate_bundle(Path::new(&args[2]), true)?;
            if bundle.manifest.test_only { return Err("Gate B rejects test-only bundles".into()); }
            verify_live(&bundle)?;
            println!("gate_b=pass manifest_sha256={}", bundle.manifest_sha256);
        }
        _ => return Err("usage: bridge-profile <derive|validate|validate-test> <json-file> | render-release-inputs <profile.json> <output-dir> | render-test-inputs <profile.json> <output-dir> | render-bundle-inputs <bundle-dir> <output-dir> | validate-bundle --offline <bundle-dir> | verify-live <bundle-dir>".into()),
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

    fn test_principal(seed: u8) -> String {
        Principal::self_authenticating([seed; 32]).to_text()
    }
    fn address(seed: u8) -> String {
        format!("0x{seed:040x}")
    }

    #[test]
    fn release_id_is_compatible_with_the_canister_challenge_domain() {
        assert!(valid_release_id("release-1"));
        assert!(valid_release_id("12345678"));
        assert!(!valid_release_id("short-1"));
        assert!(!valid_release_id("Release-1"));
        assert!(!valid_release_id("release_1"));
        assert!(!valid_release_id("release-1\naddress=0x00"));
        assert!(!valid_release_id(&"a".repeat(65)));
    }

    fn key_address(key: &SigningKey) -> String {
        let point = key.verifying_key().to_encoded_point(false);
        let mut k = Keccak::v256();
        k.update(&point.as_bytes()[1..]);
        let mut h = [0u8; 32];
        k.finalize(&mut h);
        format!("0x{}", hex(&h[12..]))
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
            ic_host: "https://icp-api.io".into(),
            base_rpc_url: "https://prod-one.example/base-mainnet".into(),
            bridge_contract: address(1),
            bsns_contract: address(8),
            deployment_block: 1,
            expected_bridge_signer: address(2),
            bridge_canister_wasm_sha256: "3".repeat(64),
            bridge_runtime_bytecode_sha256: "4".repeat(64),
            bsns_runtime_bytecode_sha256: "5".repeat(64),
            release_approver: address(7),
            ecdsa_key_name: "key_1".into(),
            ecdsa_derivation_path: vec!["KINIC-BASE-BRIDGE".into()],
            runtime_administrator: address(3),
            base_admin_wallet: address(4),
            timelock: Timelock {
                address: address(5),
                runtime_code_hash: format!("0x{}", "ab".repeat(32)),
                minimum_delay_seconds: 259_200,
                proposer: address(4),
                canceller: address(6),
                executor: address(4),
                external_admins: 0,
            },
            finance_administrator: test_principal(1),
            pause_principals: vec![test_principal(2), test_principal(3)],
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
                max_service_fee: 10_000_000,
                service_fee: 1_000_000,
                transaction_gas_limit: 100_000,
                max_fee_per_gas: 10,
                max_priority_fee_per_gas: 1,
                eth_floor_wei: 100_000_000,
                cycles_floor: 1,
                settlement_cycle_ceiling: 1,
            },
            rate_limits: RateLimits {
                deposit_window_seconds: 60,
                deposit_global: 30,
                deposit_per_principal: 3,
                settlement_window_seconds: 600,
                settlement_global: 60,
                settlement_per_principal: 6,
                settlement_per_record: 3,
            },
        }
    }

    #[test]
    fn conservative_derivation_uses_exact_boundaries() {
        let evidence = Evidence {
            ledger_fee: 100_000,
            mint_gas_used: vec![10_000; 100],
            acknowledgement_gas_used: vec![20_000; 100],
            refund_gas_used: vec![30_001; 100],
            base_fee_per_gas_30d: vec![10; 100],
            priority_fee_per_gas_30d: vec![2; 100],
            settlement_cycles: vec![1_000; 100],
            observed_daily_cycles: 10_000,
        };
        let result = derive(&evidence).unwrap();
        assert_eq!(result.transaction_gas_limit, 40_000);
        assert_eq!(result.max_fee_per_gas, 22);
        assert_eq!(result.eth_floor_wei, 88_000_000);
        assert_eq!(result.settlement_cycle_ceiling, 1_500);
        assert_eq!(result.cycles_floor, 300_000);
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
        profile.release_approver = profile.expected_bridge_signer.clone();
        assert!(validate_profile(&profile, true).is_err());
        let mut profile = valid_profile();
        profile.timelock.runtime_code_hash = format!("0x{}", "00".repeat(32));
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
        assert!(profile["parameters"]["eth_floor_wei"].is_string());
    }

    #[test]
    fn eip191_recovers_release_approver() {
        let key = SigningKey::from_bytes((&[7u8; 32]).into()).unwrap();
        let signer = key_address(&key);
        let hash = [9u8; 32];
        let mut k = Keccak::v256();
        k.update(b"\x19Ethereum Signed Message:\n32");
        k.update(&hash);
        let mut digest = [0u8; 32];
        k.finalize(&mut digest);
        let (signature, recovery) = key.sign_prehash_recoverable(&digest).unwrap();
        let mut bytes = signature.to_bytes().to_vec();
        bytes.push(recovery.to_byte() + 27);
        let approval = Approval {
            signer: signer.clone(),
            eip191_signature: format!("0x{}", hex(&bytes)),
        };
        verify_eip191(hash, &approval, &signer).unwrap();
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
        assert_eq!(
            manifest["profile_file_sha256"],
            hex(&Sha256::digest(fs::read(&profile_path).unwrap()))
        );
        let canister: Value = read_json(&first.join("canister-init.json")).unwrap();
        assert_eq!(canister["evm_rpc_canister_id"], OFFICIAL_EVM_RPC_CANISTER);
        assert_eq!(canister["install_paused"], true);
        let constructors: Value = read_json(&first.join("contract-constructor-args.json")).unwrap();
        assert_eq!(
            constructors["bridge"][6],
            valid_profile().timelock.runtime_code_hash
        );
        let ui: Value = read_json(&first.join("ui-runtime-profile.json")).unwrap();
        assert_eq!(ui["evmRpcCanisterId"], OFFICIAL_EVM_RPC_CANISTER);
        assert_eq!(
            ui["rpcProviderUrlsSha256"],
            format!(
                "0x{}",
                hex(&Sha256::digest(
                    br#"["https://prod-one.example/base-mainnet","https://prod-two.example/base-mainnet","https://prod-three.example/base-mainnet"]"#
                ))
            )
        );
        fs::remove_dir_all(root).unwrap();
    }

    fn sign_approval(key: &SigningKey, hash: [u8; 32], signer: String) -> Approval {
        let mut k = Keccak::v256();
        k.update(b"\x19Ethereum Signed Message:\n32");
        k.update(&hash);
        let mut digest = [0u8; 32];
        k.finalize(&mut digest);
        let (signature, recovery) = key.sign_prehash_recoverable(&digest).unwrap();
        let mut bytes = signature.to_bytes().to_vec();
        bytes.push(recovery.to_byte() + 27);
        Approval {
            signer,
            eip191_signature: format!("0x{}", hex(&bytes)),
        }
    }

    #[test]
    fn bundle_gate_validates_hashes_slo_signature_and_live_snapshot() {
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
        let key = SigningKey::from_bytes((&[7u8; 32]).into()).unwrap();
        let bridge_key = SigningKey::from_bytes((&[8u8; 32]).into()).unwrap();
        let canceller_key = SigningKey::from_bytes((&[9u8; 32]).into()).unwrap();
        let runtime_key = SigningKey::from_bytes((&[10u8; 32]).into()).unwrap();
        let base_admin_key = SigningKey::from_bytes((&[11u8; 32]).into()).unwrap();
        let signer = key_address(&key);
        let now = now_unix().unwrap();
        let mut profile = valid_profile();
        profile.release_approver = signer.clone();
        profile.expected_bridge_signer = key_address(&bridge_key);
        profile.timelock.canceller = key_address(&canceller_key);
        profile.runtime_administrator = key_address(&runtime_key);
        profile.base_admin_wallet = key_address(&base_admin_key);
        profile.timelock.proposer = profile.base_admin_wallet.clone();
        profile.timelock.executor = profile.base_admin_wallet.clone();
        profile.bridge_canister_wasm_sha256 = hex(&Sha256::digest(b"wasm"));
        profile.bridge_runtime_bytecode_sha256 = hex(&Sha256::digest(b"runtime"));
        profile.deployment_block = 0;
        let controller = profile.root_canister_id.clone();
        let chain_key_approval = sign_approval(
            &bridge_key,
            chain_key_challenge_hash("release-1", &profile.expected_bridge_signer),
            profile.expected_bridge_signer.clone(),
        );
        let mut snapshot = SignerSnapshot {
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
            chain_key_eip191_signature: chain_key_approval.eip191_signature,
            base_runtime_administrator: profile.runtime_administrator.clone(),
            bridge_runtime_bytecode_sha256: "1".repeat(64),
            expected_bridge_runtime_bytecode_sha256: "1".repeat(64),
            bridge_canister_wasm_sha256: profile.bridge_canister_wasm_sha256.clone(),
            bridge_canister_id: profile.bridge_canister_id.clone(),
            timelock_address: profile.timelock.address.clone(),
            timelock_runtime_code_hash: profile.timelock.runtime_code_hash.clone(),
            bridge_approved_timelock_runtime_code_hash: profile.timelock.runtime_code_hash.clone(),
            timelock_minimum_delay_seconds: 259_200,
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
        };
        let mut ceremony = Ceremony {
            release_approver: signer.clone(),
            base_admin: profile.base_admin_wallet.clone(),
            timelock_canceller: profile.timelock.canceller.clone(),
            runtime_administrator: profile.runtime_administrator.clone(),
            backup_restore_verified: true,
            contains_secret_material: false,
            role_challenges: [
                ("release_approver", &key, profile.release_approver.clone()),
                (
                    "base_admin",
                    &base_admin_key,
                    profile.base_admin_wallet.clone(),
                ),
                (
                    "timelock_canceller",
                    &canceller_key,
                    profile.timelock.canceller.clone(),
                ),
                (
                    "runtime_administrator",
                    &runtime_key,
                    profile.runtime_administrator.clone(),
                ),
            ]
            .into_iter()
            .map(|(role, key, address)| {
                let signed = sign_approval(
                    key,
                    role_challenge_hash("release-1", role, &address),
                    address.clone(),
                );
                RoleChallenge {
                    role: role.into(),
                    address,
                    custodian_id: format!("custodian-{role}"),
                    device_class: "hardware-wallet".into(),
                    device_failure_domain: format!("device-{role}"),
                    eip191_signature: signed.eip191_signature,
                }
            })
            .collect(),
        };
        let canceller_index = ceremony
            .role_challenges
            .iter()
            .position(|value| value.role == "timelock_canceller")
            .unwrap();
        let original_domain = ceremony.role_challenges[canceller_index]
            .device_failure_domain
            .clone();
        ceremony.role_challenges[canceller_index].device_failure_domain = ceremony
            .role_challenges
            .iter()
            .find(|value| value.role == "base_admin")
            .unwrap()
            .device_failure_domain
            .clone();
        assert!(validate_role_challenges(&ceremony, &profile, "release-1").is_err());
        ceremony.role_challenges[canceller_index].device_failure_domain = original_domain;
        let test_helper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/evm-rpc-rehearsal/test_rehearsal.py");
        let python = r###"import importlib.util,json,os,sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('fixture',sys.argv[1]); m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
m.SIGNER=sys.argv[3]; m.SHA_A=sys.argv[4]; m.SHA_B=sys.argv[5]; binding=m.rehearsal.validate_config(m.config()); value=m.manifest(binding); value['source']['revision']='a'*40; value['source']['source_tree_sha256']='2'*64
root=Path(sys.argv[2]).parent; tool=root/'tool'
os.environ['PATH']=str(root)+os.pathsep+os.environ.get('PATH','')
for scenario,item in m.all_evidence(binding).items():
 m.rehearsal.now=lambda: item['observed_at']
 fault_fields={'configured_provider_count','required_provider_threshold','injected_provider_failures','fault_injection_reference'}; command_details={k:v for k,v in item['details'].items() if scenario not in {'single_provider_failure','quorum_loss'} or k not in fault_fields}; audit_event=None
 if item['canister_decision'] is not None:
  timestamp_ns=int(m.rehearsal.datetime.fromisoformat(item['observed_at'].replace('Z','+00:00')).timestamp()*1_000_000_000); audit_event={'sequence':7,'timestamp_ns':timestamp_ns,'kind':{'EvmRpcDecision':item['canister_decision']}}
 payload=json.dumps({**command_details,'canister_audit':item['canister_audit'],'audit_events':[audit_event] if audit_event else []},separators=(',',':')); tool.write_text("#!/bin/sh\nprintf '%s' '"+payload+"'\n"); tool.chmod(0o755)
 for reference in item['artifacts']:
  kind=reference['kind']; output=root/reference['path']
  if kind=='fault':
   m.write_fault_artifact(item,scenario,output); reference['sha256']=m.rehearsal.hashlib.sha256(output.read_bytes()).hexdigest(); continue
  executable=root/('cast' if kind=='base' else 'dfx')
  if kind=='base': executable.write_text("#!/bin/sh\nif [ \"$1\" = \"chain-id\" ]; then printf '84532\\n'; else printf '%s' '"+payload+"'; fi\n")
  else: executable.write_bytes(tool.read_bytes())
  executable.chmod(0o755)
  if kind=='base': command=['cast','receipt',m.H32_A]
  elif kind=='module': command=['dfx','canister','status',binding['bridge_canister_id'],'--network','ic','--output','json']
  else:
   method='icrc1_fee' if kind=='ledger' else ('get_audit_events' if kind=='audit' else 'get_bridge_status')
   command=['dfx','canister','call',binding['ledger_canister_id'] if kind=='ledger' else binding['bridge_canister_id'],method,'()','--network','ic','--output','json']
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
            routing_sha256: profile.monitoring.routing_sha256.clone(),
            fault_started_at_unix: now - 100,
            detected_at_unix: now - 99,
            acknowledged_at_unix: now - 98,
            base_paused_at_unix: now - 97,
            ic_paused_at_unix: now - 96,
            base_pause_reference: "0xabc".into(),
            ic_pause_reference: "block-1".into(),
        };
        let mut docs = vec![
            ("profile.json", serde_json::to_vec(&profile).unwrap()),
            (
                "signer-snapshot.json",
                serde_json::to_vec(&snapshot).unwrap(),
            ),
            ("ceremony.json", serde_json::to_vec(&ceremony).unwrap()),
            ("rpc-e2e.json", fs::read(root.join("rpc-e2e.json")).unwrap()),
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
            schema_version: 1,
            release_id: "release-1".into(),
            test_only: false,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            created_at_unix: manifest_created,
            expires_at_unix: manifest_created + 100,
            parent_gate_a_manifest_sha256: None,
            artifacts: gate_a_artifacts,
            approval: None,
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
        let gate_a = validate_bundle(&root, false).unwrap();
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
        let mut manifest = ReleaseManifest {
            schema_version: 1,
            release_id: "release-1".into(),
            test_only: false,
            source_revision: "a".repeat(40),
            source_tree_sha256: "2".repeat(64),
            created_at_unix: manifest_created,
            expires_at_unix: manifest_created + 100,
            parent_gate_a_manifest_sha256: Some(gate_a.manifest_sha256),
            artifacts,
            approval: None,
        };
        manifest.approval = Some(sign_approval(
            &key,
            unsigned_manifest_hash(&manifest).unwrap(),
            signer,
        ));
        fs::write(
            root.join("release-manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let bundle = validate_bundle(&root, true).unwrap();
        verify_live(&bundle).unwrap();
        let valid_profile_bytes = fs::read(root.join("profile.json")).unwrap();
        let valid_receipt_bytes = fs::read(root.join("gate-a-receipt.json")).unwrap();
        let valid_manifest_bytes = fs::read(root.join("release-manifest.json")).unwrap();
        profile.finance_administrator = test_principal(30);
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
        drifted_manifest.approval = Some(sign_approval(
            &key,
            unsigned_manifest_hash(&drifted_manifest).unwrap(),
            profile.release_approver.clone(),
        ));
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
        incomplete["complete"] = Value::Bool(false);
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
