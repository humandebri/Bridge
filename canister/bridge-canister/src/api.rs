use crate::{
    config::BridgeInitArgs,
    evm_rpc, ledger,
    storage::{DepositIntent, NotificationEnqueueError, NotificationEnqueueOutcome},
    storage_or_trap, STORE,
};
use bridge_core::{
    Account, Amount, DepositEvent, DepositId, DepositRecord, DepositRequest, DepositState,
    EvmCallIntent, EvmOperationId, EvmOperationKind, EvmOperationRecord, EvmOperationState,
    LedgerCallOutcome, LedgerFailure, LedgerOperation, LedgerTransferIdentity,
    ReconciliationHoldRecord, RequestReference, WithdrawalState,
};
use candid::{CandidType, Deserialize, Nat, Principal};
use ic_stable_structures::Memory;
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositArgs {
    pub client_request_id: Vec<u8>,
    pub base_recipient: Vec<u8>,
    pub from_subaccount: Option<Vec<u8>>,
    pub gross_amount: Nat,
    pub max_service_fee: Nat,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ListDepositIdsArgs {
    pub owner: Principal,
    pub before_sequence: Option<u64>,
    pub limit: u16,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositIdPage {
    pub deposit_ids: Vec<Vec<u8>>,
    pub next_before_sequence: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ListDepositIdsError {
    InvalidLimit,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositReceipt {
    pub deposit_id: Vec<u8>,
    pub state: String,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DepositError {
    InvalidRequest(String),
    BaseObservationUnavailable,
    LedgerFeeUnavailable,
    Rejected(String),
    StorageFailure,
    DepositsPaused,
    ReserveUnavailable,
    RateLimited { retry_after_seconds: u64 },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositView {
    pub deposit_id: Vec<u8>,
    pub gross_amount: Nat,
    pub net_amount: Nat,
    pub service_fee: Nat,
    pub base_recipient: Vec<u8>,
    pub state: String,
    pub base_confirmation: Option<BaseConfirmationView>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalView {
    pub withdrawal_id: Vec<u8>,
    pub amount: Nat,
    pub min_amount_out: Nat,
    pub state: String,
    pub base_confirmation: Option<BaseConfirmationView>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum GetWithdrawalsError {
    TooManyIds,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NotifyWithdrawalArgs {
    pub transaction_hash: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotifyWithdrawalReceipt {
    Queued,
    Duplicate,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NotifyWithdrawalError {
    AnonymousCaller,
    InvalidTransactionHash,
    RateLimited { retry_after_seconds: u64 },
    QueueFull,
    StorageFailure,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseConfirmationView {
    Submitted {
        transaction_hash: Vec<u8>,
    },
    Finalized {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        observed_head: u64,
    },
    Reverted {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        observed_head: u64,
    },
}

enum AdmissionOutcome {
    Inserted,
    Existing,
}

pub fn notify_withdrawal(
    caller: Principal,
    args: NotifyWithdrawalArgs,
) -> Result<NotifyWithdrawalReceipt, NotifyWithdrawalError> {
    if caller == Principal::anonymous() {
        return Err(NotifyWithdrawalError::AnonymousCaller);
    }
    let transaction_hash: [u8; 32] = args
        .transaction_hash
        .as_slice()
        .try_into()
        .map_err(|_| NotifyWithdrawalError::InvalidTransactionHash)?;
    STORE.with(|store| {
        store
            .borrow_mut()
            .enqueue_withdrawal_notification(caller, transaction_hash, ic_cdk::api::time())
            .map(|outcome| match outcome {
                NotificationEnqueueOutcome::Queued => NotifyWithdrawalReceipt::Queued,
                NotificationEnqueueOutcome::Duplicate => NotifyWithdrawalReceipt::Duplicate,
            })
            .map_err(|error| match error {
                NotificationEnqueueError::RateLimited {
                    retry_after_seconds,
                } => NotifyWithdrawalError::RateLimited {
                    retry_after_seconds,
                },
                NotificationEnqueueError::QueueFull => NotifyWithdrawalError::QueueFull,
                NotificationEnqueueError::Storage => NotifyWithdrawalError::StorageFailure,
            })
    })
}

const BASE_SNAPSHOT_TTL_NS: u64 = 60_000_000_000;
const BASE_SNAPSHOT_REFRESH_COOLDOWN_NS: u64 = 60_000_000_000;
const BASE_SNAPSHOT_REFRESH_STALE_LOCK_NS: u64 = 300_000_000_000;

fn nat_u128(value: &Nat) -> Result<u128, DepositError> {
    value
        .0
        .to_string()
        .parse()
        .map_err(|_| DepositError::InvalidRequest("amount exceeds u128".into()))
}

fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    digest.finalize().into()
}

fn state_name(state: &DepositState) -> String {
    match state {
        DepositState::PullPending => "PullPending",
        DepositState::Escrowed { .. } => "Escrowed",
        DepositState::MintPending { .. } => "MintPending",
        DepositState::Minted { .. } => "Minted",
        DepositState::MintReverted { .. } => "MintReverted",
        DepositState::ReconciliationHold { .. } => "ReconciliationHold",
        DepositState::Cancelled { .. } => "Cancelled",
    }
    .into()
}

fn config() -> Result<BridgeInitArgs, DepositError> {
    STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| DepositError::StorageFailure)?
            .ok_or(DepositError::StorageFailure)
    })
}

pub(crate) struct ValidatedDepositArgs {
    pub client_request_id: [u8; 32],
    pub base_recipient: [u8; 20],
    pub from_subaccount: [u8; 32],
    pub gross_amount: u128,
    pub max_service_fee: u128,
}

pub(crate) fn validate_deposit_args(
    caller: Principal,
    args: &DepositArgs,
) -> Result<ValidatedDepositArgs, DepositError> {
    if caller == Principal::anonymous() {
        return Err(DepositError::InvalidRequest(
            "anonymous caller is not allowed".into(),
        ));
    }
    let client_request_id =
        args.client_request_id.as_slice().try_into().map_err(|_| {
            DepositError::InvalidRequest("client_request_id must be 32 bytes".into())
        })?;
    let base_recipient: [u8; 20] = args
        .base_recipient
        .as_slice()
        .try_into()
        .map_err(|_| DepositError::InvalidRequest("base_recipient must be 20 bytes".into()))?;
    if base_recipient == [0; 20] {
        return Err(DepositError::InvalidRequest(
            "zero Base recipient is not allowed".into(),
        ));
    }
    let from_subaccount = match args.from_subaccount.as_deref() {
        None => [0; 32],
        Some(bytes) => bytes
            .try_into()
            .map_err(|_| DepositError::InvalidRequest("from_subaccount must be 32 bytes".into()))?,
    };
    let gross_amount = nat_u128(&args.gross_amount)?;
    let max_service_fee = nat_u128(&args.max_service_fee)?;
    if gross_amount == 0 {
        return Err(DepositError::InvalidRequest(
            "gross_amount must be positive".into(),
        ));
    }
    Ok(ValidatedDepositArgs {
        client_request_id,
        base_recipient,
        from_subaccount,
        gross_amount,
        max_service_fee,
    })
}

pub async fn request_deposit(
    caller: Principal,
    args: DepositArgs,
) -> Result<DepositReceipt, DepositError> {
    let validated = validate_deposit_args(caller, &args)?;
    let client_request_id = validated.client_request_id;
    let base_recipient = validated.base_recipient;
    let from_subaccount = validated.from_subaccount;
    let gross_amount = validated.gross_amount;
    let max_service_fee = validated.max_service_fee;
    let deposit_id = hash(&[caller.as_slice(), &client_request_id]);
    let payload_hash = hash(&[
        caller.as_slice(),
        &client_request_id,
        &base_recipient,
        &from_subaccount,
        &gross_amount.to_be_bytes(),
        &max_service_fee.to_be_bytes(),
    ]);

    if let Some(receipt) = existing_receipt(deposit_id, payload_hash)? {
        return Ok(receipt);
    }

    let config = config()?;
    let deposits_paused = STORE.with(|store| {
        store
            .borrow()
            .admin_state()
            .map(|state| state.deposits_paused)
            .map_err(|_| DepositError::StorageFailure)
    })?;
    if deposits_paused {
        return Err(DepositError::DepositsPaused);
    }
    let now = ic_cdk::api::time();
    STORE.with(|store| {
        store
            .borrow_mut()
            .reserve_deposit_quota(
                caller,
                now,
                config.deposit_rate_limit_window_seconds,
                config.deposit_rate_limit_global,
                config.deposit_rate_limit_per_principal,
            )
            .map_err(|error| match error {
                crate::storage::DepositQuotaError::RateLimited(limited) => {
                    DepositError::RateLimited {
                        retry_after_seconds: limited.retry_after_seconds,
                    }
                }
                crate::storage::DepositQuotaError::Storage(_) => DepositError::StorageFailure,
            })
    })?;
    let snapshot = base_mint_snapshot(&config, now).await?;
    ensure_reserve(&config).await?;
    crate::tasks::ensure_nonce_initialized(&config)
        .await
        .map_err(|error| match error {
            crate::tasks::NonceInitializationError::Observation => {
                DepositError::BaseObservationUnavailable
            }
            crate::tasks::NonceInitializationError::Storage => DepositError::StorageFailure,
        })?;
    let ledger_fee = ledger::ledger_fee(config.ledger_canister_id)
        .await
        .map_err(|_| DepositError::LedgerFeeUnavailable)?;
    let memo = hash(&[b"KINIC-DEPOSIT", &deposit_id]);
    let canister = ic_cdk::api::canister_self();
    let transfer = LedgerTransferIdentity {
        operation: LedgerOperation::PullDeposit,
        created_at_time_ns: now,
        memo,
        amount: Amount::new(gross_amount),
        fee: ledger_fee,
        from: Account::new(caller.as_slice().to_vec(), from_subaccount)
            .map_err(|e| DepositError::Rejected(format!("{e:?}")))?,
        to: Account::new(canister.as_slice().to_vec(), [0; 32])
            .map_err(|e| DepositError::Rejected(format!("{e:?}")))?,
        spender: Some(
            Account::new(canister.as_slice().to_vec(), [0; 32])
                .map_err(|e| DepositError::Rejected(format!("{e:?}")))?,
        ),
    };
    let intent = DepositIntent {
        deposit_id,
        caller: caller.as_slice().to_vec(),
        client_request_id,
        base_recipient,
        from_subaccount,
        payload_hash,
    };
    let record = DepositRecord::accept(
        DepositRequest {
            id: DepositId::new(deposit_id),
            payload_hash,
            gross_amount: Amount::new(gross_amount),
            user_max_service_fee: Amount::new(max_service_fee),
            transfer: transfer.clone(),
        },
        snapshot,
    )
    .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    let admission = STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some(existing) = store
            .deposit(deposit_id)
            .map_err(|_| DepositError::StorageFailure)?
        {
            existing.verify_retry(payload_hash).map_err(|_| {
                DepositError::InvalidRequest(
                    "client request id conflicts with an existing payload".into(),
                )
            })?;
            return Ok(AdmissionOutcome::Existing);
        }
        if store
            .admin_state()
            .map_err(|_| DepositError::StorageFailure)?
            .deposits_paused
        {
            return Err(DepositError::DepositsPaused);
        }
        let progress = store
            .external_progress()
            .map_err(|_| DepositError::StorageFailure)?;
        if snapshot.finalized_block_number < progress.last_finalized_mint_block {
            return Err(DepositError::BaseObservationUnavailable);
        }
        let reserved = store
            .counters()
            .map_err(|_| DepositError::StorageFailure)?
            .reserved_deposit_mint_amount;
        let total = bridge_core::mint_admission_total(
            snapshot.effective_minted_in_window().get(),
            reserved,
            record.net_amount.get(),
        )
        .ok_or_else(|| DepositError::Rejected("mint admission arithmetic overflow".into()))?;
        if total > snapshot.mint_window_limit.get() {
            return Err(DepositError::Rejected("MintWindowLimitExceeded".into()));
        }
        store
            .admit_deposit(caller, &intent, &record)
            .map_err(|_| DepositError::StorageFailure)?;
        Ok(AdmissionOutcome::Inserted)
    })?;
    if matches!(admission, AdmissionOutcome::Existing) {
        return existing_receipt(deposit_id, payload_hash)?.ok_or(DepositError::StorageFailure);
    }

    match ledger::pull(config.ledger_canister_id, &transfer).await {
        LedgerCallOutcome::Succeeded { block_index }
        | LedgerCallOutcome::Duplicate { block_index } => {
            prepare_mint(deposit_id, block_index, base_recipient, &config)?;
        }
        LedgerCallOutcome::Ambiguous => {
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                let hold_id = store
                    .allocate_hold_id()
                    .map_err(|_| DepositError::StorageFailure)?;
                let mut deposit = store
                    .deposit(deposit_id)
                    .map_err(|_| DepositError::StorageFailure)?
                    .ok_or(DepositError::StorageFailure)?;
                deposit
                    .apply(DepositEvent::PullAmbiguous { hold_id })
                    .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
                let hold = ReconciliationHoldRecord::open(
                    hold_id,
                    RequestReference::Deposit(deposit.id),
                    transfer,
                );
                store
                    .put_deposit(&deposit)
                    .map_err(|_| DepositError::StorageFailure)?;
                store
                    .put_open_reconciliation_hold(&hold)
                    .map_err(|_| DepositError::StorageFailure)
            })?;
        }
        LedgerCallOutcome::DefinitiveFailure { code } => {
            STORE
                .with(|store| cancel_deposit_in_store(&mut store.borrow_mut(), deposit_id, code))?;
            return Err(DepositError::Rejected(format!("{code:?}")));
        }
        LedgerCallOutcome::RetryableFailure { .. } => {}
    }
    existing_receipt(deposit_id, payload_hash)?.ok_or(DepositError::StorageFailure)
}

pub fn list_deposit_ids(args: ListDepositIdsArgs) -> Result<DepositIdPage, ListDepositIdsError> {
    if !(1..=100).contains(&args.limit) {
        return Err(ListDepositIdsError::InvalidLimit);
    }
    STORE.with(|store| {
        let (deposit_ids, next_before_sequence) = store
            .borrow()
            .list_deposit_ids(args.owner, args.before_sequence, args.limit)
            .unwrap_or_else(|error| ic_cdk::trap(format!("deposit index read failed: {error}")));
        Ok(DepositIdPage {
            deposit_ids: deposit_ids.into_iter().map(Vec::from).collect(),
            next_before_sequence,
        })
    })
}

pub(crate) fn cancel_deposit_in_store(
    store: &mut crate::storage::StableStore<ic_stable_structures::DefaultMemoryImpl>,
    deposit_id: [u8; 32],
    code: LedgerFailure,
) -> Result<(), DepositError> {
    let mut deposit = store
        .deposit(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    deposit
        .apply(DepositEvent::PullFailed { code })
        .map_err(|error| DepositError::Rejected(format!("{error:?}")))?;
    store
        .put_deposit(&deposit)
        .map_err(|_| DepositError::StorageFailure)
}

async fn ensure_reserve(config: &BridgeInitArgs) -> Result<(), DepositError> {
    let address = cached_signer_address(config)
        .await
        .map_err(|_| DepositError::ReserveUnavailable)?;
    let eth_balance = evm_rpc::signer_eth_balance(config, address)
        .await
        .map_err(|_| DepositError::ReserveUnavailable)?;
    let withdrawals = STORE.with(|store| {
        store
            .borrow()
            .nonterminal_withdrawal_count()
            .map_err(|_| DepositError::StorageFailure)
    })?;
    let snapshot = config
        .reserve_policy()
        .snapshot(
            withdrawals,
            eth_balance,
            ic_cdk::api::canister_liquid_cycle_balance(),
        )
        .map_err(|_| DepositError::ReserveUnavailable)?;
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut progress = store
            .external_progress()
            .map_err(|_| DepositError::StorageFailure)?;
        let changed = progress.reserve_sufficient != snapshot.sufficient;
        progress.last_eth_balance_wei = eth_balance;
        progress.reserve_sufficient = snapshot.sufficient;
        progress.last_reserve_observation_ns = ic_cdk::api::time();
        store
            .set_external_progress(&progress)
            .map_err(|_| DepositError::StorageFailure)?;
        if changed {
            store
                .append_audit_event(
                    ic_cdk::api::canister_self(),
                    crate::storage::AuditEventKind::ReserveGateChanged {
                        sufficient: snapshot.sufficient,
                    },
                )
                .map_err(|_| DepositError::StorageFailure)?;
        }
        Ok(())
    })?;
    snapshot
        .sufficient
        .then_some(())
        .ok_or(DepositError::ReserveUnavailable)
}

pub(crate) async fn cached_signer_address(
    config: &BridgeInitArgs,
) -> Result<[u8; 20], DepositError> {
    if let Some(address) = STORE.with(|store| {
        store
            .borrow()
            .signer_address()
            .map_err(|_| DepositError::StorageFailure)
    })? {
        return Ok(address);
    }
    let derived = crate::signer::ethereum_address(config)
        .await
        .map_err(|_| DepositError::BaseObservationUnavailable)?;
    STORE.with(|store| {
        store
            .borrow_mut()
            .set_signer_address_if_absent(derived)
            .map_err(|_| DepositError::StorageFailure)
    })
}

async fn base_mint_snapshot(
    config: &BridgeInitArgs,
    now_ns: u64,
) -> Result<bridge_core::BaseMintSnapshot, DepositError> {
    let expected_signer = cached_signer_address(config).await?;
    let minimum_finalized_block = STORE.with(|store| {
        store
            .borrow()
            .external_progress()
            .map(|progress| progress.last_finalized_mint_block)
            .map_err(|_| DepositError::StorageFailure)
    })?;
    if let Some(snapshot) = STORE.with(|store| {
        store
            .borrow()
            .cached_base_mint_snapshot(now_ns, BASE_SNAPSHOT_TTL_NS, minimum_finalized_block)
            .map_err(|_| DepositError::StorageFailure)
    })? {
        return validate_base_deposit_snapshot(
            snapshot.snapshot,
            snapshot.bridge_signer,
            snapshot.deposits_paused,
            expected_signer,
        );
    }
    let refresh_started = STORE.with(|store| {
        store
            .borrow_mut()
            .begin_base_snapshot_refresh(
                now_ns,
                BASE_SNAPSHOT_REFRESH_STALE_LOCK_NS,
                BASE_SNAPSHOT_REFRESH_COOLDOWN_NS,
            )
            .map_err(|_| DepositError::StorageFailure)
    })?;
    if !refresh_started {
        return Err(DepositError::BaseObservationUnavailable);
    }
    let snapshot = match evm_rpc::bridge_snapshot(config).await {
        Ok(snapshot) => snapshot,
        Err(_) => {
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .fail_base_snapshot_refresh()
                    .map_err(|_| DepositError::StorageFailure)
            })?;
            return Err(DepositError::BaseObservationUnavailable);
        }
    };
    STORE.with(|store| {
        store
            .borrow_mut()
            .finish_base_snapshot_refresh(
                ic_cdk::api::time(),
                snapshot.mint,
                snapshot.bridge_signer,
                snapshot.deposits_paused,
            )
            .map_err(|_| DepositError::StorageFailure)
    })?;
    validate_base_deposit_snapshot(
        snapshot.mint,
        snapshot.bridge_signer,
        snapshot.deposits_paused,
        expected_signer,
    )
}

fn validate_base_deposit_snapshot(
    snapshot: bridge_core::BaseMintSnapshot,
    bridge_signer: [u8; 20],
    deposits_paused: bool,
    expected_signer: [u8; 20],
) -> Result<bridge_core::BaseMintSnapshot, DepositError> {
    if deposits_paused {
        return Err(DepositError::DepositsPaused);
    }
    if bridge_signer != expected_signer {
        return Err(DepositError::BaseObservationUnavailable);
    }
    Ok(snapshot)
}

pub(crate) fn prepare_mint(
    deposit_id: [u8; 32],
    block_index: u128,
    recipient: [u8; 20],
    config: &BridgeInitArgs,
) -> Result<(), DepositError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        prepare_mint_in_store(&mut store, deposit_id, block_index, recipient, config)
    })
}

pub(crate) fn prepare_mint_in_store(
    store: &mut crate::storage::StableStore<ic_stable_structures::DefaultMemoryImpl>,
    deposit_id: [u8; 32],
    block_index: u128,
    recipient: [u8; 20],
    config: &BridgeInitArgs,
) -> Result<(), DepositError> {
    let mut deposit = store
        .deposit(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    if matches!(
        deposit.state,
        DepositState::MintPending { .. } | DepositState::Minted { .. }
    ) {
        return Ok(());
    }
    deposit
        .apply(DepositEvent::PullSucceeded {
            ledger_block_index: block_index,
        })
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    let operation_id = store
        .allocate_evm_operation_id()
        .map_err(|_| DepositError::StorageFailure)?;
    deposit
        .apply(DepositEvent::PrepareMint { operation_id })
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    let operation = EvmOperationRecord::queued(
        operation_id,
        deposit.payload_hash,
        EvmOperationKind::MintDeposit,
    );
    let intent = EvmCallIntent {
        operation_id,
        payload_hash: deposit.payload_hash,
        chain_id: config.base_chain_id,
        contract: config.contract_array(),
        calldata: mint_calldata(
            deposit_id,
            recipient,
            deposit.gross_amount.get(),
            deposit.max_service_fee.get(),
            deposit.service_fee.get(),
        ),
        gas_limit: config.transaction_gas_limit,
        max_fee_per_gas: config.max_fee_per_gas,
        max_priority_fee_per_gas: config.max_priority_fee_per_gas,
    };
    store
        .put_evm_call_intent(&intent)
        .map_err(|_| DepositError::StorageFailure)?;
    store
        .put_evm_operation(&operation)
        .map_err(|_| DepositError::StorageFailure)?;
    store
        .put_deposit(&deposit)
        .map_err(|_| DepositError::StorageFailure)?;
    Ok(())
}

fn mint_calldata(
    deposit_id: [u8; 32],
    recipient: [u8; 20],
    gross: u128,
    max_fee: u128,
    charged_fee: u128,
) -> Vec<u8> {
    let mut selector_hash = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(b"mintDeposit((bytes32,address,uint256,uint256,uint256))");
    keccak.finalize(&mut selector_hash);
    let mut data = selector_hash[..4].to_vec();
    data.extend_from_slice(&deposit_id);
    data.extend_from_slice(&[0; 12]);
    data.extend_from_slice(&recipient);
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(&gross.to_be_bytes());
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(&max_fee.to_be_bytes());
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(&charged_fee.to_be_bytes());
    data
}

fn existing_receipt(
    id: [u8; 32],
    payload_hash: [u8; 32],
) -> Result<Option<DepositReceipt>, DepositError> {
    STORE.with(|store| {
        let store = store.borrow();
        let Some(record) = store
            .deposit(id)
            .map_err(|_| DepositError::StorageFailure)?
        else {
            return Ok(None);
        };
        record.verify_retry(payload_hash).map_err(|_| {
            DepositError::InvalidRequest(
                "client request id conflicts with an existing payload".into(),
            )
        })?;
        Ok(Some(DepositReceipt {
            deposit_id: id.to_vec(),
            state: state_name(&record.state),
        }))
    })
}

pub fn get_deposit(id: Vec<u8>) -> Option<DepositView> {
    let id: [u8; 32] = id.as_slice().try_into().ok()?;
    STORE.with(|store| {
        let store = store.borrow();
        let record = storage_or_trap("deposit read", store.deposit(id))?;
        let intent = storage_or_trap("deposit intent read", store.deposit_intent(id))?;
        let operation_id = match &record.state {
            DepositState::MintPending { operation_id, .. }
            | DepositState::Minted { operation_id, .. }
            | DepositState::MintReverted { operation_id, .. } => Some(*operation_id),
            _ => None,
        };
        Some(DepositView {
            deposit_id: id.to_vec(),
            gross_amount: Nat::from(record.gross_amount.get()),
            net_amount: Nat::from(record.net_amount.get()),
            service_fee: Nat::from(record.service_fee.get()),
            base_recipient: intent.base_recipient.to_vec(),
            state: state_name(&record.state),
            base_confirmation: base_confirmation(&store, operation_id),
        })
    })
}

pub fn get_withdrawal(id: Vec<u8>) -> Option<WithdrawalView> {
    let id: [u8; 32] = id.as_slice().try_into().ok()?;
    STORE.with(|store| {
        let record = storage_or_trap("withdrawal read", store.borrow().withdrawal(id))?;
        let operation_id = match &record.state {
            WithdrawalState::AcknowledgePending { operation_id, .. }
            | WithdrawalState::AcknowledgeReverted { operation_id, .. }
            | WithdrawalState::Released { operation_id, .. }
            | WithdrawalState::RefundPending { operation_id, .. }
            | WithdrawalState::RefundReverted { operation_id, .. }
            | WithdrawalState::Refunded { operation_id } => Some(*operation_id),
            _ => None,
        };
        let state = match record.state {
            WithdrawalState::Observed => "Observed",
            WithdrawalState::ReleasePending { .. } => "ReleasePending",
            WithdrawalState::ReleaseTransferred { .. } => "ReleaseTransferred",
            WithdrawalState::AcknowledgePending { .. } => "AcknowledgePending",
            WithdrawalState::AcknowledgeReverted { .. } => "AcknowledgeReverted",
            WithdrawalState::Released { .. } => "Released",
            WithdrawalState::RefundPending { .. } => "RefundPending",
            WithdrawalState::RefundReverted { .. } => "RefundReverted",
            WithdrawalState::Refunded { .. } => "Refunded",
            WithdrawalState::ReconciliationHold { .. } => "ReconciliationHold",
        };
        Some(WithdrawalView {
            withdrawal_id: id.to_vec(),
            amount: Nat::from(record.amount.get()),
            min_amount_out: Nat::from(record.min_amount_out.get()),
            state: state.into(),
            base_confirmation: base_confirmation(&store.borrow(), operation_id),
        })
    })
}

pub fn get_withdrawals(
    ids: Vec<Vec<u8>>,
) -> Result<Vec<Option<WithdrawalView>>, GetWithdrawalsError> {
    if ids.len() > 20 {
        return Err(GetWithdrawalsError::TooManyIds);
    }
    Ok(ids.into_iter().map(get_withdrawal).collect())
}

fn base_confirmation<M: Memory>(
    store: &crate::storage::StableStore<M>,
    operation_id: Option<EvmOperationId>,
) -> Option<BaseConfirmationView> {
    let operation = storage_or_trap(
        "EVM operation read",
        store.evm_operation(operation_id?.get()),
    )?;
    match operation.state {
        EvmOperationState::Queued | EvmOperationState::Prepared => None,
        EvmOperationState::Submitted { transaction_hash } => {
            Some(BaseConfirmationView::Submitted {
                transaction_hash: transaction_hash.to_vec(),
            })
        }
        EvmOperationState::Finalized {
            transaction_hash,
            receipt_block_number,
            finalized_block_number,
        } => Some(BaseConfirmationView::Finalized {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number,
            observed_head: finalized_block_number,
        }),
        EvmOperationState::Reverted {
            transaction_hash,
            receipt_block_number,
            finalized_block_number,
        } => Some(BaseConfirmationView::Reverted {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number,
            observed_head: finalized_block_number,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_identity_binds_caller_and_client_id_and_calldata_is_static() {
        assert_ne!(hash(&[b"a", &[1; 32]]), hash(&[b"b", &[1; 32]]));
        let calldata = mint_calldata([1; 32], [2; 20], 3, 4, 5);
        assert_eq!(calldata.len(), 4 + 32 * 5);
        assert_eq!(&calldata[..4], &[0x84, 0xc7, 0x27, 0xfe]);
        assert_eq!(&calldata[4..36], &[1; 32]);
        assert_eq!(&calldata[36..48], &[0; 12]);
        assert_eq!(&calldata[48..68], &[2; 20]);
        assert_eq!(&calldata[68..84], &[0; 16]);
        assert_eq!(&calldata[84..100], &3u128.to_be_bytes());
        assert_eq!(&calldata[100..116], &[0; 16]);
        assert_eq!(&calldata[116..132], &4u128.to_be_bytes());
        assert_eq!(&calldata[132..148], &[0; 16]);
        assert_eq!(&calldata[148..164], &5u128.to_be_bytes());
    }
}
