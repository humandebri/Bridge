use candid::{CandidType, Deserialize, Nat, Principal};
use evm_rpc_types::{
    Block, BlockTag, GetLogsArgs, GetTransactionCountArgs, Hex, Hex32, JsonRpcError,
    MultiRpcResult, Nat256, ProviderError, RpcConfig, RpcError, RpcService, RpcServices,
    SendRawTransactionStatus, TransactionReceipt,
};
use ic_cdk::call::Call;
use ic_cdk_management_canister::{
    ecdsa_public_key, sign_with_ecdsa, EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgs,
    SignWithEcdsaArgs,
};
use icrc_ledger_types::{
    icrc1::{
        account::Account,
        transfer::{TransferArg, TransferError},
    },
    icrc2::transfer_from::{TransferFromArgs, TransferFromError},
    icrc3::{
        archive::{ArchivedRange, QueryTxArchiveFn},
        blocks::GetBlocksRequest,
        transactions::{GetTransactionsResponse, Transaction, TransactionRange},
    },
};
use k256::ecdsa::{signature::hazmat::PrehashVerifier, Signature, VerifyingKey};
use serde::Serialize;
use std::{cell::RefCell, str::FromStr};

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct InitArgs {
    pub ledger_id: Principal,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerMode {
    Succeed,
    Duplicate,
    Trap,
    BadFee,
    InsufficientAllowance { allowance: u128 },
    InsufficientFunds { balance: u128 },
    TemporarilyUnavailable,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct WithdrawalFixture {
    pub id: Vec<u8>,
    pub owner: Vec<u8>,
    pub subaccount: Vec<u8>,
    pub amount: u128,
    pub max_service_fee: u128,
    pub charged_service_fee: u128,
    pub amount_out: u128,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct MintLogFixture {
    pub deposit_id: Vec<u8>,
    pub recipient: Vec<u8>,
    pub authorization_digest: Vec<u8>,
    pub gross_amount: u128,
    pub charged_service_fee: u128,
    pub minted_amount: u128,
    pub transaction_hash: Vec<u8>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptMode {
    Confirmed,
    DelayedConfirmed,
    Missing,
    Reverted,
    RpcFailure,
    Inconsistent,
    DecodeFailure,
    Orphaned,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainIdMode {
    Configured,
    Wrong,
    Inconsistent,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockMode {
    Canonical,
    FinalizedUnavailable,
    FinalizedInconsistent,
    CanonicalInconsistent,
    SameHeightDifferentHash,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct Status {
    num_blocks_synced: Nat,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct GetAccountTransactionsArgs {
    account: Account,
    start: Option<Nat>,
    max_results: Nat,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct IndexTransactions {
    balance: Nat,
    transactions: Vec<TransactionWithId>,
    oldest_tx_id: Option<Nat>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct TransactionWithId {
    id: Nat,
    transaction: Transaction,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
struct IndexError {
    message: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ChainKeyProbe {
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

#[ic_cdk::update]
async fn probe_chain_key(key_name: String) -> Result<ChainKeyProbe, String> {
    let key_id = EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: key_name,
    };
    let public_key = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: None,
        derivation_path: vec![],
        key_id: key_id.clone(),
    })
    .await
    .map_err(|error| format!("ecdsa_public_key: {error:?}"))?
    .public_key;
    let message_hash = [0x42; 32];
    let signature = sign_with_ecdsa(&SignWithEcdsaArgs {
        message_hash: message_hash.to_vec(),
        derivation_path: vec![],
        key_id,
    })
    .await
    .map_err(|error| format!("sign_with_ecdsa: {error:?}"))?
    .signature;
    let verifying_key = VerifyingKey::from_sec1_bytes(&public_key)
        .map_err(|error| format!("invalid SEC1 public key: {error}"))?;
    let parsed_signature = Signature::from_slice(&signature)
        .map_err(|error| format!("invalid raw signature: {error}"))?;
    verifying_key
        .verify_prehash(&message_hash, &parsed_signature)
        .map_err(|error| format!("signature verification failed: {error}"))?;
    Ok(ChainKeyProbe {
        public_key,
        signature,
    })
}

#[ic_cdk::update]
async fn derive_chain_key_address(
    canister_id: Principal,
    key_name: String,
    derivation_path: Vec<Vec<u8>>,
) -> Vec<u8> {
    let result = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: Some(canister_id),
        derivation_path,
        key_id: EcdsaKeyId {
            curve: EcdsaCurve::Secp256k1,
            name: key_name,
        },
    })
    .await
    .unwrap_or_else(|error| ic_cdk::trap(format!("ecdsa_public_key: {error:?}")));
    let key = VerifyingKey::from_sec1_bytes(&result.public_key)
        .unwrap_or_else(|error| ic_cdk::trap(format!("invalid SEC1 public key: {error}")));
    let uncompressed = key.to_encoded_point(false);
    let hash = keccak(&uncompressed.as_bytes()[1..]);
    hash[12..].to_vec()
}

#[derive(CandidType, Deserialize)]
struct StableMockState {
    ledger_id: Option<Principal>,
    ledger_mode: LedgerMode,
    ledger_fee_available: bool,
    ledger_fee: u128,
    next_block: u128,
    transactions: Vec<Transaction>,
    archive_prefix_length: u64,
    index_synced_blocks: Option<u128>,
    processed_deposit: bool,
    mint_log: Option<MintLogFixture>,
    last_tx_hash: [u8; 32],
    observed_contract: [u8; 20],
    observed_requester: [u8; 20],
    observed_block_number: u64,
    withdrawal: Option<WithdrawalFixture>,
    withdrawal_status: u8,
    receipt_mode: ReceiptMode,
    chain_id_mode: ChainIdMode,
    configured_chain_id: u64,
    bridge_runtime_code: Vec<u8>,
    timelock_runtime_code: Vec<u8>,
    bsns_runtime_code: Vec<u8>,
    bsns_address: [u8; 20],
    bsns_bridge: [u8; 20],
    block_mode: BlockMode,
    broadcast_inconsistent_after_accepts: u8,
    broadcasts: Vec<Vec<u8>>,
    accepted_tx_hashes: Vec<[u8; 32]>,
    eth_balance: u128,
    next_evm_nonce: u64,
    service_fee: u128,
    max_service_fee: u128,
    per_deposit_limit: u128,
    mint_window_limit: u128,
    minted_in_window: u128,
    mint_window_started_at: u64,
    mint_window_duration: u64,
    block_timestamp: u64,
    safe_block_sequence: Vec<u64>,
    safe_block_hash_override: Option<(u64, [u8; 32])>,
    finalized_block_sequence: Vec<u64>,
    finalized_block_hash_override: Option<(u64, [u8; 32])>,
    fallback_block_sequence: Vec<u64>,
    chain_id_call_count: u64,
    get_code_call_count: u64,
    eth_call_count: u64,
    deposit_processed_call_count: u64,
    pinned_eth_call_block_numbers: Vec<u64>,
    receipt_call_count: u64,
    receipt_mint_log_index: Option<u64>,
    bridge_signer: [u8; 20],
    mint_authorization_epoch: u64,
    deposit_mints_paused: bool,
    withdrawals_paused: bool,
}

thread_local! {
    static LEDGER_ID: RefCell<Option<Principal>> = const { RefCell::new(None) };
    static LEDGER_MODE: RefCell<LedgerMode> = const { RefCell::new(LedgerMode::Succeed) };
    static REFUND_LEDGER_MODE: RefCell<Option<LedgerMode>> = const { RefCell::new(None) };
    static LEDGER_FEE_AVAILABLE: RefCell<bool> = const { RefCell::new(true) };
    static LEDGER_FEE: RefCell<u128> = const { RefCell::new(1) };
    static LEDGER_TRANSFER_CALLS: RefCell<u64> = const { RefCell::new(0) };
    static NEXT_BLOCK: RefCell<u128> = const { RefCell::new(1) };
    static TRANSACTIONS: RefCell<Vec<Transaction>> = const { RefCell::new(Vec::new()) };
    static ARCHIVE_PREFIX_LENGTH: RefCell<u64> = const { RefCell::new(0) };
    static INDEX_SYNCED_BLOCKS: RefCell<Option<u128>> = const { RefCell::new(None) };
    static PROCESSED_DEPOSIT: RefCell<bool> = const { RefCell::new(false) };
    static MINT_LOG: RefCell<Option<MintLogFixture>> = const { RefCell::new(None) };
    static LAST_TX_HASH: RefCell<[u8; 32]> = const { RefCell::new([9; 32]) };
    static OBSERVED_CONTRACT: RefCell<[u8; 20]> = const { RefCell::new([1; 20]) };
    static OBSERVED_REQUESTER: RefCell<[u8; 20]> = const { RefCell::new([0x22; 20]) };
    static OBSERVED_BLOCK_NUMBER: RefCell<u64> = const { RefCell::new(99) };
    static WITHDRAWAL: RefCell<Option<WithdrawalFixture>> = const { RefCell::new(None) };
    static WITHDRAWAL_STATUS: RefCell<u8> = const { RefCell::new(1) };
    static RECEIPT_MODE: RefCell<ReceiptMode> = const { RefCell::new(ReceiptMode::Confirmed) };
    static CHAIN_ID_MODE: RefCell<ChainIdMode> = const { RefCell::new(ChainIdMode::Configured) };
    static CONFIGURED_CHAIN_ID: RefCell<u64> = const { RefCell::new(8_453) };
    static BRIDGE_RUNTIME_CODE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static TIMELOCK_RUNTIME_CODE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static BSNS_RUNTIME_CODE: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static BSNS_ADDRESS: RefCell<[u8; 20]> = const { RefCell::new([9; 20]) };
    static BSNS_BRIDGE: RefCell<[u8; 20]> = const { RefCell::new([1; 20]) };
    static BLOCK_MODE: RefCell<BlockMode> = const { RefCell::new(BlockMode::Canonical) };
    static BROADCAST_INCONSISTENT_AFTER_ACCEPTS: RefCell<u8> = const { RefCell::new(0) };
    static BROADCASTS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static ACCEPTED_TX_HASHES: RefCell<Vec<[u8; 32]>> = const { RefCell::new(Vec::new()) };
    static ETH_BALANCE: RefCell<u128> = const { RefCell::new(10_000_000_000_000_000_000) };
    static NEXT_EVM_NONCE: RefCell<u64> = const { RefCell::new(0) };
    static SERVICE_FEE: RefCell<u128> = const { RefCell::new(1) };
    static MAX_SERVICE_FEE: RefCell<u128> = const { RefCell::new(10) };
    static PER_DEPOSIT_LIMIT: RefCell<u128> = const { RefCell::new(1_000_000) };
    static MINT_WINDOW_LIMIT: RefCell<u128> = const { RefCell::new(10_000_000) };
    static MINTED_IN_WINDOW: RefCell<u128> = const { RefCell::new(0) };
    static MINT_WINDOW_STARTED_AT: RefCell<u64> = const { RefCell::new(0) };
    static MINT_WINDOW_DURATION: RefCell<u64> = const { RefCell::new(3_600) };
    static BLOCK_TIMESTAMP: RefCell<u64> = const { RefCell::new(1) };
    static SAFE_BLOCK_SEQUENCE: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static SAFE_BLOCK_HASH_OVERRIDE: RefCell<Option<(u64, [u8; 32])>> = const { RefCell::new(None) };
    static FINALIZED_BLOCK_SEQUENCE: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static FINALIZED_BLOCK_HASH_OVERRIDE: RefCell<Option<(u64, [u8; 32])>> = const { RefCell::new(None) };
    static FALLBACK_BLOCK_SEQUENCE: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static CHAIN_ID_CALL_COUNT: RefCell<u64> = const { RefCell::new(0) };
    static GET_CODE_CALL_COUNT: RefCell<u64> = const { RefCell::new(0) };
    static ETH_CALL_COUNT: RefCell<u64> = const { RefCell::new(0) };
    static DEPOSIT_PROCESSED_CALL_COUNT: RefCell<u64> = const { RefCell::new(0) };
    static PINNED_ETH_CALL_BLOCK_NUMBERS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static RECEIPT_CALL_COUNT: RefCell<u64> = const { RefCell::new(0) };
    static RECEIPT_MINT_LOG_INDEX: RefCell<Option<u64>> = const { RefCell::new(None) };
    static BRIDGE_SIGNER: RefCell<[u8; 20]> = const { RefCell::new([0; 20]) };
    static BASE_ADMIN_TIMELOCK: RefCell<[u8; 20]> = const { RefCell::new([0; 20]) };
    static RUNTIME_ADMINISTRATOR: RefCell<[u8; 20]> = const { RefCell::new([0; 20]) };
    static MINT_AUTHORIZATION_EPOCH: RefCell<u64> = const { RefCell::new(1) };
    static DEPOSIT_MINTS_PAUSED: RefCell<bool> = const { RefCell::new(false) };
    static WITHDRAWALS_PAUSED: RefCell<bool> = const { RefCell::new(false) };
}

#[ic_cdk::init]
fn init(args: InitArgs) {
    LEDGER_ID.with(|id| *id.borrow_mut() = Some(args.ledger_id));
}

#[ic_cdk::update]
fn set_ledger_mode(mode: LedgerMode) {
    LEDGER_MODE.with(|current| *current.borrow_mut() = mode);
}

#[ic_cdk::update]
fn set_refund_ledger_mode(mode: Option<LedgerMode>) {
    REFUND_LEDGER_MODE.with(|current| *current.borrow_mut() = mode);
}

#[ic_cdk::update]
fn set_archive_prefix_length(length: u64) {
    ARCHIVE_PREFIX_LENGTH.with(|current| *current.borrow_mut() = length);
}

#[ic_cdk::update]
fn set_index_synced_blocks(blocks: Option<u128>) {
    INDEX_SYNCED_BLOCKS.with(|current| *current.borrow_mut() = blocks);
}

#[ic_cdk::update]
fn set_ledger_fee_available(available: bool) {
    LEDGER_FEE_AVAILABLE.with(|current| *current.borrow_mut() = available);
}

#[ic_cdk::update]
fn set_ledger_fee(fee: u128) {
    LEDGER_FEE.with(|current| *current.borrow_mut() = fee);
}

#[ic_cdk::update]
fn set_withdrawal(value: Option<WithdrawalFixture>) {
    WITHDRAWAL.with(|current| *current.borrow_mut() = value);
    WITHDRAWAL_STATUS.with(|status| *status.borrow_mut() = 1);
    LAST_TX_HASH.with(|hash| *hash.borrow_mut() = [9; 32]);
}

#[ic_cdk::update]
fn set_withdrawal_status(status: u8) {
    WITHDRAWAL_STATUS.with(|current| *current.borrow_mut() = status);
}

#[ic_cdk::update]
fn set_receipt_mode(mode: ReceiptMode) {
    RECEIPT_MODE.with(|current| *current.borrow_mut() = mode);
}

#[ic_cdk::update]
fn receipt_delay_tick() {}

#[ic_cdk::update]
fn set_processed_deposit(processed: bool) {
    PROCESSED_DEPOSIT.with(|current| *current.borrow_mut() = processed);
}

#[ic_cdk::update]
fn set_mint_log(value: Option<MintLogFixture>) {
    MINT_LOG.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::update]
fn set_mint_authorization_epoch(epoch: u64) {
    MINT_AUTHORIZATION_EPOCH.with(|current| *current.borrow_mut() = epoch);
}

#[ic_cdk::update]
fn set_block_timestamp(timestamp: u64) {
    BLOCK_TIMESTAMP.with(|current| *current.borrow_mut() = timestamp);
}

#[ic_cdk::update]
fn set_chain_id_mode(mode: ChainIdMode) {
    CHAIN_ID_MODE.with(|current| *current.borrow_mut() = mode);
}

#[ic_cdk::update]
fn set_configured_chain_id(chain_id: u64) {
    CONFIGURED_CHAIN_ID.with(|current| *current.borrow_mut() = chain_id);
}

#[ic_cdk::update]
fn set_bridge_runtime_code(code: Vec<u8>) {
    BRIDGE_RUNTIME_CODE.with(|current| *current.borrow_mut() = code);
}

#[ic_cdk::update]
fn set_block_mode(mode: BlockMode) {
    BLOCK_MODE.with(|current| *current.borrow_mut() = mode);
}

#[ic_cdk::update]
fn set_broadcast_inconsistent_after_accepts(count: u8) {
    BROADCAST_INCONSISTENT_AFTER_ACCEPTS.with(|current| *current.borrow_mut() = count);
}

#[ic_cdk::update]
fn set_observed_transaction(
    transaction_hash: Vec<u8>,
    contract: Vec<u8>,
    requester: Vec<u8>,
    block_number: u64,
) -> Result<(), String> {
    let transaction_hash: [u8; 32] = transaction_hash
        .try_into()
        .map_err(|_| "transaction hash must be 32 bytes".to_string())?;
    let contract: [u8; 20] = contract
        .try_into()
        .map_err(|_| "contract must be 20 bytes".to_string())?;
    let requester: [u8; 20] = requester
        .try_into()
        .map_err(|_| "requester must be 20 bytes".to_string())?;
    LAST_TX_HASH.with(|value| *value.borrow_mut() = transaction_hash);
    OBSERVED_CONTRACT.with(|value| *value.borrow_mut() = contract);
    OBSERVED_REQUESTER.with(|value| *value.borrow_mut() = requester);
    OBSERVED_BLOCK_NUMBER.with(|value| *value.borrow_mut() = block_number);
    Ok(())
}
#[ic_cdk::update]
fn set_eth_balance(value: Nat) {
    ETH_BALANCE.with(|current| {
        *current.borrow_mut() = value.0.to_string().parse().expect("u128 ETH balance")
    });
}

#[ic_cdk::update]
fn set_next_evm_nonce(value: u64) {
    NEXT_EVM_NONCE.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::update]
fn set_service_fee(value: u128) {
    SERVICE_FEE.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::update]
fn set_max_service_fee(value: u128) {
    MAX_SERVICE_FEE.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::update]
fn set_per_deposit_limit(value: u128) {
    PER_DEPOSIT_LIMIT.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::update]
fn set_mint_window(
    minted_in_window: u128,
    mint_window_limit: u128,
    started_at: u64,
    duration: u64,
    block_timestamp: u64,
) {
    MINTED_IN_WINDOW.with(|current| *current.borrow_mut() = minted_in_window);
    MINT_WINDOW_LIMIT.with(|current| *current.borrow_mut() = mint_window_limit);
    MINT_WINDOW_STARTED_AT.with(|current| *current.borrow_mut() = started_at);
    MINT_WINDOW_DURATION.with(|current| *current.borrow_mut() = duration);
    BLOCK_TIMESTAMP.with(|current| *current.borrow_mut() = block_timestamp);
}

#[ic_cdk::update]
fn set_safe_block(block_number: u64, block_hash: Vec<u8>) -> Result<(), String> {
    let block_hash: [u8; 32] = block_hash
        .try_into()
        .map_err(|_| "safe block hash must be exactly 32 bytes".to_string())?;
    SAFE_BLOCK_SEQUENCE.with(|current| *current.borrow_mut() = vec![block_number; 512]);
    SAFE_BLOCK_HASH_OVERRIDE.with(|current| {
        *current.borrow_mut() = Some((block_number, block_hash));
    });
    Ok(())
}

#[ic_cdk::update]
fn set_finalized_block_sequence(value: Vec<u64>) {
    FINALIZED_BLOCK_SEQUENCE.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::update]
fn set_finalized_block(block_number: u64, block_hash: Vec<u8>) -> Result<(), String> {
    let block_hash: [u8; 32] = block_hash
        .try_into()
        .map_err(|_| "finalized block hash must be exactly 32 bytes".to_string())?;
    FINALIZED_BLOCK_SEQUENCE.with(|current| *current.borrow_mut() = vec![block_number; 512]);
    FINALIZED_BLOCK_HASH_OVERRIDE.with(|current| {
        *current.borrow_mut() = Some((block_number, block_hash));
    });
    Ok(())
}

#[ic_cdk::update]
async fn set_bridge_signer_for_canister(
    canister_id: Principal,
    key_name: String,
) -> Result<(), String> {
    let result = ecdsa_public_key(&EcdsaPublicKeyArgs {
        canister_id: Some(canister_id),
        derivation_path: vec![],
        key_id: EcdsaKeyId {
            curve: EcdsaCurve::Secp256k1,
            name: key_name,
        },
    })
    .await
    .map_err(|error| format!("ecdsa_public_key: {error:?}"))?;
    let key = VerifyingKey::from_sec1_bytes(&result.public_key)
        .map_err(|error| format!("invalid SEC1 public key: {error}"))?;
    let uncompressed = key.to_encoded_point(false);
    let hash = keccak(&uncompressed.as_bytes()[1..]);
    BRIDGE_SIGNER.with(|current| current.borrow_mut().copy_from_slice(&hash[12..]));
    Ok(())
}

#[ic_cdk::update]
fn set_bridge_signer(value: Vec<u8>) -> Result<(), String> {
    let signer: [u8; 20] = value
        .try_into()
        .map_err(|_| "bridge signer must be 20 bytes".to_string())?;
    BRIDGE_SIGNER.with(|current| *current.borrow_mut() = signer);
    Ok(())
}

#[ic_cdk::query]
fn bridge_signer() -> Vec<u8> {
    BRIDGE_SIGNER.with(|current| current.borrow().to_vec())
}

#[ic_cdk::update]
fn set_deployment_postconditions(
    timelock: Vec<u8>,
    governance_operator: Vec<u8>,
    bsns: Vec<u8>,
    bridge: Vec<u8>,
    timelock_runtime_code: Vec<u8>,
    bsns_runtime_code: Vec<u8>,
) -> Result<(), String> {
    let timelock: [u8; 20] = timelock
        .try_into()
        .map_err(|_| "timelock must be 20 bytes".to_string())?;
    let governance_operator: [u8; 20] = governance_operator
        .try_into()
        .map_err(|_| "governance operator must be 20 bytes".to_string())?;
    let bsns: [u8; 20] = bsns
        .try_into()
        .map_err(|_| "BSNS address must be 20 bytes".to_string())?;
    let bridge: [u8; 20] = bridge
        .try_into()
        .map_err(|_| "Bridge address must be 20 bytes".to_string())?;
    if timelock_runtime_code.is_empty() || bsns_runtime_code.is_empty() {
        return Err("deployment runtime code must be non-empty".into());
    }
    BASE_ADMIN_TIMELOCK.with(|current| *current.borrow_mut() = timelock);
    RUNTIME_ADMINISTRATOR.with(|current| *current.borrow_mut() = governance_operator);
    BSNS_ADDRESS.with(|current| *current.borrow_mut() = bsns);
    BSNS_BRIDGE.with(|current| *current.borrow_mut() = bridge);
    TIMELOCK_RUNTIME_CODE.with(|current| *current.borrow_mut() = timelock_runtime_code);
    BSNS_RUNTIME_CODE.with(|current| *current.borrow_mut() = bsns_runtime_code);
    Ok(())
}

#[ic_cdk::update]
fn set_deposit_mints_paused(value: bool) {
    DEPOSIT_MINTS_PAUSED.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::update]
fn set_withdrawals_paused(value: bool) {
    WITHDRAWALS_PAUSED.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::query]
fn broadcast_transactions() -> Vec<Vec<u8>> {
    BROADCASTS.with(|values| values.borrow().clone())
}

#[ic_cdk::query]
fn ledger_transactions() -> Vec<Transaction> {
    TRANSACTIONS.with(|values| values.borrow().clone())
}

#[ic_cdk::query]
fn ledger_transfer_calls() -> u64 {
    LEDGER_TRANSFER_CALLS.with(|value| *value.borrow())
}

#[ic_cdk::query]
fn chain_id_call_count() -> u64 {
    CHAIN_ID_CALL_COUNT.with(|value| *value.borrow())
}

#[ic_cdk::query]
fn get_code_call_count() -> u64 {
    GET_CODE_CALL_COUNT.with(|value| *value.borrow())
}

#[ic_cdk::query]
fn eth_call_count() -> u64 {
    ETH_CALL_COUNT.with(|value| *value.borrow())
}

#[ic_cdk::query]
fn deposit_processed_call_count() -> u64 {
    DEPOSIT_PROCESSED_CALL_COUNT.with(|value| *value.borrow())
}

#[ic_cdk::update]
fn set_receipt_mint_log_index(value: Option<u64>) {
    RECEIPT_MINT_LOG_INDEX.with(|current| *current.borrow_mut() = value);
}

#[ic_cdk::query]
fn receipt_mint_log_index() -> Option<u64> {
    RECEIPT_MINT_LOG_INDEX.with(|current| *current.borrow())
}

#[ic_cdk::query]
fn pinned_eth_call_block_numbers() -> Vec<u64> {
    PINNED_ETH_CALL_BLOCK_NUMBERS.with(|values| values.borrow().clone())
}

#[ic_cdk::query]
fn receipt_call_count() -> u64 {
    RECEIPT_CALL_COUNT.with(|value| *value.borrow())
}

#[ic_cdk::pre_upgrade]
fn pre_upgrade() {
    let state = StableMockState {
        ledger_id: LEDGER_ID.with(|v| *v.borrow()),
        ledger_mode: LEDGER_MODE.with(|v| *v.borrow()),
        ledger_fee_available: LEDGER_FEE_AVAILABLE.with(|v| *v.borrow()),
        ledger_fee: LEDGER_FEE.with(|v| *v.borrow()),
        next_block: NEXT_BLOCK.with(|v| *v.borrow()),
        transactions: TRANSACTIONS.with(|v| v.borrow().clone()),
        archive_prefix_length: ARCHIVE_PREFIX_LENGTH.with(|v| *v.borrow()),
        index_synced_blocks: INDEX_SYNCED_BLOCKS.with(|v| *v.borrow()),
        processed_deposit: PROCESSED_DEPOSIT.with(|v| *v.borrow()),
        mint_log: MINT_LOG.with(|v| v.borrow().clone()),
        last_tx_hash: LAST_TX_HASH.with(|v| *v.borrow()),
        observed_contract: OBSERVED_CONTRACT.with(|v| *v.borrow()),
        observed_requester: OBSERVED_REQUESTER.with(|v| *v.borrow()),
        observed_block_number: OBSERVED_BLOCK_NUMBER.with(|v| *v.borrow()),
        withdrawal: WITHDRAWAL.with(|v| v.borrow().clone()),
        withdrawal_status: WITHDRAWAL_STATUS.with(|v| *v.borrow()),
        receipt_mode: RECEIPT_MODE.with(|v| *v.borrow()),
        chain_id_mode: CHAIN_ID_MODE.with(|v| *v.borrow()),
        configured_chain_id: CONFIGURED_CHAIN_ID.with(|v| *v.borrow()),
        bridge_runtime_code: BRIDGE_RUNTIME_CODE.with(|v| v.borrow().clone()),
        timelock_runtime_code: TIMELOCK_RUNTIME_CODE.with(|v| v.borrow().clone()),
        bsns_runtime_code: BSNS_RUNTIME_CODE.with(|v| v.borrow().clone()),
        bsns_address: BSNS_ADDRESS.with(|v| *v.borrow()),
        bsns_bridge: BSNS_BRIDGE.with(|v| *v.borrow()),
        block_mode: BLOCK_MODE.with(|v| *v.borrow()),
        broadcast_inconsistent_after_accepts: BROADCAST_INCONSISTENT_AFTER_ACCEPTS
            .with(|v| *v.borrow()),
        broadcasts: BROADCASTS.with(|v| v.borrow().clone()),
        accepted_tx_hashes: ACCEPTED_TX_HASHES.with(|v| v.borrow().clone()),
        eth_balance: ETH_BALANCE.with(|v| *v.borrow()),
        next_evm_nonce: NEXT_EVM_NONCE.with(|v| *v.borrow()),
        service_fee: SERVICE_FEE.with(|v| *v.borrow()),
        max_service_fee: MAX_SERVICE_FEE.with(|v| *v.borrow()),
        per_deposit_limit: PER_DEPOSIT_LIMIT.with(|v| *v.borrow()),
        mint_window_limit: MINT_WINDOW_LIMIT.with(|v| *v.borrow()),
        minted_in_window: MINTED_IN_WINDOW.with(|v| *v.borrow()),
        mint_window_started_at: MINT_WINDOW_STARTED_AT.with(|v| *v.borrow()),
        mint_window_duration: MINT_WINDOW_DURATION.with(|v| *v.borrow()),
        block_timestamp: BLOCK_TIMESTAMP.with(|v| *v.borrow()),
        safe_block_sequence: SAFE_BLOCK_SEQUENCE.with(|v| v.borrow().clone()),
        safe_block_hash_override: SAFE_BLOCK_HASH_OVERRIDE.with(|v| *v.borrow()),
        finalized_block_sequence: FINALIZED_BLOCK_SEQUENCE.with(|v| v.borrow().clone()),
        finalized_block_hash_override: FINALIZED_BLOCK_HASH_OVERRIDE.with(|v| *v.borrow()),
        fallback_block_sequence: FALLBACK_BLOCK_SEQUENCE.with(|v| v.borrow().clone()),
        chain_id_call_count: CHAIN_ID_CALL_COUNT.with(|v| *v.borrow()),
        get_code_call_count: GET_CODE_CALL_COUNT.with(|v| *v.borrow()),
        eth_call_count: ETH_CALL_COUNT.with(|v| *v.borrow()),
        deposit_processed_call_count: DEPOSIT_PROCESSED_CALL_COUNT.with(|v| *v.borrow()),
        pinned_eth_call_block_numbers: PINNED_ETH_CALL_BLOCK_NUMBERS.with(|v| v.borrow().clone()),
        receipt_call_count: RECEIPT_CALL_COUNT.with(|v| *v.borrow()),
        receipt_mint_log_index: RECEIPT_MINT_LOG_INDEX.with(|v| *v.borrow()),
        bridge_signer: BRIDGE_SIGNER.with(|v| *v.borrow()),
        mint_authorization_epoch: MINT_AUTHORIZATION_EPOCH.with(|v| *v.borrow()),
        deposit_mints_paused: DEPOSIT_MINTS_PAUSED.with(|v| *v.borrow()),
        withdrawals_paused: WITHDRAWALS_PAUSED.with(|v| *v.borrow()),
    };
    ic_cdk::storage::stable_save((state,)).expect("save mock state");
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    let (state,): (StableMockState,) =
        ic_cdk::storage::stable_restore().expect("restore mock state");
    LEDGER_ID.with(|v| *v.borrow_mut() = state.ledger_id);
    LEDGER_MODE.with(|v| *v.borrow_mut() = state.ledger_mode);
    LEDGER_FEE_AVAILABLE.with(|v| *v.borrow_mut() = state.ledger_fee_available);
    LEDGER_FEE.with(|v| *v.borrow_mut() = state.ledger_fee);
    NEXT_BLOCK.with(|v| *v.borrow_mut() = state.next_block);
    TRANSACTIONS.with(|v| *v.borrow_mut() = state.transactions);
    ARCHIVE_PREFIX_LENGTH.with(|v| *v.borrow_mut() = state.archive_prefix_length);
    INDEX_SYNCED_BLOCKS.with(|v| *v.borrow_mut() = state.index_synced_blocks);
    PROCESSED_DEPOSIT.with(|v| *v.borrow_mut() = state.processed_deposit);
    MINT_LOG.with(|v| *v.borrow_mut() = state.mint_log);
    LAST_TX_HASH.with(|v| *v.borrow_mut() = state.last_tx_hash);
    OBSERVED_CONTRACT.with(|v| *v.borrow_mut() = state.observed_contract);
    OBSERVED_REQUESTER.with(|v| *v.borrow_mut() = state.observed_requester);
    OBSERVED_BLOCK_NUMBER.with(|v| *v.borrow_mut() = state.observed_block_number);
    WITHDRAWAL.with(|v| *v.borrow_mut() = state.withdrawal);
    WITHDRAWAL_STATUS.with(|v| *v.borrow_mut() = state.withdrawal_status);
    RECEIPT_MODE.with(|v| *v.borrow_mut() = state.receipt_mode);
    CHAIN_ID_MODE.with(|v| *v.borrow_mut() = state.chain_id_mode);
    CONFIGURED_CHAIN_ID.with(|v| *v.borrow_mut() = state.configured_chain_id);
    BRIDGE_RUNTIME_CODE.with(|v| *v.borrow_mut() = state.bridge_runtime_code);
    TIMELOCK_RUNTIME_CODE.with(|v| *v.borrow_mut() = state.timelock_runtime_code);
    BSNS_RUNTIME_CODE.with(|v| *v.borrow_mut() = state.bsns_runtime_code);
    BSNS_ADDRESS.with(|v| *v.borrow_mut() = state.bsns_address);
    BSNS_BRIDGE.with(|v| *v.borrow_mut() = state.bsns_bridge);
    BLOCK_MODE.with(|v| *v.borrow_mut() = state.block_mode);
    BROADCAST_INCONSISTENT_AFTER_ACCEPTS
        .with(|v| *v.borrow_mut() = state.broadcast_inconsistent_after_accepts);
    BROADCASTS.with(|v| *v.borrow_mut() = state.broadcasts);
    ACCEPTED_TX_HASHES.with(|v| *v.borrow_mut() = state.accepted_tx_hashes);
    ETH_BALANCE.with(|v| *v.borrow_mut() = state.eth_balance);
    NEXT_EVM_NONCE.with(|v| *v.borrow_mut() = state.next_evm_nonce);
    SERVICE_FEE.with(|v| *v.borrow_mut() = state.service_fee);
    MAX_SERVICE_FEE.with(|v| *v.borrow_mut() = state.max_service_fee);
    PER_DEPOSIT_LIMIT.with(|v| *v.borrow_mut() = state.per_deposit_limit);
    MINT_WINDOW_LIMIT.with(|v| *v.borrow_mut() = state.mint_window_limit);
    MINTED_IN_WINDOW.with(|v| *v.borrow_mut() = state.minted_in_window);
    MINT_WINDOW_STARTED_AT.with(|v| *v.borrow_mut() = state.mint_window_started_at);
    MINT_WINDOW_DURATION.with(|v| *v.borrow_mut() = state.mint_window_duration);
    BLOCK_TIMESTAMP.with(|v| *v.borrow_mut() = state.block_timestamp);
    SAFE_BLOCK_SEQUENCE.with(|v| *v.borrow_mut() = state.safe_block_sequence);
    SAFE_BLOCK_HASH_OVERRIDE.with(|v| *v.borrow_mut() = state.safe_block_hash_override);
    FINALIZED_BLOCK_SEQUENCE.with(|v| *v.borrow_mut() = state.finalized_block_sequence);
    FINALIZED_BLOCK_HASH_OVERRIDE.with(|v| *v.borrow_mut() = state.finalized_block_hash_override);
    FALLBACK_BLOCK_SEQUENCE.with(|v| *v.borrow_mut() = state.fallback_block_sequence);
    CHAIN_ID_CALL_COUNT.with(|v| *v.borrow_mut() = state.chain_id_call_count);
    GET_CODE_CALL_COUNT.with(|v| *v.borrow_mut() = state.get_code_call_count);
    ETH_CALL_COUNT.with(|v| *v.borrow_mut() = state.eth_call_count);
    DEPOSIT_PROCESSED_CALL_COUNT.with(|v| *v.borrow_mut() = state.deposit_processed_call_count);
    PINNED_ETH_CALL_BLOCK_NUMBERS.with(|v| *v.borrow_mut() = state.pinned_eth_call_block_numbers);
    RECEIPT_CALL_COUNT.with(|v| *v.borrow_mut() = state.receipt_call_count);
    RECEIPT_MINT_LOG_INDEX.with(|v| *v.borrow_mut() = state.receipt_mint_log_index);
    BRIDGE_SIGNER.with(|v| *v.borrow_mut() = state.bridge_signer);
    MINT_AUTHORIZATION_EPOCH.with(|v| *v.borrow_mut() = state.mint_authorization_epoch);
    DEPOSIT_MINTS_PAUSED.with(|v| *v.borrow_mut() = state.deposit_mints_paused);
    WITHDRAWALS_PAUSED.with(|v| *v.borrow_mut() = state.withdrawals_paused);
}

#[ic_cdk::query]
fn icrc1_fee() -> Nat {
    if !LEDGER_FEE_AVAILABLE.with(|available| *available.borrow()) {
        ic_cdk::trap("mock ledger fee unavailable");
    }
    Nat::from(LEDGER_FEE.with(|fee| *fee.borrow()))
}

#[ic_cdk::update]
fn icrc2_transfer_from(args: TransferFromArgs) -> Result<Nat, TransferFromError> {
    LEDGER_TRANSFER_CALLS.with(|value| *value.borrow_mut() += 1);
    match LEDGER_MODE.with(|mode| *mode.borrow()) {
        LedgerMode::Trap => ic_cdk::trap("ambiguous mock transfer"),
        LedgerMode::Duplicate => Err(TransferFromError::Duplicate {
            duplicate_of: Nat::from(1u8),
        }),
        LedgerMode::BadFee => Err(TransferFromError::BadFee {
            expected_fee: Nat::from(LEDGER_FEE.with(|fee| *fee.borrow())),
        }),
        LedgerMode::InsufficientAllowance { allowance } => {
            Err(TransferFromError::InsufficientAllowance {
                allowance: Nat::from(allowance),
            })
        }
        LedgerMode::InsufficientFunds { balance } => Err(TransferFromError::InsufficientFunds {
            balance: Nat::from(balance),
        }),
        LedgerMode::TemporarilyUnavailable => Err(TransferFromError::TemporarilyUnavailable),
        LedgerMode::Succeed => {
            let transaction = Transaction::transfer(
                icrc_ledger_types::icrc3::transactions::Transfer {
                    amount: args.amount,
                    from: args.from,
                    to: args.to,
                    spender: Some(Account {
                        owner: ic_cdk::api::msg_caller(),
                        subaccount: args.spender_subaccount,
                    }),
                    memo: args.memo,
                    fee: args.fee,
                    created_at_time: args.created_at_time,
                },
                ic_cdk::api::time(),
            );
            TRANSACTIONS.with(|transactions| transactions.borrow_mut().push(transaction));
            Ok(NEXT_BLOCK.with(|next| {
                let value = *next.borrow();
                *next.borrow_mut() = value + 1;
                Nat::from(value)
            }))
        }
    }
}

#[ic_cdk::update]
fn icrc1_transfer(args: TransferArg) -> Result<Nat, TransferError> {
    LEDGER_TRANSFER_CALLS.with(|value| *value.borrow_mut() += 1);
    let mode = REFUND_LEDGER_MODE
        .with(|mode| *mode.borrow())
        .unwrap_or_else(|| LEDGER_MODE.with(|mode| *mode.borrow()));
    match mode {
        LedgerMode::Trap => ic_cdk::trap("ambiguous mock transfer"),
        LedgerMode::Duplicate => {
            return Err(TransferError::Duplicate {
                duplicate_of: Nat::from(2u8),
            })
        }
        LedgerMode::BadFee => {
            return Err(TransferError::BadFee {
                expected_fee: Nat::from(LEDGER_FEE.with(|current| *current.borrow())),
            })
        }
        LedgerMode::InsufficientAllowance { allowance } => {
            return Err(TransferError::GenericError {
                error_code: Nat::from(1u8),
                message: format!("insufficient allowance: {allowance}"),
            })
        }
        LedgerMode::InsufficientFunds { balance } => {
            return Err(TransferError::InsufficientFunds {
                balance: Nat::from(balance),
            })
        }
        LedgerMode::TemporarilyUnavailable => return Err(TransferError::TemporarilyUnavailable),
        LedgerMode::Succeed => {}
    }
    let from = Account {
        owner: ic_cdk::api::msg_caller(),
        subaccount: args.from_subaccount,
    };
    let transaction = Transaction::transfer(
        icrc_ledger_types::icrc3::transactions::Transfer {
            amount: args.amount,
            from,
            to: args.to,
            spender: None,
            memo: args.memo,
            fee: args.fee,
            created_at_time: args.created_at_time,
        },
        ic_cdk::api::time(),
    );
    TRANSACTIONS.with(|transactions| transactions.borrow_mut().push(transaction));
    Ok(Nat::from(2u8))
}

#[ic_cdk::query]
fn get_transactions(_args: GetBlocksRequest) -> GetTransactionsResponse {
    let transactions = TRANSACTIONS.with(|transactions| transactions.borrow().clone());
    let archive_length = ARCHIVE_PREFIX_LENGTH
        .with(|length| *length.borrow())
        .min(transactions.len() as u64) as usize;
    let archived_transactions = if archive_length == 0 {
        vec![]
    } else {
        vec![ArchivedRange {
            start: Nat::from(0u8),
            length: Nat::from(archive_length),
            callback: QueryTxArchiveFn::new(
                ic_cdk::api::canister_self(),
                "get_archive_transactions",
            ),
        }]
    };
    GetTransactionsResponse {
        log_length: Nat::from(transactions.len()),
        first_index: Nat::from(archive_length),
        transactions: transactions.into_iter().skip(archive_length).collect(),
        archived_transactions,
    }
}

#[ic_cdk::query]
fn get_archive_transactions(args: GetBlocksRequest) -> TransactionRange {
    let start = nat_to_usize(&args.start);
    let length = nat_to_usize(&args.length);
    let archive_length = ARCHIVE_PREFIX_LENGTH.with(|value| *value.borrow()) as usize;
    let transactions = TRANSACTIONS.with(|values| {
        values
            .borrow()
            .iter()
            .skip(start)
            .take(length.min(archive_length.saturating_sub(start)))
            .cloned()
            .collect()
    });
    TransactionRange { transactions }
}

fn nat_to_usize(value: &Nat) -> usize {
    value.0.to_string().parse().unwrap_or(usize::MAX)
}

#[ic_cdk::query]
fn ledger_id() -> Principal {
    LEDGER_ID.with(|id| id.borrow().unwrap_or_else(ic_cdk::api::canister_self))
}

#[ic_cdk::query]
fn status() -> Status {
    Status {
        num_blocks_synced: INDEX_SYNCED_BLOCKS.with(|override_value| {
            Nat::from(override_value.borrow().unwrap_or_else(|| {
                TRANSACTIONS.with(|transactions| transactions.borrow().len() as u128)
            }))
        }),
    }
}

#[ic_cdk::query]
fn get_account_transactions(
    _args: GetAccountTransactionsArgs,
) -> Result<IndexTransactions, IndexError> {
    let transactions = TRANSACTIONS.with(|transactions| {
        transactions
            .borrow()
            .iter()
            .cloned()
            .enumerate()
            .rev()
            .map(|(id, transaction)| TransactionWithId {
                id: Nat::from(id),
                transaction,
            })
            .collect::<Vec<_>>()
    });
    Ok(IndexTransactions {
        balance: Nat::from(0u8),
        oldest_tx_id: transactions
            .last()
            .map(|transaction| transaction.id.clone()),
        transactions,
    })
}

#[ic_cdk::update]
fn multi_request(
    _services: RpcServices,
    _config: Option<RpcConfig>,
    request: String,
) -> MultiRpcResult<String> {
    if request.contains("eth_chainId") {
        CHAIN_ID_CALL_COUNT.with(|value| {
            let next = value.borrow().saturating_add(1);
            *value.borrow_mut() = next;
        });
        return match CHAIN_ID_MODE.with(|mode| *mode.borrow()) {
            ChainIdMode::Configured => MultiRpcResult::Consistent(Ok(
                CONFIGURED_CHAIN_ID.with(|chain_id| format!("0x{:x}", *chain_id.borrow()))
            )),
            ChainIdMode::Wrong => MultiRpcResult::Consistent(Ok("0x1".into())),
            ChainIdMode::Inconsistent => MultiRpcResult::Inconsistent(vec![
                (RpcService::Provider(1), Ok("0x2105".into())),
                (RpcService::Provider(2), Ok("0x1".into())),
            ]),
        };
    }
    if request.contains("eth_maxPriorityFeePerGas") {
        return MultiRpcResult::Consistent(Ok("0x1".into()));
    }
    if request.contains("eth_estimateGas") {
        return MultiRpcResult::Consistent(Ok("0x186a0".into()));
    }
    if request.contains("eth_getLogs") {
        let logs: Vec<evm_rpc_types::LogEntry> =
            MINT_LOG.with(|value| value.borrow().as_ref().map(mint_log).into_iter().collect());
        let response = serde_json::to_string(&logs).expect("valid Mint log response");
        return MultiRpcResult::Consistent(Ok(response));
    }
    if request.contains("eth_call") {
        ETH_CALL_COUNT.with(|value| {
            let next = value.borrow().saturating_add(1);
            *value.borrow_mut() = next;
        });
        if request.contains("d5d0d21c") {
            DEPOSIT_PROCESSED_CALL_COUNT.with(|value| {
                let next = value.borrow().saturating_add(1);
                *value.borrow_mut() = next;
            });
        }
        let Some(block_number) = eip1898_block_number(&request) else {
            if RECEIPT_MODE.with(|mode| *mode.borrow()) == ReceiptMode::Orphaned {
                return MultiRpcResult::Consistent(Err(RpcError::JsonRpcError(JsonRpcError {
                    code: -32_001,
                    message: "block is not canonical".into(),
                })));
            }
            return MultiRpcResult::Consistent(Err(RpcError::ProviderError(
                ProviderError::ProviderNotFound,
            )));
        };
        PINNED_ETH_CALL_BLOCK_NUMBERS.with(|values| values.borrow_mut().push(block_number));
        if request.contains("f702cf2b")
            && BLOCK_MODE.with(|mode| *mode.borrow()) == BlockMode::CanonicalInconsistent
        {
            return MultiRpcResult::Inconsistent(vec![
                (
                    RpcService::Provider(1),
                    Ok(bridge_snapshot_response(Some(block_number))),
                ),
                (
                    RpcService::Provider(2),
                    Ok(bridge_snapshot_response(Some(
                        block_number.saturating_add(1),
                    ))),
                ),
            ]);
        }
    }
    if RECEIPT_MODE.with(|mode| *mode.borrow()) == ReceiptMode::DecodeFailure
        && request.contains("eth_call")
    {
        return MultiRpcResult::Consistent(Ok("not-hex".into()));
    }
    let response = if request.contains("420000000000000000000000000000000000000f") {
        if request.contains("f1c7a58b") {
            word(1_000)
        } else {
            word(500)
        }
    } else if request.contains("eth_getCode") {
        GET_CODE_CALL_COUNT.with(|value| {
            let next = value.borrow().saturating_add(1);
            *value.borrow_mut() = next;
        });
        let target = rpc_target(&request);
        let timelock =
            BASE_ADMIN_TIMELOCK.with(|value| format!("0x{}", bytes_hex(&*value.borrow())));
        let bsns = BSNS_ADDRESS.with(|value| format!("0x{}", bytes_hex(&*value.borrow())));
        let code = if target
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(&timelock))
        {
            TIMELOCK_RUNTIME_CODE.with(|value| value.borrow().clone())
        } else if target
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(&bsns))
        {
            BSNS_RUNTIME_CODE.with(|value| value.borrow().clone())
        } else {
            BRIDGE_RUNTIME_CODE.with(|value| value.borrow().clone())
        };
        format!("\"0x{}\"", bytes_hex(&code))
    } else if request.contains("eth_getTransactionByHash") {
        let requested = serde_json::from_str::<serde_json::Value>(&request)
            .ok()
            .and_then(|value| {
                value
                    .get("params")?
                    .as_array()?
                    .first()?
                    .as_str()
                    .map(str::to_owned)
            });
        let known = requested.as_ref().is_some_and(|requested| {
            ACCEPTED_TX_HASHES.with(|hashes| {
                hashes
                    .borrow()
                    .iter()
                    .any(|hash| requested.eq_ignore_ascii_case(&format!("0x{}", bytes_hex(hash))))
            })
        });
        if known {
            let hash = requested.as_deref().expect("known request has hash");
            serde_json::json!({"hash": hash}).to_string()
        } else {
            "null".into()
        }
    } else if request.contains(&selector_hex("baseAdminTimelock()")) {
        address_word(BASE_ADMIN_TIMELOCK.with(|value| *value.borrow()))
    } else if request.contains(&selector_hex("runtimeAdministrator()")) {
        address_word(RUNTIME_ADMINISTRATOR.with(|value| *value.borrow()))
    } else if request.contains(&selector_hex("approvedTimelockRuntimeCodeHash()")) {
        let code = TIMELOCK_RUNTIME_CODE.with(|value| value.borrow().clone());
        format!("0x{}", bytes_hex(&keccak(&code)))
    } else if request.contains(&selector_hex("getMinDelay()")) {
        word(300)
    } else if request.contains(&selector_hex("bsns()")) {
        address_word(BSNS_ADDRESS.with(|value| *value.borrow()))
    } else if request.contains(&selector_hex("name()"))
        || request.contains(&selector_hex("symbol()"))
    {
        abi_string("KINIC")
    } else if request.contains(&selector_hex("decimals()")) {
        word(8)
    } else if request.contains(&selector_hex("bridge()")) {
        address_word(BSNS_BRIDGE.with(|value| *value.borrow()))
    } else if request.contains(&selector_hex("roleMember(bytes32)")) {
        let default_role = format!("{}{}", selector_hex("roleMember(bytes32)"), "00".repeat(32));
        if request.contains(&default_role) {
            address_word(BASE_ADMIN_TIMELOCK.with(|value| *value.borrow()))
        } else {
            address_word(RUNTIME_ADMINISTRATOR.with(|value| *value.borrow()))
        }
    } else if request.contains("f702cf2b") {
        let block_number = eip1898_block_number(&request);
        if BLOCK_MODE.with(|mode| *mode.borrow()) == BlockMode::SameHeightDifferentHash {
            bridge_snapshot_response(block_number.map(|number| number.saturating_add(1)))
        } else {
            bridge_snapshot_response(block_number)
        }
    } else if request.contains("d5d0d21c") {
        if PROCESSED_DEPOSIT.with(|processed| *processed.borrow()) {
            word(1)
        } else {
            word(0)
        }
    } else if request.contains("8a4fb16a") {
        WITHDRAWAL.with(|value| withdrawal_response(value.borrow().as_ref()))
    } else if request.contains("8abdf5aa") {
        word(SERVICE_FEE.with(|value| *value.borrow()))
    } else if request.contains("14d90e1b") {
        word(MAX_SERVICE_FEE.with(|value| *value.borrow()))
    } else if request.contains("e71fb849") {
        word(PER_DEPOSIT_LIMIT.with(|value| *value.borrow()))
    } else if request.contains("feafa875") {
        word(MINT_WINDOW_LIMIT.with(|value| *value.borrow()))
    } else if request.contains("080d3f70") {
        word(u128::from(
            MINT_WINDOW_STARTED_AT.with(|value| *value.borrow()),
        ))
    } else if request.contains("1090b877") {
        word(u128::from(
            MINT_WINDOW_DURATION.with(|value| *value.borrow()),
        ))
    } else if request.contains("23a6d88d") {
        word(MINTED_IN_WINDOW.with(|value| *value.borrow()))
    } else if request.contains("eth_getBalance") {
        word(ETH_BALANCE.with(|balance| *balance.borrow()))
    } else {
        word(0)
    };
    MultiRpcResult::Consistent(Ok(response))
}

#[ic_cdk::update(name = "eth_getTransactionCount")]
fn eth_get_transaction_count(
    _services: RpcServices,
    _config: Option<RpcConfig>,
    _args: GetTransactionCountArgs,
) -> MultiRpcResult<Nat256> {
    MultiRpcResult::Consistent(Ok(Nat256::from(
        NEXT_EVM_NONCE.with(|nonce| *nonce.borrow()),
    )))
}

#[ic_cdk::update(name = "eth_sendRawTransaction")]
fn eth_send_raw_transaction(
    _services: RpcServices,
    _config: Option<RpcConfig>,
    raw: Hex,
) -> MultiRpcResult<SendRawTransactionStatus> {
    let raw_bytes: Vec<u8> = raw.into();
    BROADCASTS.with(|values| values.borrow_mut().push(raw_bytes.clone()));
    let expected_nonce = NEXT_EVM_NONCE.with(|nonce| *nonce.borrow());
    let Some(raw_nonce) = eip1559_nonce(&raw_bytes) else {
        return MultiRpcResult::Consistent(Ok(SendRawTransactionStatus::NonceTooLow));
    };
    if raw_nonce != expected_nonce && raw_nonce.saturating_add(1) != expected_nonce {
        return MultiRpcResult::Consistent(Ok(SendRawTransactionStatus::NonceTooLow));
    }
    if raw_nonce == expected_nonce {
        NEXT_EVM_NONCE.with(|nonce| *nonce.borrow_mut() = expected_nonce.saturating_add(1));
    }
    let hash = keccak(&raw_bytes);
    ACCEPTED_TX_HASHES.with(|hashes| hashes.borrow_mut().push(hash));
    LAST_TX_HASH.with(|current| *current.borrow_mut() = hash);
    if RECEIPT_MODE.with(|mode| *mode.borrow()) != ReceiptMode::Reverted
        && eip1559_calldata(&raw_bytes).is_some()
    {
        PROCESSED_DEPOSIT.with(|processed| *processed.borrow_mut() = true);
    }
    let inconsistent = BROADCAST_INCONSISTENT_AFTER_ACCEPTS.with(|remaining| {
        let current = *remaining.borrow();
        if current == 0 {
            false
        } else {
            *remaining.borrow_mut() = current - 1;
            true
        }
    });
    if inconsistent {
        return MultiRpcResult::Inconsistent(vec![
            (
                RpcService::Provider(1),
                Ok(SendRawTransactionStatus::Ok(Some(hex32(hash)))),
            ),
            (
                RpcService::Provider(2),
                Err(RpcError::ProviderError(ProviderError::ProviderNotFound)),
            ),
        ]);
    }
    MultiRpcResult::Consistent(Ok(SendRawTransactionStatus::Ok(Some(hex32(hash)))))
}

#[ic_cdk::update(name = "eth_getBlockByNumber")]
fn eth_get_block_by_number(
    _services: RpcServices,
    _config: Option<RpcConfig>,
    tag: BlockTag,
) -> MultiRpcResult<Block> {
    match (BLOCK_MODE.with(|mode| *mode.borrow()), &tag) {
        (BlockMode::FinalizedUnavailable, BlockTag::Finalized) => MultiRpcResult::Consistent(Err(
            RpcError::ProviderError(ProviderError::ProviderNotFound),
        )),
        (BlockMode::FinalizedInconsistent, BlockTag::Finalized)
        | (BlockMode::CanonicalInconsistent, BlockTag::Number(_)) => {
            let first = mock_block(tag.clone(), false);
            let second = mock_block(tag.clone(), true);
            let third = mock_block(tag, false);
            MultiRpcResult::Inconsistent(vec![
                (RpcService::Provider(1), Ok(first)),
                (RpcService::Provider(2), Ok(second)),
                (RpcService::Provider(3), Ok(third)),
            ])
        }
        (BlockMode::SameHeightDifferentHash, BlockTag::Number(_)) => {
            MultiRpcResult::Consistent(Ok(mock_block(tag, true)))
        }
        _ => MultiRpcResult::Consistent(Ok(mock_block(tag, false))),
    }
}

#[ic_cdk::update(name = "eth_getTransactionReceipt")]
async fn eth_get_transaction_receipt(
    _services: RpcServices,
    _config: Option<RpcConfig>,
    hash: Hex32,
) -> MultiRpcResult<Option<TransactionReceipt>> {
    RECEIPT_CALL_COUNT.with(|count| {
        let next = count.borrow().saturating_add(1);
        *count.borrow_mut() = next;
    });
    if RECEIPT_MODE.with(|mode| *mode.borrow()) == ReceiptMode::DelayedConfirmed {
        for _ in 0..32 {
            Call::unbounded_wait(ic_cdk::api::canister_self(), "receipt_delay_tick")
                .await
                .unwrap_or_else(|error| {
                    ic_cdk::trap(format!("receipt delay self-call failed: {error:?}"))
                });
        }
    }
    let hash: [u8; 32] = hash.into();
    match RECEIPT_MODE.with(|mode| *mode.borrow()) {
        ReceiptMode::Confirmed | ReceiptMode::DelayedConfirmed | ReceiptMode::DecodeFailure => {
            MultiRpcResult::Consistent(Ok(Some(mock_receipt(hash, false, true))))
        }
        ReceiptMode::Missing => MultiRpcResult::Consistent(Ok(None)),
        ReceiptMode::Reverted => {
            MultiRpcResult::Consistent(Ok(Some(mock_receipt(hash, true, true))))
        }
        ReceiptMode::RpcFailure => MultiRpcResult::Consistent(Err(RpcError::ProviderError(
            ProviderError::ProviderNotFound,
        ))),
        ReceiptMode::Orphaned => {
            MultiRpcResult::Consistent(Ok(Some(mock_receipt(hash, false, false))))
        }
        ReceiptMode::Inconsistent => MultiRpcResult::Inconsistent(vec![
            (
                RpcService::Provider(1),
                Ok(Some(mock_receipt(hash, false, true))),
            ),
            (RpcService::Provider(2), Ok(None)),
        ]),
    }
}

#[ic_cdk::update(name = "eth_getLogs")]
fn eth_get_logs(
    _services: RpcServices,
    _config: Option<evm_rpc_types::GetLogsRpcConfig>,
    _args: GetLogsArgs,
) -> MultiRpcResult<Vec<evm_rpc_types::LogEntry>> {
    let logs: Vec<evm_rpc_types::LogEntry> =
        MINT_LOG.with(|value| value.borrow().as_ref().map(mint_log).into_iter().collect());
    let logs = if logs.is_empty() {
        WITHDRAWAL.with(|value| {
            value
                .borrow()
                .as_ref()
                .map(|fixture| withdrawal_log(fixture, LAST_TX_HASH.with(|hash| *hash.borrow())))
                .into_iter()
                .collect()
        })
    } else {
        logs
    };
    MultiRpcResult::Consistent(Ok(logs))
}

fn word(value: u128) -> String {
    format!("0x{value:064x}")
}

fn address_word(value: [u8; 20]) -> String {
    format!("0x{}{}", "00".repeat(12), bytes_hex(&value))
}

fn abi_string(value: &str) -> String {
    let padded = value.len().div_ceil(32) * 32;
    format!(
        "0x{}20{:064x}{}{}",
        "00".repeat(31),
        value.len(),
        bytes_hex(value.as_bytes()),
        "00".repeat(padded - value.len())
    )
}

fn selector_hex(signature: &str) -> String {
    bytes_hex(&keccak(signature.as_bytes())[..4])
}

fn rpc_target(request: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(request).ok()?;
    let first = value.get("params")?.as_array()?.first()?;
    first
        .as_str()
        .or_else(|| first.get("to")?.as_str())
        .map(str::to_owned)
}

fn eip1898_block_number(request: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(request).ok()?;
    let tag = value.get("params")?.as_array()?.get(1)?.as_object()?;
    if !tag.get("requireCanonical")?.as_bool()? {
        return None;
    }
    block_number_from_hash(tag.get("blockHash")?.as_str()?)
}

fn bridge_snapshot_response(block_number: Option<u64>) -> String {
    const WORDS: usize = 13;
    const WORD: usize = 32;
    let mut bytes = vec![0; WORDS * WORD];
    let values = [
        (
            0,
            u128::from(block_number.unwrap_or_else(|| next_block_number(&FALLBACK_BLOCK_SEQUENCE))),
        ),
        (1, u128::from(BLOCK_TIMESTAMP.with(|value| *value.borrow()))),
        (
            3,
            u128::from(MINT_AUTHORIZATION_EPOCH.with(|value| *value.borrow())),
        ),
        (4, SERVICE_FEE.with(|value| *value.borrow())),
        (5, MAX_SERVICE_FEE.with(|value| *value.borrow())),
        (6, PER_DEPOSIT_LIMIT.with(|value| *value.borrow())),
        (7, MINT_WINDOW_LIMIT.with(|value| *value.borrow())),
        (
            8,
            u128::from(MINT_WINDOW_DURATION.with(|value| *value.borrow())),
        ),
        (
            9,
            u128::from(MINT_WINDOW_STARTED_AT.with(|value| *value.borrow())),
        ),
        (10, MINTED_IN_WINDOW.with(|value| *value.borrow())),
        (
            11,
            u128::from(DEPOSIT_MINTS_PAUSED.with(|value| *value.borrow())),
        ),
        (
            12,
            u128::from(WITHDRAWALS_PAUSED.with(|value| *value.borrow())),
        ),
    ];
    for (index, value) in values {
        put_word(&mut bytes[index * WORD..(index + 1) * WORD], value);
    }
    bytes[2 * WORD + 12..3 * WORD].copy_from_slice(&BRIDGE_SIGNER.with(|value| *value.borrow()));
    format!("0x{}", bytes_hex(&bytes))
}

fn withdrawal_response(value: Option<&WithdrawalFixture>) -> String {
    const WORD: usize = 32;
    const TUPLE_START: usize = WORD;
    const HEAD_BYTES: usize = 8 * WORD;

    let owner = value
        .map(|value| value.owner.as_slice())
        .unwrap_or_default();
    let padded_owner_length = owner.len().div_ceil(WORD) * WORD;
    let owner_length_offset = TUPLE_START + HEAD_BYTES;
    let mut bytes = vec![0u8; owner_length_offset + WORD + padded_owner_length];
    put_word(&mut bytes[..WORD], TUPLE_START as u128);
    if value.is_some() {
        bytes[TUPLE_START + 12..TUPLE_START + WORD]
            .copy_from_slice(&OBSERVED_REQUESTER.with(|requester| *requester.borrow()));
    }
    put_word(
        &mut bytes[TUPLE_START + WORD..TUPLE_START + 2 * WORD],
        value.map(|value| value.amount).unwrap_or_default(),
    );
    put_word(
        &mut bytes[TUPLE_START + 2 * WORD..TUPLE_START + 3 * WORD],
        value.map(|value| value.max_service_fee).unwrap_or_default(),
    );
    put_word(
        &mut bytes[TUPLE_START + 3 * WORD..TUPLE_START + 4 * WORD],
        value
            .map(|value| value.charged_service_fee)
            .unwrap_or_default(),
    );
    put_word(
        &mut bytes[TUPLE_START + 4 * WORD..TUPLE_START + 5 * WORD],
        value.map(|value| value.amount_out).unwrap_or_default(),
    );
    put_word(
        &mut bytes[TUPLE_START + 5 * WORD..TUPLE_START + 6 * WORD],
        HEAD_BYTES as u128,
    );
    bytes[TUPLE_START + 6 * WORD..TUPLE_START + 7 * WORD].copy_from_slice(
        &value
            .map(|value| padded32(&value.subaccount))
            .unwrap_or_default(),
    );
    put_word(
        &mut bytes[TUPLE_START + 7 * WORD..TUPLE_START + 8 * WORD],
        value
            .map(|_| WITHDRAWAL_STATUS.with(|status| *status.borrow()) as u128)
            .unwrap_or_default(),
    );
    put_word(
        &mut bytes[owner_length_offset..owner_length_offset + WORD],
        owner.len() as u128,
    );
    bytes[owner_length_offset + WORD..owner_length_offset + WORD + owner.len()]
        .copy_from_slice(owner);
    format!("0x{}", bytes_hex(&bytes))
}

fn put_word(target: &mut [u8], value: u128) {
    target[16..].copy_from_slice(&value.to_be_bytes());
}
fn padded32(value: &[u8]) -> [u8; 32] {
    let mut result = [0; 32];
    let len = value.len().min(32);
    result[..len].copy_from_slice(&value[..len]);
    result
}

fn withdrawal_log(
    value: &WithdrawalFixture,
    transaction_hash: [u8; 32],
) -> evm_rpc_types::LogEntry {
    use tiny_keccak::{Hasher, Keccak};

    let id = padded32(&value.id);
    let mut event_topic = [0; 32];
    let mut hasher = Keccak::v256();
    hasher.update(
        b"WithdrawalCommitted(uint256,address,uint256,uint256,uint256,uint256,bytes,bytes32)",
    );
    hasher.finalize(&mut event_topic);
    let mut requester = [0; 32];
    requester[12..].copy_from_slice(&OBSERVED_REQUESTER.with(|value| *value.borrow()));
    let owner_length_offset = 6 * 32;
    let mut data = vec![0u8; 8 * 32];
    put_word(&mut data[..32], value.amount);
    put_word(&mut data[32..64], value.max_service_fee);
    put_word(&mut data[64..96], value.charged_service_fee);
    put_word(&mut data[96..128], value.amount_out);
    put_word(&mut data[128..160], owner_length_offset as u128);
    data[160..192].copy_from_slice(&padded32(&value.subaccount));
    put_word(&mut data[192..224], value.owner.len() as u128);
    data[224..224 + value.owner.len()].copy_from_slice(&value.owner);
    let block_number = OBSERVED_BLOCK_NUMBER.with(|value| *value.borrow());
    serde_json::from_value(serde_json::json!({
        "address":format!("0x{}", bytes_hex(&OBSERVED_CONTRACT.with(|value| *value.borrow()))), "topics":[format!("0x{}", bytes_hex(&event_topic)), format!("0x{}", bytes_hex(&id)), format!("0x{}", bytes_hex(&requester))],
        "data":format!("0x{}", bytes_hex(&data)), "blockNumber":block_number, "transactionHash":format!("0x{}", bytes_hex(&transaction_hash)),
        "transactionIndex":0, "blockHash":format!("0x{}", bytes_hex(&block_hash(block_number))), "logIndex":0, "removed":false
    })).expect("valid withdrawal log")
}

fn mint_log(value: &MintLogFixture) -> evm_rpc_types::LogEntry {
    let deposit_id = padded32(&value.deposit_id);
    let authorization_digest = padded32(&value.authorization_digest);
    let transaction_hash = padded32(&value.transaction_hash);
    let mut recipient = [0; 32];
    let recipient_len = value.recipient.len().min(20);
    recipient[32 - recipient_len..]
        .copy_from_slice(&value.recipient[value.recipient.len() - recipient_len..]);
    let event_topic = keccak(b"DepositMinted(bytes32,address,bytes32,uint256,uint256,uint256)");
    let mut data = vec![0u8; 3 * 32];
    put_word(&mut data[..32], value.gross_amount);
    put_word(&mut data[32..64], value.charged_service_fee);
    put_word(&mut data[64..96], value.minted_amount);
    let block_number = OBSERVED_BLOCK_NUMBER.with(|value| *value.borrow());
    serde_json::from_value(serde_json::json!({
        "address":format!("0x{}", bytes_hex(&OBSERVED_CONTRACT.with(|value| *value.borrow()))),
        "topics":[
            format!("0x{}", bytes_hex(&event_topic)),
            format!("0x{}", bytes_hex(&deposit_id)),
            format!("0x{}", bytes_hex(&recipient)),
            format!("0x{}", bytes_hex(&authorization_digest))
        ],
        "data":format!("0x{}", bytes_hex(&data)),
        "blockNumber":block_number,
        "transactionHash":format!("0x{}", bytes_hex(&transaction_hash)),
        "transactionIndex":0,
        "blockHash":format!("0x{}", bytes_hex(&block_hash(block_number))),
        "logIndex":0,
        "removed":false
    }))
    .expect("valid mint log")
}

fn hex32(value: [u8; 32]) -> Hex32 {
    Hex32::from_str(&format!("0x{}", bytes_hex(&value))).expect("valid hash")
}

fn keccak(bytes: &[u8]) -> [u8; 32] {
    use tiny_keccak::{Hasher, Keccak};
    let mut hash = [0; 32];
    let mut hasher = Keccak::v256();
    hasher.update(bytes);
    hasher.finalize(&mut hash);
    hash
}

fn eip1559_nonce(raw: &[u8]) -> Option<u64> {
    if raw.first() != Some(&2) {
        return None;
    }
    let (payload, _) = rlp_item(raw, 1, true)?;
    let (_, offset) = rlp_item(payload, 0, false)?;
    let (nonce, _) = rlp_item(payload, offset, false)?;
    if nonce.len() > 8 {
        return None;
    }
    Some(
        nonce
            .iter()
            .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)),
    )
}

fn eip1559_calldata(raw: &[u8]) -> Option<&[u8]> {
    if raw.first() != Some(&2) {
        return None;
    }
    let (payload, _) = rlp_item(raw, 1, true)?;
    let mut offset = 0;
    // calldata is the eighth EIP-1559 field after chain id, nonce, both fees,
    // gas, destination, and value.
    for _ in 0..7 {
        let (_, next) = rlp_item(payload, offset, false)?;
        offset = next;
    }
    let (calldata, _) = rlp_item(payload, offset, false)?;
    Some(calldata)
}

fn rlp_item(input: &[u8], offset: usize, list: bool) -> Option<(&[u8], usize)> {
    let prefix = *input.get(offset)?;
    let (payload_start, payload_len) = match prefix {
        0x00..=0x7f if !list => (offset, 1),
        0x80..=0xb7 if !list => (offset + 1, usize::from(prefix - 0x80)),
        0xb8..=0xbf if !list => {
            let length_len = usize::from(prefix - 0xb7);
            let start = offset + 1;
            (
                start + length_len,
                decode_length(input.get(start..start + length_len)?)?,
            )
        }
        0xc0..=0xf7 if list => (offset + 1, usize::from(prefix - 0xc0)),
        0xf8..=0xff if list => {
            let length_len = usize::from(prefix - 0xf7);
            let start = offset + 1;
            (
                start + length_len,
                decode_length(input.get(start..start + length_len)?)?,
            )
        }
        _ => return None,
    };
    let end = payload_start.checked_add(payload_len)?;
    Some((input.get(payload_start..end)?, end))
}

fn decode_length(bytes: &[u8]) -> Option<usize> {
    bytes.iter().try_fold(0usize, |value, byte| {
        value.checked_mul(256)?.checked_add(usize::from(*byte))
    })
}

fn bytes_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn mock_block(tag: BlockTag, alternate_hash: bool) -> Block {
    let block_number = match tag {
        BlockTag::Number(number) => u64::try_from(number).expect("mock block number fits u64"),
        BlockTag::Safe => next_block_number(&SAFE_BLOCK_SEQUENCE),
        BlockTag::Finalized => next_block_number(&FINALIZED_BLOCK_SEQUENCE),
        _ => next_block_number(&FALLBACK_BLOCK_SEQUENCE),
    };
    let hash = if alternate_hash {
        alternate_block_hash(block_number)
    } else {
        block_hash(block_number)
    };
    serde_json::from_value(serde_json::json!({
        "baseFeePerGas":1,"number":block_number,"difficulty":0,"extraData":"0x",
        "gasLimit":30_000_000,"gasUsed":0,"hash":format!("0x{}", bytes_hex(&hash)),
        "logsBloom":format!("0x{}", "00".repeat(256)),"miner":format!("0x{}", "22".repeat(20)),
        "mixHash":format!("0x{}", "33".repeat(32)),"nonce":0,"parentHash":format!("0x{}", "44".repeat(32)),
        "receiptsRoot":format!("0x{}", "55".repeat(32)),"sha3Uncles":format!("0x{}", "66".repeat(32)),
        "size":1,"stateRoot":format!("0x{}", "77".repeat(32)),"timestamp":BLOCK_TIMESTAMP.with(|value| *value.borrow()),
        "totalDifficulty":0,"transactions":[],"transactionsRoot":format!("0x{}", "88".repeat(32)),"uncles":[]
    })).expect("valid mock block")
}

fn next_block_number(sequence: &'static std::thread::LocalKey<RefCell<Vec<u64>>>) -> u64 {
    sequence.with(|values| {
        let mut values = values.borrow_mut();
        if values.is_empty() {
            100
        } else {
            values.remove(0)
        }
    })
}

fn mock_receipt(hash: [u8; 32], reverted: bool, canonical: bool) -> TransactionReceipt {
    let mut logs = MINT_LOG.with(|value| {
        value
            .borrow()
            .as_ref()
            .map(|fixture| vec![mint_log(fixture)])
            .unwrap_or_default()
    });
    if let (Some(log), Some(log_index)) = (
        logs.first_mut(),
        RECEIPT_MINT_LOG_INDEX.with(|value| *value.borrow()),
    ) {
        log.log_index = Some(Nat256::from(log_index));
    }
    let logs = if logs.is_empty() {
        WITHDRAWAL.with(|value| {
            value
                .borrow()
                .as_ref()
                .map(|fixture| vec![withdrawal_log(fixture, hash)])
                .unwrap_or_default()
        })
    } else {
        logs
    };
    let block_number = OBSERVED_BLOCK_NUMBER.with(|value| *value.borrow());
    let block_hash = if canonical {
        block_hash(block_number)
    } else {
        alternate_block_hash(block_number)
    };
    serde_json::from_value(serde_json::json!({
        "blockHash":format!("0x{}", bytes_hex(&block_hash)),"blockNumber":block_number,"effectiveGasPrice":1,
        "gasUsed":21_000,"cumulativeGasUsed":21_000,"status":if reverted {0} else {1},"root":null,
        "transactionHash":format!("0x{}", bytes_hex(&hash)),"contractAddress":null,
        "from":format!("0x{}", bytes_hex(&OBSERVED_REQUESTER.with(|value| *value.borrow()))),"logs":logs,"logsBloom":format!("0x{}", "00".repeat(256)),
        "to":format!("0x{}", bytes_hex(&OBSERVED_CONTRACT.with(|value| *value.borrow()))),"transactionIndex":0,"type":"0x2"
    })).expect("valid mock receipt")
}

fn block_hash(block_number: u64) -> [u8; 32] {
    if let Some(hash) = FINALIZED_BLOCK_HASH_OVERRIDE.with(|value| {
        value
            .borrow()
            .filter(|(number, _)| *number == block_number)
            .map(|(_, hash)| hash)
    }) {
        return hash;
    }
    let mut hash = [0x11; 32];
    hash[24..].copy_from_slice(&block_number.to_be_bytes());
    hash
}

fn alternate_block_hash(block_number: u64) -> [u8; 32] {
    let mut hash = block_hash(block_number);
    hash[0] = 0xaa;
    hash
}

fn block_number_from_hash(value: &str) -> Option<u64> {
    let bytes = decode_hex_32(value.strip_prefix("0x")?)?;
    if let Some(block_number) = FINALIZED_BLOCK_HASH_OVERRIDE.with(|value| {
        value
            .borrow()
            .filter(|(_, hash)| *hash == bytes)
            .map(|(number, _)| number)
    }) {
        return Some(block_number);
    }
    if bytes[..24] != [0x11; 24] {
        return None;
    }
    Some(u64::from_be_bytes(bytes[24..].try_into().ok()?))
}

fn decode_hex_32(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut result = [0u8; 32];
    for (index, byte) in result.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(result)
}

ic_cdk::export_candid!();

pub fn generated_candid_interface() -> String {
    format!("{}\n", __export_service())
}

#[cfg(test)]
mod candid_tests {
    #[test]
    fn checked_in_candid_matches_exported_interface() {
        assert_eq!(
            super::generated_candid_interface(),
            include_str!("../mock.did")
        );
    }
}
