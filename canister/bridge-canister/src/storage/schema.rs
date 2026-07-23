use ic_sqlite_vfs::MemoryId;

pub const SCHEMA_VERSION: u16 = 20;
pub(super) const WIRE_VERSION: u8 = 16;

pub const RETIRED_STABLE_STRUCTURE_MEMORY_IDS: core::ops::RangeInclusive<u8> = 0..=32;
pub const SQLITE_MEMORY_ID: MemoryId = MemoryId::new(120);

pub(super) const VALIDATION_TABLES: &[&str] = &[
    "deposits",
    "withdrawals",
    "evm_operations",
    "reconciliation_holds",
    "evm_execution_payloads",
    "reconciliation_scans",
    "audit_events",
    "fee_payouts",
    "deposit_owner_index",
    "fee_payout_state_index",
    "operation_owner_index",
    "evm_state_index",
    "pull_pending_deposit_index",
    "release_pending_withdrawal_index",
    "open_hold_index",
    "owner_deposit_sequences",
    "withdrawal_liability_index",
    "withdrawal_notification_index",
    "withdrawal_stop_reason_counts",
    "settlement_jobs",
];
