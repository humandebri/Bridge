use crate::config::BridgeInitArgs;
use bridge_core::{Amount, BaseMintSnapshot};
use evm_rpc_client::{CandidResponseConverter, EvmRpcClient, NoRetry};
use evm_rpc_types::{
    BlockTag, ConsensusStrategy, GetLogsArgs, GetTransactionCountArgs, Hex, Hex20, Hex32,
    MultiRpcResult, Nat256, RpcApi, RpcServices, SendRawTransactionStatus,
};
use ic_canister_runtime::IcRuntime;
use serde_json::json;
use std::str::FromStr;
use tiny_keccak::{Hasher, Keccak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationError {
    Inconsistent,
    Rpc,
    InvalidResponse,
    Overflow,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedWithdrawal {
    pub id: [u8; 32],
    pub amount: u128,
    pub min_amount_out: u128,
    pub owner: Vec<u8>,
    pub subaccount: [u8; 32],
}

fn client(args: &BridgeInitArgs) -> EvmRpcClient<IcRuntime, CandidResponseConverter, NoRetry> {
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
    EvmRpcClient::builder(IcRuntime::new(), args.evm_rpc_canister_id)
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

async fn finalized_uint(args: &BridgeInitArgs, signature: &str) -> Result<u128, ObservationError> {
    let (block_number, _) = finalized_block(args).await?;
    finalized_uint_at(args, signature, block_number).await
}

async fn finalized_uint_at(
    args: &BridgeInitArgs,
    signature: &str,
    block_number: u64,
) -> Result<u128, ObservationError> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [{
            "to": format!("0x{}", hex(&args.bridge_contract)),
            "data": format!("0x{}", hex(&selector(signature))),
        }, format!("0x{block_number:x}")],
    });
    match client(args).multi_request(request).send().await {
        MultiRpcResult::Consistent(Ok(value)) => parse_u128(&value),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

async fn finalized_block(args: &BridgeInitArgs) -> Result<(u64, u64), ObservationError> {
    let block = match client(args)
        .get_block_by_number(BlockTag::Finalized)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(block)) => block,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    Ok((
        u64::try_from(block.number).map_err(|_| ObservationError::Overflow)?,
        u64::try_from(block.timestamp).map_err(|_| ObservationError::Overflow)?,
    ))
}

pub async fn is_deposit_processed(
    args: &BridgeInitArgs,
    deposit_id: [u8; 32],
) -> Result<bool, ObservationError> {
    is_deposit_processed_at(args, deposit_id, "finalized").await
}

pub async fn is_deposit_processed_at_block(
    args: &BridgeInitArgs,
    deposit_id: [u8; 32],
    block_number: u64,
) -> Result<bool, ObservationError> {
    is_deposit_processed_at(args, deposit_id, &format!("0x{block_number:x}")).await
}

async fn is_deposit_processed_at(
    args: &BridgeInitArgs,
    deposit_id: [u8; 32],
    block: &str,
) -> Result<bool, ObservationError> {
    let mut calldata = selector("isDepositProcessed(bytes32)").to_vec();
    calldata.extend_from_slice(&deposit_id);
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_call",
        "params": [{
            "to": format!("0x{}", hex(&args.bridge_contract)),
            "data": format!("0x{}", hex(&calldata)),
        }, block],
    });
    match client(args).multi_request(request).send().await {
        MultiRpcResult::Consistent(Ok(value)) => Ok(parse_u128(&value)? == 1),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

pub async fn base_mint_snapshot(
    args: &BridgeInitArgs,
) -> Result<BaseMintSnapshot, ObservationError> {
    let (finalized_block_number, finalized_block_timestamp) = finalized_block(args).await?;
    let service_fee = finalized_uint_at(args, "serviceFee()", finalized_block_number).await?;
    let max_service_fee =
        finalized_uint_at(args, "MAX_SERVICE_FEE()", finalized_block_number).await?;
    let per_deposit_limit =
        finalized_uint_at(args, "perDepositLimit()", finalized_block_number).await?;
    let mint_window_limit =
        finalized_uint_at(args, "mintWindowLimit()", finalized_block_number).await?;
    let mint_window_started_at = u64::try_from(
        finalized_uint_at(args, "mintWindowStartedAt()", finalized_block_number).await?,
    )
    .map_err(|_| ObservationError::Overflow)?;
    let mint_window_duration = u64::try_from(
        finalized_uint_at(args, "mintWindowDuration()", finalized_block_number).await?,
    )
    .map_err(|_| ObservationError::Overflow)?;
    let minted_in_window =
        finalized_uint_at(args, "mintedInWindow()", finalized_block_number).await?;
    Ok(BaseMintSnapshot {
        finalized_block_number,
        finalized_block_timestamp,
        service_fee: Amount::new(service_fee),
        max_service_fee: Amount::new(max_service_fee),
        per_deposit_limit: Amount::new(per_deposit_limit),
        mint_window_limit: Amount::new(mint_window_limit),
        mint_window_started_at,
        mint_window_duration,
        minted_in_window: Amount::new(minted_in_window),
    })
}

pub async fn service_fee(args: &BridgeInitArgs) -> Result<(u128, u128), ObservationError> {
    Ok((
        finalized_uint(args, "serviceFee()").await?,
        finalized_uint(args, "MAX_SERVICE_FEE()").await?,
    ))
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
    let request = json!({ "jsonrpc":"2.0", "id":1, "method":"eth_getBalance", "params":[format!("0x{}", hex(&address)), "finalized"] });
    match client(args).multi_request(request).send().await {
        MultiRpcResult::Consistent(Ok(value)) => parse_u128(&value),
        MultiRpcResult::Consistent(Err(_)) => Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

pub async fn discover_withdrawals(
    args: &BridgeInitArgs,
    from_block: u64,
) -> Result<(u64, Vec<[u8; 32]>), ObservationError> {
    let finalized = match client(args)
        .get_block_by_number(BlockTag::Finalized)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(block)) => {
            u64::try_from(block.number).map_err(|_| ObservationError::Overflow)?
        }
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    if from_block > finalized {
        return Ok((from_block, Vec::new()));
    }
    let to_block = finalized.min(from_block.saturating_add(999));
    let address = Hex20::from_str(&format!("0x{}", hex(&args.bridge_contract)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let mut topic = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(b"WithdrawalCreated(uint256,address,uint256,uint256,bytes,bytes32)");
    hasher.finalize(&mut topic);
    let logs = client(args)
        .get_logs(GetLogsArgs {
            from_block: Some(BlockTag::Number(Nat256::from(from_block))),
            to_block: Some(BlockTag::Number(Nat256::from(to_block))),
            addresses: vec![address.clone()],
            topics: Some(vec![vec![Hex32::from_str(&format!("0x{}", hex(&topic)))
                .map_err(|_| {
                ObservationError::InvalidResponse
            })?]]),
        })
        .with_response_size_estimate(200_000)
        .send()
        .await;
    let logs = match logs {
        MultiRpcResult::Consistent(Ok(logs)) => logs,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    let ids = logs
        .into_iter()
        .filter(|log| !log.removed && log.address == address && log.topics.len() >= 2)
        .map(|log| *log.topics[1].as_array())
        .collect();
    Ok((to_block.saturating_add(1), ids))
}

pub async fn finalized_withdrawal(
    args: &BridgeInitArgs,
    id: [u8; 32],
) -> Result<Option<ObservedWithdrawal>, ObservationError> {
    let mut calldata = selector("getWithdrawal(uint256)").to_vec();
    calldata.extend_from_slice(&id);
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_call",
        "params": [{
            "to": format!("0x{}", hex(&args.bridge_contract)),
            "data": format!("0x{}", hex(&calldata)),
        }, "finalized"],
    });
    let value = match client(args).multi_request(request).send().await {
        MultiRpcResult::Consistent(Ok(value)) => value,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    decode_withdrawal(id, &value)
}

pub async fn finalized_withdrawal_status(
    args: &BridgeInitArgs,
    id: [u8; 32],
) -> Result<u8, ObservationError> {
    withdrawal_status_at(args, id, "finalized").await
}

pub async fn withdrawal_status_at_block(
    args: &BridgeInitArgs,
    id: [u8; 32],
    block_number: u64,
) -> Result<u8, ObservationError> {
    withdrawal_status_at(args, id, &format!("0x{block_number:x}")).await
}

async fn withdrawal_status_at(
    args: &BridgeInitArgs,
    id: [u8; 32],
    block: &str,
) -> Result<u8, ObservationError> {
    let mut calldata = selector("getWithdrawal(uint256)").to_vec();
    calldata.extend_from_slice(&id);
    let request = json!({
        "jsonrpc": "2.0", "id": 1, "method": "eth_call",
        "params": [{"to": format!("0x{}", hex(&args.bridge_contract)), "data": format!("0x{}", hex(&calldata))}, block],
    });
    let value = match client(args).multi_request(request).send().await {
        MultiRpcResult::Consistent(Ok(value)) => value,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    decode_withdrawal_status(&value)
}

const ABI_WORD_BYTES: usize = 32;
const WITHDRAWAL_TUPLE_OFFSET: usize = ABI_WORD_BYTES;
const WITHDRAWAL_HEAD_WORDS: usize = 10;

struct WithdrawalTuple<'a> {
    bytes: &'a [u8],
    start: usize,
}

impl WithdrawalTuple<'_> {
    fn word(&self, index: usize) -> Result<&[u8], ObservationError> {
        let offset = index
            .checked_mul(ABI_WORD_BYTES)
            .and_then(|offset| self.start.checked_add(offset))
            .ok_or(ObservationError::Overflow)?;
        let end = offset
            .checked_add(ABI_WORD_BYTES)
            .ok_or(ObservationError::Overflow)?;
        self.bytes
            .get(offset..end)
            .ok_or(ObservationError::InvalidResponse)
    }

    fn uint(&self, index: usize) -> Result<u128, ObservationError> {
        word_u128(self.word(index)?)
    }

    fn status(&self) -> Result<u8, ObservationError> {
        u8::try_from(self.uint(5)?).map_err(|_| ObservationError::Overflow)
    }

    fn dynamic_bytes(&self, index: usize) -> Result<&[u8], ObservationError> {
        let relative_offset =
            usize::try_from(self.uint(index)?).map_err(|_| ObservationError::Overflow)?;
        let head_bytes = WITHDRAWAL_HEAD_WORDS
            .checked_mul(ABI_WORD_BYTES)
            .ok_or(ObservationError::Overflow)?;
        if relative_offset != head_bytes {
            return Err(ObservationError::InvalidResponse);
        }
        let length_offset = self
            .start
            .checked_add(relative_offset)
            .ok_or(ObservationError::Overflow)?;
        let length_end = length_offset
            .checked_add(ABI_WORD_BYTES)
            .ok_or(ObservationError::Overflow)?;
        let length_word = self
            .bytes
            .get(length_offset..length_end)
            .ok_or(ObservationError::InvalidResponse)?;
        let length =
            usize::try_from(word_u128(length_word)?).map_err(|_| ObservationError::Overflow)?;
        let value_end = length_end
            .checked_add(length)
            .ok_or(ObservationError::Overflow)?;
        self.bytes
            .get(length_end..value_end)
            .ok_or(ObservationError::InvalidResponse)
    }
}

fn parse_withdrawal_tuple(bytes: &[u8]) -> Result<WithdrawalTuple<'_>, ObservationError> {
    let tuple_offset_word = bytes
        .get(..ABI_WORD_BYTES)
        .ok_or(ObservationError::InvalidResponse)?;
    let tuple_offset =
        usize::try_from(word_u128(tuple_offset_word)?).map_err(|_| ObservationError::Overflow)?;
    if tuple_offset != WITHDRAWAL_TUPLE_OFFSET {
        return Err(ObservationError::InvalidResponse);
    }
    let head_end = tuple_offset
        .checked_add(
            WITHDRAWAL_HEAD_WORDS
                .checked_mul(ABI_WORD_BYTES)
                .ok_or(ObservationError::Overflow)?,
        )
        .ok_or(ObservationError::Overflow)?;
    if bytes.len() < head_end {
        return Err(ObservationError::InvalidResponse);
    }
    Ok(WithdrawalTuple {
        bytes,
        start: tuple_offset,
    })
}

fn decode_withdrawal_status(value: &str) -> Result<u8, ObservationError> {
    let bytes = decode_hex(
        value
            .trim_matches('"')
            .strip_prefix("0x")
            .ok_or(ObservationError::InvalidResponse)?,
    )?;
    parse_withdrawal_tuple(&bytes)?.status()
}

fn decode_withdrawal(
    id: [u8; 32],
    value: &str,
) -> Result<Option<ObservedWithdrawal>, ObservationError> {
    let bytes = decode_hex(
        value
            .trim_matches('"')
            .strip_prefix("0x")
            .ok_or(ObservationError::InvalidResponse)?,
    )?;
    let withdrawal = parse_withdrawal_tuple(&bytes)?;
    let status = withdrawal.status()?;
    if status != 1 {
        return Ok(None);
    }
    let owner = withdrawal.dynamic_bytes(3)?;
    if owner.is_empty() || owner.len() > 29 {
        return Err(ObservationError::InvalidResponse);
    }
    Ok(Some(ObservedWithdrawal {
        id,
        amount: withdrawal.uint(1)?,
        min_amount_out: withdrawal.uint(2)?,
        owner: owner.to_vec(),
        subaccount: withdrawal
            .word(4)?
            .try_into()
            .map_err(|_| ObservationError::InvalidResponse)?,
    }))
}

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
    let raw =
        Hex::from_str(&format!("0x{}", hex(raw))).map_err(|_| ObservationError::InvalidResponse)?;
    match client(args).send_raw_transaction(raw).send().await {
        MultiRpcResult::Consistent(Ok(SendRawTransactionStatus::Ok(_))) => Ok(()),
        MultiRpcResult::Consistent(Ok(SendRawTransactionStatus::NonceTooLow)) => Ok(()),
        MultiRpcResult::Consistent(Ok(_)) | MultiRpcResult::Consistent(Err(_)) => {
            Err(ObservationError::Rpc)
        }
        MultiRpcResult::Inconsistent(_) => Err(ObservationError::Inconsistent),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinalizedReceiptOutcome {
    Missing,
    Succeeded {
        receipt_block_number: u64,
        finalized_block_number: u64,
    },
    Reverted {
        receipt_block_number: u64,
        finalized_block_number: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafeReceiptOutcome {
    Missing {
        safe_block_number: u64,
    },
    Succeeded {
        receipt_block_number: u64,
        safe_block_number: u64,
    },
    Reverted {
        receipt_block_number: u64,
        safe_block_number: u64,
    },
}

fn classify_safe_receipt(
    receipt: Option<(u64, bool)>,
    safe_block_number: u64,
) -> SafeReceiptOutcome {
    let Some((receipt_block_number, succeeded)) = receipt else {
        return SafeReceiptOutcome::Missing { safe_block_number };
    };
    if receipt_block_number > safe_block_number {
        return SafeReceiptOutcome::Missing { safe_block_number };
    }
    if succeeded {
        SafeReceiptOutcome::Succeeded {
            receipt_block_number,
            safe_block_number,
        }
    } else {
        SafeReceiptOutcome::Reverted {
            receipt_block_number,
            safe_block_number,
        }
    }
}

pub async fn safe_receipt_outcome(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<SafeReceiptOutcome, ObservationError> {
    let hash = Hex32::from_str(&format!("0x{}", hex(&transaction_hash)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let receipt = match client(args).get_transaction_receipt(hash).send().await {
        MultiRpcResult::Consistent(Ok(receipt)) => receipt,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    let safe = match client(args)
        .get_block_by_number(BlockTag::Safe)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(block)) => block,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    let safe_block_number = u64::try_from(safe.number).map_err(|_| ObservationError::Overflow)?;
    let receipt = receipt
        .map(|receipt| {
            let succeeded = match receipt.status {
                Some(status) if status == evm_rpc_types::Nat256::from(1u64) => true,
                Some(status) if status == evm_rpc_types::Nat256::from(0u64) => false,
                _ => return Err(ObservationError::InvalidResponse),
            };
            Ok((
                u64::try_from(receipt.block_number).map_err(|_| ObservationError::Overflow)?,
                succeeded,
            ))
        })
        .transpose()?;
    Ok(classify_safe_receipt(receipt, safe_block_number))
}

pub async fn finalized_receipt_outcome(
    args: &BridgeInitArgs,
    transaction_hash: [u8; 32],
) -> Result<FinalizedReceiptOutcome, ObservationError> {
    let hash = Hex32::from_str(&format!("0x{}", hex(&transaction_hash)))
        .map_err(|_| ObservationError::InvalidResponse)?;
    let receipt = match client(args).get_transaction_receipt(hash).send().await {
        MultiRpcResult::Consistent(Ok(receipt)) => receipt,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    let Some(receipt) = receipt else {
        return Ok(FinalizedReceiptOutcome::Missing);
    };
    let receipt_block =
        u64::try_from(receipt.block_number).map_err(|_| ObservationError::Overflow)?;
    let finalized = match client(args)
        .get_block_by_number(BlockTag::Finalized)
        .send()
        .await
    {
        MultiRpcResult::Consistent(Ok(block)) => block,
        MultiRpcResult::Consistent(Err(_)) => return Err(ObservationError::Rpc),
        MultiRpcResult::Inconsistent(_) => return Err(ObservationError::Inconsistent),
    };
    let finalized_block =
        u64::try_from(finalized.number).map_err(|_| ObservationError::Overflow)?;
    if receipt_block > finalized_block {
        return Ok(FinalizedReceiptOutcome::Missing);
    }
    match receipt.status {
        Some(status) if status == evm_rpc_types::Nat256::from(1u64) => {
            Ok(FinalizedReceiptOutcome::Succeeded {
                receipt_block_number: receipt_block,
                finalized_block_number: finalized_block,
            })
        }
        Some(status) if status == evm_rpc_types::Nat256::from(0u64) => {
            Ok(FinalizedReceiptOutcome::Reverted {
                receipt_block_number: receipt_block,
                finalized_block_number: finalized_block,
            })
        }
        _ => Err(ObservationError::InvalidResponse),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_word(target: &mut [u8], value: u128) {
        target[16..].copy_from_slice(&value.to_be_bytes());
    }

    fn withdrawal_abi(status: u8, owner: &[u8]) -> String {
        let tuple_start = WITHDRAWAL_TUPLE_OFFSET;
        let head_bytes = WITHDRAWAL_HEAD_WORDS * ABI_WORD_BYTES;
        let owner_length_offset = tuple_start + head_bytes;
        let padded_owner_length = owner.len().div_ceil(ABI_WORD_BYTES) * ABI_WORD_BYTES;
        let mut bytes = vec![0u8; owner_length_offset + ABI_WORD_BYTES + padded_owner_length];
        put_word(&mut bytes[..ABI_WORD_BYTES], tuple_start as u128);
        bytes[tuple_start + 12..tuple_start + ABI_WORD_BYTES].fill(0x11);
        put_word(
            &mut bytes[tuple_start + ABI_WORD_BYTES..tuple_start + 2 * ABI_WORD_BYTES],
            100,
        );
        put_word(
            &mut bytes[tuple_start + 2 * ABI_WORD_BYTES..tuple_start + 3 * ABI_WORD_BYTES],
            90,
        );
        put_word(
            &mut bytes[tuple_start + 3 * ABI_WORD_BYTES..tuple_start + 4 * ABI_WORD_BYTES],
            head_bytes as u128,
        );
        bytes[tuple_start + 4 * ABI_WORD_BYTES..tuple_start + 5 * ABI_WORD_BYTES].fill(0x07);
        put_word(
            &mut bytes[tuple_start + 5 * ABI_WORD_BYTES..tuple_start + 6 * ABI_WORD_BYTES],
            u128::from(status),
        );
        put_word(
            &mut bytes[owner_length_offset..owner_length_offset + ABI_WORD_BYTES],
            owner.len() as u128,
        );
        bytes[owner_length_offset + ABI_WORD_BYTES
            ..owner_length_offset + ABI_WORD_BYTES + owner.len()]
            .copy_from_slice(owner);
        format!("0x{}", hex(&bytes))
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
    fn withdrawal_decoding_observes_the_dynamic_tuple_boundary() {
        let owner = [0x2a, 0x2b, 0x2c];
        let decoded = decode_withdrawal([9; 32], &withdrawal_abi(1, &owner))
            .expect("valid ABI")
            .expect("pending withdrawal");
        assert_eq!(decoded.id, [9; 32]);
        assert_eq!(decoded.amount, 100);
        assert_eq!(decoded.min_amount_out, 90);
        assert_eq!(decoded.owner, owner);
        assert_eq!(decoded.subaccount, [7; 32]);
        assert_eq!(decode_withdrawal_status(&withdrawal_abi(2, &owner)), Ok(2));
        assert_eq!(decode_withdrawal_status(&withdrawal_abi(3, &owner)), Ok(3));
    }

    #[test]
    fn withdrawal_decoding_rejects_legacy_offsets_and_truncated_dynamic_data() {
        assert_eq!(
            decode_withdrawal_status(&format!("0x{}", "00".repeat(320))),
            Err(ObservationError::InvalidResponse)
        );

        let fixture = withdrawal_abi(1, &[1]);
        let mut invalid_owner_offset =
            decode_hex(fixture.strip_prefix("0x").expect("hex fixture")).expect("fixture");
        let offset_start = WITHDRAWAL_TUPLE_OFFSET + 3 * ABI_WORD_BYTES;
        put_word(
            &mut invalid_owner_offset[offset_start..offset_start + ABI_WORD_BYTES],
            ABI_WORD_BYTES as u128,
        );
        assert_eq!(
            decode_withdrawal([1; 32], &format!("0x{}", hex(&invalid_owner_offset))),
            Err(ObservationError::InvalidResponse)
        );

        put_word(
            &mut invalid_owner_offset[offset_start..offset_start + ABI_WORD_BYTES],
            u128::MAX,
        );
        assert_eq!(
            decode_withdrawal([1; 32], &format!("0x{}", hex(&invalid_owner_offset))),
            Err(ObservationError::Overflow)
        );

        let mut truncated = withdrawal_abi(1, &[1, 2, 3]);
        truncated.truncate(truncated.len() - 60);
        assert_eq!(
            decode_withdrawal([1; 32], &truncated),
            Err(ObservationError::InvalidResponse)
        );
    }

    #[test]
    fn safe_receipt_classification_observes_exact_boundary_and_revert() {
        assert_eq!(
            classify_safe_receipt(None, 98),
            SafeReceiptOutcome::Missing {
                safe_block_number: 98
            }
        );
        assert_eq!(
            classify_safe_receipt(Some((99, true)), 98),
            SafeReceiptOutcome::Missing {
                safe_block_number: 98
            }
        );
        assert_eq!(
            classify_safe_receipt(Some((99, true)), 99),
            SafeReceiptOutcome::Succeeded {
                receipt_block_number: 99,
                safe_block_number: 99
            }
        );
        assert_eq!(
            classify_safe_receipt(Some((99, false)), 100),
            SafeReceiptOutcome::Reverted {
                receipt_block_number: 99,
                safe_block_number: 100
            }
        );
    }
}
