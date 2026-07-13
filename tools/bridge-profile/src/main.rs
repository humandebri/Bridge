use candid::Principal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, env, fs, process};

const KINIC_LEDGER: &str = "73mez-iiaaa-aaaaq-aaasq-cai";
const KINIC_INDEX: &str = "7vojr-tyaaa-aaaaq-aaatq-cai";
const KINIC_ROOT: &str = "7jkta-eyaaa-aaaaq-aaarq-cai";
const KINIC_GOVERNANCE: &str = "74ncn-fqaaa-aaaaq-aaasa-cai";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    status: String,
    environment: String,
    test_assets_only: bool,
    chain_id: u64,
    ledger_canister_id: String,
    index_canister_id: String,
    root_canister_id: String,
    governance_principal: String,
    decimals: u8,
    bridge_contract: String,
    bridge_signer: String,
    ecdsa_key_name: String,
    ecdsa_derivation_path: Vec<String>,
    runtime_administrator: String,
    base_admin_wallet: String,
    timelock: Timelock,
    finance_administrator: String,
    pause_principals: Vec<String>,
    fee_recipient: String,
    rpc_providers: Vec<String>,
    parameters: Parameters,
    evidence_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Timelock {
    address: String,
    minimum_delay_seconds: u64,
    proposer: String,
    canceller: String,
    executor: String,
    external_admins: u8,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Parameters {
    ledger_fee: u128,
    per_deposit_limit: u128,
    mint_throughput_limit: u128,
    mint_window_duration_seconds: u64,
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
        .ok_or_else(|| "missing gas samples".to_string())?;
    let gas_with_margin = checked_percent(gas_max, 130, 100)?;
    let transaction_gas_limit = gas_with_margin
        .checked_add(999)
        .map(|value| value / 1_000 * 1_000)
        .ok_or_else(|| "gas limit overflow".to_string())?;
    let max_priority_fee_per_gas = percentile(&evidence.priority_fee_per_gas_30d, 95, 100)?;
    let max_fee_per_gas = percentile(&evidence.base_fee_per_gas_30d, 99, 100)?
        .checked_mul(2)
        .and_then(|value| value.checked_add(max_priority_fee_per_gas))
        .ok_or_else(|| "fee cap overflow".to_string())?;
    let eth_floor_wei = transaction_gas_limit
        .checked_mul(max_fee_per_gas)
        .and_then(|value| value.checked_mul(100))
        .ok_or_else(|| "ETH floor overflow".to_string())?;
    let settlement_cycle_ceiling = checked_percent(
        *evidence.settlement_cycles.iter().max().unwrap_or(&0),
        150,
        100,
    )?;
    let cycles_floor = evidence
        .observed_daily_cycles
        .checked_mul(30)
        .ok_or_else(|| "cycles floor overflow".to_string())?;
    Ok(DerivedParameters {
        ledger_fee: evidence.ledger_fee,
        max_service_fee: evidence
            .ledger_fee
            .checked_mul(100)
            .ok_or_else(|| "maximum service fee overflow".to_string())?,
        service_fee: evidence
            .ledger_fee
            .checked_mul(10)
            .ok_or_else(|| "service fee overflow".to_string())?,
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

fn validate(profile: &Profile) -> Result<(), String> {
    if profile.status != "validated" {
        return Err("profile status is not validated".into());
    }
    let expected_chain = match profile.environment.as_str() {
        "mainnet-candidate" => 8453,
        "base-sepolia" => 84532,
        _ => return Err("unsupported environment".into()),
    };
    if profile.chain_id != expected_chain || profile.decimals != 8 {
        return Err("KINIC or chain identity mismatch".into());
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
            .any(|component| component.is_empty() || component.len() > 128)
    {
        return Err("invalid threshold ECDSA key configuration".into());
    }
    let addresses = [
        &profile.bridge_contract,
        &profile.bridge_signer,
        &profile.runtime_administrator,
        &profile.base_admin_wallet,
        &profile.timelock.address,
    ];
    if addresses.iter().any(|value| !evm_address(value)) {
        return Err("invalid EVM role address".into());
    }
    let mut unique_addresses = BTreeSet::new();
    for value in addresses {
        if !unique_addresses.insert(value.to_ascii_lowercase()) {
            return Err("EVM roles must be distinct".into());
        }
    }
    if profile.timelock.minimum_delay_seconds != 72 * 60 * 60
        || profile.timelock.external_admins != 0
        || profile.timelock.proposer.to_lowercase() != profile.base_admin_wallet.to_lowercase()
        || profile.timelock.canceller.to_lowercase() != profile.base_admin_wallet.to_lowercase()
        || profile.timelock.executor.to_lowercase() != profile.base_admin_wallet.to_lowercase()
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
    if profile.pause_principals.len() != 3 || profile.rpc_providers.len() != 3 {
        return Err("exactly three pause principals and RPC providers are required".into());
    }
    if profile.rpc_providers.iter().collect::<BTreeSet<_>>().len() != 3 {
        return Err("RPC providers must be distinct".into());
    }
    if profile
        .rpc_providers
        .iter()
        .any(|url| !url.starts_with("https://") || url.contains('@') || url.contains("api_key"))
    {
        return Err("RPC providers must be credential-free HTTPS URLs".into());
    }
    let p = &profile.parameters;
    let _window_boundary_max = p
        .mint_throughput_limit
        .checked_mul(2)
        .ok_or_else(|| "mint window boundary overflow".to_string())?;
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
                .and_then(|value| value.checked_mul(100))
                .ok_or_else(|| "reserve overflow".to_string())?
        || p.cycles_floor == 0
        || p.settlement_cycle_ceiling == 0
    {
        return Err("unsafe or inconsistent parameter set".into());
    }
    if profile.evidence_sha256.len() != 64
        || !profile
            .evidence_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || profile.evidence_sha256.bytes().all(|byte| byte == b'0')
    {
        return Err("invalid evidence hash".into());
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: bridge-profile <derive|validate> <json-file>".into());
    }
    let bytes = fs::read(&args[2]).map_err(|error| error.to_string())?;
    match args[1].as_str() {
        "derive" => {
            let evidence: Evidence =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            let parameters = derive(&evidence)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&parameters).map_err(|error| error.to_string())?
            );
        }
        "validate" => {
            let profile: Profile =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            validate(&profile)?;
            let canonical = serde_json::to_vec(&profile).map_err(|error| error.to_string())?;
            println!("{}", hex(&Sha256::digest(canonical)));
        }
        _ => return Err("unknown command".into()),
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
    fn evidence_rejects_missing_samples_and_overflow() {
        let evidence = Evidence {
            ledger_fee: u128::MAX,
            mint_gas_used: vec![],
            acknowledgement_gas_used: vec![],
            refund_gas_used: vec![],
            base_fee_per_gas_30d: vec![],
            priority_fee_per_gas_30d: vec![],
            settlement_cycles: vec![],
            observed_daily_cycles: u128::MAX,
        };
        assert!(derive(&evidence).is_err());
    }

    fn test_principal(seed: u8) -> String {
        Principal::self_authenticating([seed; 32]).to_text()
    }

    fn valid_profile() -> Profile {
        Profile {
            status: "validated".into(),
            environment: "mainnet-candidate".into(),
            test_assets_only: false,
            chain_id: 8453,
            ledger_canister_id: KINIC_LEDGER.into(),
            index_canister_id: KINIC_INDEX.into(),
            root_canister_id: KINIC_ROOT.into(),
            governance_principal: KINIC_GOVERNANCE.into(),
            decimals: 8,
            bridge_contract: "0x0000000000000000000000000000000000000001".into(),
            bridge_signer: "0x0000000000000000000000000000000000000002".into(),
            ecdsa_key_name: "key_1".into(),
            ecdsa_derivation_path: vec!["KINIC-BASE-BRIDGE".into()],
            runtime_administrator: "0x0000000000000000000000000000000000000003".into(),
            base_admin_wallet: "0x0000000000000000000000000000000000000004".into(),
            timelock: Timelock {
                address: "0x0000000000000000000000000000000000000005".into(),
                minimum_delay_seconds: 259_200,
                proposer: "0x0000000000000000000000000000000000000004".into(),
                canceller: "0x0000000000000000000000000000000000000004".into(),
                executor: "0x0000000000000000000000000000000000000004".into(),
                external_admins: 0,
            },
            finance_administrator: test_principal(1),
            pause_principals: vec![test_principal(2), test_principal(3), test_principal(4)],
            fee_recipient: test_principal(5),
            rpc_providers: vec![
                "https://one.example".into(),
                "https://two.example".into(),
                "https://three.example".into(),
            ],
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
            evidence_sha256: "1".repeat(64),
        }
    }

    #[test]
    fn profile_rejects_role_overlap_chain_drift_and_draft_status() {
        let mut profile = valid_profile();
        assert!(validate(&profile).is_ok());
        profile.base_admin_wallet = profile.bridge_signer.clone();
        assert!(validate(&profile).is_err());
        let mut profile = valid_profile();
        profile.chain_id = 84532;
        assert!(validate(&profile).is_err());
        let mut profile = valid_profile();
        profile.status = "draft".into();
        assert!(validate(&profile).is_err());
    }

    #[test]
    fn legacy_safe_and_neuron_evidence_fields_are_rejected() {
        let mut profile = serde_json::to_value(valid_profile()).unwrap();
        profile.as_object_mut().unwrap().insert(
            "safe".into(),
            serde_json::json!({"address": "0x1", "threshold": 2, "owners": []}),
        );
        assert!(serde_json::from_value::<Profile>(profile).is_err());

        let evidence = Evidence {
            ledger_fee: 100_000,
            mint_gas_used: vec![1; 100],
            acknowledgement_gas_used: vec![1; 100],
            refund_gas_used: vec![1; 100],
            base_fee_per_gas_30d: vec![1],
            priority_fee_per_gas_30d: vec![1],
            settlement_cycles: vec![1; 100],
            observed_daily_cycles: 1,
        };
        let mut value = serde_json::to_value(evidence).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("staked_supply".into(), serde_json::json!(1));
        assert!(serde_json::from_value::<Evidence>(value).is_err());
    }

    #[test]
    fn profile_rejects_invalid_fixed_limits() {
        let mut profile = valid_profile();
        profile.parameters.per_deposit_limit = 2;
        assert!(validate(&profile).is_err());

        let mut profile = valid_profile();
        profile.parameters.mint_throughput_limit = u128::MAX;
        assert!(validate(&profile).is_err());

        let mut profile = valid_profile();
        profile.parameters.mint_window_duration_seconds = 0;
        assert!(validate(&profile).is_err());
    }
}
