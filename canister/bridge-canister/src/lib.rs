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
    pub audit_retention_warning: bool,
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
    pub cycles_balance: u128,
    pub required_cycles: u128,
    pub cycles_surplus: u128,
    pub sufficient: bool,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct RuntimeBinding {
    pub base_chain_id: u64,
    pub bridge_contract: Vec<u8>,
    pub expected_bridge_runtime_sha256: Vec<u8>,
    pub timelock_contract: Vec<u8>,
    pub deployment_instance_id: Vec<u8>,
    pub minimum_withdrawal_id: Vec<u8>,
    pub ledger_canister_id: candid::Principal,
    pub index_canister_id: candid::Principal,
    pub schema_version: u16,
    pub expected_bridge_signer: Vec<u8>,
    pub evm_rpc_canister_id: candid::Principal,
    pub rpc_provider_urls_sha256: Vec<u8>,
    pub operational_config_sha256: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ControlPlaneAddressesView {
    pub bridge_signer: Vec<u8>,
    pub governance_operator: Vec<u8>,
    pub runtime_administrator: Vec<u8>,
    pub independent_canceller: Vec<u8>,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlPlaneAddressesError {
    Uninitialized,
    StorageFailure,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct OperationalConfig {
    pub mint_authorization_ttl_seconds: u64,
    pub mint_authorization_epoch: u64,
    pub governance_operator: Vec<u8>,
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
    pub cycles_floor: u128,
    pub settlement_cycle_ceiling: u128,
    pub governance_principal: candid::Principal,
    pub pause_principal: candid::Principal,
    pub confirmation_relayer_principal: candid::Principal,
    pub fee_recipient: config::FeeRecipientConfig,
}

#[derive(CandidType)]
struct OperationalConfigBinding {
    ledger_fee: u128,
    operational_config: OperationalConfig,
}

const OPERATIONAL_CONFIG_BINDING_DOMAIN: &[u8] = b"KINIC_OPERATIONAL_CONFIG_BINDING_V1\0";

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationalConfigError {
    Unauthorized,
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

    fn acquire(caller: Principal, protected_lane: bool) -> Option<Self> {
        NOTIFICATIONS_IN_FLIGHT.with(|global| {
            NOTIFICATION_CALLERS.with(|callers| {
                let mut global = global.borrow_mut();
                let mut callers = callers.borrow_mut();
                let caller_count = callers.get(&caller).copied().unwrap_or(0);
                let lane_limit = if protected_lane {
                    Self::GLOBAL_LIMIT
                } else {
                    Self::GLOBAL_LIMIT.saturating_sub(2)
                };
                if *global >= lane_limit || caller_count >= Self::PER_CALLER_LIMIT {
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
    #[cfg(not(feature = "test-deployment"))]
    args.validate_production_bootstrap_operational_config()
        .unwrap_or_else(|error| ic_cdk::trap(error));
    let store =
        StableStore::init_configured(DefaultMemoryImpl::default(), &args).unwrap_or_else(|error| {
            ic_cdk::trap(format!("stable state initialization failed: {error}"))
        });
    install_store(store);
    scheduler::arm();
    scheduler::arm_funding_recovery();
}

pub(crate) fn current_asset_operation_lifecycle_decision(
) -> Result<bridge_core::AssetOperationLifecycleDecision, StorageError> {
    STORE.with(|store| {
        let sealed = store.borrow().operational_config_sealed()?;
        Ok(::bridge_core::kernel::asset_operation_lifecycle_decision(
            sealed,
        ))
    })
}

fn asset_operations_are_available() -> Result<bool, StorageError> {
    asset_operations_are_available_for(current_asset_operation_lifecycle_decision())
}

fn asset_operations_are_available_for(
    decision: Result<bridge_core::AssetOperationLifecycleDecision, StorageError>,
) -> Result<bool, StorageError> {
    decision.map(|decision| {
        matches!(
            decision,
            bridge_core::AssetOperationLifecycleDecision::Allow
        )
    })
}

fn require_asset_operations_for_deposit() -> Result<(), api::DepositError> {
    match asset_operations_are_available() {
        Ok(true) => Ok(()),
        Ok(false) => Err(api::DepositError::DepositsPaused),
        Err(_) => Err(api::DepositError::StorageFailure),
    }
}

fn require_asset_operations_for_refund() -> Result<(), api::RequestDepositRefundError> {
    match asset_operations_are_available() {
        Ok(true) => Ok(()),
        Ok(false) => Err(api::RequestDepositRefundError::NotClaimable),
        Err(_) => Err(api::RequestDepositRefundError::StorageFailure),
    }
}

fn require_asset_operations_for_withdrawal_notification() -> Result<(), api::NotifyWithdrawalError>
{
    match asset_operations_are_available() {
        Ok(true) => Ok(()),
        Ok(false) => Err(api::NotifyWithdrawalError::BaseStateMismatch),
        Err(_) => Err(api::NotifyWithdrawalError::StorageFailure),
    }
}

fn require_asset_operations_for_settlement() -> Result<(), tasks::SettlementActionError> {
    match asset_operations_are_available() {
        Ok(true) => Ok(()),
        Ok(false) => Err(tasks::SettlementActionError::WrongState),
        Err(_) => Err(tasks::SettlementActionError::StorageFailure),
    }
}

fn require_asset_operations_for_fee_payout() -> Result<(), admin::AdminError> {
    match asset_operations_are_available() {
        Ok(true) => Ok(()),
        Ok(false) => Err(admin::AdminError::Busy),
        Err(_) => Err(admin::AdminError::StorageFailure),
    }
}

#[cfg(not(feature = "test-deployment"))]
fn reopen_store_after_upgrade() -> StableStore {
    StableStore::reopen_after_upgrade(DefaultMemoryImpl::default())
        .unwrap_or_else(|error| ic_cdk::trap(format!("stable state reopen failed: {error}")))
}

fn finish_post_upgrade(store: StableStore) {
    install_store(store);
    ensure_supported_schema();
    STORE.with(|store| {
        let config = store
            .borrow()
            .config()
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!("stable configuration read failed: {error}"))
            })
            .unwrap_or_else(|| ic_cdk::trap("missing stable configuration"));
        config
            .validate()
            .unwrap_or_else(|error| ic_cdk::trap(format!("invalid stable configuration: {error}")));
    });
    scheduler::arm();
    scheduler::arm_funding_recovery();
}

#[cfg(not(feature = "test-deployment"))]
#[ic_cdk::post_upgrade]
fn post_upgrade() {
    finish_post_upgrade(reopen_store_after_upgrade());
}

#[cfg(feature = "test-deployment")]
fn decode_staging_upgrade_args(bytes: Vec<u8>) -> config::StagingUpgradeArgs {
    if bytes == candid::encode_args(()).expect("empty Candid tuple encoding must succeed") {
        return config::StagingUpgradeArgs::default();
    }
    candid::decode_args::<(config::StagingUpgradeArgs,)>(&bytes)
        .map(|(args,)| args)
        .unwrap_or_else(|error| {
            ic_cdk::trap(format!("staging upgrade argument decode failed: {error}"))
        })
}

#[cfg(feature = "test-deployment")]
fn apply_staging_rpc_provider_update(
    store: &mut StableStore,
    args: &config::StagingUpgradeArgs,
) -> Result<(), String> {
    if args.status_counts_guard_version != 1 {
        return Err("unsupported staging status count guard version".into());
    }
    if let Some(minimum_withdrawal_id) = args.minimum_withdrawal_id.as_ref() {
        store
            .set_staging_minimum_withdrawal_id_once(minimum_withdrawal_id)
            .map_err(|error| format!("staging withdrawal admission boundary failed: {error}"))?;
    }
    let Some(update) = args.rpc_provider_update.as_ref() else {
        return Ok(());
    };
    let counts = store
        .status_counts()
        .map_err(|error| format!("staging status count read failed: {error}"))?;
    if !counts.matches_staging_expected_status_counts(&update.expected_status_counts) {
        return Err("staging status counts do not match the reviewed policy".into());
    }
    let current = store
        .config()
        .map_err(|error| format!("staging configuration read failed: {error}"))?
        .ok_or_else(|| "missing staging configuration".to_owned())?;
    if let Some(next) = current
        .staging_rpc_replacement(&update.custom_evm_rpc_urls)
        .map_err(str::to_owned)?
    {
        store
            .apply_staging_rpc_replacement(&next)
            .map_err(|error| format!("staging RPC replacement failed: {error}"))?;
    }
    Ok(())
}

#[cfg(feature = "test-deployment")]
fn validate_staging_upgrade_status_counts(
    store: &StableStore,
    args: &config::StagingUpgradeArgs,
) -> Result<(), String> {
    if args.status_counts_guard_version != 1 {
        return Err("unsupported staging status count guard version".into());
    }
    let Some(expected) = args.expected_status_counts.as_ref() else {
        return Ok(());
    };
    let counts = store
        .status_counts()
        .map_err(|error| format!("staging status count read failed: {error}"))?;
    if !counts.matches_staging_expected_status_counts(expected) {
        return Err("staging status counts do not match the reviewed preflight snapshot".into());
    }
    Ok(())
}

#[cfg(feature = "test-deployment")]
#[ic_cdk::post_upgrade(decode_with = "decode_staging_upgrade_args")]
fn post_upgrade(args: config::StagingUpgradeArgs) {
    let mut store = storage_or_trap(
        "stable state reopen",
        StableStore::reopen_after_staging_upgrade(
            DefaultMemoryImpl::default(),
            args.confirmation_relayer_principal,
        ),
    );
    validate_staging_upgrade_status_counts(&store, &args)
        .unwrap_or_else(|error| ic_cdk::trap(error));
    apply_staging_rpc_provider_update(&mut store, &args)
        .unwrap_or_else(|error| ic_cdk::trap(error));
    finish_post_upgrade(store);
}

#[ic_cdk::update]
async fn request_deposit(args: api::DepositArgs) -> Result<api::DepositReceipt, api::DepositError> {
    require_asset_operations_for_deposit()?;
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

    require_asset_operations_for_refund()?;

    let caller = ic_cdk::api::msg_caller();
    match ::bridge_core::kernel::refund_request_identity_decision(
        caller != candid::Principal::anonymous(),
    ) {
        bridge_core::RefundRequestIdentityDecision::Allow => {}
        bridge_core::RefundRequestIdentityDecision::AnonymousCaller => {
            return Err(Error::AnonymousCaller);
        }
    }
    let id: [u8; 32] = deposit_id
        .as_slice()
        .try_into()
        .map_err(|_| Error::InvalidDepositId)?;
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| Error::StorageFailure)?
            .ok_or(Error::StorageFailure)
    })?;
    let reserve_token = STORE
        .with(|store| {
            let store = store.borrow();
            Ok::<_, storage::StorageError>((
                store.deposit_reserve_token()?,
                store.deposit_funding_reservation_count()?,
            ))
        })
        .map_err(|_| Error::StorageFailure)?;
    if !has_liability_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.reserve_policy(),
        reserve_token.0,
        reserve_token.1,
        1,
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
    let mut prepaid_quota = None;
    match state {
        bridge_core::DepositState::Minted { .. } | bridge_core::DepositState::Refunded { .. } => {
            return Err(Error::NotClaimable);
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
            prepaid_quota = Some(reserve_refund_rpc_quota(&config, id, caller)?);
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
                prepaid_quota = Some(reserve_refund_rpc_quota(&config, id, caller)?);
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
                        return Err(Error::NotClaimable);
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
        claim_refund_start(deposit, transition, caller, prepaid_quota)
            .map_err(map_refund_settlement_error)?
    } else {
        claim_job(
            storage::SettlementJobKind::Deposit,
            id,
            caller,
            prepaid_quota,
        )
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
fn list_nonterminal_deposit_refs(
    args: api::ListDepositIdsArgs,
) -> Result<api::NonterminalDepositRefPage, api::ListDepositIdsError> {
    api::list_nonterminal_deposit_refs(args)
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
    require_asset_operations_for_withdrawal_notification()?;
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
    let now_ns = ic_cdk::api::time();
    let protected_lane = caller == config.confirmation_relayer_principal;
    if STORE
        .with(|store| {
            store
                .borrow()
                .notification_failure_cooldown_active(transaction_hash, now_ns)
        })
        .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?
    {
        return Err(api::NotifyWithdrawalError::RateLimited);
    }
    let reserve_token = STORE
        .with(|store| {
            let store = store.borrow();
            Ok::<_, storage::StorageError>((
                store.deposit_reserve_token()?,
                store.deposit_funding_reservation_count()?,
            ))
        })
        .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?;
    if !has_notification_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.reserve_policy(),
        reserve_token.0,
        reserve_token.1,
    ) {
        return Err(api::NotifyWithdrawalError::InsufficientCycles);
    }
    let Some(_quota_guard) = NotificationQuotaGuard::acquire(caller, protected_lane) else {
        return Err(api::NotifyWithdrawalError::RateLimited);
    };
    let Some(notification_guard) =
        InFlightGuard::acquire(ActionKey::Notification(transaction_hash))
    else {
        return Err(api::NotifyWithdrawalError::Busy);
    };
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
                protected_lane,
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
    let receipt = match api::notify_withdrawal(caller, args).await {
        Ok(receipt) => receipt,
        Err(error) => {
            STORE
                .with(|store| {
                    store.borrow_mut().record_notification_failure_cooldown(
                        transaction_hash,
                        ic_cdk::api::time(),
                        30_000_000_000,
                    )
                })
                .map_err(|_| api::NotifyWithdrawalError::StorageFailure)?;
            return Err(error);
        }
    };
    match &receipt {
        api::NotifyWithdrawalReceipt::Ingested { .. } => {}
        api::NotifyWithdrawalReceipt::Duplicate { .. } => return Ok(receipt),
    }
    drop(notification_guard);
    Ok(receipt)
}

fn admit_control_plane_external_call() -> Result<InFlightGuard, base_governance::BaseGovernanceError>
{
    let Some(guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| base_governance::BaseGovernanceError::StorageFailure)?
            .ok_or(base_governance::BaseGovernanceError::StorageFailure)
    })?;
    let (token, active_funding) = STORE
        .with(|store| {
            let store = store.borrow();
            Ok::<_, storage::StorageError>((
                store.deposit_reserve_token()?,
                store.deposit_funding_reservation_count()?,
            ))
        })
        .map_err(|_| base_governance::BaseGovernanceError::StorageFailure)?;
    if !has_liability_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.reserve_policy(),
        token,
        active_funding,
        1,
    ) {
        return Err(base_governance::BaseGovernanceError::InsufficientCycles);
    }
    let admitted = STORE
        .with(|store| {
            store.borrow_mut().consume_notification_verification_quota(
                ic_cdk::api::time(),
                config.notification_rate_limit_window_seconds,
                config.notification_rate_limit_global,
                0,
                NotificationAdmissionGuard::PER_CALLER_LIMIT,
                true,
            )
        })
        .map_err(|_| base_governance::BaseGovernanceError::StorageFailure)?;
    if !admitted {
        return Err(base_governance::BaseGovernanceError::RateLimited);
    }
    Ok(guard)
}

fn has_notification_cycle_budget(
    current: u128,
    policy: bridge_core::ReservePolicy,
    token: storage::DepositReserveToken,
    active_funding: u64,
) -> bool {
    // One slot pays the verification fan-out and a distinct slot remains for
    // settlement of the newly ingested withdrawal liability.
    has_liability_cycle_budget(current, policy, token, active_funding, 2)
}

fn has_liability_cycle_budget(
    current: u128,
    policy: bridge_core::ReservePolicy,
    token: storage::DepositReserveToken,
    active_funding: u64,
    candidate_calls: u64,
) -> bool {
    token
        .nonterminal_deposits
        .checked_add(active_funding)
        .and_then(|deposits| {
            policy
                .required_cycles(token.nonterminal_withdrawals, deposits, candidate_calls)
                .ok()
        })
        .is_some_and(|required| current > required)
}

fn can_continue_withdrawal(caller: candid::Principal) -> bool {
    caller != candid::Principal::anonymous()
}

fn deposit_continuation_authorization_phase(state: &bridge_core::DepositState) -> bool {
    match state {
        bridge_core::DepositState::EscrowedUnquoted { .. }
        | bridge_core::DepositState::AuthorizationPending { .. } => true,
        bridge_core::DepositState::FundingPending
        | bridge_core::DepositState::AuthorizationAvailable { .. }
        | bridge_core::DepositState::RefundAvailable { .. }
        | bridge_core::DepositState::Minted { .. }
        | bridge_core::DepositState::FundingReconciliationHold { .. }
        | bridge_core::DepositState::RefundPending { .. }
        | bridge_core::DepositState::RefundReconciliationHold { .. }
        | bridge_core::DepositState::Refunded { .. }
        | bridge_core::DepositState::Cancelled { .. } => false,
    }
}

fn deposit_continuation_retryable_stop(reason: Option<&tasks::SettlementStopReason>) -> bool {
    match reason {
        Some(
            tasks::SettlementStopReason::RpcUnavailable
            | tasks::SettlementStopReason::RpcInconsistent
            | tasks::SettlementStopReason::SigningUnavailable,
        ) => true,
        None
        | Some(
            tasks::SettlementStopReason::LedgerUnavailable
            | tasks::SettlementStopReason::LedgerAmbiguous
            | tasks::SettlementStopReason::LedgerRejected(_)
            | tasks::SettlementStopReason::InvalidBaseResponse
            | tasks::SettlementStopReason::AuthorizationExpired
            | tasks::SettlementStopReason::AuthorizationWindowTooShort
            | tasks::SettlementStopReason::BaseStateMismatch
            | tasks::SettlementStopReason::BridgeSignerMismatch
            | tasks::SettlementStopReason::LedgerFeeExceedsServiceFee
            | tasks::SettlementStopReason::Unknown(_),
        ) => false,
    }
}

#[ic_cdk::update]
async fn continue_deposit(
    deposit_id: Vec<u8>,
) -> Result<tasks::SettlementActionResult, tasks::SettlementActionError> {
    require_asset_operations_for_settlement()?;
    let id = deposit_id
        .as_slice()
        .try_into()
        .map_err(|_| tasks::SettlementActionError::InvalidId)?;
    let caller = ic_cdk::api::msg_caller();
    if matches!(
        ::bridge_core::kernel::deposit_continuation_decision(
            caller != candid::Principal::anonymous(),
            false,
            false,
        ),
        bridge_core::DepositContinuationDecision::AnonymousCaller
    ) {
        return Err(tasks::SettlementActionError::AnonymousCaller);
    }
    let Some(guard) = InFlightGuard::acquire(ActionKey::Deposit(id)) else {
        return Err(tasks::SettlementActionError::Busy);
    };
    let decision = STORE.with(|store| {
        let record = store
            .borrow()
            .deposit(id)
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::NotFound)?;
        let reason = record
            .last_settlement_stop_reason
            .map(tasks::settlement_stop_reason_from_text);
        Ok::<_, tasks::SettlementActionError>(::bridge_core::kernel::deposit_continuation_decision(
            true,
            deposit_continuation_authorization_phase(&record.state),
            deposit_continuation_retryable_stop(reason.as_ref()),
        ))
    })?;
    match decision {
        bridge_core::DepositContinuationDecision::Allow => {}
        bridge_core::DepositContinuationDecision::AnonymousCaller => {
            return Err(tasks::SettlementActionError::AnonymousCaller);
        }
        bridge_core::DepositContinuationDecision::WrongState => {
            return Err(tasks::SettlementActionError::WrongState);
        }
    }
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::StorageFailure)
    })?;
    let reserve_token = STORE
        .with(|store| {
            let store = store.borrow();
            Ok::<_, storage::StorageError>((
                store.deposit_reserve_token()?,
                store.deposit_funding_reservation_count()?,
            ))
        })
        .map_err(|_| tasks::SettlementActionError::StorageFailure)?;
    if !has_liability_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.reserve_policy(),
        reserve_token.0,
        reserve_token.1,
        1,
    ) {
        return Err(tasks::SettlementActionError::InsufficientCycles);
    }
    drop(guard);
    let job = claim_manual_job(storage::SettlementJobKind::Deposit, id, caller)?;
    scheduler::run_claimed(job).await
}

#[ic_cdk::update]
async fn continue_withdrawal(
    withdrawal_id: Vec<u8>,
) -> Result<tasks::SettlementActionResult, tasks::SettlementActionError> {
    require_asset_operations_for_settlement()?;
    let id = withdrawal_id
        .as_slice()
        .try_into()
        .map_err(|_| tasks::SettlementActionError::InvalidId)?;
    let caller = ic_cdk::api::msg_caller();
    if !can_continue_withdrawal(caller) {
        return Err(tasks::SettlementActionError::AnonymousCaller);
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
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::StorageFailure)
    })?;
    let reserve_token = STORE
        .with(|store| {
            let store = store.borrow();
            Ok::<_, storage::StorageError>((
                store.deposit_reserve_token()?,
                store.deposit_funding_reservation_count()?,
            ))
        })
        .map_err(|_| tasks::SettlementActionError::StorageFailure)?;
    if !has_liability_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.reserve_policy(),
        reserve_token.0,
        reserve_token.1,
        1,
    ) {
        return Err(tasks::SettlementActionError::InsufficientCycles);
    }
    drop(guard);
    let job = claim_manual_job(storage::SettlementJobKind::Withdrawal, id, caller)?;
    scheduler::run_claimed(job).await
}

fn claim_manual_job(
    kind: storage::SettlementJobKind,
    id: [u8; 32],
    caller: candid::Principal,
) -> Result<storage::SettlementJob, tasks::SettlementActionError> {
    claim_job(kind, id, caller, None)
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
        tasks::SettlementActionError::InsufficientCycles => Error::InsufficientCycles,
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

fn reserve_refund_rpc_quota(
    config: &config::BridgeInitArgs,
    id: [u8; 32],
    caller: candid::Principal,
) -> Result<storage::PrepaidQuota, api::RequestDepositRefundError> {
    use api::RequestDepositRefundError as Error;

    STORE.with(|store| {
        store
            .borrow_mut()
            .reserve_settlement_quota(
                storage::SettlementJobKind::Deposit,
                id,
                caller,
                ic_cdk::api::time(),
                settlement_quota_limits(config),
            )
            .map_err(|error| match error {
                storage::SettlementAdmissionError::RateLimited {
                    retry_after_seconds,
                } => Error::RateLimited {
                    retry_after_seconds,
                },
                storage::SettlementAdmissionError::Storage => Error::StorageFailure,
            })
    })
}

fn claim_refund_start(
    deposit: bridge_core::DepositRecord,
    transition: bridge_core::ApplyResult,
    caller: candid::Principal,
    prepaid_quota: Option<storage::PrepaidQuota>,
) -> Result<storage::SettlementJob, tasks::SettlementActionError> {
    let config = STORE.with(|store| {
        store
            .borrow()
            .config()
            .map_err(|_| tasks::SettlementActionError::StorageFailure)?
            .ok_or(tasks::SettlementActionError::StorageFailure)
    })?;
    let now_ns = ic_cdk::api::time();
    let context = match prepaid_quota {
        Some(prepaid) => storage::ManualSettlementClaimContext::prepaid(
            storage::SettlementJobKind::Deposit,
            deposit.id.bytes(),
            caller,
            now_ns,
            now_ns.saturating_add(120_000_000_000),
            300_000_000_000,
            prepaid,
        ),
        None => storage::ManualSettlementClaimContext::new(
            storage::SettlementJobKind::Deposit,
            deposit.id.bytes(),
            caller,
            now_ns,
            now_ns.saturating_add(120_000_000_000),
            300_000_000_000,
            settlement_quota_limits(&config),
        ),
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
    prepaid_quota: Option<storage::PrepaidQuota>,
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
        let claim = match prepaid_quota {
            Some(prepaid) => store.borrow_mut().claim_prepaid_manual_settlement_job(
                kind,
                id,
                caller,
                now_ns,
                now_ns.saturating_add(120_000_000_000),
                300_000_000_000,
                prepaid,
            ),
            None => store.borrow_mut().claim_manual_settlement_job(
                kind,
                id,
                caller,
                now_ns,
                now_ns.saturating_add(120_000_000_000),
                300_000_000_000,
                limits,
            ),
        };
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
            audit_retention_warning: storage::audit_retention_warning(counts.retained_audit_events),
            last_finalized_base_block: finalized_observation
                .map(|observation| observation.block_number)
                .unwrap_or_default(),
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
                cycles_balance: reserve.cycles_balance,
                required_cycles: reserve.required_cycles,
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
                    .is_some()
                && store
                    .runtime_administrator_address()
                    .map_err(|_| PublicConfigInitializationError::StorageFailure)?
                    .is_some()
                && store
                    .independent_canceller_address()
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
    let runtime_administrator = signer::runtime_administrator_address(&config)
        .await
        .map_err(|_| PublicConfigInitializationError::DerivationUnavailable)?;
    let independent_canceller = signer::canceller_address(&config)
        .await
        .map_err(|_| PublicConfigInitializationError::DerivationUnavailable)?;
    if !ic_cdk::api::is_controller(&caller) {
        return Err(PublicConfigInitializationError::Unauthorized);
    }
    STORE.with(|store| {
        store
            .borrow_mut()
            .initialize_chain_key_addresses(
                expected_bridge_signer,
                governance_operator,
                runtime_administrator,
                independent_canceller,
            )
            .map_err(|error| match error {
                StorageError::Core(bridge_core::CoreError::ConflictingReplay) => {
                    PublicConfigInitializationError::ConflictingAddress
                }
                _ => PublicConfigInitializationError::StorageFailure,
            })
    })
}

#[ic_cdk::query]
fn get_runtime_binding() -> RuntimeBinding {
    let config = STORE.with(|store| {
        let store = store.borrow();
        storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"))
    });
    let expected_bridge_signer = STORE.with(|store| {
        let store = store.borrow();
        storage_or_trap("signer address read", store.signer_address())
            .unwrap_or_else(|| ic_cdk::trap("runtime binding is not initialized"))
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
    let operational_config_sha256 = operational_config_sha256(&current_operational_config());
    STORE.with(|store| {
        let store = store.borrow();
        RuntimeBinding {
            base_chain_id: config.base_chain_id,
            bridge_contract: config.bridge_contract,
            expected_bridge_runtime_sha256: config.expected_bridge_runtime_sha256,
            timelock_contract: config.timelock_contract,
            deployment_instance_id: config.deployment_instance_id,
            minimum_withdrawal_id: config.minimum_withdrawal_id,
            ledger_canister_id: config.ledger_canister_id,
            index_canister_id: config.index_canister_id,
            schema_version: store.schema_version(),
            expected_bridge_signer: Vec::from(expected_bridge_signer),
            evm_rpc_canister_id: config.evm_rpc_canister_id,
            rpc_provider_urls_sha256,
            operational_config_sha256,
        }
    })
}

#[ic_cdk::query]
fn get_control_plane_addresses() -> Result<ControlPlaneAddressesView, ControlPlaneAddressesError> {
    STORE.with(|store| {
        let store = store.borrow();
        let bridge_signer = store
            .signer_address()
            .map_err(|_| ControlPlaneAddressesError::StorageFailure)?
            .ok_or(ControlPlaneAddressesError::Uninitialized)?;
        let governance_operator = store
            .governance_operator_address()
            .map_err(|_| ControlPlaneAddressesError::StorageFailure)?
            .ok_or(ControlPlaneAddressesError::Uninitialized)?;
        let runtime_administrator = store
            .runtime_administrator_address()
            .map_err(|_| ControlPlaneAddressesError::StorageFailure)?
            .ok_or(ControlPlaneAddressesError::Uninitialized)?;
        let independent_canceller = store
            .independent_canceller_address()
            .map_err(|_| ControlPlaneAddressesError::StorageFailure)?
            .ok_or(ControlPlaneAddressesError::Uninitialized)?;
        Ok(ControlPlaneAddressesView {
            bridge_signer: bridge_signer.to_vec(),
            governance_operator: governance_operator.to_vec(),
            runtime_administrator: runtime_administrator.to_vec(),
            independent_canceller: independent_canceller.to_vec(),
        })
    })
}

fn current_operational_config() -> OperationalConfig {
    let (config, admin) = STORE.with(|store| {
        let store = store.borrow();
        let config = storage_or_trap("configuration read", store.config())
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let admin = storage_or_trap("administrator state read", store.admin_state());
        (config, admin)
    });
    let governance_operator = STORE.with(|store| {
        storage_or_trap(
            "governance operator address read",
            store.borrow().governance_operator_address(),
        )
        .unwrap_or_else(|| ic_cdk::trap("operational configuration is not initialized"))
    });
    STORE.with(|store| OperationalConfig {
        mint_authorization_ttl_seconds: bridge_core::MINT_AUTHORIZATION_TTL_SECONDS,
        mint_authorization_epoch: storage_or_trap(
            "mint authorization epoch read",
            store.borrow().current_mint_authorization_epoch(),
        ),
        governance_operator: Vec::from(governance_operator),
        deposit_rate_limit_window_seconds: config.deposit_rate_limit_window_seconds,
        deposit_rate_limit_global: config.deposit_rate_limit_global,
        deposit_rate_limit_per_principal: config.deposit_rate_limit_per_principal,
        notification_rate_limit_window_seconds: config.notification_rate_limit_window_seconds,
        notification_rate_limit_global: config.notification_rate_limit_global,
        notification_ingestion_rate_limit_global: config.notification_ingestion_rate_limit_global,
        settlement_rate_limit_window_seconds: config.settlement_rate_limit_window_seconds,
        settlement_rate_limit_global: config.settlement_rate_limit_global,
        settlement_rate_limit_per_principal: config.settlement_rate_limit_per_principal,
        settlement_rate_limit_per_record: config.settlement_rate_limit_per_record,
        settlement_retry_interval_seconds: config.settlement_retry_interval_seconds,
        governance_evm_fee: config.governance_evm_fee,
        governance_replacement: config.governance_replacement,
        cycles_floor: config.cycles_floor,
        settlement_cycle_ceiling: config.settlement_cycle_ceiling,
        governance_principal: admin.governance_principal,
        pause_principal: admin.pause_principal,
        confirmation_relayer_principal: config.confirmation_relayer_principal,
        fee_recipient: admin.fee_recipient,
    })
}

fn operational_config_sha256(config: &OperationalConfig) -> Vec<u8> {
    let binding = OperationalConfigBinding {
        ledger_fee: ledger::KINIC_LEDGER_FEE.get(),
        operational_config: config.clone(),
    };
    let encoded = candid::encode_one(binding).unwrap_or_else(|error| {
        ic_cdk::trap(format!("operational config encoding failed: {error}"))
    });
    let mut digest = Sha256::new();
    digest.update(OPERATIONAL_CONFIG_BINDING_DOMAIN);
    digest.update(encoded);
    digest.finalize().to_vec()
}

#[ic_cdk::query]
fn get_operational_config() -> Result<OperationalConfig, OperationalConfigError> {
    let caller = ic_cdk::api::msg_caller();
    if caller == candid::Principal::anonymous()
        || (!ic_cdk::api::is_controller(&caller) && !admin::is_governance(caller).unwrap_or(false))
    {
        return Err(OperationalConfigError::Unauthorized);
    }
    Ok(current_operational_config())
}

#[ic_cdk::update]
async fn prepare_base_governance_action(
    action: base_governance::BaseGovernanceAction,
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    base_governance::require_operational_config_sealed()?;
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    let caller = ic_cdk::api::msg_caller();
    base_governance::prepare(caller, action.into()).await
}

#[ic_cdk::update]
async fn seal_operational_config(
    args: config::OperationalConfigArgs,
) -> Result<base_governance::OperationalConfigSealReceipt, base_governance::BaseGovernanceError> {
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    base_governance::seal_operational_config(ic_cdk::api::msg_caller(), args).await
}

#[ic_cdk::update]
async fn refresh_activation_attestation(
) -> Result<config::ActivationAttestation, base_governance::BaseGovernanceError> {
    base_governance::require_operational_config_sealed()?;
    let caller = ic_cdk::api::msg_caller();
    base_governance::require_attestation_refresh_caller(caller)?;
    let now_ns = ic_cdk::api::time();
    if base_governance::activation_attestation()
        .is_ok_and(|attestation| now_ns < attestation.observed_at_ns.saturating_add(30_000_000_000))
    {
        return Err(base_governance::BaseGovernanceError::RateLimited);
    }
    let _guard = admit_control_plane_external_call()?;
    base_governance::refresh_activation_attestation(caller).await
}

#[ic_cdk::query]
fn get_activation_attestation(
) -> Result<config::ActivationAttestation, base_governance::BaseGovernanceError> {
    base_governance::activation_attestation()
}

#[ic_cdk::query]
fn get_production_lifecycle(
) -> Result<base_governance::ProductionLifecycle, base_governance::BaseGovernanceError> {
    base_governance::production_lifecycle()
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
    Vec<base_governance::SignedBaseGovernanceTransaction>,
    base_governance::BaseGovernanceError,
> {
    base_governance::get_pending()
}

#[ic_cdk::update]
async fn confirm_base_governance_transaction(
    args: base_governance::ConfirmBaseGovernanceTransactionArgs,
) -> Result<base_governance::BaseGovernanceConfirmation, base_governance::BaseGovernanceError> {
    base_governance::require_operational_config_sealed()?;
    let caller = ic_cdk::api::msg_caller();
    base_governance::require_confirmation_caller(caller)?;
    let transaction_hash: [u8; 32] = args
        .transaction_hash
        .as_slice()
        .try_into()
        .map_err(|_| base_governance::BaseGovernanceError::InvalidArgument)?;
    if STORE
        .with(|store| {
            store
                .borrow()
                .notification_failure_cooldown_active(transaction_hash, ic_cdk::api::time())
        })
        .map_err(|_| base_governance::BaseGovernanceError::StorageFailure)?
    {
        return Err(base_governance::BaseGovernanceError::RateLimited);
    }
    let _guard = admit_control_plane_external_call()?;
    let result = base_governance::confirm(caller, args).await;
    if matches!(
        &result,
        Err(base_governance::BaseGovernanceError::TransactionNotFinalized { .. })
            | Err(base_governance::BaseGovernanceError::ObservationUnavailable)
    ) {
        STORE
            .with(|store| {
                store.borrow_mut().record_notification_failure_cooldown(
                    transaction_hash,
                    ic_cdk::api::time(),
                    30_000_000_000,
                )
            })
            .map_err(|_| base_governance::BaseGovernanceError::StorageFailure)?;
    }
    result
}

#[ic_cdk::update]
async fn prepare_base_governance_replacement(
    args: base_governance::PrepareBaseGovernanceReplacementArgs,
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    base_governance::require_operational_config_sealed()?;
    let Some(_guard) = InFlightGuard::acquire(ActionKey::BaseGovernance) else {
        return Err(base_governance::BaseGovernanceError::Busy { operation_id: 0 });
    };
    base_governance::prepare_replacement(ic_cdk::api::msg_caller(), args).await
}

#[ic_cdk::update]
async fn prepare_next_emergency_base_action(
) -> Result<base_governance::SignedBaseGovernanceTransaction, base_governance::BaseGovernanceError>
{
    base_governance::require_operational_config_sealed()?;
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
    base_governance::require_operational_config_sealed()?;
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
    base_governance::require_operational_config_sealed()?;
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
    if !admit_consent_request() {
        return consent::resource_limited();
    }
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

fn admit_consent_request() -> bool {
    if !asset_operations_are_available().unwrap_or(false) {
        return false;
    }
    let Some(config) = STORE.with(|store| store.borrow().config().ok().flatten()) else {
        return false;
    };
    let Ok((token, active_funding)) = STORE.with(|store| {
        let store = store.borrow();
        Ok::<_, storage::StorageError>((
            store.deposit_reserve_token()?,
            store.deposit_funding_reservation_count()?,
        ))
    }) else {
        return false;
    };
    if !has_liability_cycle_budget(
        ic_cdk::api::canister_liquid_cycle_balance(),
        config.reserve_policy(),
        token,
        active_funding,
        0,
    ) {
        return false;
    }
    STORE
        .with(|store| {
            store.borrow_mut().consume_consent_quota(
                ic_cdk::api::time(),
                config.notification_rate_limit_window_seconds,
                config.notification_rate_limit_global.saturating_mul(2),
            )
        })
        .unwrap_or(false)
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
    require_asset_operations_for_fee_payout()?;
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
    require_asset_operations_for_settlement()?;
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
        asset_operations_are_available_for, can_continue_withdrawal,
        deposit_continuation_authorization_phase, deposit_continuation_retryable_stop,
        has_notification_cycle_budget, storage::DepositReserveToken, storage::StorageError,
        storage_or_trap, ActionKey, DefaultMemoryImpl, InFlightGuard, NotificationAdmissionGuard,
        StableStore, NOTIFICATION_CALLER_ADMISSION,
    };

    #[test]
    fn public_asset_adapters_reject_bootstrap_and_allow_sealed_lifecycle() {
        assert_eq!(
            asset_operations_are_available_for(Ok(
                bridge_core::AssetOperationLifecycleDecision::OperationalConfigNotSealed,
            )),
            Ok(false)
        );
        assert_eq!(
            asset_operations_are_available_for(Ok(
                bridge_core::AssetOperationLifecycleDecision::Allow,
            )),
            Ok(true)
        );
    }

    #[cfg(not(feature = "test-deployment"))]
    #[test]
    fn operational_config_binding_matches_the_release_profile_vector() {
        let mut governance_operator = vec![0; 20];
        governance_operator[19] = 3;
        let config = super::OperationalConfig {
            mint_authorization_ttl_seconds: 600,
            mint_authorization_epoch: 7,
            governance_operator,
            deposit_rate_limit_window_seconds: 60,
            deposit_rate_limit_global: 30,
            deposit_rate_limit_per_principal: 3,
            notification_rate_limit_window_seconds: 600,
            notification_rate_limit_global: 60,
            notification_ingestion_rate_limit_global: 30,
            settlement_rate_limit_window_seconds: 600,
            settlement_rate_limit_global: 60,
            settlement_rate_limit_per_principal: 6,
            settlement_rate_limit_per_record: 3,
            settlement_retry_interval_seconds: 60,
            governance_evm_fee: super::config::EvmFeePolicy {
                gas_limit_ceiling: 100_000,
                max_fee_per_gas_ceiling: 200,
                max_priority_fee_per_gas_ceiling: 10,
                l1_fee_per_transaction_ceiling_wei: 100,
                quote_validity_seconds: 90,
                gas_limit_multiplier_bps: 13_000,
                base_fee_multiplier_bps: 60_000,
                l1_fee_multiplier_bps: 15_000,
            },
            governance_replacement: super::config::GovernanceReplacementPolicy {
                max_replacements: 3,
                fee_bump_bps: 1_250,
            },
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: candid::Principal::from_text("74ncn-fqaaa-aaaaq-aaasa-cai")
                .unwrap(),
            pause_principal: candid::Principal::self_authenticating([2; 32]),
            confirmation_relayer_principal: candid::Principal::self_authenticating([8; 32]),
            fee_recipient: super::config::FeeRecipientConfig {
                owner: candid::Principal::self_authenticating([4; 32]),
                subaccount: Vec::new(),
            },
        };
        assert_eq!(
            super::operational_config_sha256(&config),
            [
                0xef, 0xe2, 0x86, 0x2b, 0x6c, 0xfb, 0xa2, 0x8a, 0xce, 0x6b, 0x50, 0x08, 0x22, 0x19,
                0x61, 0x55, 0x6a, 0x75, 0x49, 0x58, 0x82, 0x69, 0x00, 0x1b, 0xb5, 0xa1, 0x59, 0x04,
                0xa5, 0xd5, 0xf0, 0xc3,
            ]
        );
    }

    #[cfg(feature = "test-deployment")]
    #[test]
    fn staging_upgrade_decoder_accepts_empty_and_guarded_rpc_update_args() {
        let empty = candid::encode_args(()).expect("encode empty upgrade args");
        assert_eq!(
            super::decode_staging_upgrade_args(empty),
            super::config::StagingUpgradeArgs::default()
        );

        let expected = super::config::StagingUpgradeArgs {
            status_counts_guard_version: 1,
            expected_status_counts: Some(super::config::StagingExpectedStatusCounts {
                retained_audit_events: 11,
                reconciliation_holds: 12,
                retained_deposit_index_entries: 13,
                pending_ledger_operations: 14,
                withdrawals: 15,
                deposits: 16,
                reserved_deposit_mint_operations: 17,
                reserved_deposit_mint_amount: 18,
                pruned_audit_events: 19,
            }),
            minimum_withdrawal_id: None,
            confirmation_relayer_principal: Some(candid::Principal::from_slice(&[9])),
            rpc_provider_update: Some(super::config::StagingRpcProviderUpdate {
                custom_evm_rpc_urls: super::config::STAGING_NEW_RPC_URLS
                    .map(str::to_owned)
                    .to_vec(),
                expected_status_counts: super::config::StagingExpectedStatusCounts {
                    retained_audit_events: 1,
                    reconciliation_holds: 2,
                    retained_deposit_index_entries: 3,
                    pending_ledger_operations: 4,
                    withdrawals: 5,
                    deposits: 6,
                    reserved_deposit_mint_operations: 7,
                    reserved_deposit_mint_amount: 8,
                    pruned_audit_events: 9,
                },
            }),
        };
        let encoded = candid::encode_args((expected.clone(),)).expect("encode staging args");
        assert_eq!(super::decode_staging_upgrade_args(encoded), expected);
    }

    #[cfg(feature = "test-deployment")]
    #[test]
    fn staging_upgrade_decoder_rejects_unguarded_rpc_update_args() {
        #[derive(candid::CandidType)]
        struct UnguardedUpgradeArgs {
            rpc_provider_update: Option<UnguardedRpcProviderUpdate>,
        }

        #[derive(candid::CandidType)]
        struct UnguardedRpcProviderUpdate {
            custom_evm_rpc_urls: Vec<String>,
        }

        let encoded = candid::encode_args((UnguardedUpgradeArgs {
            rpc_provider_update: Some(UnguardedRpcProviderUpdate {
                custom_evm_rpc_urls: super::config::STAGING_NEW_RPC_URLS
                    .map(str::to_owned)
                    .to_vec(),
            }),
        },))
        .expect("encode unguarded staging args");
        assert!(candid::decode_args::<(super::config::StagingUpgradeArgs,)>(&encoded).is_err());
    }

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
        assert!(normalized.contains("get_runtime_binding:()->(RuntimeBinding)query;"));
        assert!(normalized.contains("get_control_plane_addresses:()->("));
        assert!(normalized.contains("get_operational_config:()->("));
        assert!(!normalized.contains("get_public_config"));
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
    #[serial_test::serial]
    fn busy_control_plane_admission_returns_before_quota_or_cycle_reads() {
        let guard = InFlightGuard::acquire(ActionKey::BaseGovernance)
            .expect("first control-plane call acquires guard");
        assert!(matches!(
            super::admit_control_plane_external_call(),
            Err(super::base_governance::BaseGovernanceError::Busy { operation_id: 0 })
        ));
        drop(guard);
    }

    #[test]
    fn storage_errors_are_not_converted_to_default_values() {
        let trapped = std::panic::catch_unwind(|| {
            storage_or_trap::<()>("test storage read", Err(StorageError::DecodeFailed));
        });
        assert!(trapped.is_err());
    }

    #[test]
    fn new_notifications_preserve_cycles_for_every_existing_asset_liability() {
        let policy = bridge_core::ReservePolicy {
            cycles_floor: 100,
            settlement_cycle_ceiling: 10,
        };
        let token = DepositReserveToken {
            nonterminal_withdrawals: 2,
            nonterminal_deposits: 3,
            reserved_deposit_mint_amount: 500,
            reserved_deposit_mint_operations: 3,
        };
        // 2 withdrawals + 3 formal deposits + 1 funding attempt + distinct
        // verification and newly-ingested withdrawal settlement slots.
        assert!(!has_notification_cycle_budget(180, policy, token, 1));
        assert!(has_notification_cycle_budget(181, policy, token, 1));
    }

    #[test]
    fn withdrawal_continuation_rejects_only_anonymous_callers() {
        assert!(!can_continue_withdrawal(candid::Principal::anonymous()));
        assert!(can_continue_withdrawal(
            candid::Principal::self_authenticating([1; 32])
        ));
    }

    #[test]
    fn deposit_continuation_classification_is_fail_closed() {
        use super::tasks::SettlementStopReason as Reason;
        use bridge_core::DepositContinuationDecision as Decision;

        let pending = bridge_core::DepositState::AuthorizationPending {
            funding_ledger_block_index: 1,
        };
        assert!(deposit_continuation_authorization_phase(&pending));
        assert!(!deposit_continuation_authorization_phase(
            &bridge_core::DepositState::AuthorizationAvailable {
                funding_ledger_block_index: 1,
            }
        ));
        for reason in [
            Reason::RpcUnavailable,
            Reason::RpcInconsistent,
            Reason::SigningUnavailable,
        ] {
            assert!(deposit_continuation_retryable_stop(Some(&reason)));
            assert_eq!(
                ::bridge_core::kernel::deposit_continuation_decision(true, true, true),
                Decision::Allow
            );
            assert_eq!(
                ::bridge_core::kernel::deposit_continuation_decision(false, true, true),
                Decision::AnonymousCaller
            );
        }
        for reason in [
            Reason::LedgerUnavailable,
            Reason::LedgerAmbiguous,
            Reason::LedgerRejected("rejected".to_owned()),
            Reason::AuthorizationExpired,
            Reason::InvalidBaseResponse,
            Reason::BaseStateMismatch,
            Reason::BridgeSignerMismatch,
            Reason::LedgerFeeExceedsServiceFee,
            Reason::Unknown("future stop".to_owned()),
        ] {
            assert!(!deposit_continuation_retryable_stop(Some(&reason)));
        }
        assert!(!deposit_continuation_retryable_stop(None));
        assert_eq!(
            ::bridge_core::kernel::deposit_continuation_decision(true, false, true),
            Decision::WrongState
        );
        assert_eq!(
            ::bridge_core::kernel::deposit_continuation_decision(true, true, false),
            Decision::WrongState
        );
    }

    #[test]
    fn refund_request_identity_decision_accepts_any_authenticated_caller() {
        use bridge_core::RefundRequestIdentityDecision as Decision;

        assert_eq!(
            bridge_core::refund_request_identity_decision(false),
            Decision::AnonymousCaller
        );
        assert_eq!(
            bridge_core::refund_request_identity_decision(true),
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
                .consume_notification_verification_quota(0, 600, 60, count, 6, false)
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
                false,
            )
            .expect("enforce caller quota"));
        drop(store);
        let mut reopened = StableStore::reopen(memory.clone()).expect("reopen");
        assert!(!reopened
            .consume_notification_verification_quota(0, 600, 7, 0, 6, false)
            .expect("public lane remains exhausted"));
        for _ in 0..6 {
            assert!(reopened
                .consume_notification_verification_quota(0, 600, 7, 0, 6, true)
                .expect("protected relayer slot remains available"));
        }
        assert!(!reopened
            .consume_notification_verification_quota(0, 600, 7, 0, 6, true)
            .expect("enforce protected lane limit"));
        assert!(reopened
            .consume_notification_verification_quota(600 * 1_000_000_000, 600, 7, 0, 6, false,)
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
