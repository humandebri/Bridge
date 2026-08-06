//! IC boundary and stable storage adapter for the KINIC–Base Bridge.
//!
//! This crate exposes the Candid boundary and connects the deterministic core to stable storage,
//! ICRC Ledger calls, EVM RPC observation, threshold ECDSA signing, scheduled confirmation, and runtime
//! administration.

use candid::{CandidType, Deserialize, Principal};
use ic_sqlite_vfs::DefaultMemoryImpl;
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ops::{Deref, DerefMut},
};

mod admin;
mod api;
mod base_governance;
pub mod config;
mod consent;
mod evm_rpc;
mod ledger;
mod mint_authorization;
mod phases;
mod scheduler;
mod signer;
pub mod storage;
mod tasks;

#[cfg(feature = "test-deployment")]
use storage::SettlementClaimProfile;
use storage::{
    AuditEventKind, ChecksumRefreshStatus, StableStore, StorageError, StorageMaintenanceError,
    StorageValidationStatus, SCHEMA_VERSION,
};

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub reconciliation_holds: u64,
    pub pending_ledger_operations: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reserved_deposit_mint_operations: u64,
    pub retained_audit_events: u64,
    pub pruned_audit_events: u64,
    pub retained_deposit_index_entries: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct BridgeStatus {
    pub schema_version: u16,
    pub mint_authorization_ttl_seconds: u64,
    pub mint_authorization_epoch: u64,
    pub counts: StatusCounts,
    pub last_finalized_base_block: u64,
    pub last_reserve_observation_ns: u64,
    pub last_finalized_observation_ns: u64,
    pub last_finalized_base_block_hash: Vec<u8>,
    pub observed_bridge_signer: Vec<u8>,
    pub observed_bridge_runtime_sha256: Vec<u8>,
    pub reserve: ReserveStatus,
    pub deposits_paused: bool,
    pub withdrawal_fee_guard_active: bool,
    pub withdrawal_fee_guard_ledger_fee: Option<u128>,
    pub withdrawal_fee_guard_charged_service_fee: Option<u128>,
    pub last_audit_sequence: Option<u64>,
    pub settlement_scheduler: SettlementSchedulerStatus,
    pub unpaid_withdrawal_count: u64,
    pub unpaid_withdrawal_amount_out: u128,
    pub oldest_unpaid_withdrawal_observed_at_ns: Option<u64>,
    pub withdrawal_stop_reasons: Vec<String>,
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
    pub governance_eth_floor_wei: u128,
    pub required_cycles: u128,
    pub eth_surplus_wei: u128,
    pub cycles_surplus: u128,
    pub sufficient: bool,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PublicConfig {
    pub base_chain_id: u64,
    pub bridge_contract: Vec<u8>,
    pub expected_bridge_runtime_sha256: Vec<u8>,
    pub timelock_contract: Vec<u8>,
    pub deployment_instance_id: Vec<u8>,
    pub ledger_canister_id: candid::Principal,
    pub ledger_fee: u128,
    pub index_canister_id: candid::Principal,
    pub schema_version: u16,
    pub mint_authorization_ttl_seconds: u64,
    pub mint_authorization_epoch: u64,
    pub expected_bridge_signer: Vec<u8>,
    pub governance_operator: Vec<u8>,
    pub evm_rpc_canister_id: candid::Principal,
    pub rpc_provider_urls_sha256: Vec<u8>,
    pub deposit_rate_limit_window_seconds: u64,
    pub deposit_rate_limit_global: u16,
    pub deposit_rate_limit_per_principal: u16,
    pub notification_rate_limit_window_seconds: u64,
    pub notification_rate_limit_global: u16,
    pub notification_ingestion_rate_limit_global: u16,
    pub settlement_rate_limit_window_seconds: u64,
    pub settlement_rate_limit_global: u16,
    pub settlement_rate_limit_per_principal: u16,
    pub settlement_rate_limit_per_record: u16,
    pub settlement_retry_interval_seconds: u64,
    pub governance_evm_fee: config::EvmFeePolicy,
    pub governance_replacement: config::GovernanceReplacementPolicy,
    pub governance_eth_floor_wei: u128,
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    pub governance_principal: candid::Principal,
    pub pause_principal: candid::Principal,
    pub fee_recipient: config::FeeRecipientConfig,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublicConfigInitializationError {
    Unauthorized,
    Busy,
    DerivationUnavailable,
    ConflictingAddress,
    StorageFailure,
}

thread_local! {
    static STORE: RefCell<StoreState> = const { RefCell::new(StoreState(None)) };
    static IN_FLIGHT_ACTIONS: RefCell<BTreeSet<ActionKey>> = const { RefCell::new(BTreeSet::new()) };
    static NOTIFICATION_CALLERS: RefCell<BTreeMap<Principal, u8>> = const { RefCell::new(BTreeMap::new()) };
    static NOTIFICATIONS_IN_FLIGHT: RefCell<u8> = const { RefCell::new(0) };
    static NOTIFICATION_CALLER_ADMISSION: RefCell<BTreeMap<Principal, NotificationAdmissionBucket>> =
        const { RefCell::new(BTreeMap::new()) };
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
    PublicConfigInitialization,
    BaseGovernance,
    EmergencyPause,
}

struct InFlightGuard {
    key: ActionKey,
}

struct NotificationQuotaGuard {
    caller: Principal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NotificationAdmissionBucket {
    window_id: u64,
    count: u8,
    last_seen_ns: u64,
}

struct NotificationAdmissionGuard;

impl NotificationAdmissionGuard {
    const PER_CALLER_LIMIT: u16 = 6;
    const MAX_CALLER_BUCKETS: usize = 4_096;

    fn caller_count(caller: Principal, now_ns: u64, window_seconds: u64) -> u16 {
        let window_id = now_ns / window_seconds.saturating_mul(1_000_000_000);
        NOTIFICATION_CALLER_ADMISSION
            .with(|callers| u16::from(bucket_count(&callers.borrow(), &caller, window_id)))
    }

    fn record(caller: Principal, now_ns: u64, window_seconds: u64) {
        let window_id = now_ns / window_seconds.saturating_mul(1_000_000_000);
        NOTIFICATION_CALLER_ADMISSION.with(|callers| {
            record_notification_bucket(
                &mut callers.borrow_mut(),
                caller,
                window_id,
                now_ns,
                Self::MAX_CALLER_BUCKETS,
            );
        })
    }
}

fn bucket_count<K: Ord>(
    buckets: &BTreeMap<K, NotificationAdmissionBucket>,
    key: &K,
    window_id: u64,
) -> u8 {
    buckets
        .get(key)
        .filter(|bucket| bucket.window_id == window_id)
        .map_or(0, |bucket| bucket.count)
}

fn record_notification_bucket<K: Ord + Clone>(
    buckets: &mut BTreeMap<K, NotificationAdmissionBucket>,
    key: K,
    window_id: u64,
    now_ns: u64,
    capacity: usize,
) {
    if !buckets.contains_key(&key) && buckets.len() >= capacity {
        if let Some(oldest) = buckets
            .iter()
            .min_by_key(|(_, bucket)| (bucket.window_id, bucket.last_seen_ns))
            .map(|(key, _)| key.clone())
        {
            buckets.remove(&oldest);
        }
    }
    let bucket = buckets.entry(key).or_insert(NotificationAdmissionBucket {
        window_id,
        count: 0,
        last_seen_ns: now_ns,
    });
    if bucket.window_id != window_id {
        bucket.window_id = window_id;
        bucket.count = 0;
    }
    bucket.count = bucket.count.saturating_add(1);
    bucket.last_seen_ns = now_ns;
}

impl NotificationQuotaGuard {
    const GLOBAL_LIMIT: u8 = 16;
    const PER_CALLER_LIMIT: u8 = 2;

    fn acquire(caller: Principal) -> Option<Self> {
        NOTIFICATIONS_IN_FLIGHT.with(|global| {
            NOTIFICATION_CALLERS.with(|callers| {
                let mut global = global.borrow_mut();
                let mut callers = callers.borrow_mut();
                let caller_count = callers.get(&caller).copied().unwrap_or(0);
                if *global >= Self::GLOBAL_LIMIT || caller_count >= Self::PER_CALLER_LIMIT {
                    return None;
                }
                *global += 1;
                callers.insert(caller, caller_count + 1);
                Some(Self { caller })
            })
        })
    }
}

impl Drop for NotificationQuotaGuard {
    fn drop(&mut self) {
        NOTIFICATIONS_IN_FLIGHT.with(|global| {
            let mut global = global.borrow_mut();
            *global = global.saturating_sub(1);
        });
        NOTIFICATION_CALLERS.with(|callers| {
            let mut callers = callers.borrow_mut();
            if let Some(count) = callers.get_mut(&self.caller) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    callers.remove(&self.caller);
                }
            }
        });
    }
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
    scheduler::arm_funding_recovery();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    let store = StableStore::reopen_after_upgrade(DefaultMemoryImpl::default())
        .unwrap_or_else(|error| ic_cdk::trap(format!("stable state reopen failed: {error}")));
    install_store(store);
    ensure_supported_schema();
    scheduler::arm();
    scheduler::arm_funding_recovery();
}

#[ic_cdk::update]
async fn request_deposit(args: api::DepositArgs) -> Result<api::DepositReceipt, api::DepositError> {
    let caller = ic_cdk::api::msg_caller();
    let id = api::deposit_action_id(caller, &args)?;
    let existed = STORE.with(|store| {
        store
            .borrow()
            .deposit(id)
            .map(|record| record.is_some())
            .map_err(|_| api::DepositError::StorageFailure)
    })?;
    if existed {
        return api::request_deposit(caller, args).await;
    }
    let Some(guard) = InFlightGuard::acquire(ActionKey::Deposit(id)) else {
        return Err(api::DepositError::Busy);
    };
    let receipt = api::request_deposit(caller, args).await;
    drop(guard);
    scheduler::arm_funding_recovery();
    if receipt.is_ok() {
        scheduler::arm();
    }
    receipt
}

#[ic_cdk::query]
fn get_deposit(id: Vec<u8>) -> Option<api::DepositView> {
    api::get_deposit(id)
}

fn map_deposit_refund_observation_error(
    operation: &'static str,
    error: evm_rpc::ObservationError,
) -> api::RequestDepositRefundError {
    use api::RequestDepositRefundError as Error;

    match error {
        evm_rpc::ObservationError::Inconsistent => {
            let decision = evm_rpc::quorum_loss_decision(operation, None);
            if STORE
                .with(|store| {
                    store.borrow_mut().append_audit_events_atomically(
                        ic_cdk::api::canister_self(),
                        vec![rpc_decision_event_kind(&decision)],
                    )
                })
                .is_err()
            {
                Error::StorageFailure
            } else {
                Error::RpcInconsistent
            }
        }
        evm_rpc::ObservationError::Rpc | evm_rpc::ObservationError::TransactionPending => {
            Error::FinalityUnavailable
        }
        evm_rpc::ObservationError::BaseStateMismatch
        | evm_rpc::ObservationError::InvalidResponse
        | evm_rpc::ObservationError::Overflow
        | evm_rpc::ObservationError::TransactionReverted => Error::BaseStateMismatch,
    }
}

fn map_deposit_refund_exact_mint_error(
    error: evm_rpc::ObservationError,
) -> api::RequestDepositRefundError {
    if matches!(error, evm_rpc::ObservationError::BaseStateMismatch) {
        api::RequestDepositRefundError::DepositIdentityConflict
    } else {
        map_deposit_refund_observation_error("request_deposit_refund_exact_mint", error)
    }
}

#[ic_cdk::update]
async fn request_deposit_refund(
    deposit_id: Vec<u8>,
) -> Result<api::DepositView, api::RequestDepositRefundError> {
    use api::RequestDepositRefundError as Error;

    let caller = ic_cdk::api::msg_caller();
    match bridge_core::refund_request_identity_decision(
        caller != candid::Principal::anonymous(),
        None,
    ) {
        bridge_core::RefundRequestIdentityDecision::OwnerLookupRequired => {}
        bridge_core::RefundRequestIdentityDecision::AnonymousCaller => {
            return Err(Error::AnonymousCaller);
        }
        bridge_core::RefundRequestIdentityDecision::Allow
        | bridge_core::RefundRequestIdentityDecision::OwnerMismatch => {
            return Err(Error::StorageFailure);
        }
    }
    let id: [u8; 32] = deposit_id
        .as_slice()
        .try_into()
        .map_err(|_| Error::InvalidDepositId)?;
    let owned = STORE.with(|store| {
        store
            .borrow()
            .deposit_intent(id)
            .map_err(|_| Error::StorageFailure)?
            .map(|intent| intent.caller == caller.as_slice())
            .ok_or(Error::NotFound)
    })?;
    match bridge_core::refund_request_identity_decision(true, Some(owned)) {
        bridge_core::RefundRequestIdentityDecision::Allow => {}
        bridge_core::RefundRequestIdentityDecision::OwnerMismatch => {
            return Err(Error::OwnerMismatch);
        }
        bridge_core::RefundRequestIdentityDecision::OwnerLookupRequired
        | bridge_core::RefundRequestIdentityDecision::AnonymousCaller => {
            return Err(Error::StorageFailure);
        }
    }
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| Error::StorageFailure)?
            .ok_or(Error::StorageFailure)
    })?;
    if !has_external_call_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.cycles_floor,
        config.settlement_cycle_ceiling,
    ) {
        return Err(Error::InsufficientCycles);
    }
    let Some(guard) = InFlightGuard::acquire(ActionKey::Deposit(id)) else {
        return Err(Error::Busy);
    };

    let state = STORE.with(|store| {
        store
            .borrow()
            .deposit(id)
            .map_err(|_| Error::StorageFailure)?
            .map(|record| record.state)
            .ok_or(Error::NotFound)
    })?;
    let mut refund_start = None;
    match state {
        bridge_core::DepositState::Minted { .. } | bridge_core::DepositState::Refunded { .. } => {
            return api::get_deposit(id.to_vec()).ok_or(Error::StorageFailure);
        }
        bridge_core::DepositState::AuthorizationPending { .. } => {
            let deadline = STORE.with(|store| {
                store
                    .borrow()
                    .deposit(id)
                    .map_err(|_| Error::StorageFailure)?
                    .and_then(|record| record.mint_authorization)
                    .map(|authorization| authorization.authorization.deadline)
                    .ok_or(Error::StorageFailure)
            })?;
            let (snapshot, _) = api::base_mint_snapshot(&config, ic_cdk::api::time())
                .await
                .map_err(|error| match error {
                    api::DepositError::BaseObservationUnavailable => Error::FinalityUnavailable,
                    _ => Error::StorageFailure,
                })?;
            if snapshot.confirmed_block_timestamp <= deadline {
                return Err(Error::NotClaimable);
            }
            STORE.with(|store| {
                let mut store = store.borrow_mut();
                let mut deposit = store
                    .deposit(id)
                    .map_err(|_| Error::StorageFailure)?
                    .ok_or(Error::NotFound)?;
                if matches!(
                    deposit.state,
                    bridge_core::DepositState::AuthorizationPending { .. }
                ) {
                    let result = deposit
                        .apply(bridge_core::DepositEvent::MarkRefundAvailable {
                            reason: bridge_core::DepositRefundReason::AuthorizationExpired,
                            finalized_timestamp: Some(snapshot.confirmed_block_timestamp),
                        })
                        .map_err(|_| Error::StorageFailure)?;
                    store
                        .put_deposit_transition(&deposit, result)
                        .map_err(|_| Error::StorageFailure)?;
                }
                Ok::<_, Error>(())
            })?;
            refund_start = Some(
                tasks::prepare_deposit_refund(
                    id,
                    bridge_core::DepositRefundReason::AuthorizationExpired,
                    None,
                )
                .map_err(|_| Error::StorageFailure)?,
            );
        }
        bridge_core::DepositState::AuthorizationAvailable { .. }
        | bridge_core::DepositState::RefundAvailable { .. } => {
            let (authorization, refund_reason) = STORE.with(|store| {
                let record = store
                    .borrow()
                    .deposit(id)
                    .map_err(|_| Error::StorageFailure)?
                    .ok_or(Error::NotFound)?;
                let reason = match record.state {
                    bridge_core::DepositState::RefundAvailable { reason, .. } => Some(reason),
                    _ => None,
                };
                Ok::<_, Error>((record.mint_authorization, reason))
            })?;
            if !authorization
                .as_ref()
                .is_some_and(|authorization| authorization.signature.is_some())
            {
                let reason = refund_reason.ok_or(Error::StorageFailure)?;
                refund_start = Some(
                    tasks::prepare_deposit_refund(id, reason, None)
                        .map_err(|_| Error::StorageFailure)?,
                );
            } else {
                let authorization = authorization.ok_or(Error::StorageFailure)?;
                if api::cached_authorization_observation(&config, id)
                    .map_err(|_| Error::StorageFailure)?
                    .is_some_and(|cached| {
                        cached.is_fresh_at(ic_cdk::api::time())
                            && cached.snapshot.mint.confirmed_block_timestamp
                                <= authorization.authorization.deadline
                    })
                {
                    return Err(Error::NotClaimable);
                }
                let runtime_attested =
                    api::runtime_attested(&config).map_err(|_| Error::StorageFailure)?;
                let observation = evm_rpc::recovery_observation(
                    &config,
                    evm_rpc::RecoveryTarget::Deposit(id),
                    runtime_attested,
                )
                .await
                .map_err(|error| {
                    map_deposit_refund_observation_error("request_deposit_refund_recovery", error)
                })?;
                api::cache_recovery_observation(&config, id, &observation)
                    .map_err(|_| Error::StorageFailure)?;
                if observation.snapshot.mint.confirmed_block_timestamp
                    <= authorization.authorization.deadline
                {
                    return Err(Error::NotClaimable);
                }
                STORE.with(|store| {
                    let mut store = store.borrow_mut();
                    let mut deposit = store
                        .deposit(id)
                        .map_err(|_| Error::StorageFailure)?
                        .ok_or(Error::NotFound)?;
                    if matches!(
                        deposit.state,
                        bridge_core::DepositState::AuthorizationAvailable { .. }
                    ) {
                        let result = deposit
                            .apply(bridge_core::DepositEvent::MarkRefundAvailable {
                                reason: bridge_core::DepositRefundReason::AuthorizationExpired,
                                finalized_timestamp: Some(
                                    observation.snapshot.mint.confirmed_block_timestamp,
                                ),
                            })
                            .map_err(|_| Error::StorageFailure)?;
                        store
                            .put_deposit_transition(&deposit, result)
                            .map_err(|_| Error::StorageFailure)?;
                    }
                    Ok::<_, Error>(())
                })?;
                match observation.state {
                    evm_rpc::RecoveryBaseState::DepositProcessed(false) => {
                        refund_start = Some(
                            tasks::prepare_deposit_refund(
                                id,
                                bridge_core::DepositRefundReason::AuthorizationExpired,
                                Some(bridge_core::MintExpiryEvidence {
                                    deposit_id: authorization.authorization.deposit_id,
                                    authorization_digest: authorization.digest,
                                    chain_id: config.base_chain_id,
                                    verifying_contract: authorization.domain.verifying_contract,
                                    deposit_processed: false,
                                    finalized_block_number: observation.finalized.block_number,
                                    finalized_block_hash: observation.finalized.block_hash,
                                    finalized_block_timestamp: observation
                                        .snapshot
                                        .mint
                                        .confirmed_block_timestamp,
                                    bridge_signer: observation.bridge_identity.signer,
                                    mint_authorization_epoch: observation
                                        .snapshot
                                        .mint_authorization_epoch,
                                    runtime_sha256: observation.bridge_identity.runtime_sha256,
                                    rpc_request_digest: observation.rpc_audit.request_digest,
                                    rpc_response_digest: observation
                                        .rpc_audit
                                        .quorum_response_digest,
                                }),
                            )
                            .map_err(|_| Error::StorageFailure)?,
                        );
                    }
                    evm_rpc::RecoveryBaseState::DepositProcessed(true) => {
                        let evidence = evm_rpc::exact_mint_evidence(
                            &config,
                            &authorization,
                            observation.finalized,
                        )
                        .await
                        .map_err(map_deposit_refund_exact_mint_error)?;
                        STORE.with(|store| {
                            let mut store = store.borrow_mut();
                            let mut deposit = store
                                .deposit(id)
                                .map_err(|_| Error::StorageFailure)?
                                .ok_or(Error::NotFound)?;
                            let result = deposit
                                .apply(bridge_core::DepositEvent::MintReconciled {
                                    evidence: Box::new(evidence),
                                })
                                .map_err(|_| Error::BaseStateMismatch)?;
                            store
                                .put_deposit_transition(&deposit, result)
                                .map_err(|_| Error::StorageFailure)
                        })?;
                        return api::get_deposit(id.to_vec()).ok_or(Error::StorageFailure);
                    }
                }
            }
        }
        bridge_core::DepositState::RefundPending { .. }
        | bridge_core::DepositState::RefundReconciliationHold { .. } => {}
        _ => return Err(Error::NotClaimable),
    }
    drop(guard);

    let job = if let Some((deposit, transition)) = refund_start {
        claim_refund_start(deposit, transition, caller).map_err(map_refund_settlement_error)?
    } else {
        claim_job(storage::SettlementJobKind::Deposit, id, caller)
            .map_err(map_refund_settlement_error)?
    };
    scheduler::run_claimed(job)
        .await
        .map_err(map_refund_settlement_error)?;
    api::get_deposit(id.to_vec()).ok_or(Error::StorageFailure)
}

#[ic_cdk::query]
fn get_deposit_by_owner_sequence(
    owner: candid::Principal,
    owner_sequence: u64,
) -> Option<api::DepositView> {
    api::get_deposit_by_owner_sequence(owner, owner_sequence)
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
    if let Some(receipt) = api::existing_notified_withdrawal_by_hash(transaction_hash)? {
        return Ok(receipt);
    }
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?
            .ok_or(api::NotifyWithdrawalError::StorageFailure)
    })?;
    if !has_external_call_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.cycles_floor,
        config.settlement_cycle_ceiling,
    ) {
        return Err(api::NotifyWithdrawalError::InsufficientCycles);
    }
    let Some(_quota_guard) = NotificationQuotaGuard::acquire(caller) else {
        return Err(api::NotifyWithdrawalError::RateLimited);
    };
    let Some(notification_guard) =
        InFlightGuard::acquire(ActionKey::Notification(transaction_hash))
    else {
        return Err(api::NotifyWithdrawalError::Busy);
    };
    let now_ns = ic_cdk::api::time();
    let caller_count = NotificationAdmissionGuard::caller_count(
        caller,
        now_ns,
        config.notification_rate_limit_window_seconds,
    );
    let admitted = STORE
        .with(|store| {
            store.borrow_mut().consume_notification_verification_quota(
                now_ns,
                config.notification_rate_limit_window_seconds,
                config.notification_rate_limit_global,
                caller_count,
                NotificationAdmissionGuard::PER_CALLER_LIMIT,
            )
        })
        .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?;
    if !admitted {
        return Err(api::NotifyWithdrawalError::RateLimited);
    }
    NotificationAdmissionGuard::record(
        caller,
        now_ns,
        config.notification_rate_limit_window_seconds,
    );
    let receipt = api::notify_withdrawal(caller, args).await?;
    match &receipt {
        api::NotifyWithdrawalReceipt::Ingested { .. } => {}
        api::NotifyWithdrawalReceipt::Duplicate { .. } => return Ok(receipt),
    }
    drop(notification_guard);
    scheduler::arm();
    Ok(receipt)
}

fn has_external_call_cycle_budget(current: u128, floor: u128, call_ceiling: u128) -> bool {
    current > floor.saturating_add(call_ceiling)
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
    claim_job(kind, id, caller)
}

fn manual_claim_result(
    claim: storage::ManualSettlementClaim,
) -> Result<storage::SettlementJob, tasks::SettlementActionError> {
    match claim {
        storage::ManualSettlementClaim::Claimed(job) => Ok(job),
        storage::ManualSettlementClaim::AutomaticProgressPending { next_run_at_ns } => {
            Err(tasks::SettlementActionError::AutomaticProgressPending { next_run_at_ns })
        }
        storage::ManualSettlementClaim::Busy => Err(tasks::SettlementActionError::Busy),
    }
}

fn map_refund_settlement_error(
    error: tasks::SettlementActionError,
) -> api::RequestDepositRefundError {
    use api::RequestDepositRefundError as Error;
    match error {
        tasks::SettlementActionError::Busy => Error::Busy,
        tasks::SettlementActionError::AutomaticProgressPending { next_run_at_ns } => {
            Error::AutomaticProgressPending { next_run_at_ns }
        }
        tasks::SettlementActionError::RateLimited {
            retry_after_seconds,
        } => Error::RateLimited {
            retry_after_seconds,
        },
        _ => Error::StorageFailure,
    }
}

fn settlement_quota_limits(config: &config::BridgeInitArgs) -> storage::SettlementQuotaLimits {
    storage::SettlementQuotaLimits {
        window_seconds: config.settlement_rate_limit_window_seconds,
        global: config.settlement_rate_limit_global,
        per_principal: config.settlement_rate_limit_per_principal,
        per_record: config.settlement_rate_limit_per_record,
    }
}

fn claim_refund_start(
    deposit: bridge_core::DepositRecord,
    transition: bridge_core::ApplyResult,
    caller: candid::Principal,
) -> Result<storage::SettlementJob, tasks::SettlementActionError> {
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::StorageFailure)
    })?;
    let now_ns = ic_cdk::api::time();
    let context = storage::ManualSettlementClaimContext {
        kind: storage::SettlementJobKind::Deposit,
        settlement_id: deposit.id.bytes(),
        caller,
        now_ns,
        lease_until_ns: now_ns.saturating_add(120_000_000_000),
        overdue_after_ns: 300_000_000_000,
        limits: settlement_quota_limits(&config),
    };
    STORE.with(|store| {
        store
            .borrow_mut()
            .put_deposit_transition_and_claim_manual_job(&deposit, transition, context)
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
            .and_then(manual_claim_result)
    })
}

fn claim_job(
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
        let now_ns = ic_cdk::api::time();
        let limits = settlement_quota_limits(&config);
        let claim = store.borrow_mut().claim_manual_settlement_job(
            kind,
            id,
            caller,
            now_ns,
            now_ns.saturating_add(120_000_000_000),
            300_000_000_000,
            limits,
        );
        claim
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
            .and_then(manual_claim_result)
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
                storage_or_trap(
                    "nonterminal deposit count read",
                    store.nonterminal_deposit_count(),
                ),
                storage_or_trap(
                    "deposit funding reservation count read",
                    store.deposit_funding_reservation_count(),
                ),
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
            store.settlement_scheduler_health(),
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
            schema_version: store.schema_version(),
            mint_authorization_ttl_seconds: bridge_core::MINT_AUTHORIZATION_TTL_SECONDS,
            mint_authorization_epoch: storage_or_trap(
                "mint authorization epoch read",
                store.current_mint_authorization_epoch(),
            ),
            counts: StatusCounts {
                deposits: counts.deposits,
                withdrawals: counts.withdrawals,
                reconciliation_holds: counts.reconciliation_holds,
                pending_ledger_operations: counts.pending_ledger_operations,
                reserved_deposit_mint_amount: counts.reserved_deposit_mint_amount,
                reserved_deposit_mint_operations: counts.reserved_deposit_mint_operations,
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
                governance_eth_floor_wei: config.governance_eth_floor_wei,
                required_cycles: reserve.required_cycles,
                eth_surplus_wei: reserve.eth_surplus_wei,
                cycles_surplus: reserve.cycles_surplus,
                sufficient: reserve.sufficient,
            },
            deposits_paused: admin.deposits_paused,
            withdrawal_fee_guard_active: admin.withdrawal_fee_guard.is_some(),
            withdrawal_fee_guard_ledger_fee: admin
                .withdrawal_fee_guard
                .map(|guard| guard.ledger_fee),
            withdrawal_fee_guard_charged_service_fee: admin
                .withdrawal_fee_guard
                .map(|guard| guard.charged_service_fee),
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

fn require_controller() -> Result<(), StorageMaintenanceError> {
    let caller = ic_cdk::api::msg_caller();
    if ic_cdk::api::is_controller(&caller) {
        Ok(())
    } else {
        Err(StorageMaintenanceError::Unauthorized)
    }
}

#[ic_cdk::update]
fn start_storage_validation() -> Result<StorageValidationStatus, StorageMaintenanceError> {
    require_controller()?;
    STORE.with(|store| store.borrow().start_storage_validation())
}

#[ic_cdk::update]
fn continue_storage_validation(
    max_rows: u16,
) -> Result<StorageValidationStatus, StorageMaintenanceError> {
    require_controller()?;
    STORE.with(|store| store.borrow().continue_storage_validation(max_rows))
}

#[ic_cdk::query]
fn storage_integrity_check() -> Result<String, StorageMaintenanceError> {
    require_controller()?;
    STORE.with(|store| store.borrow().storage_integrity_check())
}

#[ic_cdk::update]
fn refresh_storage_checksum(
    max_bytes: u64,
) -> Result<ChecksumRefreshStatus, StorageMaintenanceError> {
    require_controller()?;
    STORE.with(|store| store.borrow_mut().refresh_storage_checksum(max_bytes))
}

#[cfg(feature = "test-deployment")]
#[ic_cdk::update]
fn seed_storage_test_data(start: u64, count: u16) -> Result<u16, StorageMaintenanceError> {
    require_controller()?;
    STORE.with(|store| store.borrow_mut().seed_storage_test_data(start, count))
}

#[cfg(feature = "test-deployment")]
#[ic_cdk::update]
fn profile_due_settlement_claim() -> Result<SettlementClaimProfile, StorageMaintenanceError> {
    require_controller()?;
    STORE.with(|store| store.borrow_mut().profile_due_settlement_claim())
}

#[cfg(feature = "test-deployment")]
#[ic_cdk::update]
fn profile_rejected_manual_settlement_claim(
    settlement_id: [u8; 32],
) -> Result<SettlementClaimProfile, StorageMaintenanceError> {
    require_controller()?;
    STORE.with(|store| {
        store
            .borrow_mut()
            .profile_rejected_manual_settlement_claim(settlement_id)
    })
}

#[ic_cdk::update]
async fn initialize_public_config() -> Result<(), PublicConfigInitializationError> {
    let caller = ic_cdk::api::msg_caller();
    if !ic_cdk::api::is_controller(&caller) {
        return Err(PublicConfigInitializationError::Unauthorized);
    }
    let initialized = STORE.with(|store| {
        let store = store.borrow();
        Ok::<_, PublicConfigInitializationError>(
            store
                .signer_address()
                .map_err(|_| PublicConfigInitializationError::StorageFailure)?
                .is_some()
                && store
                    .governance_operator_address()
                    .map_err(|_| PublicConfigInitializationError::StorageFailure)?
                    .is_some(),
        )
    })?;
    if initialized {
        return Ok(());
    }
    let Some(_guard) = InFlightGuard::acquire(ActionKey::PublicConfigInitialization) else {
        return Err(PublicConfigInitializationError::Busy);
    };
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| PublicConfigInitializationError::StorageFailure)?
            .ok_or(PublicConfigInitializationError::StorageFailure)
    })?;
    let expected_bridge_signer = signer::ethereum_address(&config)
        .await
        .map_err(|_| PublicConfigInitializationError::DerivationUnavailable)?;
    let governance_operator = signer::governance_operator_address(&config)
        .await
        .map_err(|_| PublicConfigInitializationError::DerivationUnavailable)?;
    STORE.with(|store| {
        store
            .borrow_mut()
            .initialize_chain_key_addresses(expected_bridge_signer, governance_operator)
            .map_err(|error| match error {
                StorageError::Core(bridge_core::CoreError::ConflictingReplay) => {
                    PublicConfigInitializationError::ConflictingAddress
                }
                _ => PublicConfigInitializationError::StorageFailure,
            })
    })
}

#[ic_cdk::query]
fn get_public_config() -> PublicConfig {
    let (config, admin) = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let admin = storage_or_trap("administrator state read", store.admin_state());
        (config, admin)
    });
    let (expected_bridge_signer, governance_operator) = STORE.with(|store| {
        let store = store.borrow();
        let expected_bridge_signer = storage_or_trap("signer address read", store.signer_address())
            .unwrap_or_else(|| ic_cdk::trap("public configuration is not initialized"));
        let governance_operator = storage_or_trap(
            "governance operator address read",
            store.governance_operator_address(),
        )
        .unwrap_or_else(|| ic_cdk::trap("public configuration is not initialized"));
        (expected_bridge_signer, governance_operator)
    });
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
            expected_bridge_runtime_sha256: config.expected_bridge_runtime_sha256,
            timelock_contract: config.timelock_contract,
            deployment_instance_id: config.deployment_instance_id,
            ledger_canister_id: config.ledger_canister_id,
            ledger_fee: ledger::KINIC_LEDGER_FEE.get(),
            index_canister_id: config.index_canister_id,
            schema_version: store.schema_version(),
            mint_authorization_ttl_seconds: bridge_core::MINT_AUTHORIZATION_TTL_SECONDS,
            mint_authorization_epoch: storage_or_trap(
                "mint authorization epoch read",
                store.current_mint_authorization_epoch(),
            ),
            expected_bridge_signer: Vec::from(expected_bridge_signer),
            governance_operator: Vec::from(governance_operator),
            evm_rpc_canister_id: config.evm_rpc_canister_id,
            rpc_provider_urls_sha256,
            deposit_rate_limit_window_seconds: config.deposit_rate_limit_window_seconds,
            deposit_rate_limit_global: config.deposit_rate_limit_global,
            deposit_rate_limit_per_principal: config.deposit_rate_limit_per_principal,
            notification_rate_limit_window_seconds: config.notification_rate_limit_window_seconds,
            notification_rate_limit_global: config.notification_rate_limit_global,
            notification_ingestion_rate_limit_global: config
                .notification_ingestion_rate_limit_global,
            settlement_rate_limit_window_seconds: config.settlement_rate_limit_window_seconds,
            settlement_rate_limit_global: config.settlement_rate_limit_global,
            settlement_rate_limit_per_principal: config.settlement_rate_limit_per_principal,
            settlement_rate_limit_per_record: config.settlement_rate_limit_per_record,
            settlement_retry_interval_seconds: config.settlement_retry_interval_seconds,
            governance_evm_fee: config.governance_evm_fee,
            governance_replacement: config.governance_replacement,
            governance_eth_floor_wei: config.governance_eth_floor_wei,
            cycles_floor: config.cycles_floor,
            settlement_cycle_ceiling: config.settlement_cycle_ceiling,
            governance_principal: admin.governance_principal,
            pause_principal: admin.pause_principal,
            fee_recipient: admin.fee_recipient,
        }
    })
}

#[ic_cdk::update]
async fn prepare_base_governance_action(
    action: base_governance::BaseGovernanceAction,
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    let caller = ic_cdk::api::msg_caller();
    base_governance::prepare(caller, action.into()).await
}

#[ic_cdk::update]
fn emergency_pause(
) -> Result<base_governance::EmergencyPauseReceipt, base_governance::BaseGovernanceError> {
    let Some(_guard) = InFlightGuard::acquire(ActionKey::EmergencyPause) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    base_governance::emergency_pause(ic_cdk::api::msg_caller())
}

#[ic_cdk::query]
fn get_pending_base_governance_transaction() -> Result<
    Option<base_governance::SignedBaseGovernanceTransaction>,
    base_governance::BaseGovernanceError,
> {
    base_governance::get_pending(ic_cdk::api::msg_caller())
}

#[ic_cdk::update]
async fn confirm_base_governance_transaction(
    args: base_governance::ConfirmBaseGovernanceTransactionArgs,
) -> Result<base_governance::BaseGovernanceConfirmation, base_governance::BaseGovernanceError> {
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    base_governance::confirm(ic_cdk::api::msg_caller(), args).await
}

#[ic_cdk::update]
async fn prepare_base_governance_replacement(
    args: base_governance::PrepareBaseGovernanceReplacementArgs,
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    base_governance::prepare_replacement(ic_cdk::api::msg_caller(), args).await
}

#[ic_cdk::update]
async fn prepare_next_emergency_base_action(
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    base_governance::prepare_next_emergency(ic_cdk::api::msg_caller()).await
}

#[ic_cdk::query]
fn get_activation_status(
) -> Result<base_governance::ActivationStatus, base_governance::BaseGovernanceError> {
    base_governance::activation_status()
}

#[ic_cdk::update]
async fn schedule_activation(
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    let caller = ic_cdk::api::msg_caller();
    base_governance::prepare(
        caller,
        base_governance::GovernanceAction::ScheduleActivation,
    )
    .await
}

#[ic_cdk::update]
async fn execute_activation(
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    let caller = ic_cdk::api::msg_caller();
    base_governance::prepare(caller, base_governance::GovernanceAction::ExecuteActivation).await
}

#[ic_cdk::query]
fn icrc10_supported_standards() -> Vec<consent::Icrc10SupportedStandard> {
    consent::supported_standards()
}

#[ic_cdk::update]
fn icrc21_canister_call_consent_message(
    request: consent::Icrc21ConsentMessageRequest,
) -> consent::Icrc21ConsentMessageResponse {
    let ledger_fee =
        if request.method == "request_deposit" || request.method == "request_deposit_refund" {
            Some(ledger::KINIC_LEDGER_FEE.get())
        } else {
            None
        };
    consent::consent_message(
        ic_cdk::api::msg_caller(),
        ic_cdk::api::canister_self(),
        request,
        ledger_fee,
    )
}

#[ic_cdk::update]
fn pause_new_deposits() -> Result<(), admin::AdminError> {
    admin::pause(ic_cdk::api::msg_caller())
}
#[ic_cdk::update]
fn rotate_pause_principal(args: admin::RotatePausePrincipalArgs) -> Result<(), admin::AdminError> {
    admin::rotate_pause_principal(ic_cdk::api::msg_caller(), args)
}
#[ic_cdk::update]
fn rotate_fee_recipient(args: config::FeeRecipientConfig) -> Result<(), admin::AdminError> {
    admin::rotate_fee_recipient(ic_cdk::api::msg_caller(), args)
}
#[ic_cdk::update]
fn request_fee_payout(amount: candid::Nat) -> Result<admin::FeePayoutReceipt, admin::AdminError> {
    let Some(_guard) = InFlightGuard::acquire(ActionKey::FeePayoutCreation) else {
        return Err(admin::AdminError::Busy);
    };
    let receipt = admin::request_fee_payout(ic_cdk::api::msg_caller(), amount)?;
    scheduler::arm();
    Ok(receipt)
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
    let Some(guard) = InFlightGuard::acquire(ActionKey::FeePayout(payout_id)) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    let terminal = STORE.with(|store| {
        store
            .borrow()
            .fee_payout(payout_id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .map(|record| match record.state {
                admin::FeePayoutState::Succeeded { .. } | admin::FeePayoutState::Failed => {
                    Some(record.state)
                }
                _ => None,
            })
            .ok_or(tasks::SettlementActionError::NotFound)
    })?;
    if let Some(state) = terminal {
        return Ok(tasks::FeePayoutActionResult::Complete { state });
    }
    drop(guard);
    let job = claim_manual_job(
        storage::SettlementJobKind::FeePayout,
        storage::fee_payout_job_id(payout_id),
        caller,
    )?;
    let result = scheduler::run_claimed_fee_payout(job).await?;
    scheduler::arm();
    Ok(result)
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
        has_external_call_cycle_budget, storage::StorageError, storage_or_trap, ActionKey,
        DefaultMemoryImpl, InFlightGuard, NotificationAdmissionGuard, StableStore,
        NOTIFICATION_CALLER_ADMISSION,
    };

    #[cfg(not(feature = "test-deployment"))]
    fn normalize(candid: &str) -> String {
        candid
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect()
    }

    #[cfg(not(feature = "test-deployment"))]
    #[test]
    fn checked_in_production_candid_matches_rust_interface() {
        let generated = super::__export_service();
        let checked_in = include_str!("../bridge.did");
        assert_eq!(normalize(&generated), normalize(checked_in));
        assert!(!generated.contains("refresh_base_observation"));
        assert!(!generated.contains("resume_new_deposits"));
        let normalized = normalize(&generated);
        assert!(normalized.contains("get_public_config:()->(PublicConfig)query;"));
        assert!(normalized.contains("initialize_public_config:()->("));
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
    fn storage_errors_are_not_converted_to_default_values() {
        let trapped = std::panic::catch_unwind(|| {
            storage_or_trap::<()>("test storage read", Err(StorageError::DecodeFailed));
        });
        assert!(trapped.is_err());
    }

    #[test]
    fn external_calls_require_budget_above_floor_and_call_ceiling() {
        assert!(!has_external_call_cycle_budget(150, 100, 50));
        assert!(has_external_call_cycle_budget(151, 100, 50));
        assert!(!has_external_call_cycle_budget(u128::MAX, u128::MAX, 1));
    }

    #[test]
    fn refund_request_identity_decision_preserves_endpoint_error_order() {
        use bridge_core::RefundRequestIdentityDecision as Decision;

        assert_eq!(
            bridge_core::refund_request_identity_decision(false, None),
            Decision::AnonymousCaller
        );
        assert_eq!(
            bridge_core::refund_request_identity_decision(true, None),
            Decision::OwnerLookupRequired
        );
        assert_eq!(
            bridge_core::refund_request_identity_decision(true, Some(false)),
            Decision::OwnerMismatch
        );
        assert_eq!(
            bridge_core::refund_request_identity_decision(true, Some(true)),
            Decision::Allow
        );
    }

    #[test]
    fn exact_mint_digest_mismatch_is_a_typed_identity_conflict() {
        assert_eq!(
            super::map_deposit_refund_exact_mint_error(
                super::evm_rpc::ObservationError::BaseStateMismatch,
            ),
            super::api::RequestDepositRefundError::DepositIdentityConflict
        );
    }

    #[test]
    #[serial_test::serial]
    fn notification_admission_persists_global_budget_and_keeps_caller_isolation() {
        NOTIFICATION_CALLER_ADMISSION.with(|buckets| buckets.borrow_mut().clear());
        let caller = candid::Principal::from_slice(&[1]);
        let memory = DefaultMemoryImpl::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize");
        for _ in 0..NotificationAdmissionGuard::PER_CALLER_LIMIT {
            let count = NotificationAdmissionGuard::caller_count(caller, 0, 600);
            assert!(store
                .consume_notification_verification_quota(0, 600, 60, count, 6)
                .expect("consume notification quota"));
            NotificationAdmissionGuard::record(caller, 0, 600);
        }
        assert!(!store
            .consume_notification_verification_quota(
                0,
                600,
                60,
                NotificationAdmissionGuard::caller_count(caller, 0, 600),
                6,
            )
            .expect("enforce caller quota"));
        drop(store);
        let mut reopened = StableStore::reopen(memory.clone()).expect("reopen");
        assert!(reopened
            .consume_notification_verification_quota(0, 600, 7, 0, 6)
            .expect("persisted global budget"));
        assert!(!reopened
            .consume_notification_verification_quota(0, 600, 7, 0, 6)
            .expect("enforce persisted global budget"));
        assert!(reopened
            .consume_notification_verification_quota(600 * 1_000_000_000, 600, 7, 0, 6)
            .expect("reset notification window"));
        assert!(reopened
            .consume_notification_ingestion_quota(600 * 1_000_000_000, 600, 1)
            .expect("consume canonical ingestion budget"));
        assert!(!reopened
            .consume_notification_ingestion_quota(600 * 1_000_000_000, 600, 1)
            .expect("enforce canonical ingestion budget"));
        drop(reopened);
        let mut reopened = StableStore::reopen(memory).expect("reopen ingestion budget");
        assert!(!reopened
            .consume_notification_ingestion_quota(600 * 1_000_000_000, 600, 1)
            .expect("persist canonical ingestion budget"));
        assert!(reopened
            .consume_notification_ingestion_quota(1_200 * 1_000_000_000, 600, 1)
            .expect("reset canonical ingestion window"));

        let hash = [9; 32];
        let guard = InFlightGuard::acquire(ActionKey::Notification(hash))
            .expect("first hash notification acquires");
        assert!(InFlightGuard::acquire(ActionKey::Notification(hash)).is_none());
        drop(guard);
        assert!(InFlightGuard::acquire(ActionKey::Notification(hash)).is_some());
    }
}
