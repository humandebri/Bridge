//! IC boundary and stable storage adapter for the KINIC–Base Bridge.
//!
//! This crate exposes the Candid boundary and connects the deterministic core to stable storage,
//! ICRC Ledger calls, EVM RPC observation, threshold ECDSA signing, scheduled settlement, and
//! runtime administration.

use candid::{CandidType, Deserialize};
use ic_stable_structures::DefaultMemoryImpl;
use std::cell::{Cell, RefCell};

mod admin;
mod api;
pub mod config;
mod evm_rpc;
mod ledger;
mod signer;
pub mod storage;
mod tasks;

use storage::{StableStore, SCHEMA_VERSION};

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
    pub pending_ledger_operations: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reverted_evm_operations: u64,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeStatus {
    pub schema_version: u16,
    pub counts: StatusCounts,
    pub withdrawal_log_cursor: u64,
    pub last_finalized_base_block: u64,
    pub last_safe_base_block: u64,
    pub last_reserve_observation_ns: u64,
    pub last_finalized_observation_ns: u64,
    pub last_safe_observation_ns: u64,
    pub reserve: ReserveStatus,
    pub deposits_paused: bool,
    pub queued_evm_operations: u64,
    pub safe_evm_operations: u64,
    pub last_audit_sequence: Option<u64>,
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

thread_local! {
    static STORE: RefCell<StableStore<DefaultMemoryImpl>> = RefCell::new(
        StableStore::init(DefaultMemoryImpl::default()).unwrap_or_else(|error| {
            ic_cdk::trap(format!("stable state initialization failed: {error}"))
        })
    );
    static TIMER_TICK_RUNNING: Cell<bool> = const { Cell::new(false) };
}

struct TimerTickGuard;

impl TimerTickGuard {
    fn acquire() -> Option<Self> {
        TIMER_TICK_RUNNING.with(|running| {
            if running.replace(true) {
                None
            } else {
                Some(Self)
            }
        })
    }
}

impl Drop for TimerTickGuard {
    fn drop(&mut self) {
        TIMER_TICK_RUNNING.with(|running| running.set(false));
    }
}

#[ic_cdk::init]
fn init(args: config::BridgeInitArgs) {
    args.validate().unwrap_or_else(|error| ic_cdk::trap(error));
    ensure_supported_schema();
    STORE.with(|store| {
        store
            .borrow_mut()
            .set_config_once(&args)
            .unwrap_or_else(|error| ic_cdk::trap(format!("configuration write failed: {error}")));
        store
            .borrow_mut()
            .initialize_admin(&args)
            .unwrap_or_else(|error| {
                ic_cdk::trap(format!("administrator initialization failed: {error}"))
            });
    });
    schedule_timer(args.poll_interval_seconds);
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    ensure_supported_schema();
    let interval = STORE.with(|store| {
        store
            .borrow()
            .config()
            .ok()
            .flatten()
            .map(|config| config.poll_interval_seconds)
    });
    if let Some(interval) = interval {
        schedule_timer(interval);
    }
}

fn schedule_timer(seconds: u64) {
    ic_cdk_timers::set_timer_interval(std::time::Duration::from_secs(seconds), || async {
        timer_tick().await;
    });
}

async fn timer_tick() {
    let Some(_guard) = TimerTickGuard::acquire() else {
        return;
    };
    // Asset-moving work is serialized per canister. Individual adapters still compare stable
    // state before committing so retries from other entry points cannot create duplicate work.
    tasks::tick().await;
}

#[ic_cdk::update]
async fn request_deposit(args: api::DepositArgs) -> Result<api::DepositReceipt, api::DepositError> {
    api::request_deposit(ic_cdk::api::msg_caller(), args).await
}

#[ic_cdk::query]
fn get_deposit(id: Vec<u8>) -> Option<api::DepositView> {
    api::get_deposit(id)
}

#[ic_cdk::query]
fn get_withdrawal(id: Vec<u8>) -> Option<api::WithdrawalView> {
    api::get_withdrawal(id)
}

fn ensure_supported_schema() {
    STORE.with(|store| {
        if store.borrow().schema_version() != SCHEMA_VERSION {
            ic_cdk::trap("unsupported stable schema version");
        }
    });
}

#[ic_cdk::query]
fn get_bridge_status() -> BridgeStatus {
    STORE.with(|store| {
        let store = store.borrow();
        let counts = store
            .status_counts()
            .unwrap_or_else(|error| ic_cdk::trap(format!("stable state read failed: {error}")));
        let config = store
            .config()
            .ok()
            .flatten()
            .unwrap_or_else(|| ic_cdk::trap("missing configuration"));
        let progress = store.external_progress().unwrap_or_default();
        let reserve = config
            .reserve_policy()
            .snapshot(
                store.nonterminal_withdrawal_count().unwrap_or(0),
                progress.last_eth_balance_wei,
                ic_cdk::api::canister_liquid_cycle_balance(),
            )
            .unwrap_or_else(|_| ic_cdk::trap("reserve arithmetic overflow"));
        let admin = store
            .admin_state()
            .unwrap_or_else(|_| ic_cdk::trap("missing administrator state"));
        BridgeStatus {
            schema_version: store.schema_version(),
            counts: StatusCounts {
                deposits: counts.deposits,
                withdrawals: counts.withdrawals,
                pending_evm_operations: counts.pending_evm_operations,
                reconciliation_holds: counts.reconciliation_holds,
                pending_ledger_operations: counts.pending_ledger_operations,
                reserved_deposit_mint_amount: counts.reserved_deposit_mint_amount,
                reverted_evm_operations: counts.reverted_evm_operations,
            },
            withdrawal_log_cursor: counts.withdrawal_log_cursor,
            last_finalized_base_block: counts.last_finalized_base_block,
            last_safe_base_block: progress.last_safe_base_block,
            last_reserve_observation_ns: progress.last_reserve_observation_ns,
            last_finalized_observation_ns: progress.last_finalized_observation_ns,
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
            queued_evm_operations: store.queued_evm_count().unwrap_or(0),
            safe_evm_operations: store.safe_evm_count(),
            last_audit_sequence: store.last_audit_sequence().unwrap_or(None),
        }
    })
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
    admin::request_fee_payout(ic_cdk::api::msg_caller(), amount).await
}
#[ic_cdk::query]
fn get_audit_events(start: u64, limit: u16) -> Result<Vec<storage::AuditEvent>, admin::AdminError> {
    admin::audit_events(start, limit)
}

ic_cdk::export_candid!();

#[cfg(test)]
mod candid_tests {
    use super::TimerTickGuard;

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
    fn timer_tick_guard_rejects_overlap_and_reopens_after_drop() {
        let guard = TimerTickGuard::acquire().expect("first tick acquires guard");
        assert!(TimerTickGuard::acquire().is_none());
        drop(guard);
        assert!(TimerTickGuard::acquire().is_some());
    }
}
