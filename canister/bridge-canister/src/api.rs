use crate::{
    config::BridgeInitArgs,
    evm_rpc, ledger,
    phases::{DepositPhase, WithdrawalPhase},
    rpc_audit_event_kind, rpc_decision_event_kind,
    storage::{
        DepositCycleAdmission, DepositFundingAttempt, DepositFundingAttemptState, DepositIntent,
    },
    storage_or_trap, STORE,
};
use bridge_core::{
    Account, Amount, DepositEvent, DepositId, DepositQuote, DepositRecord, DepositRefundReason,
    DepositRequest, DepositState, FinalizedObservationRecord, LedgerFailure, LedgerOperation,
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
pub enum FundingFailure {
    BadFee { expected_fee: Nat },
    BadBurn { minimum: Nat },
    InsufficientFunds { balance: Nat },
    InsufficientAllowance { allowance: Nat },
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
    FundingRejected(FundingFailure),
    FundingUnavailable { retry_after_seconds: u64 },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DepositView {
    pub deposit_id: Vec<u8>,
    pub owner_sequence: u64,
    pub created_at_ns: u64,
    pub gross_amount: Nat,
    pub quote: Option<DepositQuoteView>,
    pub refund: Option<DepositRefundView>,
    pub max_service_fee: Nat,
    pub base_recipient: Vec<u8>,
    pub from_subaccount: Option<Vec<u8>>,
    pub state: DepositPhase,
    pub last_settlement_stop_reason: Option<String>,
    pub mint_authorization: Option<MintAuthorizationView>,
    pub automatic_progress: Option<AutomaticProgressView>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MintAuthorizationView {
    pub deposit_id: Vec<u8>,
    pub recipient: Vec<u8>,
    pub gross_amount: Nat,
    pub max_service_fee: Nat,
    pub charged_service_fee: Nat,
    pub deadline: u64,
    pub authorization_epoch: u64,
    pub domain_name: String,
    pub domain_version: String,
    pub chain_id: u64,
    pub verifying_contract: Vec<u8>,
    pub digest: Vec<u8>,
    pub finalized_block_number: u64,
    pub finalized_block_hash: Vec<u8>,
    pub finalized_block_timestamp: u64,
    pub signature_dispatch_attempt: u32,
    pub signature: Option<Vec<u8>>,
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
    AuthorizationExpired,
}

impl From<DepositRefundReason> for DepositRefundReasonView {
    fn from(value: DepositRefundReason) -> Self {
        match value {
            DepositRefundReason::BasePaused => Self::BasePaused,
            DepositRefundReason::ServiceFeeRejected => Self::ServiceFeeRejected,
            DepositRefundReason::PerDepositLimitExceeded => Self::PerDepositLimitExceeded,
            DepositRefundReason::MintWindowLimitExceeded => Self::MintWindowLimitExceeded,
            DepositRefundReason::AuthorizationExpired => Self::AuthorizationExpired,
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
pub enum AutomaticProgressState {
    Scheduled { next_run_at_ns: u64 },
    Running { lease_until_ns: u64 },
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutomaticProgressView {
    pub state: AutomaticProgressState,
}

fn automatic_progress(job: Option<crate::storage::SettlementJob>) -> Option<AutomaticProgressView> {
    let job = job?;
    let state = match job.status {
        crate::storage::SettlementJobStatus::Scheduled => AutomaticProgressState::Scheduled {
            next_run_at_ns: job.next_run_at_ns?,
        },
        crate::storage::SettlementJobStatus::Leased => AutomaticProgressState::Running {
            lease_until_ns: job.lease_until_ns?,
        },
        crate::storage::SettlementJobStatus::Stopped => return None,
    };
    Some(AutomaticProgressView { state })
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
pub struct NotifyDepositMintArgs {
    pub deposit_id: Vec<u8>,
    pub transaction_hash: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NotifyDepositMintReceipt {
    Minted {
        deposit_id: Vec<u8>,
        transaction_hash: Vec<u8>,
        finalized_head_block_number: u64,
    },
    Duplicate {
        deposit_id: Vec<u8>,
        transaction_hash: Vec<u8>,
    },
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum NotifyDepositMintError {
    AnonymousCaller,
    InvalidDepositId,
    InvalidTransactionHash,
    NotFound,
    OwnerMismatch,
    InvalidState,
    TransactionNotConfirmed,
    TransactionReverted,
    RpcUnavailable,
    RpcInconsistent,
    InvalidBaseResponse,
    StorageFailure,
    Busy,
    InsufficientCycles,
}

pub async fn notify_deposit_mint(
    caller: Principal,
    args: NotifyDepositMintArgs,
) -> Result<NotifyDepositMintReceipt, NotifyDepositMintError> {
    if caller == Principal::anonymous() {
        return Err(NotifyDepositMintError::AnonymousCaller);
    }
    let deposit_id: [u8; 32] = args
        .deposit_id
        .as_slice()
        .try_into()
        .map_err(|_| NotifyDepositMintError::InvalidDepositId)?;
    let transaction_hash: [u8; 32] = args
        .transaction_hash
        .as_slice()
        .try_into()
        .map_err(|_| NotifyDepositMintError::InvalidTransactionHash)?;
    let (authorization, duplicate) = STORE.with(|store| {
        let store = store.borrow();
        let intent = store
            .deposit_intent(deposit_id)
            .map_err(|_| NotifyDepositMintError::StorageFailure)?
            .ok_or(NotifyDepositMintError::NotFound)?;
        if intent.caller != caller.as_slice() {
            return Err(NotifyDepositMintError::OwnerMismatch);
        }
        let deposit = store
            .deposit(deposit_id)
            .map_err(|_| NotifyDepositMintError::StorageFailure)?
            .ok_or(NotifyDepositMintError::NotFound)?;
        if let bridge_core::DepositState::Minted { .. } = deposit.state {
            let duplicate = deposit
                .mint_finalization_evidence
                .as_ref()
                .is_some_and(|evidence| evidence.transaction_hash == transaction_hash);
            return Ok((deposit.mint_authorization, duplicate));
        }
        if !matches!(
            deposit.state,
            bridge_core::DepositState::AuthorizationAvailable { .. }
                | bridge_core::DepositState::ExpiryReconciliation { .. }
        ) {
            return Err(NotifyDepositMintError::InvalidState);
        }
        Ok((deposit.mint_authorization, false))
    })?;
    if duplicate {
        return Ok(NotifyDepositMintReceipt::Duplicate {
            deposit_id: deposit_id.to_vec(),
            transaction_hash: transaction_hash.to_vec(),
        });
    }
    let authorization = authorization.ok_or(NotifyDepositMintError::InvalidState)?;
    let config = STORE
        .with(|store| store.borrow().config())
        .map_err(|_| NotifyDepositMintError::StorageFailure)?
        .ok_or(NotifyDepositMintError::StorageFailure)?;
    let finalized = evm_rpc::finalized_observation(&config)
        .await
        .map_err(map_deposit_mint_observation_error)?;
    let evidence = evm_rpc::notified_mint_evidence(
        &config,
        &authorization,
        finalized.block_number,
        transaction_hash,
    )
    .await
    .map_err(map_deposit_mint_observation_error)?;
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut current = store
            .deposit(deposit_id)
            .map_err(|_| NotifyDepositMintError::StorageFailure)?
            .ok_or(NotifyDepositMintError::NotFound)?;
        if current.mint_authorization.as_ref() != Some(&authorization) {
            return Err(NotifyDepositMintError::InvalidState);
        }
        current
            .apply(bridge_core::DepositEvent::MintReconciled {
                evidence: Box::new(evidence),
            })
            .map_err(|_| NotifyDepositMintError::InvalidState)?;
        store
            .put_deposit(&current)
            .map_err(|_| NotifyDepositMintError::StorageFailure)
    })?;
    Ok(NotifyDepositMintReceipt::Minted {
        deposit_id: deposit_id.to_vec(),
        transaction_hash: transaction_hash.to_vec(),
        finalized_head_block_number: finalized.block_number,
    })
}

fn map_deposit_mint_observation_error(error: evm_rpc::ObservationError) -> NotifyDepositMintError {
    match error {
        evm_rpc::ObservationError::Rpc => NotifyDepositMintError::RpcUnavailable,
        evm_rpc::ObservationError::Inconsistent => NotifyDepositMintError::RpcInconsistent,
        evm_rpc::ObservationError::TransactionPending => {
            NotifyDepositMintError::TransactionNotConfirmed
        }
        evm_rpc::ObservationError::TransactionReverted => {
            NotifyDepositMintError::TransactionReverted
        }
        evm_rpc::ObservationError::BaseStateMismatch
        | evm_rpc::ObservationError::ChainIdMismatch
        | evm_rpc::ObservationError::InvalidResponse
        | evm_rpc::ObservationError::Overflow => NotifyDepositMintError::InvalidBaseResponse,
    }
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
        evm_rpc::ObservationError::TransactionPending => {
            NotifyWithdrawalError::TransactionNotConfirmed
        }
        evm_rpc::ObservationError::TransactionReverted => {
            NotifyWithdrawalError::TransactionReverted
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

fn cached_deposit_preflight(
    snapshot: bridge_core::BaseMintSnapshot,
    reserved_mint_amount: u128,
    gross_amount: Amount,
    user_max_service_fee: Amount,
) -> Result<(), bridge_core::CoreError> {
    let net_amount = snapshot.quote(gross_amount, user_max_service_fee)?;
    let decision = bridge_core::deposit_admission_decision(
        gross_amount.get(),
        snapshot.service_fee.get(),
        snapshot.max_service_fee.get(),
        snapshot.per_deposit_limit.get(),
        snapshot.effective_minted_in_window().get(),
        reserved_mint_amount,
        snapshot.mint_window_limit.get(),
    )
    .ok_or(bridge_core::CoreError::MintWindowLimitExceeded)?;
    if decision.net_amount != net_amount.get() {
        return Err(bridge_core::CoreError::MintWindowLimitExceeded);
    }
    Ok(())
}

fn preflight_error(error: bridge_core::CoreError) -> DepositError {
    match error {
        bridge_core::CoreError::ServiceFeeAboveMaximum
        | bridge_core::CoreError::ServiceFeeAboveUserMaximum
        | bridge_core::CoreError::InvalidAmount
        | bridge_core::CoreError::ArithmeticOverflow
        | bridge_core::CoreError::ArithmeticUnderflow => {
            DepositError::Rejected("ServiceFeeRejected".into())
        }
        bridge_core::CoreError::PerDepositLimitExceeded => {
            DepositError::Rejected("PerDepositLimitExceeded".into())
        }
        bridge_core::CoreError::MintWindowLimitExceeded => {
            DepositError::Rejected("MintWindowLimitExceeded".into())
        }
        other => DepositError::Rejected(format!("{other:?}")),
    }
}

pub fn next_deposit_sequence(owner: Principal) -> u64 {
    STORE.with(|store| {
        store
            .borrow()
            .next_deposit_sequence(owner)
            .unwrap_or_else(|error| ic_cdk::trap(format!("deposit sequence read failed: {error}")))
    })
}

fn funding_failure(code: LedgerFailure) -> Result<FundingFailure, DepositError> {
    match code {
        LedgerFailure::BadFee { expected_fee } => Ok(FundingFailure::BadFee {
            expected_fee: Nat::from(expected_fee.get()),
        }),
        LedgerFailure::BadBurn { minimum } => Ok(FundingFailure::BadBurn {
            minimum: Nat::from(minimum.get()),
        }),
        LedgerFailure::InsufficientFunds { balance } => Ok(FundingFailure::InsufficientFunds {
            balance: Nat::from(balance.get()),
        }),
        LedgerFailure::InsufficientAllowance { allowance } => {
            Ok(FundingFailure::InsufficientAllowance {
                allowance: Nat::from(allowance.get()),
            })
        }
        _ => Err(DepositError::StorageFailure),
    }
}

fn deposit_storage_error(error: crate::storage::StorageError) -> DepositError {
    match error {
        crate::storage::StorageError::SequenceMismatch { expected } => {
            DepositError::SequenceMismatch { expected }
        }
        crate::storage::StorageError::DepositsPaused => DepositError::DepositsPaused,
        crate::storage::StorageError::ReserveUnavailable => DepositError::ReserveUnavailable,
        crate::storage::StorageError::DepositRateLimited {
            retry_after_seconds,
        } => DepositError::RateLimited {
            retry_after_seconds,
        },
        crate::storage::StorageError::Core(bridge_core::CoreError::ConflictingReplay) => {
            DepositError::DepositConflict
        }
        _ => DepositError::StorageFailure,
    }
}

fn funding_quota(config: &BridgeInitArgs, now_ns: u64) -> crate::storage::DepositQuotaAdmission {
    crate::storage::DepositQuotaAdmission {
        now_ns,
        window_seconds: config.deposit_rate_limit_window_seconds,
        global_limit: config.deposit_rate_limit_global,
        per_principal_limit: config.deposit_rate_limit_per_principal,
    }
}

pub(crate) fn promote_funding_success(
    attempt: &DepositFundingAttempt,
    block_index: u128,
    config: &BridgeInitArgs,
) -> Result<DepositReceipt, DepositError> {
    let caller = Principal::try_from_slice(&attempt.intent.caller)
        .map_err(|_| DepositError::StorageFailure)?;
    let mut record = DepositRecord::accept(DepositRequest {
        id: DepositId::new(attempt.intent.deposit_id),
        payload_hash: attempt.intent.payload_hash,
        gross_amount: Amount::new(attempt.gross_amount),
        user_max_service_fee: Amount::new(attempt.max_service_fee),
        transfer: attempt.transfer.clone(),
    })
    .map_err(|error| DepositError::Rejected(format!("{error:?}")))?;
    let result = record
        .apply(DepositEvent::FundingSucceeded {
            ledger_block_index: block_index,
        })
        .map_err(|_| DepositError::StorageFailure)?;
    if result.deposit_effects != Some(bridge_core::DepositAccountingEffects::ZERO) {
        return Err(DepositError::StorageFailure);
    }
    STORE.with(|store| {
        store
            .borrow_mut()
            .admit_deposit(
                caller,
                &attempt.intent,
                &record,
                Some(funding_quota(config, ic_cdk::api::time())),
                Some((attempt, None)),
            )
            .map_err(deposit_storage_error)
    })?;
    existing_receipt(attempt.intent.deposit_id, attempt.intent.payload_hash)?
        .ok_or(DepositError::StorageFailure)
}

pub(crate) fn promote_funding_ambiguous(
    attempt: &DepositFundingAttempt,
    config: &BridgeInitArgs,
) -> Result<DepositReceipt, DepositError> {
    let caller = Principal::try_from_slice(&attempt.intent.caller)
        .map_err(|_| DepositError::StorageFailure)?;
    let hold_id = STORE.with(|store| {
        store
            .borrow()
            .next_hold_id()
            .map_err(|_| DepositError::StorageFailure)
    })?;
    let mut record = DepositRecord::accept(DepositRequest {
        id: DepositId::new(attempt.intent.deposit_id),
        payload_hash: attempt.intent.payload_hash,
        gross_amount: Amount::new(attempt.gross_amount),
        user_max_service_fee: Amount::new(attempt.max_service_fee),
        transfer: attempt.transfer.clone(),
    })
    .map_err(|error| DepositError::Rejected(format!("{error:?}")))?;
    let result = record
        .apply(DepositEvent::FundingAmbiguous { hold_id })
        .map_err(|_| DepositError::StorageFailure)?;
    if result.deposit_effects != Some(bridge_core::DepositAccountingEffects::ZERO) {
        return Err(DepositError::StorageFailure);
    }
    let hold = bridge_core::ReconciliationHoldRecord::open(
        hold_id,
        bridge_core::RequestReference::DepositFunding(record.id),
        record.transfer.clone(),
    );
    STORE.with(|store| {
        store
            .borrow_mut()
            .admit_deposit(
                caller,
                &attempt.intent,
                &record,
                Some(funding_quota(config, ic_cdk::api::time())),
                Some((attempt, Some(&hold))),
            )
            .map_err(deposit_storage_error)
    })?;
    existing_receipt(attempt.intent.deposit_id, attempt.intent.payload_hash)?
        .ok_or(DepositError::StorageFailure)
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
    let (mint_snapshot, _snapshot_generation) = base_mint_snapshot(&config, now).await?;
    mint_snapshot
        .quote(Amount::new(gross_amount), Amount::new(max_service_fee))
        .map_err(preflight_error)?;
    let cached_preflight = STORE.with(|store| {
        let store = store.borrow();
        let minimum_finalized_block = store
            .external_progress()
            .map_err(|_| DepositError::StorageFailure)?
            .last_finalized_base_block;
        let Some(cached) = store
            .cached_base_mint_snapshot(now, BASE_SNAPSHOT_TTL_NS, minimum_finalized_block)
            .map_err(|_| DepositError::StorageFailure)?
        else {
            return Ok(None);
        };
        if cached.deposits_paused {
            return Ok(Some(Err(DepositError::DepositsPaused)));
        }
        let reserved = store
            .counters()
            .map_err(|_| DepositError::StorageFailure)?
            .reserved_deposit_mint_amount;
        Ok(Some(
            cached_deposit_preflight(
                cached.snapshot,
                reserved,
                Amount::new(gross_amount),
                Amount::new(max_service_fee),
            )
            .map_err(preflight_error),
        ))
    })?;
    if let Some(result) = cached_preflight {
        result?;
    }
    let existing_attempt = STORE.with(|store| {
        store
            .borrow()
            .deposit_funding_attempt(deposit_id)
            .map_err(|_| DepositError::StorageFailure)
    })?;
    let ledger_fee = ledger::KINIC_LEDGER_FEE;
    let memo = hash(&[b"KINIC-DEPOSIT", &deposit_id]);
    let canister = ic_cdk::api::canister_self();
    let fresh_transfer = LedgerTransferIdentity {
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
    if existing_attempt
        .as_ref()
        .is_some_and(|attempt| attempt.intent != intent)
    {
        return Err(DepositError::DepositConflict);
    }
    let transfer = existing_attempt
        .as_ref()
        .map_or(fresh_transfer, |attempt| attempt.transfer.clone());
    let quota = funding_quota(&config, now);
    let proposed = DepositFundingAttempt {
        intent: intent.clone(),
        gross_amount,
        max_service_fee,
        transfer: transfer.clone(),
        state: DepositFundingAttemptState::Dispatched {
            dispatched_at_ns: now,
        },
        created_at_ns: now,
        updated_at_ns: now,
        last_failure: None,
    };
    let outcome = if existing_attempt.is_some() {
        crate::storage::DepositAdmissionOutcome::Existing
    } else {
        STORE.with(|store| {
            store
                .borrow_mut()
                .prepare_deposit_funding_attempt(
                    caller,
                    &proposed,
                    quota,
                    DepositCycleAdmission {
                        cycles_balance: ic_cdk::api::canister_liquid_cycle_balance(),
                        reserve_policy: config.reserve_policy(),
                    },
                )
                .map_err(deposit_storage_error)
        })?
    };
    let attempt = if matches!(outcome, crate::storage::DepositAdmissionOutcome::Existing) {
        let previous = STORE.with(|store| {
            store
                .borrow()
                .deposit_funding_attempt(deposit_id)
                .map_err(|_| DepositError::StorageFailure)?
                .ok_or(DepositError::StorageFailure)
        })?;
        if previous.intent != intent || previous.transfer != transfer {
            return Err(DepositError::DepositConflict);
        }
        match previous.state {
            DepositFundingAttemptState::Retryable { retry_after_ns } if now >= retry_after_ns => {
                let mut next = previous.clone();
                next.state = DepositFundingAttemptState::Dispatched {
                    dispatched_at_ns: now,
                };
                next.updated_at_ns = now;
                STORE.with(|store| {
                    store
                        .borrow_mut()
                        .update_deposit_funding_attempt(&previous, &next)
                        .map_err(|_| DepositError::StorageFailure)
                })?;
                next
            }
            DepositFundingAttemptState::Prepared => {
                let mut next = previous.clone();
                next.state = DepositFundingAttemptState::Dispatched {
                    dispatched_at_ns: now,
                };
                next.updated_at_ns = now;
                STORE.with(|store| {
                    store
                        .borrow_mut()
                        .update_deposit_funding_attempt(&previous, &next)
                        .map_err(|_| DepositError::StorageFailure)
                })?;
                next
            }
            DepositFundingAttemptState::Retryable { retry_after_ns } => {
                return Err(DepositError::FundingUnavailable {
                    retry_after_seconds: retry_after_ns
                        .saturating_sub(now)
                        .saturating_add(999_999_999)
                        / 1_000_000_000,
                });
            }
            DepositFundingAttemptState::Dispatched { .. }
            | DepositFundingAttemptState::Reconciling { .. } => {
                return Err(DepositError::FundingUnavailable {
                    retry_after_seconds: 30,
                });
            }
        }
    } else {
        proposed
    };

    let ledger_outcome = ledger::pull(config.ledger_canister_id, &attempt.transfer).await;
    let outcome_kind = match &ledger_outcome {
        bridge_core::LedgerCallOutcome::Succeeded { .. } => 0,
        bridge_core::LedgerCallOutcome::Duplicate { .. } => 1,
        bridge_core::LedgerCallOutcome::Ambiguous => 2,
        bridge_core::LedgerCallOutcome::DefinitiveFailure { .. } => 3,
        bridge_core::LedgerCallOutcome::RetryableFailure { .. } => 4,
    };
    match (
        bridge_core::funding_attempt_decision(outcome_kind),
        ledger_outcome,
    ) {
        (
            bridge_core::FundingAttemptDecision::PromoteSuccess,
            bridge_core::LedgerCallOutcome::Succeeded { block_index }
            | bridge_core::LedgerCallOutcome::Duplicate { block_index },
        ) => promote_funding_success(&attempt, block_index, &config),
        (
            bridge_core::FundingAttemptDecision::PromoteAmbiguous,
            bridge_core::LedgerCallOutcome::Ambiguous,
        ) => promote_funding_ambiguous(&attempt, &config),
        (
            bridge_core::FundingAttemptDecision::Release,
            bridge_core::LedgerCallOutcome::DefinitiveFailure { code },
        ) => {
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .remove_deposit_funding_attempt(caller, &attempt)
                    .map_err(|_| DepositError::StorageFailure)
            })?;
            Err(DepositError::FundingRejected(funding_failure(code)?))
        }
        (
            bridge_core::FundingAttemptDecision::Retain,
            bridge_core::LedgerCallOutcome::RetryableFailure { code },
        ) => {
            let mut next = attempt.clone();
            next.state = DepositFundingAttemptState::Retryable {
                retry_after_ns: ic_cdk::api::time().saturating_add(30_000_000_000),
            };
            next.updated_at_ns = ic_cdk::api::time();
            next.last_failure = Some(code);
            STORE.with(|store| {
                store
                    .borrow_mut()
                    .update_deposit_funding_attempt(&attempt, &next)
                    .map_err(|_| DepositError::StorageFailure)
            })?;
            Err(DepositError::FundingUnavailable {
                retry_after_seconds: 30,
            })
        }
        _ => Err(DepositError::StorageFailure),
    }
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
    callback_token: &crate::storage::SettlementCallbackToken,
) -> Result<(), DepositError> {
    let mut deposit = store
        .deposit(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    let result = deposit
        .apply(DepositEvent::FundingFailed { code })
        .map_err(|error| DepositError::Rejected(format!("{error:?}")))?;
    store
        .put_deposit_transition_funding_callback(&deposit, callback_token, result)
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
) -> Result<(bridge_core::BaseMintSnapshot, u64), DepositError> {
    let expected_signer = cached_signer_address(config).await?;
    let minimum_finalized_block = STORE.with(|store| {
        store
            .borrow()
            .external_progress()
            .map(|progress| progress.last_finalized_base_block)
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
        )
        .map(|mint| (mint, snapshot.generation));
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
                pause_deposits_for_safety()?;
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
            snapshot.mint_authorization_epoch,
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
    .map(|mint| (mint, refresh_owner))
}

pub(crate) fn pause_deposits_for_safety() -> Result<(), DepositError> {
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

pub(crate) fn commit_deposit_authorization(
    store: &mut crate::storage::StableStore,
    deposit_id: [u8; 32],
    quote: DepositQuote,
    authorization: bridge_core::MintAuthorizationRecord,
) -> Result<(), DepositError> {
    let intent = store
        .deposit_intent(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    let config = store
        .config()
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    let expected_contract: [u8; 20] = config
        .bridge_contract
        .as_slice()
        .try_into()
        .map_err(|_| DepositError::StorageFailure)?;
    if authorization.authorization.recipient != intent.base_recipient
        || authorization.domain.chain_id != config.base_chain_id
        || authorization.domain.verifying_contract != expected_contract
        || authorization.digest
            != crate::mint_authorization::digest(&authorization.domain, authorization.authorization)
    {
        return Err(DepositError::Rejected(
            "Mint Authorization does not match the canonical Deposit".into(),
        ));
    }
    let mut deposit = store
        .deposit(deposit_id)
        .map_err(|_| DepositError::StorageFailure)?
        .ok_or(DepositError::StorageFailure)?;
    let result = deposit
        .apply(DepositEvent::CommitAuthorization {
            quote,
            authorization: Box::new(authorization),
        })
        .map_err(|e| DepositError::Rejected(format!("{e:?}")))?;
    store
        .put_deposit_transition(&deposit, result)
        .map_err(|_| DepositError::StorageFailure)?;
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
        let job = storage_or_trap(
            "settlement job read",
            store.settlement_job(crate::storage::SettlementJobKind::Deposit, id),
        );
        Some(DepositView {
            deposit_id: id.to_vec(),
            owner_sequence: intent.owner_sequence,
            created_at_ns: deposit_created_at_ns(&record),
            gross_amount: Nat::from(record.gross_amount.get()),
            quote: record.quote.map(|quote| DepositQuoteView {
                service_fee: Nat::from(quote.service_fee.get()),
                net_amount: Nat::from(quote.net_amount.get()),
            }),
            refund: deposit_refund_view(&record),
            max_service_fee: Nat::from(record.max_service_fee.get()),
            base_recipient: intent.base_recipient.to_vec(),
            from_subaccount: (intent.from_subaccount != [0; 32])
                .then(|| intent.from_subaccount.to_vec()),
            state: DepositPhase::from(&record.state),
            last_settlement_stop_reason: record.last_settlement_stop_reason,
            mint_authorization: record
                .mint_authorization
                .as_ref()
                .map(mint_authorization_view),
            automatic_progress: automatic_progress(job),
        })
    })
}

fn mint_authorization_view(record: &bridge_core::MintAuthorizationRecord) -> MintAuthorizationView {
    let authorization = record.authorization;
    MintAuthorizationView {
        deposit_id: authorization.deposit_id.to_vec(),
        recipient: authorization.recipient.to_vec(),
        gross_amount: Nat::from(authorization.gross_amount.get()),
        max_service_fee: Nat::from(authorization.max_service_fee.get()),
        charged_service_fee: Nat::from(authorization.charged_service_fee.get()),
        deadline: authorization.deadline,
        authorization_epoch: authorization.authorization_epoch,
        domain_name: record.domain.name.clone(),
        domain_version: record.domain.version.clone(),
        chain_id: record.domain.chain_id,
        verifying_contract: record.domain.verifying_contract.to_vec(),
        digest: record.digest.to_vec(),
        finalized_block_number: record.origin.finalized_block_number,
        finalized_block_hash: record.origin.finalized_block_hash.to_vec(),
        finalized_block_timestamp: record.origin.finalized_block_timestamp,
        signature_dispatch_attempt: record.signature_dispatch_attempt,
        signature: record.signature.clone(),
    }
}

fn deposit_created_at_ns(record: &DepositRecord) -> u64 {
    record.transfer.created_at_time_ns
}

fn deposit_refund_view(record: &DepositRecord) -> Option<DepositRefundView> {
    let (reason, attempt, block_index) = match &record.state {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn preflight_snapshot() -> bridge_core::BaseMintSnapshot {
        bridge_core::BaseMintSnapshot {
            finalized_head_block_number: 10,
            confirmed_block_timestamp: 20,
            service_fee: Amount::new(5),
            max_service_fee: Amount::new(5),
            per_deposit_limit: Amount::new(100),
            mint_window_limit: Amount::new(200),
            mint_window_started_at: 0,
            mint_window_duration: 100,
            minted_in_window: Amount::new(50),
        }
    }

    #[test]
    fn cached_preflight_rejects_only_deterministic_quote_and_window_failures() {
        let snapshot = preflight_snapshot();
        assert_eq!(
            cached_deposit_preflight(snapshot, 40, Amount::new(100), Amount::new(5)),
            Ok(())
        );
        assert_eq!(
            cached_deposit_preflight(snapshot, 60, Amount::new(100), Amount::new(5)),
            Err(bridge_core::CoreError::MintWindowLimitExceeded)
        );
        assert_eq!(
            cached_deposit_preflight(snapshot, 0, Amount::new(106), Amount::new(5)),
            Err(bridge_core::CoreError::PerDepositLimitExceeded)
        );
        assert_eq!(
            cached_deposit_preflight(snapshot, 0, Amount::new(100), Amount::new(4)),
            Err(bridge_core::CoreError::ServiceFeeAboveUserMaximum)
        );
    }

    #[test]
    fn reserve_rejection_remains_a_typed_deposit_error() {
        assert_eq!(
            deposit_storage_error(crate::storage::StorageError::ReserveUnavailable),
            DepositError::ReserveUnavailable
        );
    }

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
    fn deposit_view_time_uses_the_original_request_time() {
        let from = Account::new(vec![1], [0; 32]).expect("valid source");
        let to = Account::new(vec![2], [0; 32]).expect("valid destination");
        let record = DepositRecord::accept(DepositRequest {
            id: DepositId::new([3; 32]),
            payload_hash: [4; 32],
            gross_amount: Amount::new(100),
            user_max_service_fee: Amount::new(10),
            transfer: LedgerTransferIdentity {
                operation: LedgerOperation::PullDeposit,
                created_at_time_ns: 123_456,
                memo: [5; 32],
                amount: Amount::new(100),
                fee: Amount::new(10),
                from,
                to,
                spender: None,
            },
        })
        .expect("valid deposit");

        assert_eq!(deposit_created_at_ns(&record), 123_456);
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
