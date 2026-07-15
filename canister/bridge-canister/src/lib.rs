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
mod evm_rpc;
mod ledger;
mod scheduler;
mod signer;
pub mod storage;
mod tasks;

use storage::{StableStore, StorageError, SCHEMA_VERSION};

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
    pub pending_ledger_operations: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reserved_deposit_mint_operations: u64,
    pub reverted_evm_operations: u64,
    pub active_evm_payloads: u64,
    pub retained_audit_events: u64,
    pub pruned_audit_events: u64,
    pub retained_deposit_index_entries: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BridgeStatus {
    pub schema_version: u16,
    pub counts: StatusCounts,
    pub last_safe_base_block: u64,
    pub last_reserve_observation_ns: u64,
    pub last_safe_observation_ns: u64,
    pub reserve: ReserveStatus,
    pub deposits_paused: bool,
    pub last_audit_sequence: Option<u64>,
    pub confirmation_scheduler: ConfirmationSchedulerStatus,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationSchedulerStatus {
    pub healthy: bool,
    pub scheduled_operations: u64,
    pub next_check_at_ns: Option<u64>,
    pub last_run_ns: u64,
    pub last_error: Option<String>,
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
    let Some(_guard) = InFlightGuard::acquire(ActionKey::Deposit(id)) else {
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
    let settlement = tasks::advance_deposit(id)
        .await
        .map_err(|_| api::DepositError::StorageFailure)?;
    persist_deposit_settlement_result(id, &settlement)
        .map_err(|_| api::DepositError::StorageFailure)?;
    receipt.state = api::deposit_state(id)?;
    receipt.settlement = Some(settlement);
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
    let Some(_notification_guard) =
        InFlightGuard::acquire(ActionKey::Notification(transaction_hash))
    else {
        return Err(api::NotifyWithdrawalError::Busy);
    };
    let mut receipt = api::notify_withdrawal(caller, args).await?;
    if matches!(receipt, api::NotifyWithdrawalReceipt::Duplicate { .. }) {
        return Ok(receipt);
    }
    let id = match &receipt {
        api::NotifyWithdrawalReceipt::Ingested { withdrawal_id, .. }
        | api::NotifyWithdrawalReceipt::Duplicate { withdrawal_id, .. } => withdrawal_id
            .as_slice()
            .try_into()
            .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?,
    };
    let Some(_withdrawal_guard) = InFlightGuard::acquire(ActionKey::Withdrawal(id)) else {
        return Err(api::NotifyWithdrawalError::Busy);
    };
    let settlement = tasks::advance_withdrawal(id)
        .await
        .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?;
    persist_withdrawal_settlement_result(id, &settlement)
        .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?;
    match &mut receipt {
        api::NotifyWithdrawalReceipt::Ingested {
            settlement: slot, ..
        }
        | api::NotifyWithdrawalReceipt::Duplicate {
            settlement: slot, ..
        } => *slot = Some(settlement),
    }
    scheduler::arm();
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
    if let Some(next_run_at_ns) = deposit_automatic_progress(id)? {
        return Err(tasks::SettlementActionError::AutomaticProgressPending { next_run_at_ns });
    }
    let Some(_guard) = InFlightGuard::acquire(ActionKey::Deposit(id)) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    let terminal = STORE.with(|store| {
        store
            .borrow()
            .deposit(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .map(|record| {
                matches!(
                    record.state,
                    bridge_core::DepositState::Minted { .. }
                        | bridge_core::DepositState::MintReverted { .. }
                        | bridge_core::DepositState::Cancelled { .. }
                )
            })
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    if !terminal {
        reserve_settlement_quota(caller, 0, id)?;
    }
    let result = tasks::advance_deposit(id).await?;
    persist_deposit_settlement_result(id, &result)?;
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
    if let Some(next_run_at_ns) = withdrawal_automatic_progress(id)? {
        return Err(tasks::SettlementActionError::AutomaticProgressPending { next_run_at_ns });
    }
    let Some(_guard) = InFlightGuard::acquire(ActionKey::Withdrawal(id)) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    let terminal = STORE.with(|store| {
        store
            .borrow()
            .withdrawal(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .map(|record| {
                matches!(
                    record.state,
                    bridge_core::WithdrawalState::Released { .. }
                        | bridge_core::WithdrawalState::Refunded { .. }
                        | bridge_core::WithdrawalState::AcknowledgeReverted { .. }
                        | bridge_core::WithdrawalState::RefundReverted { .. }
                )
            })
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    if !terminal {
        reserve_settlement_quota(caller, 1, id)?;
    }
    let result = tasks::advance_withdrawal(id).await?;
    persist_withdrawal_settlement_result(id, &result)?;
    scheduler::arm();
    Ok(result)
}

fn deposit_automatic_progress(
    id: [u8; 32],
) -> Result<Option<Option<u64>>, tasks::SettlementActionError> {
    STORE.with(|store| {
        let job = store
            .borrow()
            .settlement_job(storage::SettlementJobKind::Deposit, id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?;
        Ok(job.and_then(|job| match job.status {
            storage::SettlementJobStatus::Scheduled => job.next_run_at_ns.map(Some),
            storage::SettlementJobStatus::Leased => Some(None),
            storage::SettlementJobStatus::Stopped => None,
        }))
    })
}

fn withdrawal_automatic_progress(
    id: [u8; 32],
) -> Result<Option<Option<u64>>, tasks::SettlementActionError> {
    STORE.with(|store| {
        let job = store
            .borrow()
            .settlement_job(storage::SettlementJobKind::Withdrawal, id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?;
        Ok(job.and_then(|job| match job.status {
            storage::SettlementJobStatus::Scheduled => job.next_run_at_ns.map(Some),
            storage::SettlementJobStatus::Leased => Some(None),
            storage::SettlementJobStatus::Stopped => None,
        }))
    })
}

fn reserve_settlement_quota(
    caller: candid::Principal,
    kind: u8,
    id: [u8; 32],
) -> Result<(), tasks::SettlementActionError> {
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::StorageFailure)
    })?;
    let mut key = Vec::with_capacity(33);
    key.push(kind);
    key.extend_from_slice(&id);
    STORE.with(|store| {
        store
            .borrow_mut()
            .reserve_settlement_quota(
                caller,
                key,
                ic_cdk::api::time(),
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
    })
}

fn persist_deposit_settlement_result(
    id: [u8; 32],
    result: &tasks::SettlementActionResult,
) -> Result<(), tasks::SettlementActionError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut record = store
            .deposit(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::NotFound)?;
        record.last_settlement_stop_reason = tasks::stop_reason_text(result);
        store
            .put_deposit(&record)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)
    })
}

fn persist_withdrawal_settlement_result(
    id: [u8; 32],
    result: &tasks::SettlementActionResult,
) -> Result<(), tasks::SettlementActionError> {
    STORE.with(|store| {
        let mut store = store.borrow_mut();
        let mut record = store
            .withdrawal(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::NotFound)?;
        record.last_settlement_stop_reason = tasks::stop_reason_text(result);
        store
            .put_withdrawal(&record)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)
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

#[ic_cdk::query]
fn get_bridge_status() -> BridgeStatus {
    STORE.with(|store| {
        let store = store.borrow();
        let counts = store
            .status_counts()
            .unwrap_or_else(|error| ic_cdk::trap(format!("stable state read failed: {error}")));
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
        let mut scheduler_health = storage_or_trap(
            "confirmation scheduler health read",
            store.confirmation_scheduler_health(),
        );
        let next_schedule = match store.earliest_confirmation_schedule() {
            Ok(schedule) => schedule,
            Err(error) => {
                scheduler_health.healthy = false;
                scheduler_health.last_error = Some(format!(
                    "failed to read the next confirmation schedule: {error}"
                ));
                None
            }
        };
        BridgeStatus {
            schema_version: store.schema_version(),
            counts: StatusCounts {
                deposits: counts.deposits,
                withdrawals: counts.withdrawals,
                pending_evm_operations: counts.pending_evm_operations,
                reconciliation_holds: counts.reconciliation_holds,
                pending_ledger_operations: counts.pending_ledger_operations,
                reserved_deposit_mint_amount: counts.reserved_deposit_mint_amount,
                reserved_deposit_mint_operations: counts.reserved_deposit_mint_operations,
                reverted_evm_operations: counts.reverted_evm_operations,
                active_evm_payloads: counts.active_evm_payloads,
                retained_audit_events: counts.retained_audit_events,
                pruned_audit_events: counts.pruned_audit_events,
                retained_deposit_index_entries: counts.retained_deposit_index_entries,
            },
            last_safe_base_block: counts.last_safe_base_block,
            last_reserve_observation_ns: progress.last_reserve_observation_ns,
            last_safe_observation_ns: progress.last_safe_observation_ns,
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
            confirmation_scheduler: ConfirmationSchedulerStatus {
                healthy: scheduler_health.healthy,
                scheduled_operations: store.confirmation_schedule_count(),
                next_check_at_ns: next_schedule.map(|schedule| schedule.next_check_at_ns),
                last_run_ns: scheduler_health.last_run_ns,
                last_error: scheduler_health.last_error,
            },
        }
    })
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
) -> Result<tasks::SettlementActionResult, tasks::SettlementActionError> {
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
