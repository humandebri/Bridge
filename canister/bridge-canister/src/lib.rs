//! IC boundary and stable storage adapter for the KINIC–Base Bridge.
//!
//! This crate exposes the Candid boundary and connects the deterministic core to stable storage,
//! ICRC Ledger calls, EVM RPC observation, threshold ECDSA signing, scheduled confirmation, and runtime
//! administration.

use candid::{CandidType, Deserialize};
use ic_sqlite_vfs::DefaultMemoryImpl;
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    ops::{Deref, DerefMut},
};

mod admin;
mod api;
pub mod config;
mod consent;
mod evm_calls;
mod evm_rpc;
mod ledger;
mod phases;
mod recovery;
mod scheduler;
mod signer;
pub mod storage;
mod tasks;

use storage::{AuditEventKind, StableStore, StorageError, SCHEMA_VERSION};

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
    pub pending_ledger_operations: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reserved_deposit_mint_operations: u64,
    pub unresolved_evm_reverts: u64,
    pub active_evm_payloads: u64,
    pub retained_audit_events: u64,
    pub pruned_audit_events: u64,
    pub retained_deposit_index_entries: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BridgeStatus {
    pub base_chain_id_matches_config: bool,
    pub schema_version: u16,
    pub counts: StatusCounts,
    pub last_finalized_base_block: u64,
    pub last_reserve_observation_ns: u64,
    pub last_finalized_observation_ns: u64,
    pub last_finalized_base_block_hash: Vec<u8>,
    pub observed_base_chain_id: Option<u64>,
    pub observed_bridge_signer: Vec<u8>,
    pub observed_bridge_runtime_sha256: Vec<u8>,
    pub reserve: ReserveStatus,
    pub deposits_paused: bool,
    pub last_audit_sequence: Option<u64>,
    pub settlement_scheduler: SettlementSchedulerStatus,
    pub unpaid_withdrawal_count: u64,
    pub unpaid_withdrawal_amount_out: u128,
    pub oldest_unpaid_withdrawal_observed_at_ns: Option<u64>,
    pub withdrawal_stop_reasons: Vec<String>,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshBaseObservationError {
    Busy,
    BaseStateMismatch,
    ObservationUnavailable,
    StorageFailure,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SettlementSchedulerStatus {
    pub health: SettlementSchedulerHealth,
    pub scheduled: u64,
    pub leased: u64,
    pub stopped: u64,
    pub expired: u64,
    pub next_wakeup_at_ns: Option<u64>,
    pub last_dispatcher_run_at_ns: u64,
    pub last_internal_error: Option<String>,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementSchedulerHealth {
    Healthy,
    Degraded,
    Faulted,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReserveStatus {
    pub eth_balance_wei: u128,
    pub cycles_balance: u128,
    pub required_eth_wei: u128,
    pub required_cycles: u128,
    pub eth_surplus_wei: u128,
    pub cycles_surplus: u128,
    pub sufficient: bool,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PublicConfig {
    pub base_chain_id: u64,
    pub bridge_contract: Vec<u8>,
    pub ledger_canister_id: candid::Principal,
    pub index_canister_id: candid::Principal,
    pub schema_version: u16,
    pub expected_bridge_signer: Vec<u8>,
    pub evm_rpc_canister_id: candid::Principal,
    pub rpc_provider_urls_sha256: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ChainKeyChallengeError {
    Busy,
    Unauthorized,
    InvalidReleaseId,
    StorageFailure,
    SigningUnavailable,
}

thread_local! {
    static STORE: RefCell<StoreState> = const { RefCell::new(StoreState(None)) };
    static IN_FLIGHT_ACTIONS: RefCell<BTreeSet<ActionKey>> = const { RefCell::new(BTreeSet::new()) };
}

struct StoreState(Option<StableStore>);

impl Deref for StoreState {
    type Target = StableStore;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .unwrap_or_else(|| ic_cdk::trap("stable state is not initialized"))
    }
}

impl DerefMut for StoreState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
            .as_mut()
            .unwrap_or_else(|| ic_cdk::trap("stable state is not initialized"))
    }
}

fn install_store(store: StableStore) {
    STORE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.0.replace(store).is_some() {
            ic_cdk::trap("stable state initialized twice");
        }
    });
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ActionKey {
    Deposit([u8; 32]),
    Withdrawal([u8; 32]),
    Notification([u8; 32]),
    FeePayout(u64),
    FeePayoutCreation,
    ChainKeyChallenge,
    BaseObservation,
}

fn valid_release_id(release_id: &str) -> bool {
    (8..=64).contains(&release_id.len())
        && release_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

struct InFlightGuard {
    key: ActionKey,
}

impl InFlightGuard {
    fn acquire(key: ActionKey) -> Option<Self> {
        IN_FLIGHT_ACTIONS.with(|actions| {
            if !actions.borrow_mut().insert(key.clone()) {
                return None;
            }
            Some(Self { key })
        })
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        IN_FLIGHT_ACTIONS.with(|actions| {
            actions.borrow_mut().remove(&self.key);
        });
    }
}

#[ic_cdk::init]
fn init(args: config::BridgeInitArgs) {
    args.validate().unwrap_or_else(|error| ic_cdk::trap(error));
    let store =
        StableStore::init_configured(DefaultMemoryImpl::default(), &args).unwrap_or_else(|error| {
            ic_cdk::trap(format!("stable state initialization failed: {error}"))
        });
    install_store(store);
    scheduler::arm();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    let store = StableStore::reopen(DefaultMemoryImpl::default())
        .unwrap_or_else(|error| ic_cdk::trap(format!("stable state reopen failed: {error}")));
    install_store(store);
    ensure_supported_schema();
    scheduler::arm();
}

#[ic_cdk::update]
async fn request_deposit(args: api::DepositArgs) -> Result<api::DepositReceipt, api::DepositError> {
    let caller = ic_cdk::api::msg_caller();
    let id = api::deposit_action_id(caller, &args)?;
    let Some(guard) = InFlightGuard::acquire(ActionKey::Deposit(id)) else {
        return Err(api::DepositError::Busy);
    };
    let existed = STORE.with(|store| {
        store
            .borrow()
            .deposit(id)
            .map(|record| record.is_some())
            .map_err(|_| api::DepositError::StorageFailure)
    })?;
    let mut receipt = api::request_deposit(caller, args).await?;
    if existed {
        return Ok(receipt);
    }
    drop(guard);
    if let Some(settlement) =
        scheduler::run_newly_enqueued(storage::SettlementJobKind::Deposit, id).await
    {
        receipt.state = STORE.with(|store| {
            store
                .borrow()
                .deposit(id)
                .map_err(|_| api::DepositError::StorageFailure)?
                .map(|record| phases::DepositPhase::from(&record.state))
                .ok_or(api::DepositError::StorageFailure)
        })?;
        receipt.settlement = Some(settlement);
    }
    scheduler::arm();
    Ok(receipt)
}

#[ic_cdk::query]
fn get_deposit(id: Vec<u8>) -> Option<api::DepositView> {
    api::get_deposit(id)
}

#[ic_cdk::query]
fn list_deposit_ids(
    args: api::ListDepositIdsArgs,
) -> Result<api::DepositIdPage, api::ListDepositIdsError> {
    api::list_deposit_ids(args)
}

#[ic_cdk::query]
fn get_next_deposit_sequence(owner: candid::Principal) -> u64 {
    api::next_deposit_sequence(owner)
}

#[ic_cdk::query]
fn get_withdrawal(id: Vec<u8>) -> Option<api::WithdrawalView> {
    api::get_withdrawal(id)
}

#[ic_cdk::query]
fn get_withdrawals(
    ids: Vec<Vec<u8>>,
) -> Result<Vec<Option<api::WithdrawalView>>, api::GetWithdrawalsError> {
    api::get_withdrawals(ids)
}

#[ic_cdk::update]
async fn notify_withdrawal(
    args: api::NotifyWithdrawalArgs,
) -> Result<api::NotifyWithdrawalReceipt, api::NotifyWithdrawalError> {
    let caller = ic_cdk::api::msg_caller();
    let transaction_hash = api::notification_action_hash(caller, &args)?;
    let Some(notification_guard) =
        InFlightGuard::acquire(ActionKey::Notification(transaction_hash))
    else {
        return Err(api::NotifyWithdrawalError::Busy);
    };
    let mut receipt = api::notify_withdrawal(caller, args).await?;
    let id = match &receipt {
        api::NotifyWithdrawalReceipt::Ingested { withdrawal_id, .. } => withdrawal_id
            .as_slice()
            .try_into()
            .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?,
        api::NotifyWithdrawalReceipt::Duplicate { .. } => return Ok(receipt),
    };
    drop(notification_guard);
    if let Some(settlement) =
        scheduler::run_newly_enqueued(storage::SettlementJobKind::Withdrawal, id).await
    {
        match &mut receipt {
            api::NotifyWithdrawalReceipt::Ingested {
                settlement: slot, ..
            } => *slot = Some(settlement),
            api::NotifyWithdrawalReceipt::Duplicate { .. } => unreachable!(),
        }
    }
    scheduler::arm();
    Ok(receipt)
}

#[ic_cdk::update]
async fn recover_mint_revert(
    args: recovery::RecoverMintRevertArgs,
) -> Result<recovery::RecoverMintRevertReceipt, recovery::RecoverMintRevertError> {
    let target = recovery::validate_target(&args.deposit_id)?;
    let key = match &target {
        recovery::ValidatedTarget::Deposit(id) => ActionKey::Deposit(*id),
    };
    let Some(guard) = InFlightGuard::acquire(key) else {
        return Err(recovery::RecoverMintRevertError::Busy);
    };
    let mut receipt = recovery::recover(ic_cdk::api::msg_caller(), args).await?;
    if matches!(receipt, recovery::RecoverMintRevertReceipt::Enqueued { .. }) {
        drop(guard);
        let settlement = recovery::run_enqueued(&target).await;
        if let recovery::RecoverMintRevertReceipt::Enqueued {
            state,
            settlement: slot,
            ..
        } = &mut receipt
        {
            *state = recovery::current_state(&target)?;
            *slot = settlement;
        }
        scheduler::arm();
    }
    Ok(receipt)
}

fn can_advance_deposit(
    caller: candid::Principal,
    id: [u8; 32],
) -> Result<bool, tasks::SettlementActionError> {
    if caller == candid::Principal::anonymous() {
        return Err(tasks::SettlementActionError::AnonymousCaller);
    }
    let owned = STORE.with(|store| {
        store
            .borrow()
            .deposit_intent(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .map(|intent| intent.caller == caller.as_slice())
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    if owned {
        return Ok(true);
    }
    admin::can_advance_settlement(caller).map_err(|_| tasks::SettlementActionError::StorageFailure)
}

fn can_advance_withdrawal(
    caller: candid::Principal,
    id: [u8; 32],
) -> Result<bool, tasks::SettlementActionError> {
    if caller == candid::Principal::anonymous() {
        return Err(tasks::SettlementActionError::AnonymousCaller);
    }
    let owned = STORE.with(|store| {
        store
            .borrow()
            .withdrawal(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .map(|record| record.owner == caller.as_slice())
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    if owned {
        return Ok(true);
    }
    admin::can_advance_settlement(caller).map_err(|_| tasks::SettlementActionError::StorageFailure)
}

fn current_operation_id(
    kind: storage::SettlementJobKind,
    id: [u8; 32],
) -> Result<bridge_core::EvmOperationId, tasks::SettlementActionError> {
    STORE.with(|store| {
        let store = store.borrow();
        match kind {
            storage::SettlementJobKind::Deposit => store
                .deposit(id)
                .map_err(|_| tasks::SettlementActionError::StorageFailure)?
                .and_then(|record| match record.state {
                    bridge_core::DepositState::MintPending { operation_id, .. } => {
                        Some(operation_id)
                    }
                    _ => None,
                }),
            storage::SettlementJobKind::Withdrawal => None,
        }
        .ok_or(tasks::SettlementActionError::WrongState)
    })
}

fn submitted_transaction(
    kind: storage::SettlementJobKind,
    id: [u8; 32],
) -> Result<(bridge_core::EvmOperationId, [u8; 32]), tasks::SettlementActionError> {
    let operation_id = current_operation_id(kind, id)?;
    let operation = STORE.with(|store| {
        store
            .borrow()
            .evm_operation(operation_id.get())
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    match operation.state {
        bridge_core::EvmOperationState::Submitted { transaction_hash } => {
            Ok((operation_id, transaction_hash))
        }
        _ => Err(tasks::SettlementActionError::WrongState),
    }
}

async fn confirm_evm(
    kind: storage::SettlementJobKind,
    args: api::ConfirmEvmArgs,
) -> Result<tasks::SettlementActionResult, tasks::SettlementActionError> {
    let id: [u8; 32] = args
        .settlement_id
        .as_slice()
        .try_into()
        .map_err(|_| tasks::SettlementActionError::InvalidId)?;
    let transaction_hash: [u8; 32] = args
        .transaction_hash
        .as_slice()
        .try_into()
        .map_err(|_| tasks::SettlementActionError::TransactionMismatch)?;
    if args.observed_finalized_block_number < args.receipt_block_number {
        return Err(tasks::SettlementActionError::InvalidConfirmationObservation);
    }
    let caller = ic_cdk::api::msg_caller();
    let authorized = match kind {
        storage::SettlementJobKind::Deposit => can_advance_deposit(caller, id)?,
        storage::SettlementJobKind::Withdrawal => can_advance_withdrawal(caller, id)?,
    };
    if !authorized {
        return Err(tasks::SettlementActionError::Unauthorized);
    }
    let (operation_id, stored_hash) = submitted_transaction(kind, id)?;
    if stored_hash != transaction_hash {
        return Err(tasks::SettlementActionError::TransactionMismatch);
    }
    let job = STORE.with(|store| {
        store
            .borrow()
            .settlement_job(kind, id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::WrongState)
    })?;
    if job.phase != storage::SettlementJobPhase::Confirmation
        || job.operation_id != Some(operation_id.get())
        || !matches!(
            job.status,
            storage::SettlementJobStatus::AwaitingConfirmation
                | storage::SettlementJobStatus::Stopped
        )
    {
        return Err(tasks::SettlementActionError::WrongState);
    }
    let Some(guard) = InFlightGuard::acquire(match kind {
        storage::SettlementJobKind::Deposit => ActionKey::Deposit(id),
        storage::SettlementJobKind::Withdrawal => ActionKey::Withdrawal(id),
    }) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    drop(guard);
    let job = claim_manual_job(kind, id, caller)?;
    let result = scheduler::run_claimed_confirmation(
        job,
        args.receipt_block_number,
        args.observed_finalized_block_number,
    )
    .await?;
    scheduler::arm();
    Ok(result)
}

#[ic_cdk::update]
async fn confirm_deposit(
    args: api::ConfirmEvmArgs,
) -> Result<tasks::SettlementActionResult, tasks::SettlementActionError> {
    confirm_evm(storage::SettlementJobKind::Deposit, args).await
}

#[ic_cdk::update]
async fn continue_deposit(
    deposit_id: Vec<u8>,
) -> Result<tasks::SettlementActionResult, tasks::SettlementActionError> {
    let id = deposit_id
        .as_slice()
        .try_into()
        .map_err(|_| tasks::SettlementActionError::InvalidId)?;
    let caller = ic_cdk::api::msg_caller();
    if !can_advance_deposit(caller, id)? {
        return Err(tasks::SettlementActionError::Unauthorized);
    }
    match submitted_transaction(storage::SettlementJobKind::Deposit, id) {
        Ok(_) => return Err(tasks::SettlementActionError::ConfirmationRequired),
        Err(tasks::SettlementActionError::WrongState) => {}
        Err(error) => return Err(error),
    }
    let Some(guard) = InFlightGuard::acquire(ActionKey::Deposit(id)) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    let terminal_state = STORE.with(|store| {
        store
            .borrow()
            .deposit(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .map(|record| {
                let terminal = matches!(
                    record.state,
                    bridge_core::DepositState::Minted { .. }
                        | bridge_core::DepositState::MintReverted { .. }
                        | bridge_core::DepositState::Cancelled { .. }
                );
                terminal.then(|| {
                    phases::SettlementState::Deposit(phases::DepositPhase::from(&record.state))
                })
            })
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    if let Some(state) = terminal_state {
        return Ok(tasks::SettlementActionResult::Complete { state });
    }
    drop(guard);
    let job = claim_manual_job(storage::SettlementJobKind::Deposit, id, caller)?;
    let result = scheduler::run_claimed(job).await?;
    scheduler::arm();
    Ok(result)
}

#[ic_cdk::update]
async fn continue_withdrawal(
    withdrawal_id: Vec<u8>,
) -> Result<tasks::SettlementActionResult, tasks::SettlementActionError> {
    let id = withdrawal_id
        .as_slice()
        .try_into()
        .map_err(|_| tasks::SettlementActionError::InvalidId)?;
    let caller = ic_cdk::api::msg_caller();
    if !can_advance_withdrawal(caller, id)? {
        return Err(tasks::SettlementActionError::Unauthorized);
    }
    let Some(guard) = InFlightGuard::acquire(ActionKey::Withdrawal(id)) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    let terminal_state = STORE.with(|store| {
        store
            .borrow()
            .withdrawal(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .map(|record| {
                let terminal = matches!(record.state, bridge_core::WithdrawalState::Paid { .. });
                terminal.then(|| {
                    phases::SettlementState::Withdrawal(phases::WithdrawalPhase::from(
                        &record.state,
                    ))
                })
            })
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    if let Some(state) = terminal_state {
        return Ok(tasks::SettlementActionResult::Complete { state });
    }
    drop(guard);
    let job = claim_manual_job(storage::SettlementJobKind::Withdrawal, id, caller)?;
    let result = scheduler::run_claimed(job).await?;
    scheduler::arm();
    Ok(result)
}

fn claim_manual_job(
    kind: storage::SettlementJobKind,
    id: [u8; 32],
    caller: candid::Principal,
) -> Result<storage::SettlementJob, tasks::SettlementActionError> {
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::StorageFailure)
    })?;
    STORE.with(|store| {
        store
            .borrow_mut()
            .claim_manual_settlement_job(
                kind,
                id,
                caller,
                ic_cdk::api::time(),
                ic_cdk::api::time().saturating_add(120_000_000_000),
                300_000_000_000,
                storage::SettlementQuotaLimits {
                    window_seconds: config.settlement_rate_limit_window_seconds,
                    global: config.settlement_rate_limit_global,
                    per_principal: config.settlement_rate_limit_per_principal,
                    per_record: config.settlement_rate_limit_per_record,
                },
            )
            .map_err(|error| match error {
                storage::SettlementAdmissionError::RateLimited {
                    retry_after_seconds,
                } => tasks::SettlementActionError::RateLimited {
                    retry_after_seconds,
                },
                storage::SettlementAdmissionError::Storage => {
                    tasks::SettlementActionError::StorageFailure
                }
            })
            .and_then(|claim| match claim {
                storage::ManualSettlementClaim::Claimed(job) => Ok(job),
                storage::ManualSettlementClaim::AutomaticProgressPending { next_run_at_ns } => {
                    Err(tasks::SettlementActionError::AutomaticProgressPending { next_run_at_ns })
                }
                storage::ManualSettlementClaim::Busy => Err(tasks::SettlementActionError::Busy),
            })
    })
}

fn ensure_supported_schema() {
    STORE.with(|store| {
        if store.borrow().schema_version() != SCHEMA_VERSION {
            ic_cdk::trap("unsupported stable schema version");
        }
    });
}

pub(crate) fn storage_or_trap<T>(context: &str, result: Result<T, StorageError>) -> T {
    result.unwrap_or_else(|error| ic_cdk::trap(format!("{context} failed: {error}")))
}

pub(crate) fn rpc_audit_event_kind(evidence: &evm_rpc::RpcAuditEvidence) -> AuditEventKind {
    AuditEventKind::EvmRpcObservation {
        evm_rpc_canister_id: evidence.evm_rpc_canister_id,
        call_method: evidence.call_method.clone(),
        request_digest: evidence.request_digest.to_vec(),
        quorum_response_digest: evidence.quorum_response_digest.to_vec(),
        finalized_block_number: evidence.finalized_block_number,
        finalized_block_hash: evidence.finalized_block_hash.to_vec(),
        transaction_hash: evidence.transaction_hash.map(|hash| hash.to_vec()),
    }
}

pub(crate) fn rpc_decision_event_kind(evidence: &evm_rpc::RpcDecisionEvidence) -> AuditEventKind {
    AuditEventKind::EvmRpcDecision {
        kind: format!("{:?}", evidence.kind),
        operation: evidence.operation.clone(),
        configured_provider_count: evidence.configured_provider_count,
        required_threshold: evidence.required_threshold,
        stop_reason: evidence.stop_reason.clone(),
        ledger_call_performed: evidence.ledger_call_performed,
        bridge_operation_continued: evidence.bridge_operation_continued,
        deposits_paused: evidence.deposits_paused,
        automatically_resigned: evidence.automatically_resigned,
        transaction_hash: evidence.transaction_hash.map(|hash| hash.to_vec()),
    }
}

fn append_rpc_decision(evidence: &evm_rpc::RpcDecisionEvidence) -> Result<(), StorageError> {
    STORE.with(|store| {
        store.borrow_mut().append_audit_event(
            ic_cdk::api::canister_self(),
            rpc_decision_event_kind(evidence),
        )?;
        Ok(())
    })
}

#[ic_cdk::query]
fn get_bridge_status() -> BridgeStatus {
    STORE.with(|store| {
        let store = store.borrow();
        let counts = store
            .status_counts()
            .unwrap_or_else(|error| ic_cdk::trap(format!("stable state read failed: {error}")));
        let withdrawal_liabilities = storage_or_trap(
            "withdrawal liability summary read",
            store.withdrawal_liability_summary(),
        );
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let progress = storage_or_trap("external progress read", store.external_progress());
        let reserve = config
            .reserve_policy()
            .snapshot(
                storage_or_trap(
                    "nonterminal withdrawal count read",
                    store.nonterminal_withdrawal_count(),
                ),
                counts.reserved_deposit_mint_operations,
                0,
                progress.last_eth_balance_wei,
                ic_cdk::api::canister_liquid_cycle_balance(),
            )
            .unwrap_or_else(|_| ic_cdk::trap("reserve arithmetic overflow"));
        let admin = store
            .admin_state()
            .unwrap_or_else(|_| ic_cdk::trap("missing administrator state"));
        let finalized_observation = progress.finalized_observation;
        let scheduler_diagnostics = storage_or_trap(
            "settlement scheduler diagnostics read",
            store.confirmation_scheduler_health(),
        );
        let now_ns = ic_cdk::api::time();
        let scheduler_summary = storage_or_trap(
            "settlement job summary read",
            store.settlement_job_summary(now_ns, 300_000_000_000),
        );
        let scheduler_health = if !scheduler_diagnostics.healthy {
            SettlementSchedulerHealth::Faulted
        } else if scheduler_summary.stopped > 0
            || scheduler_summary.expired > 0
            || scheduler_summary.overdue > 0
        {
            SettlementSchedulerHealth::Degraded
        } else {
            SettlementSchedulerHealth::Healthy
        };
        BridgeStatus {
            base_chain_id_matches_config: finalized_observation
                .is_some_and(|observation| observation.chain_id == config.base_chain_id),
            schema_version: store.schema_version(),
            counts: StatusCounts {
                deposits: counts.deposits,
                withdrawals: counts.withdrawals,
                pending_evm_operations: counts.pending_evm_operations,
                reconciliation_holds: counts.reconciliation_holds,
                pending_ledger_operations: counts.pending_ledger_operations,
                reserved_deposit_mint_amount: counts.reserved_deposit_mint_amount,
                reserved_deposit_mint_operations: counts.reserved_deposit_mint_operations,
                unresolved_evm_reverts: counts.unresolved_evm_reverts,
                active_evm_payloads: counts.active_evm_payloads,
                retained_audit_events: counts.retained_audit_events,
                pruned_audit_events: counts.pruned_audit_events,
                retained_deposit_index_entries: counts.retained_deposit_index_entries,
            },
            last_finalized_base_block: finalized_observation
                .map(|observation| observation.block_number)
                .unwrap_or_default(),
            last_reserve_observation_ns: progress.last_reserve_observation_ns,
            last_finalized_observation_ns: finalized_observation
                .map(|observation| observation.observed_at_ns)
                .unwrap_or_default(),
            last_finalized_base_block_hash: finalized_observation
                .map(|observation| observation.block_hash.to_vec())
                .unwrap_or_default(),
            observed_base_chain_id: finalized_observation.map(|observation| observation.chain_id),
            observed_bridge_signer: finalized_observation
                .map(|observation| observation.bridge_signer.to_vec())
                .unwrap_or_default(),
            observed_bridge_runtime_sha256: finalized_observation
                .map(|observation| observation.runtime_sha256.to_vec())
                .unwrap_or_default(),
            reserve: ReserveStatus {
                eth_balance_wei: reserve.eth_balance_wei,
                cycles_balance: reserve.cycles_balance,
                required_eth_wei: reserve.required_eth_wei,
                required_cycles: reserve.required_cycles,
                eth_surplus_wei: reserve.eth_surplus_wei,
                cycles_surplus: reserve.cycles_surplus,
                sufficient: reserve.sufficient,
            },
            deposits_paused: admin.deposits_paused,
            last_audit_sequence: storage_or_trap(
                "last audit sequence read",
                store.last_audit_sequence(),
            ),
            settlement_scheduler: SettlementSchedulerStatus {
                health: scheduler_health,
                scheduled: scheduler_summary.scheduled,
                leased: scheduler_summary.leased,
                stopped: scheduler_summary.stopped,
                expired: scheduler_summary.expired,
                next_wakeup_at_ns: scheduler_summary.next_wakeup_at_ns,
                last_dispatcher_run_at_ns: scheduler_diagnostics.last_run_ns,
                last_internal_error: scheduler_diagnostics.last_error,
            },
            unpaid_withdrawal_count: withdrawal_liabilities.count,
            unpaid_withdrawal_amount_out: withdrawal_liabilities.amount_out,
            oldest_unpaid_withdrawal_observed_at_ns: withdrawal_liabilities.oldest_observed_at_ns,
            withdrawal_stop_reasons: withdrawal_liabilities.stop_reasons,
        }
    })
}

#[ic_cdk::update]
async fn refresh_base_observation() -> Result<(), RefreshBaseObservationError> {
    const MIN_REFRESH_INTERVAL_NS: u64 = 30_000_000_000;

    let now = ic_cdk::api::time();
    let recent_observation = STORE.with(|store| {
        store
            .borrow()
            .external_progress()
            .map_err(|_| RefreshBaseObservationError::StorageFailure)
            .map(|progress| {
                progress.finalized_observation.filter(|observation| {
                    now.saturating_sub(observation.observed_at_ns) <= MIN_REFRESH_INTERVAL_NS
                })
            })
    })?;
    if recent_observation.is_some() {
        return Ok(());
    }
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseObservation) else {
        return Err(RefreshBaseObservationError::Busy);
    };
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| RefreshBaseObservationError::StorageFailure)?
            .ok_or(RefreshBaseObservationError::StorageFailure)
    })?;
    match evm_rpc::bridge_snapshot(&config).await {
        Ok(completed) => STORE.with(|store| {
            let mut store = store.borrow_mut();
            match store.finish_base_snapshot_refresh_with_rpc_audit_and_observation(
                ic_cdk::api::time(),
                completed.snapshot.mint,
                completed.snapshot.bridge_signer,
                completed.snapshot.deposits_paused,
                Some(evm_rpc::stable_observation(&completed)),
                ic_cdk::api::canister_self(),
                vec![
                    rpc_audit_event_kind(&completed.rpc_audit),
                    rpc_decision_event_kind(&evm_rpc::quorum_continued_decision(
                        "refresh_base_observation",
                        None,
                        false,
                    )),
                ],
            ) {
                Ok(()) => Ok(()),
                Err(storage::StorageError::Core(
                    bridge_core::CoreError::StaleFinalizedObservation
                    | bridge_core::CoreError::ConflictingFinalizedObservation,
                )) => {
                    store
                        .fail_base_snapshot_refresh()
                        .map_err(|_| RefreshBaseObservationError::StorageFailure)?;
                    Err(RefreshBaseObservationError::BaseStateMismatch)
                }
                Err(_) => Err(RefreshBaseObservationError::StorageFailure),
            }
        }),
        Err(evm_rpc::ObservationError::ChainIdMismatch) => {
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                let mut admin = store
                    .admin_state()
                    .map_err(|_| RefreshBaseObservationError::StorageFailure)?;
                if !admin.deposits_paused {
                    admin.deposits_paused = true;
                    store
                        .set_admin_state(&admin)
                        .map_err(|_| RefreshBaseObservationError::StorageFailure)?;
                }
                Ok(())
            })?;
            Err(RefreshBaseObservationError::BaseStateMismatch)
        }
        Err(evm_rpc::ObservationError::Inconsistent) => {
            append_rpc_decision(&evm_rpc::quorum_loss_decision(
                "refresh_base_observation",
                None,
            ))
            .map_err(|_| RefreshBaseObservationError::StorageFailure)?;
            Err(RefreshBaseObservationError::ObservationUnavailable)
        }
        Err(evm_rpc::ObservationError::BaseStateMismatch) => {
            Err(RefreshBaseObservationError::BaseStateMismatch)
        }
        Err(_) => Err(RefreshBaseObservationError::ObservationUnavailable),
    }
}

#[ic_cdk::update]
async fn get_public_config() -> PublicConfig {
    let config = STORE.with(|store| {
        let store = store.borrow();
        storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"))
    });
    let expected_bridge_signer = api::cached_signer_address(&config)
        .await
        .unwrap_or_else(|_| ic_cdk::trap("chain-key signer derivation failed"));
    let normalized_rpc_urls = config
        .custom_evm_rpc_urls
        .iter()
        .map(|url| url.trim())
        .collect::<Vec<_>>();
    let rpc_provider_urls_sha256 = Sha256::digest(
        serde_json::to_vec(&normalized_rpc_urls)
            .unwrap_or_else(|_| ic_cdk::trap("RPC URL digest serialization failed")),
    )
    .to_vec();
    STORE.with(|store| {
        let store = store.borrow();
        PublicConfig {
            base_chain_id: config.base_chain_id,
            bridge_contract: config.bridge_contract,
            ledger_canister_id: config.ledger_canister_id,
            index_canister_id: config.index_canister_id,
            schema_version: store.schema_version(),
            expected_bridge_signer: Vec::from(expected_bridge_signer),
            evm_rpc_canister_id: config.evm_rpc_canister_id,
            rpc_provider_urls_sha256,
        }
    })
}

#[ic_cdk::update]
async fn sign_chain_key_challenge(release_id: String) -> Result<String, ChainKeyChallengeError> {
    if !valid_release_id(&release_id) {
        return Err(ChainKeyChallengeError::InvalidReleaseId);
    }
    let caller = ic_cdk::api::msg_caller();
    if !admin::is_governance(caller).map_err(|_| ChainKeyChallengeError::StorageFailure)? {
        return Err(ChainKeyChallengeError::Unauthorized);
    }
    let Some(_guard) = InFlightGuard::acquire(ActionKey::ChainKeyChallenge) else {
        return Err(ChainKeyChallengeError::Busy);
    };
    let config = STORE.with(|store| {
        storage_or_trap("configuration read", store.borrow().config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"))
    });
    signer::sign_chain_key_challenge(&release_id, &config)
        .await
        .map_err(|_| ChainKeyChallengeError::SigningUnavailable)
}

#[ic_cdk::query]
fn icrc10_supported_standards() -> Vec<consent::Icrc10SupportedStandard> {
    consent::supported_standards()
}

#[ic_cdk::update]
fn icrc21_canister_call_consent_message(
    request: consent::Icrc21ConsentMessageRequest,
) -> consent::Icrc21ConsentMessageResponse {
    consent::consent_message(
        ic_cdk::api::msg_caller(),
        ic_cdk::api::canister_self(),
        request,
    )
}

#[ic_cdk::update]
fn pause_new_deposits() -> Result<(), admin::AdminError> {
    admin::pause(ic_cdk::api::msg_caller())
}
#[ic_cdk::update]
fn resume_new_deposits() -> Result<(), admin::AdminError> {
    admin::resume(ic_cdk::api::msg_caller())
}
#[ic_cdk::update]
fn set_fee_recipient(value: config::FeeRecipientConfig) -> Result<(), admin::AdminError> {
    admin::set_fee_recipient(ic_cdk::api::msg_caller(), value)
}
#[ic_cdk::update]
fn rotate_runtime_administrators(
    args: admin::RotateRuntimeAdministratorsArgs,
) -> Result<(), admin::AdminError> {
    admin::rotate(ic_cdk::api::msg_caller(), args)
}
#[ic_cdk::update]
async fn request_fee_payout(
    amount: candid::Nat,
) -> Result<admin::FeePayoutReceipt, admin::AdminError> {
    let Some(_guard) = InFlightGuard::acquire(ActionKey::FeePayoutCreation) else {
        return Err(admin::AdminError::Busy);
    };
    admin::request_fee_payout(ic_cdk::api::msg_caller(), amount).await
}

#[ic_cdk::update]
async fn continue_fee_payout(
    payout_id: u64,
) -> Result<tasks::FeePayoutActionResult, tasks::SettlementActionError> {
    let caller = ic_cdk::api::msg_caller();
    if caller == candid::Principal::anonymous() {
        return Err(tasks::SettlementActionError::AnonymousCaller);
    }
    if !admin::can_manage_fee_payout(caller)
        .map_err(|_| tasks::SettlementActionError::StorageFailure)?
    {
        return Err(tasks::SettlementActionError::Unauthorized);
    }
    let Some(_guard) = InFlightGuard::acquire(ActionKey::FeePayout(payout_id)) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    tasks::advance_fee_payout(payout_id).await
}
#[ic_cdk::query]
fn get_audit_events(start: u64, limit: u16) -> Result<storage::AuditEventPage, admin::AdminError> {
    admin::audit_events(start, limit)
}

ic_cdk::export_candid!();

/// Returns the Candid interface generated from the Rust service definitions.
pub fn generated_candid_interface() -> String {
    __export_service()
}

#[cfg(test)]
mod candid_tests {
    use super::{
        storage::StorageError, storage_or_trap, valid_release_id, ActionKey, InFlightGuard,
    };

    fn normalize(candid: &str) -> String {
        candid
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }

    #[test]
    fn checked_in_candid_matches_rust_interface() {
        let generated = super::__export_service();
        let checked_in = include_str!("../bridge.did");
        assert_eq!(normalize(&generated), normalize(checked_in));
    }

    #[test]
    fn in_flight_guard_blocks_only_the_same_key() {
        let guard =
            InFlightGuard::acquire(ActionKey::Deposit([1; 32])).expect("first call acquires guard");
        assert!(InFlightGuard::acquire(ActionKey::Deposit([1; 32])).is_none());
        assert!(InFlightGuard::acquire(ActionKey::Deposit([2; 32])).is_some());
        drop(guard);
        assert!(InFlightGuard::acquire(ActionKey::Deposit([1; 32])).is_some());
    }

    #[test]
    fn in_flight_guard_releases_the_key_on_drop() {
        let key = ActionKey::Withdrawal([3; 32]);
        let guard = InFlightGuard::acquire(key.clone()).expect("first call acquires guard");
        assert!(InFlightGuard::acquire(key.clone()).is_none());
        drop(guard);
        assert!(InFlightGuard::acquire(key).is_some());
    }

    #[test]
    fn release_id_is_strictly_bounded_and_domain_safe() {
        assert!(valid_release_id("release-1"));
        assert!(valid_release_id("12345678"));
        assert!(!valid_release_id("short-1"));
        assert!(!valid_release_id("Release-1"));
        assert!(!valid_release_id("release_1"));
        assert!(!valid_release_id("release-1\naddress=0x00"));
        assert!(!valid_release_id(&"a".repeat(65)));
    }

    #[test]
    fn storage_errors_are_not_converted_to_default_values() {
        let trapped = std::panic::catch_unwind(|| {
            storage_or_trap::<()>("test storage read", Err(StorageError::DecodeFailed));
        });
        assert!(trapped.is_err());
    }
}
