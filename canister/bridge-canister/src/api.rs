use crate::{
    config::BridgeInitArgs,
    evm_calls, evm_rpc, ledger,
    phases::{DepositPhase, WithdrawalPhase},
    rpc_audit_event_kind, rpc_decision_event_kind,
    storage::{DepositIntent, DepositReserveAdmission},
    storage_or_trap, STORE,
};
use bridge_core::{
    Account, Amount, DepositEvent, DepositId, DepositQuote, DepositRecord, DepositRefundReason,
    DepositRequest, DepositState, EvmOperationId, EvmOperationKind, EvmOperationRecord,
    EvmOperationState, FinalizedObservationRecord, LedgerFailure, LedgerOperation,
    LedgerTransferIdentity, Settlement, TransferAttempt, WithdrawalEvent, WithdrawalId,
    WithdrawalRecord, WithdrawalState,
};
use candid::{CandidType, Deserialize, Nat, Principal};
use sha2::{Digest, Sha256};

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
    pub state: DepositPhase,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DepositError {
    InvalidRequest(String),
    BaseObservationUnavailable,
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
    pub quote: Option<DepositQuoteView>,
    pub refund: Option<DepositRefundView>,
    pub max_service_fee: Nat,
    pub base_recipient: Vec<u8>,
    pub from_subaccount: Option<Vec<u8>>,
    pub state: DepositPhase,
    pub last_settlement_stop_reason: Option<String>,
    pub base_confirmation: Option<BaseConfirmationView>,
    pub automatic_progress: Option<AutomaticProgressView>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositQuoteView {
    pub service_fee: Nat,
    pub net_amount: Nat,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum DepositRefundReasonView {
    BasePaused,
    ServiceFeeRejected,
    PerDepositLimitExceeded,
    MintWindowLimitExceeded,
    ReserveInsufficient,
}

impl From<DepositRefundReason> for DepositRefundReasonView {
    fn from(value: DepositRefundReason) -> Self {
        match value {
            DepositRefundReason::BasePaused => Self::BasePaused,
            DepositRefundReason::ServiceFeeRejected => Self::ServiceFeeRejected,
            DepositRefundReason::PerDepositLimitExceeded => Self::PerDepositLimitExceeded,
            DepositRefundReason::MintWindowLimitExceeded => Self::MintWindowLimitExceeded,
            DepositRefundReason::ReserveInsufficient => Self::ReserveInsufficient,
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositRefundView {
    pub reason: DepositRefundReasonView,
    pub amount: Nat,
    pub ledger_fee: Nat,
    pub attempt_no: u64,
    pub block_index: Option<Nat>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalView {
    pub withdrawal_id: Vec<u8>,
    pub amount: Nat,
    pub max_service_fee: Nat,
    pub charged_service_fee: Nat,
    pub amount_out: Nat,
    pub ledger_fee: Nat,
    pub state: WithdrawalPhase,
    pub last_settlement_stop_reason: Option<String>,
    pub automatic_progress: Option<AutomaticProgressView>,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomaticProgressPhase {
    Confirmation,
    Settlement,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomaticProgressState {
    Scheduled { next_run_at_ns: u64 },
    Running { lease_until_ns: u64 },
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomaticProgressView {
    pub phase: AutomaticProgressPhase,
    pub state: AutomaticProgressState,
}

fn automatic_progress(job: Option<crate::storage::SettlementJob>) -> Option<AutomaticProgressView> {
    let job = job?;
    let phase = match job.phase {
        crate::storage::SettlementJobPhase::Confirmation => AutomaticProgressPhase::Confirmation,
        crate::storage::SettlementJobPhase::Settlement => AutomaticProgressPhase::Settlement,
    };
    let state = match job.status {
        crate::storage::SettlementJobStatus::Scheduled => AutomaticProgressState::Scheduled {
            next_run_at_ns: job.next_run_at_ns?,
        },
        crate::storage::SettlementJobStatus::Leased => AutomaticProgressState::Running {
            lease_until_ns: job.lease_until_ns?,
        },
        crate::storage::SettlementJobStatus::Stopped
        | crate::storage::SettlementJobStatus::AwaitingConfirmation => return None,
    };
    Some(AutomaticProgressView { phase, state })
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
pub struct ConfirmEvmArgs {
    pub settlement_id: Vec<u8>,
    pub transaction_hash: Vec<u8>,
    pub receipt_block_number: u64,
    pub observed_finalized_block_number: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NotifyWithdrawalReceipt {
    Ingested {
        withdrawal_id: Vec<u8>,
        finalized_head_block_number: u64,
    },
    Duplicate {
        withdrawal_id: Vec<u8>,
    },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NotifyWithdrawalError {
    AnonymousCaller,
    InvalidTransactionHash,
    RpcUnavailable,
    RpcInconsistent,
    InvalidBaseResponse,
    TransactionNotFound,
    TransactionNotConfirmed,
    TransactionReverted,
    OwnerMismatch,
    LedgerFeeExceedsServiceFee {
        ledger_fee: Nat,
        charged_service_fee: Nat,
    },
    WithdrawalConflict,
    BaseStateMismatch,
    BridgeSignerMismatch,
    StorageFailure,
    Busy,
    RateLimited,
    InsufficientCycles,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum BaseConfirmationView {
    Submitted {
        transaction_hash: Vec<u8>,
    },
    Confirmed {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        finalized_head_block_number: u64,
    },
    Reverted {
        transaction_hash: Vec<u8>,
        receipt_block_number: u64,
        finalized_head_block_number: u64,
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
    let config = STORE
        .with(|store| store.borrow().config())
        .map_err(|_| NotifyWithdrawalError::StorageFailure)?
        .ok_or(NotifyWithdrawalError::StorageFailure)?;
    let outcome = match evm_rpc::notified_withdrawal_outcome(&config, transaction_hash).await {
        Ok(outcome) => outcome,
        Err(evm_rpc::ObservationError::Inconsistent) => {
            let decision =
                evm_rpc::quorum_loss_decision("notify_withdrawal", Some(transaction_hash));
            STORE
                .with(|store| {
                    store.borrow_mut().append_audit_events_atomically(
                        ic_cdk::api::canister_self(),
                        vec![rpc_decision_event_kind(&decision)],
                    )
                })
                .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
            return Err(NotifyWithdrawalError::RpcInconsistent);
        }
        Err(error) => return Err(map_withdrawal_observation_error(error)),
    };
    let (observed, snapshot, rpc_audit, stable_observation, finalized_head_block_number) =
        match outcome {
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
                rpc_audit,
                stable_observation,
                finalized_head_block_number,
                ..
            } => (
                withdrawal,
                snapshot,
                rpc_audit,
                stable_observation,
                finalized_head_block_number,
            ),
        };
    let owner = Principal::try_from_slice(&observed.owner)
        .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
    let administrator = crate::admin::can_advance_settlement(caller)
        .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
    if !notification_caller_allowed(caller, owner, administrator) {
        return Err(NotifyWithdrawalError::OwnerMismatch);
    }
    let expected_signer = cached_signer_address(&config)
        .await
        .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
    if snapshot.bridge_signer != expected_signer {
        return Err(NotifyWithdrawalError::BridgeSignerMismatch);
    }
    if let Some(receipt) = existing_notified_withdrawal(&observed)? {
        return Ok(receipt);
    }
    let ledger_fee = ledger::KINIC_LEDGER_FEE;
    let receipt = ingest_notified_withdrawal(
        observed,
        transaction_hash,
        ledger_fee,
        finalized_head_block_number,
        *stable_observation,
        vec![
            rpc_audit_event_kind(&rpc_audit),
            rpc_decision_event_kind(&evm_rpc::quorum_continued_decision(
                "notify_withdrawal",
                Some(transaction_hash),
                true,
            )),
        ],
    )?;
    Ok(receipt)
}

fn notification_caller_allowed(caller: Principal, owner: Principal, administrator: bool) -> bool {
    caller == owner || administrator
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
        evm_rpc::ObservationError::ChainIdMismatch => NotifyWithdrawalError::BaseStateMismatch,
        evm_rpc::ObservationError::InvalidResponse | evm_rpc::ObservationError::Overflow => {
            NotifyWithdrawalError::InvalidBaseResponse
        }
    }
}

fn notified_withdrawal_payload_hash(observed: &evm_rpc::ObservedWithdrawal) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(observed.id);
    digest.update(&observed.owner);
    digest.update(observed.subaccount);
    digest.update(observed.amount.to_be_bytes());
    digest.update(observed.max_service_fee.to_be_bytes());
    digest.update(observed.charged_service_fee.to_be_bytes());
    digest.update(observed.amount_out.to_be_bytes());
    digest.finalize().into()
}

fn existing_notified_withdrawal(
    observed: &evm_rpc::ObservedWithdrawal,
) -> Result<Option<NotifyWithdrawalReceipt>, NotifyWithdrawalError> {
    let payload_hash = notified_withdrawal_payload_hash(observed);
    STORE.with(|store| {
        let existing = store
            .borrow()
            .withdrawal(observed.id)
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
        match existing {
            Some(existing) if existing.payload_hash == payload_hash => {
                Ok(Some(NotifyWithdrawalReceipt::Duplicate {
                    withdrawal_id: observed.id.to_vec(),
                }))
            }
            Some(_) => Err(NotifyWithdrawalError::WithdrawalConflict),
            None => Ok(None),
        }
    })
}

fn notification_commit_error(error: crate::storage::StorageError) -> NotifyWithdrawalError {
    match error {
        crate::storage::StorageError::Core(
            bridge_core::CoreError::ConflictingReplay | bridge_core::CoreError::PayloadConflict,
        ) => NotifyWithdrawalError::WithdrawalConflict,
        _ => NotifyWithdrawalError::StorageFailure,
    }
}

pub(crate) fn existing_notified_withdrawal_by_hash(
    caller: Principal,
    transaction_hash: [u8; 32],
) -> Result<Option<NotifyWithdrawalReceipt>, NotifyWithdrawalError> {
    let found = STORE.with(|store| {
        let store = store.borrow();
        let Some(withdrawal_id) = store
            .notified_withdrawal_id(transaction_hash)
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?
        else {
            return Ok(None);
        };
        let withdrawal = store
            .withdrawal(withdrawal_id)
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?
            .ok_or(NotifyWithdrawalError::StorageFailure)?;
        let admin = store
            .admin_state()
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
        Ok(Some((withdrawal_id, withdrawal.owner, admin)))
    })?;
    let Some((withdrawal_id, owner, admin)) = found else {
        return Ok(None);
    };
    let administrator = caller == admin.governance_principal || caller == admin.pause_principal;
    if !notification_caller_allowed(
        caller,
        Principal::try_from_slice(&owner)
            .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?,
        administrator,
    ) {
        return Err(NotifyWithdrawalError::OwnerMismatch);
    }
    Ok(Some(NotifyWithdrawalReceipt::Duplicate {
        withdrawal_id: withdrawal_id.to_vec(),
    }))
}

#[allow(clippy::too_many_arguments)]
fn ingest_notified_withdrawal(
    observed: evm_rpc::ObservedWithdrawal,
    transaction_hash: [u8; 32],
    ledger_fee: Amount,
    finalized_head_block_number: u64,
    stable_observation: FinalizedObservationRecord,
    rpc_audit: Vec<crate::storage::AuditEventKind>,
) -> Result<NotifyWithdrawalReceipt, NotifyWithdrawalError> {
    let payload_hash = notified_withdrawal_payload_hash(&observed);
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        if let Some(existing) = store
            .withdrawal(observed.id)
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?
        {
            if existing.payload_hash == payload_hash {
                return Ok(NotifyWithdrawalReceipt::Duplicate {
                    withdrawal_id: observed.id.to_vec(),
                });
            }
            return Err(NotifyWithdrawalError::WithdrawalConflict);
        }
        if let Some(guard) = store
            .admin_state()
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?
            .withdrawal_fee_guard
        {
            return Err(NotifyWithdrawalError::LedgerFeeExceedsServiceFee {
                ledger_fee: Nat::from(guard.ledger_fee),
                charged_service_fee: Nat::from(guard.charged_service_fee),
            });
        }
        if ledger_fee.get() > observed.charged_service_fee {
            let mut withdrawal = WithdrawalRecord::observed(
                WithdrawalId::new(observed.id),
                observed.requester,
                observed.owner.clone(),
                observed.subaccount,
                payload_hash,
                Amount::new(observed.amount),
                Amount::new(observed.max_service_fee),
                Amount::new(observed.charged_service_fee),
                Amount::new(observed.amount_out),
                stable_observation.observed_at_ns,
            )
            .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
            withdrawal.last_settlement_stop_reason = Some("LedgerFeeExceedsServiceFee".to_owned());
            let mut progress = store
                .external_progress()
                .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
            if finalized_head_block_number != stable_observation.block_number {
                return Err(NotifyWithdrawalError::BaseStateMismatch);
            }
            progress
                .observe_finalized(stable_observation)
                .map_err(|_| NotifyWithdrawalError::BaseStateMismatch)?;
            let mut admin = store
                .admin_state()
                .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
            let now_ns = ic_cdk::api::time();
            admin.withdrawal_fee_guard = Some(crate::admin::WithdrawalFeeGuard {
                ledger_fee: ledger_fee.get(),
                charged_service_fee: observed.charged_service_fee,
                tripped_at_ns: now_ns,
            });
            let mut audit = rpc_audit;
            audit.push(crate::storage::AuditEventKind::WithdrawalFeeGuardTripped {
                ledger_fee: ledger_fee.get(),
                charged_service_fee: observed.charged_service_fee,
            });
            store
                .commit_withdrawal_fee_guard_trip_bundle(
                    &withdrawal,
                    &progress,
                    &admin,
                    ic_cdk::api::canister_self(),
                    now_ns,
                    audit,
                    transaction_hash,
                )
                .map_err(notification_commit_error)?;
            return Err(NotifyWithdrawalError::LedgerFeeExceedsServiceFee {
                ledger_fee: Nat::from(ledger_fee.get()),
                charged_service_fee: Nat::from(observed.charged_service_fee),
            });
        }
        let mut withdrawal = WithdrawalRecord::observed(
            WithdrawalId::new(observed.id),
            observed.requester,
            observed.owner.clone(),
            observed.subaccount,
            payload_hash,
            Amount::new(observed.amount),
            Amount::new(observed.max_service_fee),
            Amount::new(observed.charged_service_fee),
            Amount::new(observed.amount_out),
            stable_observation.observed_at_ns,
        )
        .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
        let mut progress = store
            .external_progress()
            .map_err(|_| NotifyWithdrawalError::StorageFailure)?;
        if finalized_head_block_number != stable_observation.block_number {
            return Err(NotifyWithdrawalError::BaseStateMismatch);
        }
        progress
            .observe_finalized(stable_observation)
            .map_err(|_| NotifyWithdrawalError::BaseStateMismatch)?;

        let transfer = LedgerTransferIdentity {
            operation: LedgerOperation::ReleaseWithdrawal,
            created_at_time_ns: ic_cdk::api::time(),
            memo: payload_hash,
            amount: Amount::new(observed.amount_out),
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
                    amount_out: Amount::new(observed.amount_out),
                    service_fee: Amount::new(observed.charged_service_fee),
                    ledger_fee,
                },
            })
            .map_err(|_| NotifyWithdrawalError::InvalidBaseResponse)?;
        store
            .commit_new_withdrawal_release_bundle_with_rpc_audit(
                &withdrawal,
                &progress,
                ic_cdk::api::canister_self(),
                ic_cdk::api::time(),
                rpc_audit,
                transaction_hash,
            )
            .map_err(notification_commit_error)?;
        Ok(NotifyWithdrawalReceipt::Ingested {
            withdrawal_id: observed.id.to_vec(),
            finalized_head_block_number,
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
    if gross_amount <= ledger::KINIC_LEDGER_FEE.get() {
        return Err(DepositError::InvalidRequest(
            "gross_amount must exceed the fixed ledger fee".into(),
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
    let ledger_fee = ledger::KINIC_LEDGER_FEE;
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
    let record = DepositRecord::accept(DepositRequest {
        id: DepositId::new(deposit_id),
        payload_hash,
        gross_amount: Amount::new(gross_amount),
        user_max_service_fee: Amount::new(max_service_fee),
        transfer: transfer.clone(),
    })
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
        let outcome = store
            .admit_deposit(
                caller,
                &intent,
                &record,
                None,
                Some(crate::storage::DepositQuotaAdmission {
                    now_ns: now,
                    window_seconds: config.deposit_rate_limit_window_seconds,
                    global_limit: config.deposit_rate_limit_global,
                    per_principal_limit: config.deposit_rate_limit_per_principal,
                }),
            )
            .map_err(|error| match error {
                crate::storage::StorageError::SequenceMismatch { expected } => {
                    DepositError::SequenceMismatch { expected }
                }
                crate::storage::StorageError::DepositsPaused => DepositError::DepositsPaused,
                crate::storage::StorageError::DepositRateLimited {
                    retry_after_seconds,
                } => DepositError::RateLimited {
                    retry_after_seconds,
                },
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
        Ok(match outcome {
            crate::storage::DepositAdmissionOutcome::Inserted => AdmissionOutcome::Inserted,
            crate::storage::DepositAdmissionOutcome::Existing => AdmissionOutcome::Existing,
        })
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
        .apply(DepositEvent::FundingFailed { code })
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

pub(crate) async fn cached_governance_operator_address(
    config: &BridgeInitArgs,
) -> Result<[u8; 20], DepositError> {
    if let Some(address) = STORE.with(|store| {
        store
            .borrow()
            .governance_operator_address()
            .map_err(|_| DepositError::StorageFailure)
    })? {
        return Ok(address);
    }
    let derived = crate::signer::governance_operator_address(config)
        .await
        .map_err(|_| DepositError::BaseObservationUnavailable)?;
    STORE.with(|store| {
        store
            .borrow_mut()
            .set_governance_operator_address_if_absent(derived)
            .map_err(|_| DepositError::StorageFailure)
    })
}

pub(crate) async fn base_mint_snapshot(
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
    let refresh_owner = STORE.with(|store| {
        store
            .borrow_mut()
            .begin_base_snapshot_refresh(
                now_ns,
                BASE_SNAPSHOT_REFRESH_STALE_LOCK_NS,
                BASE_SNAPSHOT_REFRESH_COOLDOWN_NS,
            )
            .map_err(|_| DepositError::StorageFailure)
    })?;
    let Some(refresh_owner) = refresh_owner else {
        return Err(DepositError::BaseObservationUnavailable);
    };
    let completed = match evm_rpc::bridge_snapshot(config).await {
        Ok(completed) => completed,
        Err(error) => {
            if matches!(error, evm_rpc::ObservationError::Inconsistent) {
                let decision = evm_rpc::quorum_loss_decision("request_deposit", None);
                STORE.with(|store| {
                    store
                        .borrow_mut()
                        .fail_base_snapshot_refresh_with_rpc_audit(
                            refresh_owner,
                            ic_cdk::api::canister_self(),
                            ic_cdk::api::time(),
                            vec![rpc_decision_event_kind(&decision)],
                        )
                        .map_err(|_| DepositError::StorageFailure)
                })?;
            } else {
                STORE.with(|store| {
                    store
                        .borrow_mut()
                        .fail_base_snapshot_refresh(refresh_owner)
                        .map_err(|_| DepositError::StorageFailure)
                })?;
            }
            if matches!(error, evm_rpc::ObservationError::ChainIdMismatch) {
                pause_deposits_for_chain_mismatch()?;
            }
            return Err(DepositError::BaseObservationUnavailable);
        }
    };
    let snapshot = completed.snapshot;
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        match store.finish_base_snapshot_refresh_with_rpc_audit_and_observation(
            refresh_owner,
            ic_cdk::api::time(),
            snapshot.mint,
            snapshot.bridge_signer,
            snapshot.deposits_paused,
            Some(evm_rpc::stable_observation(&completed)),
            ic_cdk::api::canister_self(),
            vec![
                rpc_audit_event_kind(&completed.rpc_audit),
                rpc_decision_event_kind(&evm_rpc::quorum_continued_decision(
                    "request_deposit",
                    None,
                    false,
                )),
            ],
        ) {
            Ok(()) => Ok(()),
            Err(crate::storage::StorageError::Core(
                bridge_core::CoreError::StaleFinalizedObservation
                | bridge_core::CoreError::ConflictingFinalizedObservation,
            )) => {
                store
                    .fail_base_snapshot_refresh(refresh_owner)
                    .map_err(|_| DepositError::StorageFailure)?;
                Err(DepositError::BaseObservationUnavailable)
            }
            Err(_) => Err(DepositError::StorageFailure),
        }
    })?;
    validate_base_deposit_snapshot(
        snapshot.mint,
        snapshot.bridge_signer,
        snapshot.deposits_paused,
        expected_signer,
    )
}

fn pause_deposits_for_chain_mismatch() -> Result<(), DepositError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut admin = store
            .admin_state()
            .map_err(|_| DepositError::StorageFailure)?;
        if admin.deposits_paused {
            return Ok(());
        }
        admin.deposits_paused = true;
        store
            .set_admin_state(&admin)
            .map_err(|_| DepositError::StorageFailure)
    })
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

pub(crate) fn commit_deposit_quote(
    store: &mut crate::storage::StableStore,
    deposit_id: [u8; 32],
    recipient: [u8; 20],
    config: &BridgeInitArgs,
    quote: DepositQuote,
    reserve_admission: DepositReserveAdmission,
) -> Result<(), DepositError> {
    let mut deposit = store
        .deposit(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    let operation_id = store
        .next_evm_operation_id()
        .map_err(|_| DepositError::StorageFailure)?;
    deposit
        .apply(DepositEvent::CommitQuote {
            quote,
            operation_id,
        })
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    let operation = EvmOperationRecord::queued(
        operation_id,
        deposit.payload_hash,
        EvmOperationKind::MintDeposit,
    );
    let intent = evm_calls::mint_deposit(
        config,
        operation_id,
        deposit.payload_hash,
        evm_calls::MintDepositArgs {
            deposit_id,
            recipient,
            gross_amount: deposit.gross_amount.get(),
            max_service_fee: deposit.max_service_fee.get(),
            charged_service_fee: quote.service_fee.get(),
        },
    );
    store
        .commit_deposit_mint_bundle_and_scan(
            &deposit,
            &operation,
            &intent,
            None,
            Some(reserve_admission),
        )
        .map_err(|error| match error {
            crate::storage::StorageError::ReserveUnavailable => DepositError::ReserveUnavailable,
            crate::storage::StorageError::StaleReserveObservation => {
                DepositError::BaseObservationUnavailable
            }
            crate::storage::StorageError::Core(bridge_core::CoreError::MintWindowLimitExceeded) => {
                DepositError::Rejected("MintWindowLimitExceeded".into())
            }
            _ => DepositError::StorageFailure,
        })?;
    Ok(())
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
            state: DepositPhase::from(&record.state),
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
        let job = storage_or_trap(
            "settlement job read",
            store.settlement_job(crate::storage::SettlementJobKind::Deposit, id),
        );
        Some(DepositView {
            deposit_id: id.to_vec(),
            owner_sequence: intent.owner_sequence,
            gross_amount: Nat::from(record.gross_amount.get()),
            quote: record.quote.map(|quote| DepositQuoteView {
                service_fee: Nat::from(quote.service_fee.get()),
                net_amount: Nat::from(quote.net_amount.get()),
            }),
            refund: deposit_refund_view(&record.state),
            max_service_fee: Nat::from(record.max_service_fee.get()),
            base_recipient: intent.base_recipient.to_vec(),
            from_subaccount: (intent.from_subaccount != [0; 32])
                .then(|| intent.from_subaccount.to_vec()),
            state: DepositPhase::from(&record.state),
            last_settlement_stop_reason: record.last_settlement_stop_reason,
            base_confirmation: base_confirmation(&store, operation_id),
            automatic_progress: automatic_progress(job),
        })
    })
}

fn deposit_refund_view(state: &DepositState) -> Option<DepositRefundView> {
    let (reason, attempt, block_index) = match state {
        DepositState::RefundPending { reason, attempt } => (*reason, attempt, None),
        DepositState::RefundReconciliationHold {
            reason, attempt, ..
        } => (*reason, attempt, None),
        DepositState::Refunded {
            reason,
            attempt,
            ledger_block_index,
            ..
        } => (*reason, attempt, Some(Nat::from(*ledger_block_index))),
        _ => return None,
    };
    Some(DepositRefundView {
        reason: reason.into(),
        amount: Nat::from(attempt.identity.amount.get()),
        ledger_fee: Nat::from(attempt.identity.fee.get()),
        attempt_no: attempt.attempt_no,
        block_index,
    })
}

pub fn get_deposit_by_owner_sequence(owner: Principal, owner_sequence: u64) -> Option<DepositView> {
    get_deposit(derive_deposit_id(owner, owner_sequence).to_vec())
}

pub fn get_withdrawal(id: Vec<u8>) -> Option<WithdrawalView> {
    let id: [u8; 32] = id.as_slice().try_into().ok()?;
    STORE.with(|store| {
        let record = storage_or_trap("withdrawal read", store.borrow().withdrawal(id))?;
        let state = WithdrawalPhase::from(&record.state);
        let borrowed = store.borrow();
        let job = storage_or_trap(
            "settlement job read",
            borrowed.settlement_job(crate::storage::SettlementJobKind::Withdrawal, id),
        );
        Some(WithdrawalView {
            withdrawal_id: id.to_vec(),
            amount: Nat::from(record.amount.get()),
            max_service_fee: Nat::from(record.max_service_fee.get()),
            charged_service_fee: Nat::from(record.charged_service_fee.get()),
            amount_out: Nat::from(record.amount_out.get()),
            ledger_fee: Nat::from(match &record.state {
                WithdrawalState::Observed => 0,
                WithdrawalState::ReleasePending { settlement, .. }
                | WithdrawalState::Paid { settlement, .. }
                | WithdrawalState::ReconciliationHold { settlement, .. } => {
                    settlement.ledger_fee.get()
                }
            }),
            state,
            last_settlement_stop_reason: record.last_settlement_stop_reason,
            automatic_progress: automatic_progress(job),
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
            finalized_head_block_number,
        } => Some(BaseConfirmationView::Confirmed {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number,
            finalized_head_block_number,
        }),
        EvmOperationState::Reverted {
            transaction_hash,
            receipt_block_number,
            finalized_head_block_number,
        }
        | EvmOperationState::RecoveryPending {
            transaction_hash,
            receipt_block_number,
            finalized_head_block_number,
            ..
        }
        | EvmOperationState::Recovered {
            transaction_hash,
            receipt_block_number,
            finalized_head_block_number,
            ..
        } => Some(BaseConfirmationView::Reverted {
            transaction_hash: transaction_hash.to_vec(),
            receipt_block_number,
            finalized_head_block_number,
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
            gross_amount: Nat::from(30_000u64),
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
    }

    #[test]
    fn withdrawal_notification_allows_only_the_ic_owner_or_an_administrator() {
        let owner = Principal::self_authenticating([3; 32]);
        let third_party = Principal::self_authenticating([4; 32]);
        assert!(notification_caller_allowed(owner, owner, false));
        assert!(notification_caller_allowed(third_party, owner, true));
        assert!(!notification_caller_allowed(third_party, owner, false));
    }
}
