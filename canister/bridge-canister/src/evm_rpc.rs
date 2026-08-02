use crate::config::BridgeInitArgs;
use async_trait::async_trait;
use bridge_core::{Amount, BaseMintSnapshot, FinalizedObservationRecord};
use candid::{utils::ArgumentEncoder, CandidType, Principal};
use evm_rpc_client::{CandidResponseConverter, EvmRpcClient, NoRetry};
use evm_rpc_types::{
    Block, BlockTag, ConsensusStrategy, GetTransactionCountArgs, Hex20, Hex32, MultiRpcResult,
    Nat256, RpcApi, RpcError, RpcServices, TransactionReceipt,
};
use ic_canister_runtime::{IcError, Runtime};
use ic_cdk::call::Call;
use serde::de::DeserializeOwned;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use tiny_keccak::{Hasher, Keccak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationError {
    Inconsistent,
    Rpc,
    InvalidResponse,
    Overflow,
    BaseStateMismatch,
    ChainIdMismatch,
    TransactionPending,
    TransactionReverted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalizedObservation {
    pub chain_id: u64,
    pub block_number: u64,
    pub block_hash: [u8; 32],
    pub observed_at_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObservedBridgeIdentity {
    pub signer: [u8; 20],
    pub runtime_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcAuditEvidence {
    pub evm_rpc_canister_id: Principal,
    pub call_method: String,
    pub request_digest: [u8; 32],
    pub quorum_response_digest: [u8; 32],
    pub finalized_block_number: u64,
    pub finalized_block_hash: [u8; 32],
    pub transaction_hash: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpcDecisionKind {
    QuorumContinued,
    QuorumLoss,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpcDecisionEvidence {
    pub kind: RpcDecisionKind,
    pub operation: String,
    pub configured_provider_count: u8,
    pub required_threshold: u8,
    pub stop_reason: Option<String>,
    pub ledger_call_performed: bool,
    pub bridge_operation_continued: bool,
    pub deposits_paused: bool,
    pub automatically_resigned: bool,
    pub transaction_hash: Option<[u8; 32]>,
}

pub fn quorum_continued_decision(
    operation: impl Into<String>,
    transaction_hash: Option<[u8; 32]>,
    ledger_call_performed: bool,
) -> RpcDecisionEvidence {
    RpcDecisionEvidence {
        kind: RpcDecisionKind::QuorumContinued,
        operation: operation.into(),
        configured_provider_count: 3,
        required_threshold: 2,
        stop_reason: None,
        ledger_call_performed,
        bridge_operation_continued: true,
        deposits_paused: false,
        automatically_resigned: false,
        transaction_hash,
    }
}

pub fn quorum_loss_decision(
    operation: impl Into<String>,
    transaction_hash: Option<[u8; 32]>,
) -> RpcDecisionEvidence {
    RpcDecisionEvidence {
        kind: RpcDecisionKind::QuorumLoss,
        operation: operation.into(),
        configured_provider_count: 3,
        required_threshold: 2,
        stop_reason: Some("RpcInconsistent".to_owned()),
        ledger_call_performed: false,
        bridge_operation_continued: false,
        deposits_paused: false,
        automatically_resigned: false,
        transaction_hash,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletedFinalizedObservation {
    pub finalized: FinalizedObservation,
    pub snapshot: BridgeSnapshot,
    pub bridge_identity: ObservedBridgeIdentity,
    pub rpc_audit: RpcAuditEvidence,
}

pub fn stable_observation(
    observation: &CompletedFinalizedObservation,
) -> FinalizedObservationRecord {
    stable_observation_parts(&observation.finalized, &observation.bridge_identity)
}

fn stable_observation_parts(
    finalized: &FinalizedObservation,
    bridge_identity: &ObservedBridgeIdentity,
) -> FinalizedObservationRecord {
    FinalizedObservationRecord {
        chain_id: finalized.chain_id,
        block_number: finalized.block_number,
        block_hash: finalized.block_hash,
        observed_at_ns: finalized.observed_at_ns,
        bridge_signer: bridge_identity.signer,
        runtime_sha256: bridge_identity.runtime_sha256,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedWithdrawal {
    pub id: [u8; 32],
    pub amount: u128,
    pub max_service_fee: u128,
    pub charged_service_fee: u128,
    pub amount_out: u128,
    pub owner: Vec<u8>,
    pub subaccount: [u8; 32],
    pub requester: [u8; 20],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeSnapshot {
    pub mint: BaseMintSnapshot,
    pub bridge_signer: [u8; 20],
    pub mint_authorization_epoch: u64,
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentWithdrawal {
    pub requester: [u8; 20],
    pub amount: u128,
    pub max_service_fee: u128,
    pub charged_service_fee: u128,
    pub amount_out: u128,
    pub owner: Vec<u8>,
    pub subaccount: [u8; 32],
    pub status: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryTarget {
    Deposit([u8; 32]),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryBaseState {
    DepositProcessed(bool),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryObservation {
    pub finalized: FinalizedObservation,
    pub snapshot: BridgeSnapshot,
    pub bridge_identity: ObservedBridgeIdentity,
    pub state: RecoveryBaseState,
    pub rpc_audit: RpcAuditEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositPreflightObservation {
    pub finalized: FinalizedObservation,
    pub snapshot: BridgeSnapshot,
    pub bridge_identity: ObservedBridgeIdentity,
    pub processed: bool,
    pub runtime_attestation_refreshed: bool,
    pub rpc_audit: RpcAuditEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifiedWithdrawalOutcome {
    Missing,
    Pending {
        receipt_block_number: u64,
    },
    Reverted {
        receipt_block_number: u64,
        finalized_head_block_number: u64,
    },
    Confirmed {
        withdrawal: ObservedWithdrawal,
        snapshot: Box<BridgeSnapshot>,
        rpc_audit: Box<RpcAuditEvidence>,
        stable_observation: Box<FinalizedObservationRecord>,
        receipt_block_number: u64,
        finalized_head_block_number: u64,
    },
}

const SMALL_RESPONSE_BYTES: u64 = 4 * 1024;
const DEPOSIT_PREFLIGHT_RPC_CALLS: [&str; 4] = [
    "eth_chainId",
    "eth_getBlockByNumber(finalized)",
    "eth_call(isDepositProcessed,EIP-1898-finalized-hash)",
    "eth_call(bridgeSnapshot,EIP-1898-finalized-hash)",
];
// `eth_getBlockByNumber` includes the transaction-hash vector even when full
// transaction objects are disabled. Busy Base blocks can therefore be much
// larger than the fixed-size header fields.
const BLOCK_RESPONSE_BYTES: u64 = 16 * 1024;
const RECEIPT_RESPONSE_BYTES: u64 = 32 * 1024;
const EVM_RPC_TIMEOUT_SECONDS: u32 = 30;

#[derive(Clone, Copy, Debug, Default)]
struct BoundedRuntime;

#[async_trait]
impl Runtime for BoundedRuntime {
    async fn update_call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
        cycles: u128,
    ) -> Result<Out, IcError>
    where
        In: ArgumentEncoder + Send,
        Out: CandidType + DeserializeOwned,
    {
        if !matches!(
            ic_cdk::api::canister_status(),
            ic_cdk::api::CanisterStatusCode::Running
        ) {
            return Err(IcError::CallPerformFailed);
        }
        Call::bounded_wait(id, method)
            .change_timeout(EVM_RPC_TIMEOUT_SECONDS)
            .with_args(&args)
            .with_cycles(cycles)
            .await
            .map_err(IcError::from)
            .and_then(|response| response.candid::<Out>().map_err(IcError::from))
    }

    async fn query_call<In, Out>(
        &self,
        id: Principal,
        method: &str,
        args: In,
    ) -> Result<Out, IcError>
    where
        In: ArgumentEncoder + Send,
        Out: CandidType + DeserializeOwned,
    {
        if !matches!(
            ic_cdk::api::canister_status(),
            ic_cdk::api::CanisterStatusCode::Running
        ) {
            return Err(IcError::CallPerformFailed);
        }
        Call::bounded_wait(id, method)
            .change_timeout(EVM_RPC_TIMEOUT_SECONDS)
            .with_args(&args)
            .await
            .map_err(IcError::from)
            .and_then(|response| response.candid::<Out>().map_err(IcError::from))
    }
}

fn client(args: &BridgeInitArgs) -> EvmRpcClient<BoundedRuntime, CandidResponseConverter, NoRetry> {
    let services = if args.custom_evm_rpc_urls.is_empty() {
        RpcServices::BaseMainnet(None)
    } else {
        RpcServices::Custom {
            chain_id: args.base_chain_id,
            services: args
                .custom_evm_rpc_urls
                .iter()
                .map(|url| RpcApi {
                    url: url.clone(),
                    headers: None,
                })
                .collect(),
        }
    };
    EvmRpcClient::builder(BoundedRuntime, args.evm_rpc_canister_id)
        .with_rpc_sources(services)
        .with_consensus_strategy(ConsensusStrategy::Threshold {
            total: Some(3),
            min: 2,
        })
        .build()
}

fn selector(signature: &str) -> [u8; 4] {
    let mut hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut hash);
    hash[..4].try_into().expect("four byte selector")
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    result
}

fn parse_u128(value: &str) -> Result<u128, ObservationError> {
    let value = value
        .trim_matches('"')
        .strip_prefix("0x")
        .ok_or(ObservationError::InvalidResponse)?;
    if value.len() > 64 {
        return Err(ObservationError::InvalidResponse);
    }
    let significant = value.trim_start_matches('0');
    if significant.len() > 32 {
        return Err(ObservationError::Overflow);
    }
    if significant.is_empty() {
        return Ok(0);
    }
    u128::from_str_radix(significant, 16).map_err(|_| ObservationError::InvalidResponse)
}

async fn eth_call_at_observation(
    args: &BridgeInitArgs,
    calldata: &[u8],
    observation: FinalizedObservation,
) -> Result<String, ObservationError> {
    let request = eth_call_request(&args.bridge_contract, calldata, observation);
    match client(args)
        .multi_request(request)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(value)) => Ok(value),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

fn eth_call_request(
    bridge_contract: &[u8],
    calldata: &[u8],
    observation: FinalizedObservation,
) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_call",
        "params": [{
            "to": format!("0x{}", hex(bridge_contract)),
            "data": format!("0x{}", hex(calldata)),
        }, {
            "blockHash": format!("0x{}", hex(&observation.block_hash)),
            "requireCanonical": true,
        }],
    })
}

pub async fn bridge_snapshot(
    args: &BridgeInitArgs,
) -> Result<CompletedFinalizedObservation, ObservationError> {
    let finalized = finalized_observation(args).await?;
    bridge_snapshot_at(args, finalized).await
}

pub async fn bridge_snapshot_at(
    args: &BridgeInitArgs,
    finalized: FinalizedObservation,
) -> Result<CompletedFinalizedObservation, ObservationError> {
    if finalized.chain_id != args.base_chain_id {
        return Err(ObservationError::ChainIdMismatch);
    }
    let (snapshot, bridge_identity) = observe_bridge_at(args, finalized).await?;
    Ok(CompletedFinalizedObservation {
        finalized,
        snapshot,
        bridge_identity,
        rpc_audit: rpc_audit_evidence(args, finalized, snapshot, bridge_identity, None, None, None),
    })
}

pub async fn recovery_observation(
    args: &BridgeInitArgs,
    target: RecoveryTarget,
) -> Result<RecoveryObservation, ObservationError> {
    let finalized = finalized_observation(args).await?;
    let state_call = async {
        match target {
            RecoveryTarget::Deposit(deposit_id) => {
                let mut calldata = selector("isDepositProcessed(bytes32)").to_vec();
                calldata.extend_from_slice(&deposit_id);
                let value = eth_call_at_observation(args, &calldata, finalized).await?;
                Ok(RecoveryBaseState::DepositProcessed(decode_bool_word(
                    &value,
                )?))
            }
        }
    };
    let (state, bridge) = futures::join!(state_call, observe_bridge_at(args, finalized));
    let state = state?;
    let (snapshot, bridge_identity) = bridge?;
    let rpc_audit =
        recovery_rpc_audit_evidence(args, finalized, snapshot, bridge_identity, target, &state);
    Ok(RecoveryObservation {
        finalized,
        snapshot,
        bridge_identity,
        state,
        rpc_audit,
    })
}

pub async fn deposit_preflight_observation(
    args: &BridgeInitArgs,
    deposit_id: [u8; 32],
    refresh_generation: u64,
    runtime_attested: bool,
) -> Result<DepositPreflightObservation, ObservationError> {
    let finalized = finalized_observation(args).await?;
    let mut calldata = selector("isDepositProcessed(bytes32)").to_vec();
    calldata.extend_from_slice(&deposit_id);
    let expected_runtime: [u8; 32] = args
        .expected_bridge_runtime_sha256
        .as_slice()
        .try_into()
        .map_err(|_| ObservationError::InvalidResponse)?;
    let (processed, snapshot, bridge_identity, runtime_attestation_refreshed) = if runtime_attested
    {
        let (processed, snapshot) = futures::join!(
            eth_call_at_observation(args, &calldata, finalized),
            observe_bridge_snapshot_at(args, finalized)
        );
        let snapshot = snapshot?;
        (
            decode_bool_word(&processed?)?,
            snapshot,
            ObservedBridgeIdentity {
                signer: snapshot.bridge_signer,
                runtime_sha256: expected_runtime,
            },
            false,
        )
    } else {
        let (processed, snapshot, runtime) = futures::join!(
            eth_call_at_observation(args, &calldata, finalized),
            observe_bridge_snapshot_at(args, finalized),
            bridge_runtime_at_observation(args, finalized)
        );
        let snapshot = snapshot?;
        let runtime = runtime?;
        if !runtime_matches_expected(&runtime, expected_runtime) {
            return Err(ObservationError::BaseStateMismatch);
        }
        (
            decode_bool_word(&processed?)?,
            snapshot,
            ObservedBridgeIdentity {
                signer: snapshot.bridge_signer,
                runtime_sha256: expected_runtime,
            },
            true,
        )
    };
    let rpc_audit = deposit_preflight_rpc_audit_evidence(
        args,
        finalized,
        snapshot,
        deposit_id,
        refresh_generation,
        processed,
        runtime_attestation_refreshed,
    );
    Ok(DepositPreflightObservation {
        finalized,
        snapshot,
        bridge_identity,
        processed,
        runtime_attestation_refreshed,
        rpc_audit,
    })
}

pub async fn exact_mint_evidence(
    args: &BridgeInitArgs,
    authorization: &bridge_core::MintAuthorizationRecord,
    finalized: FinalizedObservation,
) -> Result<bridge_core::MintFinalizationEvidence, ObservationError> {
    exact_mint_evidence_inner(args, authorization, finalized, None).await
}

async fn exact_mint_evidence_inner(
    args: &BridgeInitArgs,
    authorization: &bridge_core::MintAuthorizationRecord,
    finalized: FinalizedObservation,
    expected_transaction_hash: Option<[u8; 32]>,
) -> Result<bridge_core::MintFinalizationEvidence, ObservationError> {
    if finalized.chain_id != args.base_chain_id {
        return Err(ObservationError::ChainIdMismatch);
    }
    let finalized_head_block_number = finalized.block_number;
    let mut event_topic = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(b"DepositMinted(bytes32,address,bytes32,uint256,uint256,uint256)");
    hasher.finalize(&mut event_topic);
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getLogs",
        "params": [{
            "address": format!("0x{}", hex(&args.bridge_contract)),
            "fromBlock": format!("0x{:x}", authorization.origin.finalized_block_number),
            "toBlock": format!("0x{finalized_head_block_number:x}"),
            "topics": [
                format!("0x{}", hex(&event_topic)),
                format!("0x{}", hex(&authorization.authorization.deposit_id))
            ]
        }]
    });
    let rpc_request_digest = Sha256::digest(
        serde_json::to_vec(&request).map_err(|_| ObservationError::InvalidResponse)?,
    )
    .into();
    let value = match client(args)
        .multi_request(request)
        .with_response_size_estimate(RECEIPT_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(value)) => value,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    let rpc_response_digest = Sha256::digest(value.as_bytes()).into();
    let logs: Vec<evm_rpc_types::LogEntry> =
        serde_json::from_str(&value).map_err(|_| ObservationError::InvalidResponse)?;
    if logs.len() != 1 {
        return Err(ObservationError::BaseStateMismatch);
    }
    let log = &logs[0];
    let bridge = Hex20::from_str(&format!("0x{}", hex(&args.bridge_contract)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    if log.removed
        || log.address != bridge
        || log.topics.len() != 4
        || log.topics[0].as_array() != &event_topic
        || log.topics[1].as_array() != &authorization.authorization.deposit_id
        || log.topics[2].as_array()[..12].iter().any(|byte| *byte != 0)
        || log.topics[2].as_array()[12..] != authorization.authorization.recipient
        || log.topics[3].as_array() != &authorization.digest
    {
        return Err(ObservationError::BaseStateMismatch);
    }
    let data = log.data.as_ref();
    if data.len() != 3 * ABI_WORD_BYTES
        || word_u128(&data[..ABI_WORD_BYTES])? != authorization.authorization.gross_amount.get()
        || word_u128(&data[ABI_WORD_BYTES..2 * ABI_WORD_BYTES])?
            != authorization.authorization.charged_service_fee.get()
        || word_u128(&data[2 * ABI_WORD_BYTES..])?
            != authorization
                .authorization
                .gross_amount
                .get()
                .checked_sub(authorization.authorization.charged_service_fee.get())
                .ok_or(ObservationError::Overflow)?
    {
        return Err(ObservationError::BaseStateMismatch);
    }
    let transaction_hash = log
        .transaction_hash
        .as_ref()
        .map(|hash| *hash.as_array())
        .ok_or(ObservationError::InvalidResponse)?;
    if expected_transaction_hash.is_some_and(|expected| expected != transaction_hash) {
        return Err(ObservationError::BaseStateMismatch);
    }
    let log_index = log
        .log_index
        .clone()
        .ok_or(ObservationError::InvalidResponse)
        .and_then(|value| u64::try_from(value).map_err(|_| ObservationError::Overflow))?;
    let (receipt, finalized_observation, receipt_observation) =
        match canonical_finalized_receipt_at(args, transaction_hash, finalized).await? {
            CanonicalFinalizedReceiptOutcome::Confirmed {
                receipt,
                finalized_observation,
                receipt_observation,
            } => (receipt, finalized_observation, receipt_observation),
            CanonicalFinalizedReceiptOutcome::Missing
            | CanonicalFinalizedReceiptOutcome::Pending { .. } => {
                return Err(ObservationError::TransactionPending);
            }
        };
    if receipt.status == Some(Nat256::from(0u64)) {
        return Err(ObservationError::TransactionReverted);
    }
    if receipt.status != Some(Nat256::from(1u64))
        || receipt.to.as_ref() != Some(&bridge)
        || receipt
            .logs
            .iter()
            .filter(|candidate| exact_receipt_log_matches(candidate, log))
            .count()
            != 1
    {
        return Err(ObservationError::BaseStateMismatch);
    }
    Ok(bridge_core::MintFinalizationEvidence {
        deposit_id: authorization.authorization.deposit_id,
        recipient: authorization.authorization.recipient,
        authorization_digest: authorization.digest,
        chain_id: finalized_observation.chain_id,
        verifying_contract: authorization.domain.verifying_contract,
        gross_amount: authorization.authorization.gross_amount,
        charged_service_fee: authorization.authorization.charged_service_fee,
        minted_amount: authorization
            .authorization
            .gross_amount
            .checked_sub(authorization.authorization.charged_service_fee)
            .map_err(|_| ObservationError::Overflow)?,
        transaction_hash,
        log_index,
        receipt_succeeded: true,
        receipt_block_number: receipt_observation.block_number,
        receipt_block_hash: receipt_observation.block_hash,
        finalized_block_number: finalized_observation.block_number,
        finalized_block_hash: finalized_observation.block_hash,
        rpc_request_digest,
        rpc_response_digest,
    })
}

fn exact_receipt_log_matches(
    candidate: &evm_rpc_types::LogEntry,
    observed: &evm_rpc_types::LogEntry,
) -> bool {
    !candidate.removed
        && candidate.address == observed.address
        && candidate.topics == observed.topics
        && candidate.data == observed.data
        && candidate.block_number == observed.block_number
        && candidate.block_hash == observed.block_hash
        && candidate.transaction_hash == observed.transaction_hash
        && candidate.log_index == observed.log_index
}

async fn observe_bridge_at(
    args: &BridgeInitArgs,
    observation: FinalizedObservation,
) -> Result<(BridgeSnapshot, ObservedBridgeIdentity), ObservationError> {
    let (snapshot, runtime) = futures::join!(
        observe_bridge_snapshot_at(args, observation),
        bridge_runtime_at_observation(args, observation)
    );
    let snapshot = snapshot?;
    let runtime = runtime?;
    if runtime.is_empty() {
        return Err(ObservationError::InvalidResponse);
    }
    let expected_runtime: [u8; 32] = args
        .expected_bridge_runtime_sha256
        .as_slice()
        .try_into()
        .map_err(|_| ObservationError::InvalidResponse)?;
    if !runtime_matches_expected(&runtime, expected_runtime) {
        return Err(ObservationError::BaseStateMismatch);
    }
    let runtime_sha256: [u8; 32] = Sha256::digest(&runtime).into();
    let identity = ObservedBridgeIdentity {
        signer: snapshot.bridge_signer,
        runtime_sha256,
    };
    Ok((snapshot, identity))
}

fn runtime_matches_expected(runtime: &[u8], expected_sha256: [u8; 32]) -> bool {
    !runtime.is_empty() && <[u8; 32]>::from(Sha256::digest(runtime)) == expected_sha256
}

async fn observe_bridge_snapshot_at(
    args: &BridgeInitArgs,
    observation: FinalizedObservation,
) -> Result<BridgeSnapshot, ObservationError> {
    let snapshot_selector = selector("bridgeSnapshot()");
    let value = eth_call_at_observation(args, &snapshot_selector, observation).await?;
    let snapshot = decode_bridge_snapshot(&value)?;
    if !bridge_core::canonical_probe_matches(
        observation.block_number,
        snapshot.mint.finalized_head_block_number,
    ) {
        return Err(ObservationError::InvalidResponse);
    }
    Ok(snapshot)
}

fn rpc_audit_evidence(
    args: &BridgeInitArgs,
    finalized: FinalizedObservation,
    snapshot: BridgeSnapshot,
    identity: ObservedBridgeIdentity,
    transaction_hash: Option<[u8; 32]>,
    receipt_block_number: Option<u64>,
    withdrawal: Option<&ObservedWithdrawal>,
) -> RpcAuditEvidence {
    // These digests bind the exact logical JSON-RPC transcript that reached 2-of-3
    // consensus.  They intentionally hash normalized semantics instead of provider
    // response bytes, whose insignificant JSON formatting is provider-dependent.
    let calls = if transaction_hash.is_some() {
        vec![
            "eth_getBlockByNumber(finalized)",
            "eth_getTransactionReceipt",
            "eth_call(bridgeSnapshot,EIP-1898-receipt-hash)",
            "eth_call(getWithdrawal,EIP-1898-finalized-hash)",
            "eth_call(bridgeSnapshot,EIP-1898-finalized-hash)",
            "eth_getCode(EIP-1898-finalized-hash)",
        ]
    } else {
        vec![
            "eth_getBlockByNumber(finalized)",
            "eth_call(bridgeSnapshot,EIP-1898-finalized-hash)",
            "eth_getCode(EIP-1898-finalized-hash)",
        ]
    };
    let request = json!({
        "evm_rpc_canister_id": args.evm_rpc_canister_id.to_text(),
        "candid_method": "multi_request",
        "calls": calls,
        "configured_chain_id": args.base_chain_id,
        "bridge_contract": format!("0x{}", hex(&args.bridge_contract)),
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "transaction_hash": transaction_hash.map(|hash| format!("0x{}", hex(&hash))),
    });
    let withdrawal_response = withdrawal.map(|value| {
        json!({
            "id": format!("0x{}", hex(&value.id)),
            "requester": format!("0x{}", hex(&value.requester)),
            "amount": value.amount.to_string(),
            "max_service_fee": value.max_service_fee.to_string(),
            "charged_service_fee": value.charged_service_fee.to_string(),
            "amount_out": value.amount_out.to_string(),
            "owner": format!("0x{}", hex(&value.owner)),
            "subaccount": format!("0x{}", hex(&value.subaccount)),
            "current_status": 1,
        })
    });
    let response = json!({
        "configured_chain_id": finalized.chain_id,
        "finalized_block_number": finalized.block_number,
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "receipt_block_number": receipt_block_number,
        "withdrawal": withdrawal_response,
        "bridge_signer": format!("0x{}", hex(&snapshot.bridge_signer)),
        "bridge_runtime_sha256": format!("0x{}", hex(&identity.runtime_sha256)),
        "snapshot_finalized_head_block_number": snapshot.mint.finalized_head_block_number,
        "snapshot_confirmed_block_timestamp": snapshot.mint.confirmed_block_timestamp,
        "snapshot_service_fee": snapshot.mint.service_fee.get().to_string(),
        "snapshot_max_service_fee": snapshot.mint.max_service_fee.get().to_string(),
        "snapshot_per_deposit_limit": snapshot.mint.per_deposit_limit.get().to_string(),
        "snapshot_mint_window_limit": snapshot.mint.mint_window_limit.get().to_string(),
        "snapshot_mint_window_duration": snapshot.mint.mint_window_duration,
        "snapshot_mint_window_started_at": snapshot.mint.mint_window_started_at,
        "snapshot_minted_in_window": snapshot.mint.minted_in_window.get().to_string(),
        "deposits_paused": snapshot.deposits_paused,
        "withdrawals_paused": snapshot.withdrawals_paused,
    });
    RpcAuditEvidence {
        evm_rpc_canister_id: args.evm_rpc_canister_id,
        call_method: "multi_request".to_owned(),
        request_digest: Sha256::digest(
            serde_json::to_vec(&request).expect("JSON value serialization is infallible"),
        )
        .into(),
        quorum_response_digest: Sha256::digest(
            serde_json::to_vec(&response).expect("JSON value serialization is infallible"),
        )
        .into(),
        finalized_block_number: finalized.block_number,
        finalized_block_hash: finalized.block_hash,
        transaction_hash,
    }
}

fn recovery_rpc_audit_evidence(
    args: &BridgeInitArgs,
    finalized: FinalizedObservation,
    snapshot: BridgeSnapshot,
    identity: ObservedBridgeIdentity,
    target: RecoveryTarget,
    state: &RecoveryBaseState,
) -> RpcAuditEvidence {
    let (method, target_value, state_value) = match (target, state) {
        (RecoveryTarget::Deposit(id), RecoveryBaseState::DepositProcessed(processed)) => (
            "eth_call(isDepositProcessed,EIP-1898-finalized-hash)",
            format!("0x{}", hex(&id)),
            json!({ "processed": processed }),
        ),
    };
    let request = json!({
        "evm_rpc_canister_id": args.evm_rpc_canister_id.to_text(),
        "candid_method": "multi_request",
        "calls": [
            "eth_getBlockByNumber(finalized)",
            method,
            "eth_call(bridgeSnapshot,EIP-1898-finalized-hash)",
            "eth_getCode(EIP-1898-finalized-hash)",
        ],
        "configured_chain_id": args.base_chain_id,
        "bridge_contract": format!("0x{}", hex(&args.bridge_contract)),
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "target": target_value,
    });
    let response = json!({
        "configured_chain_id": finalized.chain_id,
        "finalized_block_number": finalized.block_number,
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "bridge_signer": format!("0x{}", hex(&snapshot.bridge_signer)),
        "bridge_runtime_sha256": format!("0x{}", hex(&identity.runtime_sha256)),
        "state": state_value,
    });
    RpcAuditEvidence {
        evm_rpc_canister_id: args.evm_rpc_canister_id,
        call_method: "multi_request:recovery_observation".into(),
        request_digest: Sha256::digest(
            serde_json::to_vec(&request).expect("serializable recovery request"),
        )
        .into(),
        quorum_response_digest: Sha256::digest(
            serde_json::to_vec(&response).expect("serializable recovery response"),
        )
        .into(),
        finalized_block_number: finalized.block_number,
        finalized_block_hash: finalized.block_hash,
        transaction_hash: None,
    }
}

fn deposit_preflight_rpc_audit_evidence(
    args: &BridgeInitArgs,
    finalized: FinalizedObservation,
    snapshot: BridgeSnapshot,
    deposit_id: [u8; 32],
    refresh_generation: u64,
    processed: bool,
    runtime_attestation_refreshed: bool,
) -> RpcAuditEvidence {
    let mut calls = DEPOSIT_PREFLIGHT_RPC_CALLS.to_vec();
    if runtime_attestation_refreshed {
        calls.push("eth_getCode(EIP-1898-finalized-hash)");
    }
    let request = json!({
        "evm_rpc_canister_id": args.evm_rpc_canister_id.to_text(),
        "candid_method": "multi_request",
        "calls": calls,
        "configured_chain_id": args.base_chain_id,
        "bridge_contract": format!("0x{}", hex(&args.bridge_contract)),
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "deposit_id": format!("0x{}", hex(&deposit_id)),
        "refresh_generation": refresh_generation,
    });
    let response = json!({
        "configured_chain_id": finalized.chain_id,
        "finalized_block_number": finalized.block_number,
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "refresh_generation": refresh_generation,
        "processed": processed,
        "bridge_signer": format!("0x{}", hex(&snapshot.bridge_signer)),
        "mint_authorization_epoch": snapshot.mint_authorization_epoch,
        "snapshot_confirmed_block_timestamp": snapshot.mint.confirmed_block_timestamp,
        "snapshot_service_fee": snapshot.mint.service_fee.get().to_string(),
        "snapshot_max_service_fee": snapshot.mint.max_service_fee.get().to_string(),
        "snapshot_per_deposit_limit": snapshot.mint.per_deposit_limit.get().to_string(),
        "snapshot_mint_window_limit": snapshot.mint.mint_window_limit.get().to_string(),
        "snapshot_mint_window_duration": snapshot.mint.mint_window_duration,
        "snapshot_mint_window_started_at": snapshot.mint.mint_window_started_at,
        "snapshot_minted_in_window": snapshot.mint.minted_in_window.get().to_string(),
        "deposits_paused": snapshot.deposits_paused,
        "bridge_runtime_sha256": format!(
            "0x{}",
            hex(&args.expected_bridge_runtime_sha256)
        ),
        "runtime_attestation_refreshed": runtime_attestation_refreshed,
    });
    RpcAuditEvidence {
        evm_rpc_canister_id: args.evm_rpc_canister_id,
        call_method: "multi_request:deposit_preflight".into(),
        request_digest: Sha256::digest(
            serde_json::to_vec(&request).expect("serializable deposit preflight request"),
        )
        .into(),
        quorum_response_digest: Sha256::digest(
            serde_json::to_vec(&response).expect("serializable deposit preflight response"),
        )
        .into(),
        finalized_block_number: finalized.block_number,
        finalized_block_hash: finalized.block_hash,
        transaction_hash: None,
    }
}

fn transaction_rpc_audit_evidence(
    args: &BridgeInitArgs,
    call_method: &str,
    finalized: FinalizedObservation,
    transaction_hash: [u8; 32],
    response: serde_json::Value,
) -> RpcAuditEvidence {
    let request = json!({
        "evm_rpc_canister_id": args.evm_rpc_canister_id.to_text(),
        "call_method": call_method,
        "configured_chain_id": args.base_chain_id,
        "finalized_block_number": finalized.block_number,
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "transaction_hash": format!("0x{}", hex(&transaction_hash)),
    });
    let response = json!({
        "configured_chain_id": finalized.chain_id,
        "finalized_block_number": finalized.block_number,
        "finalized_block_hash": format!("0x{}", hex(&finalized.block_hash)),
        "transaction_hash": format!("0x{}", hex(&transaction_hash)),
        "outcome": response,
    });
    RpcAuditEvidence {
        evm_rpc_canister_id: args.evm_rpc_canister_id,
        call_method: call_method.to_owned(),
        request_digest: Sha256::digest(
            serde_json::to_vec(&request).expect("JSON value serialization is infallible"),
        )
        .into(),
        quorum_response_digest: Sha256::digest(
            serde_json::to_vec(&response).expect("JSON value serialization is infallible"),
        )
        .into(),
        finalized_block_number: finalized.block_number,
        finalized_block_hash: finalized.block_hash,
        transaction_hash: Some(transaction_hash),
    }
}

async fn bridge_runtime_at_observation(
    args: &BridgeInitArgs,
    observation: FinalizedObservation,
) -> Result<Vec<u8>, ObservationError> {
    let request = bridge_runtime_request(&args.bridge_contract, observation);
    let value = match client(args)
        .multi_request(request)
        .with_response_size_estimate(RECEIPT_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(value)) => value,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    decode_hex(
        value
            .trim_matches('"')
            .strip_prefix("0x")
            .ok_or(ObservationError::InvalidResponse)?,
    )
}

fn bridge_runtime_request(
    bridge_contract: &[u8],
    observation: FinalizedObservation,
) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_getCode",
        "params": [format!("0x{}", hex(bridge_contract)), {
            "blockHash": format!("0x{}", hex(&observation.block_hash)),
            "requireCanonical": true,
        }],
    })
}

fn decode_bridge_snapshot(value: &str) -> Result<BridgeSnapshot, ObservationError> {
    let bytes = decode_hex(
        value
            .trim_matches('"')
            .strip_prefix("0x")
            .ok_or(ObservationError::InvalidResponse)?,
    )?;
    if bytes.len() != 13 * ABI_WORD_BYTES {
        return Err(ObservationError::InvalidResponse);
    }
    let word = |index: usize| -> Result<&[u8], ObservationError> {
        bytes
            .get(index * ABI_WORD_BYTES..(index + 1) * ABI_WORD_BYTES)
            .ok_or(ObservationError::InvalidResponse)
    };
    let address_word = word(2)?;
    if address_word[..12].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::InvalidResponse);
    }
    let boolean = |index: usize| -> Result<bool, ObservationError> {
        match word_u128(word(index)?)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ObservationError::InvalidResponse),
        }
    };
    Ok(BridgeSnapshot {
        mint: BaseMintSnapshot {
            finalized_head_block_number: u64::try_from(word_u128(word(0)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            confirmed_block_timestamp: u64::try_from(word_u128(word(1)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            service_fee: Amount::new(word_u128(word(4)?)?),
            max_service_fee: Amount::new(word_u128(word(5)?)?),
            per_deposit_limit: Amount::new(word_u128(word(6)?)?),
            mint_window_limit: Amount::new(word_u128(word(7)?)?),
            mint_window_duration: u64::try_from(word_u128(word(8)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            mint_window_started_at: u64::try_from(word_u128(word(9)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            minted_in_window: Amount::new(word_u128(word(10)?)?),
        },
        bridge_signer: address_word[12..]
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        mint_authorization_epoch: u64::try_from(word_u128(word(3)?)?)
            .map_err(|_| ObservationError::Overflow)?,
        deposits_paused: boolean(11)?,
        withdrawals_paused: boolean(12)?,
    })
}

pub async fn finalized_observation(
    args: &BridgeInitArgs,
) -> Result<FinalizedObservation, ObservationError> {
    let (chain_id, block) = futures::join!(observed_chain_id(args), finalized_block(args));
    let chain_id = chain_id?;
    if chain_id != args.base_chain_id {
        return Err(ObservationError::ChainIdMismatch);
    }
    let block = block?;
    let observation = FinalizedObservation {
        chain_id,
        block_number: u64::try_from(block.number).map_err(|_| ObservationError::Overflow)?,
        block_hash: *block.hash.as_array(),
        observed_at_ns: ic_cdk::api::time(),
    };
    Ok(observation)
}

async fn observed_chain_id(args: &BridgeInitArgs) -> Result<u64, ObservationError> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_chainId",
        "params": [],
    });
    match client(args)
        .multi_request(request)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(value)) => {
            u64::try_from(parse_u128(&value)?).map_err(|_| ObservationError::Overflow)
        }
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

async fn finalized_block(args: &BridgeInitArgs) -> Result<Block, ObservationError> {
    match client(args)
        .get_block_by_number(BlockTag::Finalized)
        .with_response_size_estimate(BLOCK_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(block)) => Ok(block),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

/// Returns a receipt only after proving that its block hash is canonical at its height
/// and that the height is not newer than the provider-consistent finalized head.
enum CanonicalFinalizedReceiptOutcome {
    Missing,
    Pending {
        receipt_block_number: u64,
    },
    Confirmed {
        receipt: Box<TransactionReceipt>,
        finalized_observation: FinalizedObservation,
        receipt_observation: FinalizedObservation,
    },
}

async fn canonical_finalized_receipt(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<CanonicalFinalizedReceiptOutcome, ObservationError> {
    let hash = Hex32::from_str(&format!("0x{}", hex(&transaction_hash)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let receipt_call = async {
        match client(args)
            .get_transaction_receipt(hash.clone())
            .with_response_size_estimate(RECEIPT_RESPONSE_BYTES)
            .try_send()
            .await
            .map_err(|_| ObservationError::Rpc)?
        {
            MultiRpcResult::Consistent(Ok(receipt)) => Ok(receipt),
            MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
            MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
        }
    };
    let (finalized, receipt) = futures::join!(finalized_observation(args), receipt_call);
    canonical_finalized_receipt_with_hash(args, hash, transaction_hash, finalized?, receipt?).await
}

async fn canonical_finalized_receipt_at(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
    finalized: FinalizedObservation,
) -> Result<CanonicalFinalizedReceiptOutcome, ObservationError> {
    if finalized.chain_id != args.base_chain_id {
        return Err(ObservationError::ChainIdMismatch);
    }
    let hash = Hex32::from_str(&format!("0x{}", hex(&transaction_hash)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let receipt = match client(args)
        .get_transaction_receipt(hash.clone())
        .with_response_size_estimate(RECEIPT_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(receipt)) => receipt,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    canonical_finalized_receipt_with_hash(args, hash, transaction_hash, finalized, receipt).await
}

async fn canonical_finalized_receipt_with_hash(
    args: &BridgeInitArgs,
    hash: Hex32,
    _transaction_hash: [u8; 32],
    finalized: FinalizedObservation,
    receipt: Option<TransactionReceipt>,
) -> Result<CanonicalFinalizedReceiptOutcome, ObservationError> {
    let Some(receipt) = receipt else {
        return Ok(CanonicalFinalizedReceiptOutcome::Missing);
    };
    if receipt.transaction_hash != hash {
        return Err(ObservationError::InvalidResponse);
    }
    let receipt_block =
        u64::try_from(receipt.block_number.clone()).map_err(|_| ObservationError::Overflow)?;
    if receipt_block > finalized.block_number {
        return Ok(CanonicalFinalizedReceiptOutcome::Pending {
            receipt_block_number: receipt_block,
        });
    }
    let receipt_observation = FinalizedObservation {
        chain_id: finalized.chain_id,
        block_number: receipt_block,
        block_hash: *receipt.block_hash.as_array(),
        observed_at_ns: finalized.observed_at_ns,
    };
    let probe = eth_call_request(
        &args.bridge_contract,
        &selector("bridgeSnapshot()"),
        receipt_observation,
    );
    let probe_value = match client(args)
        .multi_request(probe)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(value)) => value,
        MultiRpcResult::Consistent(Err(error)) => return Err(canonical_probe_error(error)),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    validate_canonical_probe_response(receipt_observation, &probe_value)?;
    Ok(CanonicalFinalizedReceiptOutcome::Confirmed {
        receipt: Box::new(receipt),
        finalized_observation: finalized,
        receipt_observation,
    })
}

fn canonical_probe_error(error: RpcError) -> ObservationError {
    match error {
        RpcError::JsonRpcError(_) | RpcError::ValidationError(_) => {
            ObservationError::InvalidResponse
        }
        RpcError::ProviderError(_) | RpcError::HttpOutcallError(_) => ObservationError::Rpc,
    }
}

fn validate_canonical_probe_response(
    observation: FinalizedObservation,
    value: &str,
) -> Result<(), ObservationError> {
    let snapshot = decode_bridge_snapshot(value)?;
    if snapshot.mint.finalized_head_block_number != observation.block_number {
        return Err(ObservationError::InvalidResponse);
    }
    Ok(())
}

pub async fn transaction_count(
    args: &BridgeInitArgs,
    address: [u8; 20],
) -> Result<u64, ObservationError> {
    let address = Hex20::from_str(&format!("0x{}", hex(&address)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let nonce = async {
        match client(args)
            .get_transaction_count(GetTransactionCountArgs {
                address,
                block: BlockTag::Pending,
            })
            .with_response_size_estimate(SMALL_RESPONSE_BYTES)
            .try_send()
            .await
            .map_err(|_| ObservationError::Rpc)?
        {
            MultiRpcResult::Consistent(Ok(value)) => {
                u64::try_from(value).map_err(|_| ObservationError::Overflow)
            }
            MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
            MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
        }
    };
    let (chain_id, nonce) = futures::join!(observed_chain_id(args), nonce);
    if chain_id? != args.base_chain_id {
        return Err(ObservationError::ChainIdMismatch);
    }
    nonce
}

pub async fn signer_eth_balance_on_attested_chain(
    args: &BridgeInitArgs,
    address: [u8; 20],
    attestation: FinalizedObservation,
) -> Result<u128, ObservationError> {
    if attestation.chain_id != args.base_chain_id {
        return Err(ObservationError::ChainIdMismatch);
    }
    let request = json!({ "jsonrpc":"2.0", "id":1, "method":"eth_getBalance", "params":[format!("0x{}", hex(&address)), "safe"] });
    match client(args)
        .multi_request(request)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(value)) => parse_u128(&value),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

pub async fn signer_eth_balance_at(
    args: &BridgeInitArgs,
    address: [u8; 20],
    observation: FinalizedObservation,
) -> Result<u128, ObservationError> {
    let request = json!({
        "jsonrpc":"2.0", "id":1, "method":"eth_getBalance",
        "params":[format!("0x{}", hex(&address)), {
            "blockHash": format!("0x{}", hex(&observation.block_hash)),
            "requireCanonical": true,
        }]
    });
    match client(args)
        .multi_request(request)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .try_send()
        .await
        .map_err(|_| ObservationError::Rpc)?
    {
        MultiRpcResult::Consistent(Ok(value)) => parse_u128(&value),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

const ABI_WORD_BYTES: usize = 32;
fn word_u128(word: &[u8]) -> Result<u128, ObservationError> {
    if word.len() != 32 || word[..16].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::Overflow);
    }
    Ok(u128::from_be_bytes(
        word[16..]
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
    ))
}

fn decode_hex(value: &str) -> Result<Vec<u8>, ObservationError> {
    if !value.len().is_multiple_of(2) {
        return Err(ObservationError::InvalidResponse);
    }
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|_| ObservationError::InvalidResponse)
        })
        .collect()
}

pub(crate) fn signed_transaction_hash(raw: &[u8]) -> [u8; 32] {
    let mut transaction_hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(raw);
    hasher.finalize(&mut transaction_hash);
    transaction_hash
}

pub async fn notified_withdrawal_outcome(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<NotifiedWithdrawalOutcome, ObservationError> {
    let hash = Hex32::from_str(&format!("0x{}", hex(&transaction_hash)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let (receipt, finalized_observation, _receipt_observation) =
        match canonical_finalized_receipt(args, transaction_hash).await? {
            CanonicalFinalizedReceiptOutcome::Missing => {
                return Ok(NotifiedWithdrawalOutcome::Missing);
            }
            CanonicalFinalizedReceiptOutcome::Pending {
                receipt_block_number,
            } => {
                return Ok(NotifiedWithdrawalOutcome::Pending {
                    receipt_block_number,
                });
            }
            CanonicalFinalizedReceiptOutcome::Confirmed {
                receipt,
                finalized_observation,
                receipt_observation,
            } => (*receipt, finalized_observation, receipt_observation),
        };
    let receipt_block_number =
        u64::try_from(receipt.block_number.clone()).map_err(|_| ObservationError::Overflow)?;
    match receipt.status {
        Some(status) if status == Nat256::from(0u64) => {
            return Ok(NotifiedWithdrawalOutcome::Reverted {
                receipt_block_number,
                finalized_head_block_number: finalized_observation.block_number,
            });
        }
        Some(status) if status == Nat256::from(1u64) => {}
        _ => return Err(ObservationError::InvalidResponse),
    }
    let address = Hex20::from_str(&format!("0x{}", hex(&args.bridge_contract)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let mut topic = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(
        b"WithdrawalCommitted(uint256,address,uint256,uint256,uint256,uint256,bytes,bytes32)",
    );
    hasher.finalize(&mut topic);
    let topic = Hex32::from_str(&format!("0x{}", hex(&topic)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let mut matching = receipt.logs.iter().filter(|log| {
        !log.removed
            && log.address == address
            && log.topics.first() == Some(&topic)
            && log.transaction_hash.as_ref() == Some(&hash)
            && log.block_hash.as_ref() == Some(&receipt.block_hash)
    });
    let log = matching.next().ok_or(ObservationError::InvalidResponse)?;
    if matching.next().is_some() || log.topics.len() != 3 {
        return Err(ObservationError::InvalidResponse);
    }
    if log.topics[2].as_array()[..12].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::InvalidResponse);
    }
    let withdrawal = decode_withdrawal_created_log(log)?;
    let mut withdrawal_calldata = selector("getWithdrawal(uint256)").to_vec();
    withdrawal_calldata.extend_from_slice(&withdrawal.id);
    // Bind every event field to the same provider-consistent Finalized state used for release.
    let (current, bridge) = futures::join!(
        eth_call_at_observation(args, &withdrawal_calldata, finalized_observation),
        observe_bridge_at(args, finalized_observation)
    );
    let current = decode_current_withdrawal(&current?)?;
    if !is_same_committed_withdrawal(&current, &withdrawal) {
        return Err(ObservationError::BaseStateMismatch);
    }
    let (snapshot, bridge_identity) = bridge?;
    let rpc_audit = rpc_audit_evidence(
        args,
        finalized_observation,
        snapshot,
        bridge_identity,
        Some(transaction_hash),
        Some(receipt_block_number),
        Some(&withdrawal),
    );
    let stable_observation = Box::new(FinalizedObservationRecord {
        chain_id: finalized_observation.chain_id,
        block_number: finalized_observation.block_number,
        block_hash: finalized_observation.block_hash,
        observed_at_ns: finalized_observation.observed_at_ns,
        bridge_signer: bridge_identity.signer,
        runtime_sha256: bridge_identity.runtime_sha256,
    });
    Ok(NotifiedWithdrawalOutcome::Confirmed {
        withdrawal,
        snapshot: Box::new(snapshot),
        rpc_audit: Box::new(rpc_audit),
        stable_observation,
        receipt_block_number,
        finalized_head_block_number: finalized_observation.block_number,
    })
}

fn is_same_committed_withdrawal(
    current: &CurrentWithdrawal,
    observed: &ObservedWithdrawal,
) -> bool {
    current.status == 1
        && current.requester == observed.requester
        && current.amount == observed.amount
        && current.max_service_fee == observed.max_service_fee
        && current.charged_service_fee == observed.charged_service_fee
        && current.amount_out == observed.amount_out
        && current.owner == observed.owner
        && current.subaccount == observed.subaccount
}

fn decode_withdrawal_created_log(
    log: &evm_rpc_types::LogEntry,
) -> Result<ObservedWithdrawal, ObservationError> {
    let data = log.data.as_ref();
    if data.len() < 8 * ABI_WORD_BYTES {
        return Err(ObservationError::InvalidResponse);
    }
    let word = |index: usize| -> Result<&[u8], ObservationError> {
        data.get(index * ABI_WORD_BYTES..(index + 1) * ABI_WORD_BYTES)
            .ok_or(ObservationError::InvalidResponse)
    };
    let owner_offset =
        usize::try_from(word_u128(word(4)?)?).map_err(|_| ObservationError::Overflow)?;
    if owner_offset != 6 * ABI_WORD_BYTES {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_len =
        usize::try_from(word_u128(word(6)?)?).map_err(|_| ObservationError::Overflow)?;
    if !(1..=29).contains(&owner_len) {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_end = 7 * ABI_WORD_BYTES + owner_len;
    let expected_len = 7 * ABI_WORD_BYTES + owner_len.div_ceil(ABI_WORD_BYTES) * ABI_WORD_BYTES;
    if data.len() != expected_len || data[owner_end..].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::InvalidResponse);
    }
    let owner = data
        .get(7 * ABI_WORD_BYTES..owner_end)
        .ok_or(ObservationError::InvalidResponse)?
        .to_vec();
    Ok(ObservedWithdrawal {
        id: *log.topics[1].as_array(),
        requester: log.topics[2].as_array()[12..]
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        amount: word_u128(word(0)?)?,
        max_service_fee: word_u128(word(1)?)?,
        charged_service_fee: word_u128(word(2)?)?,
        amount_out: word_u128(word(3)?)?,
        owner,
        subaccount: word(5)?
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
    })
}

fn decode_current_withdrawal(value: &str) -> Result<CurrentWithdrawal, ObservationError> {
    let bytes = decode_hex(
        value
            .trim_matches('"')
            .strip_prefix("0x")
            .ok_or(ObservationError::InvalidResponse)?,
    )?;
    if bytes.len() < 10 * ABI_WORD_BYTES
        || word_u128(&bytes[..ABI_WORD_BYTES])? != ABI_WORD_BYTES as u128
    {
        return Err(ObservationError::InvalidResponse);
    }
    let tuple_start = ABI_WORD_BYTES;
    let word = |index: usize| -> Result<&[u8], ObservationError> {
        bytes
            .get(tuple_start + index * ABI_WORD_BYTES..tuple_start + (index + 1) * ABI_WORD_BYTES)
            .ok_or(ObservationError::InvalidResponse)
    };
    let requester_word = word(0)?;
    if requester_word[..12].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_offset =
        usize::try_from(word_u128(word(5)?)?).map_err(|_| ObservationError::Overflow)?;
    if owner_offset != 8 * ABI_WORD_BYTES {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_len =
        usize::try_from(word_u128(word(8)?)?).map_err(|_| ObservationError::Overflow)?;
    if !(1..=29).contains(&owner_len) {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_start = tuple_start + 9 * ABI_WORD_BYTES;
    let owner_end = owner_start
        .checked_add(owner_len)
        .ok_or(ObservationError::Overflow)?;
    let expected_len = owner_start
        .checked_add(owner_len.div_ceil(ABI_WORD_BYTES) * ABI_WORD_BYTES)
        .ok_or(ObservationError::Overflow)?;
    if bytes.len() != expected_len || bytes[owner_end..].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::InvalidResponse);
    }
    let status =
        u8::try_from(word_u128(word(7)?)?).map_err(|_| ObservationError::InvalidResponse)?;
    Ok(CurrentWithdrawal {
        requester: requester_word[12..]
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        amount: word_u128(word(1)?)?,
        max_service_fee: word_u128(word(2)?)?,
        charged_service_fee: word_u128(word(3)?)?,
        amount_out: word_u128(word(4)?)?,
        owner: bytes[owner_start..owner_end].to_vec(),
        subaccount: word(6)?
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        status,
    })
}

fn decode_bool_word(value: &str) -> Result<bool, ObservationError> {
    let bytes = decode_hex(
        value
            .trim_matches('"')
            .strip_prefix("0x")
            .ok_or(ObservationError::InvalidResponse)?,
    )?;
    if bytes.len() != ABI_WORD_BYTES || bytes[..ABI_WORD_BYTES - 1].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::InvalidResponse);
    }
    match bytes[ABI_WORD_BYTES - 1] {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ObservationError::InvalidResponse),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfirmedReceiptOutcome {
    Missing,
    Pending {
        receipt_block_number: u64,
    },
    Succeeded {
        receipt_block_number: u64,
        finalized_head_block_number: u64,
        finalized_observation: FinalizedObservation,
        rpc_audit: Box<RpcAuditEvidence>,
    },
    Reverted {
        receipt_block_number: u64,
        finalized_head_block_number: u64,
        finalized_observation: FinalizedObservation,
        rpc_audit: Box<RpcAuditEvidence>,
    },
}

pub async fn confirmed_receipt_outcome(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<ConfirmedReceiptOutcome, ObservationError> {
    let (receipt, finalized) = match canonical_finalized_receipt(args, transaction_hash).await? {
        CanonicalFinalizedReceiptOutcome::Missing => return Ok(ConfirmedReceiptOutcome::Missing),
        CanonicalFinalizedReceiptOutcome::Pending {
            receipt_block_number,
        } => {
            return Ok(ConfirmedReceiptOutcome::Pending {
                receipt_block_number,
            });
        }
        CanonicalFinalizedReceiptOutcome::Confirmed {
            receipt,
            finalized_observation,
            ..
        } => (*receipt, finalized_observation),
    };
    let receipt_block =
        u64::try_from(receipt.block_number.clone()).map_err(|_| ObservationError::Overflow)?;
    let receipt_hash = *receipt.block_hash.as_array();
    match receipt.status {
        Some(status) if status == evm_rpc_types::Nat256::from(1u64) => {
            Ok(ConfirmedReceiptOutcome::Succeeded {
                receipt_block_number: receipt_block,
                finalized_head_block_number: finalized.block_number,
                finalized_observation: finalized,
                rpc_audit: Box::new(transaction_rpc_audit_evidence(
                    args,
                    "eth_getTransactionReceipt+multi_request",
                    finalized,
                    transaction_hash,
                    json!({
                        "status": "succeeded",
                        "receipt_block_number": receipt_block,
                        "receipt_block_hash": format!("0x{}", hex(&receipt_hash)),
                    }),
                )),
            })
        }
        Some(status) if status == evm_rpc_types::Nat256::from(0u64) => {
            Ok(ConfirmedReceiptOutcome::Reverted {
                receipt_block_number: receipt_block,
                finalized_head_block_number: finalized.block_number,
                finalized_observation: finalized,
                rpc_audit: Box::new(transaction_rpc_audit_evidence(
                    args,
                    "eth_getTransactionReceipt+multi_request",
                    finalized,
                    transaction_hash,
                    json!({
                        "status": "reverted",
                        "receipt_block_number": receipt_block,
                        "receipt_block_hash": format!("0x{}", hex(&receipt_hash)),
                    }),
                )),
            })
        }
        _ => Err(ObservationError::InvalidResponse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evm_rpc_runtime_uses_the_fixed_thirty_second_bound() {
        assert_eq!(EVM_RPC_TIMEOUT_SECONDS, 30);
        assert_eq!(SMALL_RESPONSE_BYTES, 4 * 1024);
        assert_eq!(BLOCK_RESPONSE_BYTES, 16 * 1024);
    }

    #[test]
    fn deposit_preflight_attests_chain_then_uses_one_finalized_anchor_for_two_state_calls() {
        assert_eq!(
            DEPOSIT_PREFLIGHT_RPC_CALLS,
            [
                "eth_chainId",
                "eth_getBlockByNumber(finalized)",
                "eth_call(isDepositProcessed,EIP-1898-finalized-hash)",
                "eth_call(bridgeSnapshot,EIP-1898-finalized-hash)",
            ]
        );
        assert!(DEPOSIT_PREFLIGHT_RPC_CALLS
            .iter()
            .any(|call| call.contains("chainId")));
        assert!(!DEPOSIT_PREFLIGHT_RPC_CALLS
            .iter()
            .any(|call| call.contains("getCode")));
    }

    #[test]
    fn runtime_attestation_requires_nonempty_code_and_the_configured_hash() {
        let code = [0x60, 0x00];
        let expected: [u8; 32] = Sha256::digest(code).into();
        assert!(runtime_matches_expected(&code, expected));
        assert!(!runtime_matches_expected(&[], expected));
        assert!(!runtime_matches_expected(&code, [0; 32]));
    }

    #[test]
    fn selectors_and_uint_decoding_match_the_frozen_abi() {
        assert_eq!(hex(&selector("serviceFee()")), "8abdf5aa");
        assert_eq!(
            parse_u128("0x000000000000000000000000000000000000000000000000000000000000002a"),
            Ok(42)
        );
        assert_eq!(
            parse_u128("0x100000000000000000000000000000000"),
            Err(ObservationError::Overflow)
        );
    }

    #[test]
    fn current_withdrawal_decoder_handles_dynamic_struct_tuple() {
        const WORD: usize = 32;
        let owner = [0x2a, 0x2b, 0x2c];
        let mut bytes = vec![0u8; 11 * WORD];
        bytes[31] = WORD as u8;
        let base = WORD;
        bytes[base + 12..base + WORD].fill(0x11);
        bytes[base + 2 * WORD - 1] = 100;
        bytes[base + 3 * WORD - 1] = 20;
        bytes[base + 4 * WORD - 1] = 10;
        bytes[base + 5 * WORD - 1] = 90;
        bytes[base + 6 * WORD - 2] = 1;
        bytes[base + 6 * WORD - 1] = 0;
        bytes[base + 6 * WORD..base + 7 * WORD].fill(0x22);
        bytes[base + 8 * WORD - 1] = 1;
        bytes[base + 9 * WORD - 1] = owner.len() as u8;
        bytes[base + 9 * WORD..base + 9 * WORD + owner.len()].copy_from_slice(&owner);
        let decoded = decode_current_withdrawal(&format!("0x{}", hex(&bytes)))
            .expect("valid dynamic Withdrawal tuple");
        assert_eq!(decoded.requester, [0x11; 20]);
        assert_eq!(decoded.amount, 100);
        assert_eq!(decoded.max_service_fee, 20);
        assert_eq!(decoded.charged_service_fee, 10);
        assert_eq!(decoded.amount_out, 90);
        assert_eq!(decoded.owner, owner);
        assert_eq!(decoded.subaccount, [0x22; 32]);
        assert_eq!(decoded.status, 1);
    }

    #[test]
    fn eth_call_is_bound_to_a_canonical_block_hash() {
        let observation = FinalizedObservation {
            chain_id: 8453,
            block_number: 42,
            block_hash: [0xabu8; 32],
            observed_at_ns: 7,
        };
        let request = eth_call_request(&[0x42; 20], &[1, 2, 3, 4], observation);
        assert_eq!(
            request["params"][1],
            json!({
                "blockHash": format!("0x{}", "ab".repeat(32)),
                "requireCanonical": true,
            })
        );
        assert!(request["params"][1].get("blockNumber").is_none());

        let code_request = bridge_runtime_request(&[0x42; 20], observation);
        assert_eq!(code_request["method"], "eth_getCode");
        assert_eq!(code_request["params"][1], request["params"][1]);
    }

    #[test]
    fn receipt_probe_requires_a_decodable_snapshot_at_the_receipt_height() {
        let observation = FinalizedObservation {
            chain_id: 8453,
            block_number: 42,
            block_hash: [0xabu8; 32],
            observed_at_ns: 7,
        };
        let mut words = vec![[0u8; ABI_WORD_BYTES]; 13];
        words[0][ABI_WORD_BYTES - 8..].copy_from_slice(&42u64.to_be_bytes());
        let response = format!(
            "0x{}",
            hex(&words.into_iter().flatten().collect::<Vec<_>>())
        );
        assert_eq!(
            validate_canonical_probe_response(observation, &response),
            Ok(())
        );

        let mismatched = response.replacen(
            &format!("{:016x}", observation.block_number),
            &format!("{:016x}", observation.block_number + 1),
            1,
        );
        assert_eq!(
            validate_canonical_probe_response(observation, &mismatched),
            Err(ObservationError::InvalidResponse)
        );
        assert_eq!(
            validate_canonical_probe_response(observation, "not-hex"),
            Err(ObservationError::InvalidResponse)
        );
    }

    #[test]
    fn receipt_probe_classifies_canonicality_rejections_separately_from_transport_failures() {
        assert_eq!(
            canonical_probe_error(RpcError::JsonRpcError(evm_rpc_types::JsonRpcError {
                code: -32_001,
                message: "block is not canonical".into(),
            })),
            ObservationError::InvalidResponse
        );
        assert_eq!(
            canonical_probe_error(RpcError::ProviderError(
                evm_rpc_types::ProviderError::ProviderNotFound
            )),
            ObservationError::Rpc
        );
    }

    #[test]
    fn committed_observation_requires_an_exact_finalized_state_match() {
        let observed = ObservedWithdrawal {
            id: [0x33; 32],
            amount: 100,
            max_service_fee: 20,
            charged_service_fee: 10,
            amount_out: 90,
            owner: vec![1, 2, 3],
            subaccount: [0x22; 32],
            requester: [0x11; 20],
        };
        let mut current = CurrentWithdrawal {
            requester: observed.requester,
            amount: observed.amount,
            max_service_fee: observed.max_service_fee,
            charged_service_fee: observed.charged_service_fee,
            amount_out: observed.amount_out,
            owner: observed.owner.clone(),
            subaccount: observed.subaccount,
            status: 1,
        };
        assert!(is_same_committed_withdrawal(&current, &observed));

        current.amount_out -= 1;
        assert!(!is_same_committed_withdrawal(&current, &observed));
        current.amount_out = observed.amount_out;
        current.status = 0;
        assert!(!is_same_committed_withdrawal(&current, &observed));
    }

    #[test]
    fn receipt_log_binding_includes_log_index() {
        let value = json!({
            "address": format!("0x{}", "11".repeat(20)),
            "topics": [format!("0x{}", "22".repeat(32))],
            "data": "0x",
            "blockNumber": 42,
            "transactionHash": format!("0x{}", "33".repeat(32)),
            "transactionIndex": 0,
            "blockHash": format!("0x{}", "44".repeat(32)),
            "logIndex": 1,
            "removed": false
        });
        let observed: evm_rpc_types::LogEntry =
            serde_json::from_value(value.clone()).expect("valid observed log");
        let mut different_index = value;
        different_index["logIndex"] = json!(2);
        let different_index: evm_rpc_types::LogEntry =
            serde_json::from_value(different_index).expect("valid receipt log");

        assert!(exact_receipt_log_matches(&observed, &observed));
        assert!(!exact_receipt_log_matches(&different_index, &observed));
    }

    #[test]
    fn completed_observation_cache_cannot_publish_torn_fields() {
        let finalized = FinalizedObservation {
            chain_id: 8453,
            block_number: 42,
            block_hash: [0xaa; 32],
            observed_at_ns: 7,
        };
        let snapshot = BridgeSnapshot {
            mint: BaseMintSnapshot {
                finalized_head_block_number: 42,
                confirmed_block_timestamp: 1,
                service_fee: Amount::new(2),
                max_service_fee: Amount::new(3),
                per_deposit_limit: Amount::new(4),
                mint_window_limit: Amount::new(5),
                mint_window_duration: 6,
                mint_window_started_at: 7,
                minted_in_window: Amount::new(8),
            },
            bridge_signer: [0xbb; 20],
            mint_authorization_epoch: 1,
            deposits_paused: false,
            withdrawals_paused: false,
        };
        let bridge_identity = ObservedBridgeIdentity {
            signer: snapshot.bridge_signer,
            runtime_sha256: [0xcc; 32],
        };
        let args = test_init_args();
        let rpc_audit = rpc_audit_evidence(
            &args,
            finalized,
            snapshot,
            bridge_identity,
            None,
            None,
            None,
        );
        let completed = CompletedFinalizedObservation {
            finalized,
            snapshot,
            bridge_identity,
            rpc_audit: rpc_audit.clone(),
        };
        assert_eq!(
            stable_observation(&completed),
            FinalizedObservationRecord {
                chain_id: finalized.chain_id,
                block_number: finalized.block_number,
                block_hash: finalized.block_hash,
                observed_at_ns: finalized.observed_at_ns,
                bridge_signer: bridge_identity.signer,
                runtime_sha256: bridge_identity.runtime_sha256,
            }
        );
    }

    #[test]
    fn rpc_audit_evidence_binds_transaction_and_finalized_hash() {
        let finalized = FinalizedObservation {
            chain_id: 8453,
            block_number: 42,
            block_hash: [0xaa; 32],
            observed_at_ns: 7,
        };
        let snapshot = BridgeSnapshot {
            mint: BaseMintSnapshot {
                finalized_head_block_number: 42,
                confirmed_block_timestamp: 1,
                service_fee: Amount::new(2),
                max_service_fee: Amount::new(3),
                per_deposit_limit: Amount::new(4),
                mint_window_limit: Amount::new(5),
                mint_window_duration: 6,
                mint_window_started_at: 7,
                minted_in_window: Amount::new(8),
            },
            bridge_signer: [0xbb; 20],
            mint_authorization_epoch: 1,
            deposits_paused: false,
            withdrawals_paused: false,
        };
        let identity = ObservedBridgeIdentity {
            signer: snapshot.bridge_signer,
            runtime_sha256: [0xcc; 32],
        };
        let args = test_init_args();
        let first = rpc_audit_evidence(
            &args,
            finalized,
            snapshot,
            identity,
            Some([1; 32]),
            Some(40),
            None,
        );
        let second = rpc_audit_evidence(
            &args,
            FinalizedObservation {
                block_hash: [0xdd; 32],
                ..finalized
            },
            snapshot,
            identity,
            Some([2; 32]),
            Some(40),
            None,
        );
        assert_ne!(first.request_digest, second.request_digest);
        assert_ne!(first.quorum_response_digest, second.quorum_response_digest);
    }

    #[test]
    fn rpc_decisions_encode_fail_closed_and_no_resign_guarantees() {
        let continued = quorum_continued_decision("notify_withdrawal", Some([1; 32]), true);
        assert_eq!(continued.configured_provider_count, 3);
        assert_eq!(continued.required_threshold, 2);
        assert!(continued.ledger_call_performed);
        assert!(continued.bridge_operation_continued);
        assert!(continued.stop_reason.is_none());

        let loss = quorum_loss_decision("notify_withdrawal", Some([1; 32]));
        assert_eq!(loss.configured_provider_count, 3);
        assert_eq!(loss.required_threshold, 2);
        assert_eq!(loss.stop_reason.as_deref(), Some("RpcInconsistent"));
        assert!(!loss.ledger_call_performed);
        assert!(!loss.bridge_operation_continued);
    }

    fn test_init_args() -> BridgeInitArgs {
        BridgeInitArgs {
            ledger_canister_id: Principal::from_slice(&[3]),
            index_canister_id: Principal::from_slice(&[4]),
            evm_rpc_canister_id: Principal::from_slice(&[5]),
            bridge_contract: vec![0x42; 20],
            expected_bridge_runtime_sha256: vec![0x55; 32],
            timelock_contract: vec![0x43; 20],
            deployment_instance_id: vec![0x44; 32],
            base_chain_id: 8453,
            custom_evm_rpc_urls: vec![
                "https://rpc-1.example".to_owned(),
                "https://rpc-2.example".to_owned(),
                "https://rpc-3.example".to_owned(),
            ],
            ecdsa_key_name: "test_key_1".to_owned(),
            ecdsa_derivation_path: vec![],
            governance_ecdsa_derivation_path: vec![b"governance-operator".to_vec()],
            deposit_rate_limit_window_seconds: 60,
            deposit_rate_limit_global: 2,
            deposit_rate_limit_per_principal: 1,
            notification_rate_limit_window_seconds: 600,
            notification_rate_limit_global: 60,
            notification_ingestion_rate_limit_global: 30,
            settlement_rate_limit_window_seconds: 60,
            settlement_rate_limit_global: 3,
            settlement_rate_limit_per_principal: 2,
            settlement_rate_limit_per_record: 1,
            settlement_retry_interval_seconds: 60,
            governance_evm_fee: crate::config::EvmFeePolicy {
                gas_limit_ceiling: 1,
                max_fee_per_gas_ceiling: 2,
                max_priority_fee_per_gas_ceiling: 1,
                l1_fee_per_transaction_ceiling_wei: 1,
                quote_validity_seconds: 90,
                gas_limit_multiplier_bps: 13_000,
                base_fee_multiplier_bps: 60_000,
                l1_fee_multiplier_bps: 15_000,
            },
            governance_replacement: crate::config::GovernanceReplacementPolicy::default(),
            governance_eth_floor_wei: 1,
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: Principal::from_slice(&[1]),
            pause_principal: Principal::from_slice(&[2]),
            fee_recipient: crate::config::FeeRecipientConfig {
                owner: Principal::from_slice(&[7]),
                subaccount: vec![],
            },
        }
    }
}
