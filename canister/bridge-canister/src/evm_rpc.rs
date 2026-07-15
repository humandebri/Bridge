use crate::config::BridgeInitArgs;
use async_trait::async_trait;
use bridge_core::{Amount, BaseMintSnapshot};
use candid::{utils::ArgumentEncoder, CandidType, Principal};
use evm_rpc_client::{CandidResponseConverter, EvmRpcClient, NoRetry};
use evm_rpc_types::{
    Block, BlockTag, ConsensusStrategy, GetTransactionCountArgs, Hex, Hex20, Hex32, MultiRpcResult,
    Nat256, RpcApi, RpcServices, SendRawTransactionStatus, TransactionReceipt,
};
use ic_canister_runtime::{IcError, Runtime};
use ic_cdk::call::Call;
use serde::de::DeserializeOwned;
use serde_json::json;
use std::str::FromStr;
use tiny_keccak::{Hasher, Keccak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationError {
    Inconsistent,
    Rpc,
    InvalidResponse,
    Overflow,
    BaseStateMismatch,
    NonceConflict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedWithdrawal {
    pub id: [u8; 32],
    pub amount: u128,
    pub min_amount_out: u128,
    pub owner: Vec<u8>,
    pub subaccount: [u8; 32],
    pub requester: [u8; 20],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeSnapshot {
    pub mint: BaseMintSnapshot,
    pub bridge_signer: [u8; 20],
    pub deposits_paused: bool,
    pub withdrawals_paused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CurrentWithdrawal {
    requester: [u8; 20],
    amount: u128,
    min_amount_out: u128,
    owner: Vec<u8>,
    subaccount: [u8; 32],
    status: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotifiedWithdrawalOutcome {
    Missing,
    Pending {
        receipt_block_number: u64,
    },
    Reverted {
        receipt_block_number: u64,
        confirmed_block_number: u64,
    },
    Confirmed {
        withdrawal: ObservedWithdrawal,
        snapshot: Box<BridgeSnapshot>,
        receipt_block_number: u64,
        confirmed_block_number: u64,
    },
}

const SMALL_RESPONSE_BYTES: u64 = 4 * 1024;
const RECEIPT_RESPONSE_BYTES: u64 = 32 * 1024;
const SEND_RESPONSE_BYTES: u64 = 2 * 1024;
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

async fn eth_call_at(
    args: &BridgeInitArgs,
    calldata: &[u8],
    block_number: u64,
) -> Result<String, ObservationError> {
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_call",
        "params": [{
            "to": format!("0x{}", hex(&args.bridge_contract)),
            "data": format!("0x{}", hex(calldata)),
        }, format!("0x{block_number:x}")],
    });
    match client(args)
        .multi_request(request)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(value)) => Ok(value),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

pub async fn bridge_snapshot(args: &BridgeInitArgs) -> Result<BridgeSnapshot, ObservationError> {
    let safe = safe_block(args).await?;
    let block_number = u64::try_from(safe.number).map_err(|_| ObservationError::Overflow)?;
    let value = eth_call_at(args, &selector("bridgeSnapshot()"), block_number).await?;
    let snapshot = decode_bridge_snapshot(&value)?;
    if snapshot.mint.confirmed_block_number != block_number {
        return Err(ObservationError::InvalidResponse);
    }
    Ok(snapshot)
}

fn decode_bridge_snapshot(value: &str) -> Result<BridgeSnapshot, ObservationError> {
    let bytes = decode_hex(
        value
            .trim_matches('"')
            .strip_prefix("0x")
            .ok_or(ObservationError::InvalidResponse)?,
    )?;
    if bytes.len() != 12 * ABI_WORD_BYTES {
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
            confirmed_block_number: u64::try_from(word_u128(word(0)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            confirmed_block_timestamp: u64::try_from(word_u128(word(1)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            service_fee: Amount::new(word_u128(word(3)?)?),
            max_service_fee: Amount::new(word_u128(word(4)?)?),
            per_deposit_limit: Amount::new(word_u128(word(5)?)?),
            mint_window_limit: Amount::new(word_u128(word(6)?)?),
            mint_window_duration: u64::try_from(word_u128(word(7)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            mint_window_started_at: u64::try_from(word_u128(word(8)?)?)
                .map_err(|_| ObservationError::Overflow)?,
            minted_in_window: Amount::new(word_u128(word(9)?)?),
        },
        bridge_signer: address_word[12..]
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        deposits_paused: boolean(10)?,
        withdrawals_paused: boolean(11)?,
    })
}

async fn safe_block(args: &BridgeInitArgs) -> Result<Block, ObservationError> {
    match client(args)
        .get_block_by_number(BlockTag::Safe)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(block)) => Ok(block),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

async fn confirmation_block(args: &BridgeInitArgs) -> Result<Block, ObservationError> {
    match client(args)
        .get_block_by_number(BlockTag::Safe)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(block)) => Ok(block),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

/// Returns a receipt only after proving that its block hash is canonical at its height
/// and that the height is not newer than the provider-consistent safe head.
enum CanonicalReceiptOutcome {
    Missing,
    Pending {
        receipt_block_number: u64,
    },
    Confirmed {
        receipt: Box<TransactionReceipt>,
        confirmed_block_number: u64,
    },
}

async fn canonical_confirmed_receipt(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<CanonicalReceiptOutcome, ObservationError> {
    let hash = Hex32::from_str(&format!("0x{}", hex(&transaction_hash)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let receipt = match client(args)
        .get_transaction_receipt(hash.clone())
        .with_response_size_estimate(RECEIPT_RESPONSE_BYTES)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(receipt)) => receipt,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    let Some(receipt) = receipt else {
        return Ok(CanonicalReceiptOutcome::Missing);
    };
    if receipt.transaction_hash != hash {
        return Err(ObservationError::InvalidResponse);
    }
    let receipt_block =
        u64::try_from(receipt.block_number.clone()).map_err(|_| ObservationError::Overflow)?;
    let confirmed = confirmation_block(args).await?;
    let confirmed_number =
        u64::try_from(confirmed.number).map_err(|_| ObservationError::Overflow)?;
    if receipt_block > confirmed_number {
        return Ok(CanonicalReceiptOutcome::Pending {
            receipt_block_number: receipt_block,
        });
    }
    let canonical = match client(args)
        .get_block_by_number(BlockTag::Number(Nat256::from(receipt_block)))
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(block)) => block,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    if canonical.number != Nat256::from(receipt_block) || canonical.hash != receipt.block_hash {
        return Err(ObservationError::InvalidResponse);
    }
    Ok(CanonicalReceiptOutcome::Confirmed {
        receipt: Box::new(receipt),
        confirmed_block_number: confirmed_number,
    })
}

pub async fn transaction_count(
    args: &BridgeInitArgs,
    address: [u8; 20],
) -> Result<u64, ObservationError> {
    let address = Hex20::from_str(&format!("0x{}", hex(&address)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    match client(args)
        .get_transaction_count(GetTransactionCountArgs {
            address,
            block: BlockTag::Pending,
        })
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(value)) => {
            u64::try_from(value).map_err(|_| ObservationError::Overflow)
        }
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

pub async fn signer_eth_balance(
    args: &BridgeInitArgs,
    address: [u8; 20],
) -> Result<u128, ObservationError> {
    let request = json!({ "jsonrpc":"2.0", "id":1, "method":"eth_getBalance", "params":[format!("0x{}", hex(&address)), "safe"] });
    match client(args)
        .multi_request(request)
        .with_response_size_estimate(SMALL_RESPONSE_BYTES)
        .send()
        .await
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

pub async fn broadcast(args: &BridgeInitArgs, raw: &[u8]) -> Result<(), ObservationError> {
    let mut transaction_hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(raw);
    hasher.finalize(&mut transaction_hash);
    let raw =
        Hex::from_str(&format!("0x{}", hex(raw))).map_err(|_| ObservationError::InvalidResponse)?;
    match client(args)
        .send_raw_transaction(raw)
        .with_response_size_estimate(SEND_RESPONSE_BYTES)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(SendRawTransactionStatus::Ok(_))) => Ok(()),
        MultiRpcResult::Consistent(Ok(SendRawTransactionStatus::NonceTooLow)) => {
            let request = json!({
                "jsonrpc": "2.0", "id": 1, "method": "eth_getTransactionByHash",
                "params": [format!("0x{}", hex(&transaction_hash))],
            });
            let known = match client(args)
                .multi_request(request)
                .with_response_size_estimate(SMALL_RESPONSE_BYTES)
                .send()
                .await
            {
                MultiRpcResult::Consistent(Ok(value)) => {
                    serde_json::from_str::<serde_json::Value>(&value)
                        .ok()
                        .and_then(|value| {
                            value
                                .get("hash")
                                .and_then(|hash| hash.as_str())
                                .map(str::to_owned)
                        })
                        .is_some_and(|hash| {
                            hash.eq_ignore_ascii_case(&format!("0x{}", hex(&transaction_hash)))
                        })
                }
                MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
                MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
            };
            if bridge_core::nonce_too_low_is_submitted(true, known) {
                Ok(())
            } else {
                Err(ObservationError::NonceConflict)
            }
        }
        MultiRpcResult::Consistent(Ok(_)) | MultiRpcResult::Consistent(Err(_)) => {
            Err(ObservationError::Rpc)
        }
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

pub async fn notified_withdrawal_outcome(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<NotifiedWithdrawalOutcome, ObservationError> {
    let hash = Hex32::from_str(&format!("0x{}", hex(&transaction_hash)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let (receipt, confirmed_block_number) =
        match canonical_confirmed_receipt(args, transaction_hash).await? {
            CanonicalReceiptOutcome::Missing => return Ok(NotifiedWithdrawalOutcome::Missing),
            CanonicalReceiptOutcome::Pending {
                receipt_block_number,
            } => {
                return Ok(NotifiedWithdrawalOutcome::Pending {
                    receipt_block_number,
                })
            }
            CanonicalReceiptOutcome::Confirmed {
                receipt,
                confirmed_block_number,
            } => (*receipt, confirmed_block_number),
        };
    let receipt_block_number =
        u64::try_from(receipt.block_number.clone()).map_err(|_| ObservationError::Overflow)?;
    match receipt.status {
        Some(status) if status == Nat256::from(0u64) => {
            return Ok(NotifiedWithdrawalOutcome::Reverted {
                receipt_block_number,
                confirmed_block_number,
            });
        }
        Some(status) if status == Nat256::from(1u64) => {}
        _ => return Err(ObservationError::InvalidResponse),
    }
    let address = Hex20::from_str(&format!("0x{}", hex(&args.bridge_contract)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let mut topic = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(b"WithdrawalCreated(uint256,address,uint256,uint256,bytes,bytes32)");
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
    let current = decode_current_withdrawal(
        &eth_call_at(args, &withdrawal_calldata, confirmed_block_number).await?,
    )?;
    if current.status != 2
        || current.requester != withdrawal.requester
        || current.amount != withdrawal.amount
        || current.min_amount_out != withdrawal.min_amount_out
        || current.owner != withdrawal.owner
        || current.subaccount != withdrawal.subaccount
    {
        return Err(ObservationError::BaseStateMismatch);
    }
    let snapshot = decode_bridge_snapshot(
        &eth_call_at(args, &selector("bridgeSnapshot()"), confirmed_block_number).await?,
    )?;
    if snapshot.mint.confirmed_block_number != confirmed_block_number {
        return Err(ObservationError::InvalidResponse);
    }
    Ok(NotifiedWithdrawalOutcome::Confirmed {
        withdrawal,
        snapshot: Box::new(snapshot),
        receipt_block_number,
        confirmed_block_number,
    })
}

fn decode_withdrawal_created_log(
    log: &evm_rpc_types::LogEntry,
) -> Result<ObservedWithdrawal, ObservationError> {
    let data = log.data.as_ref();
    if data.len() < 6 * ABI_WORD_BYTES {
        return Err(ObservationError::InvalidResponse);
    }
    let word = |index: usize| -> Result<&[u8], ObservationError> {
        data.get(index * ABI_WORD_BYTES..(index + 1) * ABI_WORD_BYTES)
            .ok_or(ObservationError::InvalidResponse)
    };
    let owner_offset =
        usize::try_from(word_u128(word(2)?)?).map_err(|_| ObservationError::Overflow)?;
    if owner_offset != 4 * ABI_WORD_BYTES {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_len =
        usize::try_from(word_u128(word(4)?)?).map_err(|_| ObservationError::Overflow)?;
    if !(1..=29).contains(&owner_len) {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_end = 5 * ABI_WORD_BYTES + owner_len;
    let expected_len = 5 * ABI_WORD_BYTES + owner_len.div_ceil(ABI_WORD_BYTES) * ABI_WORD_BYTES;
    if data.len() != expected_len || data[owner_end..].iter().any(|byte| *byte != 0) {
        return Err(ObservationError::InvalidResponse);
    }
    let owner = data
        .get(5 * ABI_WORD_BYTES..owner_end)
        .ok_or(ObservationError::InvalidResponse)?
        .to_vec();
    Ok(ObservedWithdrawal {
        id: *log.topics[1].as_array(),
        requester: log.topics[2].as_array()[12..]
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        amount: word_u128(word(0)?)?,
        min_amount_out: word_u128(word(1)?)?,
        owner,
        subaccount: word(3)?
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
    if bytes.len() < 12 * ABI_WORD_BYTES
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
        usize::try_from(word_u128(word(3)?)?).map_err(|_| ObservationError::Overflow)?;
    if owner_offset != 10 * ABI_WORD_BYTES {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_len =
        usize::try_from(word_u128(word(10)?)?).map_err(|_| ObservationError::Overflow)?;
    if !(1..=29).contains(&owner_len) {
        return Err(ObservationError::InvalidResponse);
    }
    let owner_start = tuple_start + 11 * ABI_WORD_BYTES;
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
        u8::try_from(word_u128(word(5)?)?).map_err(|_| ObservationError::InvalidResponse)?;
    Ok(CurrentWithdrawal {
        requester: requester_word[12..]
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        amount: word_u128(word(1)?)?,
        min_amount_out: word_u128(word(2)?)?,
        owner: bytes[owner_start..owner_end].to_vec(),
        subaccount: word(4)?
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
        status,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmedReceiptOutcome {
    Missing,
    Succeeded {
        receipt_block_number: u64,
        confirmed_block_number: u64,
    },
    Reverted {
        receipt_block_number: u64,
        confirmed_block_number: u64,
    },
}

pub async fn confirmed_receipt_outcome(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<ConfirmedReceiptOutcome, ObservationError> {
    let (receipt, confirmed_head) =
        match canonical_confirmed_receipt(args, transaction_hash).await? {
            CanonicalReceiptOutcome::Missing | CanonicalReceiptOutcome::Pending { .. } => {
                return Ok(ConfirmedReceiptOutcome::Missing)
            }
            CanonicalReceiptOutcome::Confirmed {
                receipt,
                confirmed_block_number,
            } => (*receipt, confirmed_block_number),
        };
    let receipt_block =
        u64::try_from(receipt.block_number).map_err(|_| ObservationError::Overflow)?;
    match receipt.status {
        Some(status) if status == evm_rpc_types::Nat256::from(1u64) => {
            Ok(ConfirmedReceiptOutcome::Succeeded {
                receipt_block_number: receipt_block,
                confirmed_block_number: confirmed_head,
            })
        }
        Some(status) if status == evm_rpc_types::Nat256::from(0u64) => {
            Ok(ConfirmedReceiptOutcome::Reverted {
                receipt_block_number: receipt_block,
                confirmed_block_number: confirmed_head,
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
        let mut bytes = vec![0u8; 13 * WORD];
        bytes[31] = WORD as u8;
        let base = WORD;
        bytes[base + 12..base + WORD].fill(0x11);
        bytes[base + 2 * WORD - 1] = 100;
        bytes[base + 3 * WORD - 1] = 80;
        bytes[base + 4 * WORD - 2] = 1;
        bytes[base + 4 * WORD - 1] = 64;
        bytes[base + 4 * WORD..base + 5 * WORD].fill(0x22);
        bytes[base + 6 * WORD - 1] = 2;
        bytes[base + 11 * WORD - 1] = owner.len() as u8;
        bytes[base + 11 * WORD..base + 11 * WORD + owner.len()].copy_from_slice(&owner);
        let decoded = decode_current_withdrawal(&format!("0x{}", hex(&bytes)))
            .expect("valid dynamic Withdrawal tuple");
        assert_eq!(decoded.requester, [0x11; 20]);
        assert_eq!(decoded.amount, 100);
        assert_eq!(decoded.min_amount_out, 80);
        assert_eq!(decoded.owner, owner);
        assert_eq!(decoded.subaccount, [0x22; 32]);
        assert_eq!(decoded.status, 2);
    }
}
