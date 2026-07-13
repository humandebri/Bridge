//! IC boundary and stable storage adapter for the KINIC–Base Bridge.
//!
//! Phase 2 intentionally exposes observation only. Asset-moving update methods and every external
//! call remain absent until their async saga and authorization boundaries are implemented.

use candid::{CandidType, Deserialize};
use ic_stable_structures::DefaultMemoryImpl;
use std::cell::RefCell;

pub mod config;
pub mod storage;

use storage::{StableStore, SCHEMA_VERSION};

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
}

#[derive(CandidType, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BridgeStatus {
    pub schema_version: u16,
    pub counts: StatusCounts,
}

thread_local! {
    static STORE: RefCell<StableStore<DefaultMemoryImpl>> = RefCell::new(
        StableStore::init(DefaultMemoryImpl::default()).unwrap_or_else(|error| {
            ic_cdk::trap(format!("stable state initialization failed: {error}"))
        })
    );
}

#[ic_cdk::init]
fn init() {
    ensure_supported_schema();
}

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    ensure_supported_schema();
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
        BridgeStatus {
            schema_version: store.schema_version(),
            counts: StatusCounts {
                deposits: counts.deposits,
                withdrawals: counts.withdrawals,
                pending_evm_operations: counts.pending_evm_operations,
                reconciliation_holds: counts.reconciliation_holds,
            },
        }
    })
}

ic_cdk::export_candid!();

#[cfg(test)]
mod candid_tests {
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
}
