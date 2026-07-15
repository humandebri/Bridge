use crate::{
    config::BridgeInitArgs,
    evm_rpc, ledger,
    storage::{DepositIntent, DepositReserveAdmission, WithdrawalAttemptAdmissionError},
    storage_or_trap, STORE,
};
use bridge_core::{
    Account, Amount, DepositEvent, DepositId, DepositRecord, DepositRequest, DepositState,
    EvmCallIntent, EvmOperationId, EvmOperationKind, EvmOperationRecord, EvmOperationState,
    LedgerFailure, LedgerOperation, LedgerTransferIdentity, Settlement, TransferAttempt,
    WithdrawalEvent, WithdrawalId, WithdrawalRecord, WithdrawalState,
};
use candid::{CandidType, Deserialize, Nat, Principal};
use sha2::{Digest, Sha256};
use tiny_keccak::{Hasher, Keccak};

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositArgs {
    pub owner_sequence: u64,
    pub base_recipient: Vec<u8>,
    pub from_subaccount: Option<Vec<u8>>,
    pub gross_amount: Nat,
    pub max_service_fee: Nat,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ListDepositIdsArgs {
    pub owner: Principal,
    pub before_cursor: Option<u64>,
    pub limit: u16,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositIdPage {
    pub deposit_ids: Vec<Vec<u8>>,
    pub next_cursor: Option<u64>,
    pub oldest_available_cursor: Option<u64>,
    pub history_truncated: bool,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ListDepositIdsError {
    InvalidLimit,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositReceipt {
    pub deposit_id: Vec<u8>,
    pub owner_sequence: u64,
    pub state: String,
    pub settlement: Option<crate::tasks::SettlementActionResult>,
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
    SequenceMismatch { expected: u64 },
    DepositConflict,
    Busy,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositView {
    pub deposit_id: Vec<u8>,
    pub owner_sequence: u64,
    pub gross_amount: Nat,
    pub net_amount: Nat,
    pub service_fee: Nat,
    pub base_recipient: Vec<u8>,
    pub state: String,
    pub last_settlement_stop_reason: Option<String>,
    pub base_confirmation: Option<BaseConfirmationView>,
    pub next_automatic_confirmation_check_at_ns: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalView {
    pub withdrawal_id: Vec<u8>,
    pub amount: Nat,
    pub min_amount_out: Nat,
    pub state: String,
    pub last_settlement_stop_reason: Option<String>,
    pub base_confirmation: Option<BaseConfirmationView>,
    pub next_automatic_confirmation_check_at_ns: Option<u64>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum GetWithdrawalsError {
    TooManyIds,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct NotifyWithdrawalArgs {
    pub transaction_hash: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NotifyWithdrawalReceipt {
    Ingested {
        withdrawal_id: Vec<u8>,
        confirmed_head_block_number: u64,
        settlement: Option<crate::tasks::SettlementActionResult>,
    },
    Duplicate {
        withdrawal_id: Vec<u8>,
        settlement: Option<crate::tasks::SettlementActionResult>,
    },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NotifyWithdrawalError {
    AnonymousCaller,
    InvalidTransactionHash,
    RateLimited { retry_after_seconds: u64 },
    RpcUnavailable,
    RpcInconsistent,
    InvalidBaseResponse,
    TransactionNotFound,
    TransactionNotConfirmed,
    TransactionReverted,
    OwnerMismatch,
    LedgerFeeUnavailable,
    WithdrawalConflict,
    BaseStateMismatch,
    BridgeSignerMismatch,
    StorageFailure,
    Busy,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseConfirmationView {
    Submitted {
        transaction_hash: Vec<u8>,
    },
    Confirmed {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        confirmed_head_block_number: u64,
    },
    Reverted {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        confirmed_head_block_number: u64,
    },
}

enum AdmissionOutcome {
    Inserted,
    Existing,
}

pub async fn notify_withdrawal(
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
            .admit_withdrawal_notification_attempt(caller, ic_cdk::api::time())
            .map_err(|error| match error {
                WithdrawalAttemptAdmissionError::RateLimited {
                    retry_after_seconds,
                } => NotifyWithdrawalError::RateLimited {
                    retry_after_seconds,
                },
                WithdrawalAttemptAdmissionError::Storage => NotifyWithdrawalError::StorageFailure,
            })
    })?;
    let config = STORE
        .with(|store| store.borrow().config())
        .map_err(|_| NotifyWithdrawalError::StorageFailure)?
        .ok_or(NotifyWithdrawalError::StorageFailure)?;
    let outcome = evm_rpc::notified_withdrawal_outcome(&config, transaction_hash)
        .await
        .map_err(map_withdrawal_observation_error)?;
    let (observed, snapshot, confirmed_block_number) = match outcome {
        evm_rpc::NotifiedWithdrawalOutcome::Missing => {
            return Err(NotifyWithdrawalError::TransactionNotFound)
        }
        evm_rpc::NotifiedWithdrawalOutcome::Pending { .. } => {
            return Err(NotifyWithdrawalError::TransactionNotConfirmed)
        }
        evm_rpc::NotifiedWithdrawalOutcome::Reverted { .. } => {
            return Err(NotifyWithdrawalError::TransactionReverted)
        }
        evm_rpc::NotifiedWithdrawalOutcome::Confirmed {
            withdrawal,
            snapshot,
            confirmed_block_number,
            ..
        } => (withdrawal, snapshot, confirmed_block_number),
    };
    let owner = Principal::try_from_slice(&observed.owner)
        .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
    if owner != caller {
        return Err(NotifyWithdrawalError::OwnerMismatch);
    }
    let expected_signer = cached_signer_address(&config)
        .await
        .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
    if snapshot.bridge_signer != expected_signer {
        return Err(NotifyWithdrawalError::BridgeSignerMismatch);
    }
    let ledger_fee = ledger::ledger_fee(config.ledger_canister_id)
        .await
        .map_err(|_| NotifyWithdrawalError::LedgerFeeUnavailable)?;
    ingest_notified_withdrawal(
        &config,
        observed,
        snapshot.mint.service_fee.get(),
        snapshot.mint.max_service_fee.get(),
        ledger_fee,
        confirmed_block_number,
    )
}

pub(crate) fn notification_action_hash(
    caller: Principal,
    args: &NotifyWithdrawalArgs,
) -> Result<[u8; 32], NotifyWithdrawalError> {
    if caller == Principal::anonymous() {
        return Err(NotifyWithdrawalError::AnonymousCaller);
    }
    args.transaction_hash
        .as_slice()
        .try_into()
        .map_err(|_| NotifyWithdrawalError::InvalidTransactionHash)
}

fn map_withdrawal_observation_error(error: evm_rpc::ObservationError) -> NotifyWithdrawalError {
    match error {
        evm_rpc::ObservationError::Rpc => NotifyWithdrawalError::RpcUnavailable,
        evm_rpc::ObservationError::Inconsistent => NotifyWithdrawalError::RpcInconsistent,
        evm_rpc::ObservationError::BaseStateMismatch => NotifyWithdrawalError::BaseStateMismatch,
        evm_rpc::ObservationError::NonceConflict => NotifyWithdrawalError::InvalidBaseResponse,
        evm_rpc::ObservationError::InvalidResponse | evm_rpc::ObservationError::Overflow => {
            NotifyWithdrawalError::InvalidBaseResponse
        }
    }
}

fn ingest_notified_withdrawal(
    config: &BridgeInitArgs,
    observed: evm_rpc::ObservedWithdrawal,
    service_fee: u128,
    max_service_fee: u128,
    ledger_fee: Amount,
    confirmed_block_number: u64,
) -> Result<NotifyWithdrawalReceipt, NotifyWithdrawalError> {
    let mut digest = Sha256::new();
    digest.update(observed.id);
    digest.update(&observed.owner);
    digest.update(observed.subaccount);
    digest.update(observed.amount.to_be_bytes());
    digest.update(observed.min_amount_out.to_be_bytes());
    let payload_hash: [u8; 32] = digest.finalize().into();
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some(existing) = store
            .withdrawal(observed.id)
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?
        {
            if existing.payload_hash == payload_hash {
                return Ok(NotifyWithdrawalReceipt::Duplicate {
                    withdrawal_id: observed.id.to_vec(),
                    settlement: None,
                });
            }
            return Err(NotifyWithdrawalError::WithdrawalConflict);
        }

        let mut withdrawal = WithdrawalRecord::observed(
            WithdrawalId::new(observed.id),
            observed.owner.clone(),
            payload_hash,
            Amount::new(observed.amount),
            Amount::new(observed.min_amount_out),
            Amount::new(max_service_fee),
        )
        .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
        let mut progress = store
            .external_progress()
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
        progress.last_safe_base_block = progress.last_safe_base_block.max(confirmed_block_number);
        progress.last_safe_observation_ns = ic_cdk::api::time();

        let amount_out = observed
            .amount
            .checked_sub(service_fee)
            .and_then(|amount| amount.checked_sub(ledger_fee.get()));
        if amount_out.is_none_or(|amount| amount < observed.min_amount_out) {
            let operation_id = store
                .next_evm_operation_id()
                .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
            withdrawal
                .apply(WithdrawalEvent::PrepareReleaseCancellation {
                    operation_id,
                    expected_ledger_fee: ledger_fee,
                })
                .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
            let operation = EvmOperationRecord::queued(
                operation_id,
                withdrawal.payload_hash,
                EvmOperationKind::CancelRelease,
            );
            let mut calldata = selector("cancelRelease(uint256)").to_vec();
            calldata.extend_from_slice(&withdrawal.id.bytes());
            let intent = EvmCallIntent {
                operation_id,
                payload_hash: withdrawal.payload_hash,
                chain_id: config.base_chain_id,
                contract: config.contract_array(),
                calldata,
                gas_limit: config.transaction_gas_limit,
                max_fee_per_gas: config.max_fee_per_gas,
                max_priority_fee_per_gas: config.max_priority_fee_per_gas,
            };
            store
                .commit_new_withdrawal_operation_bundle(&withdrawal, &operation, &intent, &progress)
                .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
        } else {
            let amount_out = amount_out.expect("validated withdrawal amount");
            let transfer = LedgerTransferIdentity {
                operation: LedgerOperation::ReleaseWithdrawal,
                created_at_time_ns: ic_cdk::api::time(),
                memo: payload_hash,
                amount: Amount::new(amount_out),
                fee: ledger_fee,
                from: Account::new(ic_cdk::api::canister_self().as_slice().to_vec(), [0; 32])
                    .map_err(|_| NotifyWithdrawalError::StorageFailure)?,
                to: Account::new(observed.owner, observed.subaccount)
                    .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?,
                spender: None,
            };
            withdrawal
                .apply(WithdrawalEvent::StartRelease {
                    attempt: Box::new(TransferAttempt {
                        attempt_no: 0,
                        identity: transfer,
                    }),
                    settlement: Settlement {
                        amount_out: Amount::new(amount_out),
                        service_fee: Amount::new(service_fee),
                        ledger_fee,
                    },
                })
                .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
            store
                .commit_new_withdrawal_release_bundle(&withdrawal, &progress)
                .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
        }
        Ok(NotifyWithdrawalReceipt::Ingested {
            withdrawal_id: observed.id.to_vec(),
            confirmed_head_block_number: confirmed_block_number,
            settlement: None,
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

fn selector(signature: &str) -> [u8; 4] {
    let mut hash = [0u8; 32];
    let mut hasher = Keccak::v256();
    hasher.update(signature.as_bytes());
    hasher.finalize(&mut hash);
    hash[..4].try_into().expect("four byte selector")
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
    pub owner_sequence: u64,
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
        owner_sequence: args.owner_sequence,
        base_recipient,
        from_subaccount,
        gross_amount,
        max_service_fee,
    })
}

pub(crate) fn deposit_action_id(
    caller: Principal,
    args: &DepositArgs,
) -> Result<[u8; 32], DepositError> {
    validate_deposit_args(caller, args)?;
    Ok(derive_deposit_id(caller, args.owner_sequence))
}

fn derive_deposit_id(caller: Principal, owner_sequence: u64) -> [u8; 32] {
    hash(&[
        b"KINIC-DEPOSIT-ID-V2",
        caller.as_slice(),
        &owner_sequence.to_be_bytes(),
    ])
}

pub fn next_deposit_sequence(owner: Principal) -> u64 {
    STORE.with(|store| {
        store
            .borrow()
            .next_deposit_sequence(owner)
            .unwrap_or_else(|error| ic_cdk::trap(format!("deposit sequence read failed: {error}")))
    })
}

pub async fn request_deposit(
    caller: Principal,
    args: DepositArgs,
) -> Result<DepositReceipt, DepositError> {
    let validated = validate_deposit_args(caller, &args)?;
    let owner_sequence = validated.owner_sequence;
    let base_recipient = validated.base_recipient;
    let from_subaccount = validated.from_subaccount;
    let gross_amount = validated.gross_amount;
    let max_service_fee = validated.max_service_fee;
    let deposit_id = derive_deposit_id(caller, owner_sequence);
    let payload_hash = hash(&[
        b"KINIC-DEPOSIT-PAYLOAD-V2",
        caller.as_slice(),
        &owner_sequence.to_be_bytes(),
        &base_recipient,
        &from_subaccount,
        &gross_amount.to_be_bytes(),
        &max_service_fee.to_be_bytes(),
    ]);

    if let Some(receipt) = existing_receipt(deposit_id, payload_hash)? {
        return Ok(receipt);
    }

    let expected_sequence = STORE.with(|store| {
        store
            .borrow()
            .next_deposit_sequence(caller)
            .map_err(|_| DepositError::StorageFailure)
    })?;
    if owner_sequence != expected_sequence {
        return Err(DepositError::SequenceMismatch {
            expected: expected_sequence,
        });
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
    let signer_address = cached_signer_address(&config)
        .await
        .map_err(|_| DepositError::ReserveUnavailable)?;
    let (expected_counters, expected_observation_generation) = STORE.with(|store| {
        let store = store.borrow();
        Ok::<_, DepositError>((
            store.counters().map_err(|_| DepositError::StorageFailure)?,
            store
                .external_progress()
                .map_err(|_| DepositError::StorageFailure)?
                .reserve_observation_generation,
        ))
    })?;
    // Keep the ETH balance call as the final await. The following synchronous segment validates
    // the observation generation and pre-observation counters in the admission transaction, so a
    // competing message cannot combine this balance with newer, less conservative counters.
    let eth_balance_wei = evm_rpc::signer_eth_balance(&config, signer_address)
        .await
        .map_err(|_| DepositError::ReserveUnavailable)?;
    let cycles_balance = ic_cdk::api::canister_liquid_cycle_balance();
    let reserve_observed_at_ns = ic_cdk::api::time();
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
        owner_sequence,
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
            existing
                .verify_retry(payload_hash)
                .map_err(|_| DepositError::DepositConflict)?;
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
        if snapshot.confirmed_block_number < progress.last_safe_mint_block {
            return Err(DepositError::BaseObservationUnavailable);
        }
        store
            .admit_deposit(
                caller,
                &intent,
                &record,
                Some(DepositReserveAdmission {
                    audit_caller: ic_cdk::api::canister_self(),
                    expected_counters,
                    expected_observation_generation,
                    observed_at_ns: reserve_observed_at_ns,
                    eth_balance_wei,
                    cycles_balance,
                    reserve_policy: config.reserve_policy(),
                    mint_snapshot: snapshot,
                }),
            )
            .map_err(|error| match error {
                crate::storage::StorageError::SequenceMismatch { expected } => {
                    DepositError::SequenceMismatch { expected }
                }
                crate::storage::StorageError::Core(bridge_core::CoreError::ConflictingReplay) => {
                    DepositError::DepositConflict
                }
                crate::storage::StorageError::ReserveUnavailable
                | crate::storage::StorageError::StaleReserveObservation => {
                    DepositError::ReserveUnavailable
                }
                crate::storage::StorageError::Core(
                    bridge_core::CoreError::MintWindowLimitExceeded,
                ) => DepositError::Rejected("MintWindowLimitExceeded".into()),
                _ => DepositError::StorageFailure,
            })?;
        Ok(AdmissionOutcome::Inserted)
    })?;
    if matches!(admission, AdmissionOutcome::Existing) {
        return existing_receipt(deposit_id, payload_hash)?.ok_or(DepositError::StorageFailure);
    }

    existing_receipt(deposit_id, payload_hash)?.ok_or(DepositError::StorageFailure)
}

pub fn list_deposit_ids(args: ListDepositIdsArgs) -> Result<DepositIdPage, ListDepositIdsError> {
    if !(1..=100).contains(&args.limit) {
        return Err(ListDepositIdsError::InvalidLimit);
    }
    STORE.with(|store| {
        let page = store
            .borrow()
            .list_deposit_ids(args.owner, args.before_cursor, args.limit)
            .unwrap_or_else(|error| ic_cdk::trap(format!("deposit index read failed: {error}")));
        Ok(DepositIdPage {
            deposit_ids: page.deposit_ids.into_iter().map(Vec::from).collect(),
            next_cursor: page.next_cursor,
            oldest_available_cursor: page.oldest_available_cursor,
            history_truncated: page.history_truncated,
        })
    })
}

pub(crate) fn cancel_deposit_in_store(
    store: &mut crate::storage::StableStore,
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
    let minimum_confirmed_block = STORE.with(|store| {
        store
            .borrow()
            .external_progress()
            .map(|progress| progress.last_safe_mint_block)
            .map_err(|_| DepositError::StorageFailure)
    })?;
    if let Some(snapshot) = STORE.with(|store| {
        store
            .borrow()
            .cached_base_mint_snapshot(now_ns, BASE_SNAPSHOT_TTL_NS, minimum_confirmed_block)
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
    store: &mut crate::storage::StableStore,
    deposit_id: [u8; 32],
    block_index: u128,
    recipient: [u8; 20],
    config: &BridgeInitArgs,
) -> Result<(), DepositError> {
    prepare_mint_in_store_and_scan(store, deposit_id, block_index, recipient, config, None)
}

pub(crate) fn prepare_mint_in_store_and_scan(
    store: &mut crate::storage::StableStore,
    deposit_id: [u8; 32],
    block_index: u128,
    recipient: [u8; 20],
    config: &BridgeInitArgs,
    scan_target: Option<&bridge_core::ReconciliationTarget>,
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
    if let DepositState::ReconciliationHold { hold_id } = deposit.state {
        let mut hold = store
            .reconciliation_hold(hold_id.get())
            .map_err(|_| DepositError::StorageFailure)?
            .ok_or(DepositError::StorageFailure)?;
        bridge_core::resolve_deposit_hold(
            &mut deposit,
            &mut hold,
            bridge_core::DepositHoldResolution::Succeeded {
                ledger_block_index: block_index,
            },
        )
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    } else {
        deposit
            .apply(DepositEvent::PullSucceeded {
                ledger_block_index: block_index,
            })
            .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    }
    let operation_id = store
        .next_evm_operation_id()
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
        .commit_deposit_mint_bundle_and_scan(&deposit, &operation, &intent, scan_target)
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
        record
            .verify_retry(payload_hash)
            .map_err(|_| DepositError::DepositConflict)?;
        let intent = store
            .deposit_intent(id)
            .map_err(|_| DepositError::StorageFailure)?
            .ok_or(DepositError::StorageFailure)?;
        Ok(Some(DepositReceipt {
            deposit_id: id.to_vec(),
            owner_sequence: intent.owner_sequence,
            state: state_name(&record.state),
            settlement: None,
        }))
    })
}

pub(crate) fn deposit_state(id: [u8; 32]) -> Result<String, DepositError> {
    STORE.with(|store| {
        store
            .borrow()
            .deposit(id)
            .map_err(|_| DepositError::StorageFailure)?
            .map(|record| state_name(&record.state))
            .ok_or(DepositError::StorageFailure)
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
        let schedule = operation_id.and_then(|id| {
            storage_or_trap(
                "confirmation schedule read",
                store.confirmation_schedule(id.get()),
            )
        });
        let scheduler_health = storage_or_trap(
            "confirmation scheduler health read",
            store.confirmation_scheduler_health(),
        );
        let scheduler_fault = schedule
            .is_some()
            .then(|| scheduler_health.last_error.clone())
            .flatten()
            .filter(|_| !scheduler_health.healthy);
        Some(DepositView {
            deposit_id: id.to_vec(),
            owner_sequence: intent.owner_sequence,
            gross_amount: Nat::from(record.gross_amount.get()),
            net_amount: Nat::from(record.net_amount.get()),
            service_fee: Nat::from(record.service_fee.get()),
            base_recipient: intent.base_recipient.to_vec(),
            state: state_name(&record.state),
            last_settlement_stop_reason: record.last_settlement_stop_reason.or(scheduler_fault),
            base_confirmation: base_confirmation(&store, operation_id),
            next_automatic_confirmation_check_at_ns: scheduler_health
                .healthy
                .then_some(schedule)
                .flatten()
                .map(|schedule| schedule.next_check_at_ns),
        })
    })
}

pub fn get_withdrawal(id: Vec<u8>) -> Option<WithdrawalView> {
    let id: [u8; 32] = id.as_slice().try_into().ok()?;
    STORE.with(|store| {
        let record = storage_or_trap("withdrawal read", store.borrow().withdrawal(id))?;
        let operation_id = match &record.state {
            WithdrawalState::ReleaseCancellationPending { operation_id, .. }
            | WithdrawalState::ReleaseCancelled { operation_id }
            | WithdrawalState::AcknowledgePending { operation_id, .. }
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
            WithdrawalState::ReleaseCancellationPending { .. } => "ReleaseCancellationPending",
            WithdrawalState::ReleaseCancelled { .. } => "ReleaseCancelled",
        };
        let borrowed = store.borrow();
        let schedule = operation_id.and_then(|id| {
            storage_or_trap(
                "confirmation schedule read",
                borrowed.confirmation_schedule(id.get()),
            )
        });
        let scheduler_health = storage_or_trap(
            "confirmation scheduler health read",
            borrowed.confirmation_scheduler_health(),
        );
        let scheduler_fault = schedule
            .is_some()
            .then(|| scheduler_health.last_error.clone())
            .flatten()
            .filter(|_| !scheduler_health.healthy);
        Some(WithdrawalView {
            withdrawal_id: id.to_vec(),
            amount: Nat::from(record.amount.get()),
            min_amount_out: Nat::from(record.min_amount_out.get()),
            state: state.into(),
            last_settlement_stop_reason: record.last_settlement_stop_reason.or(scheduler_fault),
            base_confirmation: base_confirmation(&borrowed, operation_id),
            next_automatic_confirmation_check_at_ns: scheduler_health
                .healthy
                .then_some(schedule)
                .flatten()
                .map(|schedule| schedule.next_check_at_ns),
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

fn base_confirmation(
    store: &crate::storage::StableStore,
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
        EvmOperationState::Confirmed {
            transaction_hash,
            receipt_block_number,
            confirmed_block_number,
        } => Some(BaseConfirmationView::Confirmed {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number,
            confirmed_head_block_number: confirmed_block_number,
        }),
        EvmOperationState::Reverted {
            transaction_hash,
            receipt_block_number,
            confirmed_block_number,
        } => Some(BaseConfirmationView::Reverted {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number,
            confirmed_head_block_number: confirmed_block_number,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_identity_binds_caller_and_owner_sequence_and_calldata_is_static() {
        let first = Principal::self_authenticating([1; 32]);
        let second = Principal::self_authenticating([2; 32]);
        let args = DepositArgs {
            owner_sequence: 7,
            base_recipient: vec![2; 20],
            from_subaccount: None,
            gross_amount: Nat::from(3u8),
            max_service_fee: Nat::from(1u8),
        };
        assert_eq!(
            deposit_action_id(first, &args),
            deposit_action_id(first, &args)
        );
        assert_ne!(
            deposit_action_id(first, &args),
            deposit_action_id(second, &args)
        );
        let mut next = args.clone();
        next.owner_sequence += 1;
        assert_ne!(
            deposit_action_id(first, &args),
            deposit_action_id(first, &next)
        );
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
