mod admission;
mod evm;
mod schema;
mod settlement;
mod transaction;
mod validation;

use admission::consume_deposit_quota;
pub use admission::{
    DepositAdmissionOutcome, DepositQuotaAdmission, DepositRecoveryAdmission,
    DepositReserveAdmission,
};
use evm::*;
pub use schema::{RETIRED_STABLE_STRUCTURE_MEMORY_IDS, SCHEMA_VERSION, SQLITE_MEMORY_ID};
use schema::{VALIDATION_TABLES, WIRE_VERSION};
pub(crate) use settlement::{fee_payout_id_from_job, fee_payout_job_id};
pub use settlement::{
    ConfirmationSchedule, SettlementAdmissionError, SettlementJobKind, SettlementQuotaLimits,
};
use transaction::*;
use validation::expect_row_shape;

use crate::admin::AdminState;
use crate::config::{BridgeInitArgs, FeeRecipientConfig, ImmutableBridgeConfig};
use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, AccountingState, Amount, ApplyResult,
    BaseMintSnapshot, CoreError, DepositHoldResolution, DepositId, DepositRecord, EvmCallIntent,
    EvmOperationEvent, EvmOperationKind, EvmOperationRecord, EvmOperationState,
    EvmTransactionEnvelope, ExternalProgress, FeeKind, FinalizedObservationRecord, HoldId,
    LedgerTransferIdentity, ReconciliationHoldRecord, ReconciliationHoldState,
    ReconciliationScanProgress, ReconciliationTarget, WithdrawalEvent, WithdrawalHoldResolution,
    WithdrawalId, WithdrawalRecord, WithdrawalState,
};
use candid::{CandidType, Principal};
use ic_sqlite_vfs::db::migrate::Migration;
use ic_sqlite_vfs::db::{ChecksumRefresh, UpdateConnection, Value};
#[cfg(test)]
use ic_sqlite_vfs::MemoryId;
use ic_sqlite_vfs::{params, DbError, DbHandle, DefaultMemoryImpl, MemoryManager};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};
use std::{fmt, io::Cursor, marker::PhantomData, ops::Bound as RangeBound, ops::RangeBounds};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum OperationBundleFailpoint {
    Encode,
    ExecutionPayload,
    EvmOperation,
    EvmStateIndex,
    OperationOwnerIndex,
    Parent,
    ParentIndex,
    ReconciliationHold,
    OpenHoldIndex,
    ReconciliationScan,
    SingletonState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum TerminalBundleFailpoint {
    Parent,
    ParentIndex,
    EvmOperation,
    EvmStateIndex,
    OperationOwnerIndex,
    ConfirmationSchedule,
    Audit,
    SingletonState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum HoldBundleFailpoint {
    Encode,
    Parent,
    ParentIndex,
    Hold,
    OpenHoldIndex,
    SingletonState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum ResolveHoldBundleFailpoint {
    Encode,
    Parent,
    ParentIndex,
    Hold,
    OpenHoldIndex,
    ReconciliationScan,
    SingletonState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum FeePayoutBundleFailpoint {
    Encode,
    Record,
    Job,
    ReconciliationScan,
    Audit,
    SingletonState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum RecordWriteFailpoint {
    Encode,
    RemoveIndex,
    AddIndex,
    OperationOwner,
    Record,
    SingletonState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum RpcAtomicFailpoint {
    Business,
    Audit,
    Singleton,
}

#[cfg(test)]
thread_local! {
    static OPERATION_BUNDLE_FAILPOINT: std::cell::Cell<Option<OperationBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static TERMINAL_BUNDLE_FAILPOINT: std::cell::Cell<Option<TerminalBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static HOLD_BUNDLE_FAILPOINT: std::cell::Cell<Option<HoldBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static RESOLVE_HOLD_BUNDLE_FAILPOINT: std::cell::Cell<Option<ResolveHoldBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static FEE_PAYOUT_BUNDLE_FAILPOINT: std::cell::Cell<Option<FeePayoutBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static RECORD_WRITE_FAILPOINT: std::cell::Cell<Option<RecordWriteFailpoint>> = const { std::cell::Cell::new(None) };
    static RPC_ATOMIC_FAILPOINT: std::cell::Cell<Option<RpcAtomicFailpoint>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn set_rpc_atomic_failpoint(value: Option<RpcAtomicFailpoint>) {
    RPC_ATOMIC_FAILPOINT.with(|slot| slot.set(value));
}

fn rpc_atomic_db_failpoint(point: RpcAtomicFailpoint) -> Result<(), DbError> {
    #[cfg(test)]
    if RPC_ATOMIC_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(DbError::Constraint("test RPC atomic failpoint".into()));
    }
    let _ = point;
    Ok(())
}

#[cfg(test)]
fn set_record_write_failpoint(value: Option<RecordWriteFailpoint>) {
    RECORD_WRITE_FAILPOINT.with(|slot| slot.set(value));
}

fn record_write_storage_failpoint(point: RecordWriteFailpoint) -> Result<(), StorageError> {
    #[cfg(test)]
    if RECORD_WRITE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(StorageError::EncodeFailed);
    }
    let _ = point;
    Ok(())
}

fn record_write_db_failpoint(point: RecordWriteFailpoint) -> Result<(), DbError> {
    #[cfg(test)]
    if RECORD_WRITE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(DbError::Constraint("test record write failpoint".into()));
    }
    let _ = point;
    Ok(())
}

#[cfg(test)]
fn set_fee_payout_bundle_failpoint(value: Option<FeePayoutBundleFailpoint>) {
    FEE_PAYOUT_BUNDLE_FAILPOINT.with(|slot| slot.set(value));
}

fn fee_payout_bundle_storage_failpoint(
    point: FeePayoutBundleFailpoint,
) -> Result<(), StorageError> {
    #[cfg(test)]
    if FEE_PAYOUT_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(StorageError::EncodeFailed);
    }
    let _ = point;
    Ok(())
}

fn fee_payout_bundle_db_failpoint(point: FeePayoutBundleFailpoint) -> Result<(), DbError> {
    #[cfg(test)]
    if FEE_PAYOUT_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(DbError::Constraint(
            "test fee payout bundle failpoint".into(),
        ));
    }
    let _ = point;
    Ok(())
}

#[cfg(test)]
fn set_operation_bundle_failpoint(value: Option<OperationBundleFailpoint>) {
    OPERATION_BUNDLE_FAILPOINT.with(|slot| slot.set(value));
}

fn operation_bundle_storage_failpoint(point: OperationBundleFailpoint) -> Result<(), StorageError> {
    #[cfg(test)]
    if OPERATION_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(StorageError::EncodeFailed);
    }
    let _ = point;
    Ok(())
}

fn operation_bundle_db_failpoint(point: OperationBundleFailpoint) -> Result<(), DbError> {
    #[cfg(test)]
    if OPERATION_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(DbError::Constraint(
            "test operation bundle failpoint".into(),
        ));
    }
    let _ = point;
    Ok(())
}

#[cfg(test)]
fn set_terminal_bundle_failpoint(value: Option<TerminalBundleFailpoint>) {
    TERMINAL_BUNDLE_FAILPOINT.with(|slot| slot.set(value));
}

fn terminal_bundle_db_failpoint(point: TerminalBundleFailpoint) -> Result<(), DbError> {
    #[cfg(test)]
    if TERMINAL_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(DbError::Constraint("test terminal bundle failpoint".into()));
    }
    let _ = point;
    Ok(())
}

#[cfg(test)]
fn set_hold_bundle_failpoint(value: Option<HoldBundleFailpoint>) {
    HOLD_BUNDLE_FAILPOINT.with(|slot| slot.set(value));
}

fn hold_bundle_storage_failpoint(point: HoldBundleFailpoint) -> Result<(), StorageError> {
    #[cfg(test)]
    if HOLD_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(StorageError::EncodeFailed);
    }
    let _ = point;
    Ok(())
}

fn hold_bundle_db_failpoint(point: HoldBundleFailpoint) -> Result<(), DbError> {
    #[cfg(test)]
    if HOLD_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(DbError::Constraint("test hold bundle failpoint".into()));
    }
    let _ = point;
    Ok(())
}

#[cfg(test)]
fn set_resolve_hold_bundle_failpoint(value: Option<ResolveHoldBundleFailpoint>) {
    RESOLVE_HOLD_BUNDLE_FAILPOINT.with(|slot| slot.set(value));
}

fn resolve_hold_bundle_storage_failpoint(
    point: ResolveHoldBundleFailpoint,
) -> Result<(), StorageError> {
    #[cfg(test)]
    if RESOLVE_HOLD_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(StorageError::EncodeFailed);
    }
    let _ = point;
    Ok(())
}

fn resolve_hold_bundle_db_failpoint(point: ResolveHoldBundleFailpoint) -> Result<(), DbError> {
    #[cfg(test)]
    if RESOLVE_HOLD_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
        return Err(DbError::Constraint(
            "test resolve hold bundle failpoint".into(),
        ));
    }
    let _ = point;
    Ok(())
}

const MAX_STABLE_VALUE_BYTES: usize = 16 * 1024;
const MAX_AUDIT_EVENTS: u64 = 10_000;
const MAX_AUDIT_BATCH: usize = 32;
const MAX_OWNER_DEPOSIT_INDEX_ENTRIES: usize = 100;
pub const MAX_VALIDATION_ROWS: u16 = 100;
pub const MAX_CHECKSUM_REFRESH_BYTES: u64 = 4 * 1024 * 1024;
const AUDIT_DIGEST_DOMAIN: &[u8] = b"KINIC_BRIDGE_AUDIT_V1";
const SQLITE_SCHEMA: &str = r#"
CREATE TABLE bridge_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    application_schema_version INTEGER NOT NULL,
    record_wire_version INTEGER NOT NULL
) STRICT;
INSERT INTO bridge_metadata VALUES (1, 22, 18);

CREATE TABLE singleton_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    accounting BLOB NOT NULL,
    counters BLOB NOT NULL,
    external_progress BLOB NOT NULL,
    config BLOB NOT NULL,
    admin_state BLOB NOT NULL,
    deposit_admission BLOB NOT NULL,
    audit_retention BLOB NOT NULL,
    settlement_admission BLOB NOT NULL,
    confirmation_scheduler_health BLOB NOT NULL,
    storage_revision BLOB NOT NULL CHECK (length(storage_revision) = 8),
    withdrawal_liability_amount BLOB NOT NULL CHECK (length(withdrawal_liability_amount) = 16),
    storage_validation BLOB
) STRICT;
CREATE TABLE table_counts (
    name TEXT PRIMARY KEY NOT NULL,
    count BLOB NOT NULL CHECK (length(count) = 8)
) STRICT, WITHOUT ROWID;

CREATE TABLE deposits (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE withdrawals (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE evm_operations (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE reconciliation_holds (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE evm_execution_payloads (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE reconciliation_scans (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE audit_events (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE fee_payouts (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE deposit_owner_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE operation_owner_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE evm_state_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE pull_pending_deposit_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE release_pending_withdrawal_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE open_hold_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE owner_deposit_sequences (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE withdrawal_liability_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE withdrawal_notification_index (
    key BLOB PRIMARY KEY NOT NULL CHECK (length(key) = 32),
    value BLOB NOT NULL CHECK (length(value) = 32)
) STRICT, WITHOUT ROWID;
CREATE TABLE withdrawal_stop_reason_counts (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL CHECK (length(value) = 8)) STRICT, WITHOUT ROWID;
CREATE TABLE settlement_job_status_counts (
    status INTEGER PRIMARY KEY CHECK (status IN (0, 1, 2, 3)),
    count INTEGER NOT NULL CHECK (count >= 0)
) STRICT, WITHOUT ROWID;
CREATE TABLE settlement_jobs (
    settlement_kind INTEGER NOT NULL CHECK (settlement_kind IN (0, 1, 2)),
    settlement_id BLOB NOT NULL CHECK (length(settlement_id) = 32),
    operation_id BLOB CHECK (operation_id IS NULL OR length(operation_id) = 8),
    phase INTEGER NOT NULL CHECK (phase IN (0, 1)),
    status INTEGER NOT NULL CHECK (status IN (0, 1, 2, 3)),
    next_run_at_ns BLOB CHECK (next_run_at_ns IS NULL OR length(next_run_at_ns) = 8),
    confirmation_checks INTEGER NOT NULL CHECK (confirmation_checks BETWEEN 0 AND 255),
    confirmation_started_at_ns BLOB CHECK (confirmation_started_at_ns IS NULL OR length(confirmation_started_at_ns) = 8),
    lease_generation BLOB NOT NULL CHECK (length(lease_generation) = 8),
    lease_until_ns BLOB CHECK (lease_until_ns IS NULL OR length(lease_until_ns) = 8),
    last_error_code TEXT,
    last_error_detail TEXT,
    updated_at_ns BLOB NOT NULL CHECK (length(updated_at_ns) = 8),
    PRIMARY KEY (settlement_kind, settlement_id),
    CHECK ((status = 0 AND next_run_at_ns IS NOT NULL AND lease_until_ns IS NULL)
        OR (status = 1 AND next_run_at_ns IS NULL AND lease_until_ns IS NOT NULL)
        OR (status IN (2, 3) AND next_run_at_ns IS NULL AND lease_until_ns IS NULL)),
    CHECK (phase = 1 OR (operation_id IS NOT NULL AND confirmation_started_at_ns IS NOT NULL))
) STRICT, WITHOUT ROWID;
CREATE INDEX settlement_jobs_due
ON settlement_jobs(status, next_run_at_ns, settlement_kind, settlement_id);
CREATE INDEX settlement_jobs_lease
ON settlement_jobs(status, lease_until_ns, settlement_kind, settlement_id);

CREATE TRIGGER settlement_jobs_count_insert AFTER INSERT ON settlement_jobs BEGIN
    UPDATE settlement_job_status_counts SET count = count + 1 WHERE status = NEW.status;
END;
CREATE TRIGGER settlement_jobs_count_delete AFTER DELETE ON settlement_jobs BEGIN
    UPDATE settlement_job_status_counts SET count = count - 1 WHERE status = OLD.status;
END;
CREATE TRIGGER settlement_jobs_count_status AFTER UPDATE OF status ON settlement_jobs
WHEN OLD.status != NEW.status BEGIN
    UPDATE settlement_job_status_counts SET count = count - 1 WHERE status = OLD.status;
    UPDATE settlement_job_status_counts SET count = count + 1 WHERE status = NEW.status;
END;
INSERT INTO settlement_job_status_counts(status, count) VALUES
 (0, 0),
 (1, 0),
 (2, 0),
 (3, 0);

INSERT INTO table_counts(name, count) VALUES
 ('deposits', X'0000000000000000'),
 ('withdrawals', X'0000000000000000'),
 ('evm_operations', X'0000000000000000'),
 ('reconciliation_holds', X'0000000000000000'),
 ('evm_execution_payloads', X'0000000000000000'),
 ('reconciliation_scans', X'0000000000000000'),
 ('audit_events', X'0000000000000000'),
 ('fee_payouts', X'0000000000000000'),
 ('deposit_owner_index', X'0000000000000000'),
 ('operation_owner_index', X'0000000000000000'),
 ('evm_state_index', X'0000000000000000'),
 ('pull_pending_deposit_index', X'0000000000000000'),
 ('release_pending_withdrawal_index', X'0000000000000000'),
 ('open_hold_index', X'0000000000000000'),
 ('owner_deposit_sequences', X'0000000000000000'),
 ('withdrawal_liability_index', X'0000000000000000'),
 ('withdrawal_notification_index', X'0000000000000000'),
 ('withdrawal_stop_reason_counts', X'0000000000000000');
"#;

const MIGRATIONS: &[Migration] = &[Migration {
    version: SCHEMA_VERSION as u64,
    sql: SQLITE_SCHEMA,
}];

fn deposit_owner_index_prefix(owner: Principal) -> Vec<u8> {
    let owner_bytes = owner.as_slice();
    let mut prefix = Vec::with_capacity(1 + owner_bytes.len());
    prefix.push(owner_bytes.len() as u8);
    prefix.extend_from_slice(owner_bytes);
    prefix
}

fn owner_sequence_key(owner: Principal) -> Result<StableBlob, StorageError> {
    StableBlob::new(owner.as_slice().to_vec())
}

fn deposit_owner_index_bytes(prefix: &[u8], reverse_sequence: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(prefix.len() + 8);
    bytes.extend_from_slice(prefix);
    bytes.extend_from_slice(&reverse_sequence.to_be_bytes());
    bytes
}

fn deposit_owner_index_key(owner: Principal, sequence: u64) -> Result<StableBlob, StorageError> {
    StableBlob::new(deposit_owner_index_bytes(
        &deposit_owner_index_prefix(owner),
        u64::MAX - sequence,
    ))
}

fn deposit_sequence_from_index_key(key: &StableBlob) -> Result<u64, StorageError> {
    let reverse_bytes: [u8; 8] = key
        .as_slice()
        .get(key.as_slice().len().saturating_sub(8)..)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(StorageError::DecodeFailed)?;
    Ok(u64::MAX - u64::from_be_bytes(reverse_bytes))
}

fn reconciliation_scan_key(target: &ReconciliationTarget) -> [u8; 9] {
    let (tag, id) = match target {
        ReconciliationTarget::Hold(id) => (0, id.get()),
        ReconciliationTarget::FeePayout(id) => (1, *id),
    };
    let mut key = [0; 9];
    key[0] = tag;
    key[1..].copy_from_slice(&id.to_be_bytes());
    key
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StableBlob(Vec<u8>);

impl StableBlob {
    pub fn new(bytes: Vec<u8>) -> Result<Self, StorageError> {
        if bytes.len() > MAX_STABLE_VALUE_BYTES {
            return Err(StorageError::ValueTooLarge {
                actual: bytes.len(),
                maximum: MAX_STABLE_VALUE_BYTES,
            });
        }
        Ok(Self(bytes))
    }

    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

trait SqlCodec: Clone + Sized {
    fn to_sql_bytes(&self) -> Vec<u8>;
    fn from_sql_bytes(bytes: Vec<u8>) -> Result<Self, StorageError>;
}

impl SqlCodec for StableBlob {
    fn to_sql_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    fn from_sql_bytes(bytes: Vec<u8>) -> Result<Self, StorageError> {
        Self::new(bytes)
    }
}

impl SqlCodec for u8 {
    fn to_sql_bytes(&self) -> Vec<u8> {
        vec![*self]
    }

    fn from_sql_bytes(bytes: Vec<u8>) -> Result<Self, StorageError> {
        bytes
            .as_slice()
            .try_into()
            .map(u8::from_be_bytes)
            .map_err(|_| StorageError::DecodeFailed)
    }
}

impl SqlCodec for u16 {
    fn to_sql_bytes(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn from_sql_bytes(bytes: Vec<u8>) -> Result<Self, StorageError> {
        bytes
            .as_slice()
            .try_into()
            .map(u16::from_be_bytes)
            .map_err(|_| StorageError::DecodeFailed)
    }
}

impl SqlCodec for u64 {
    fn to_sql_bytes(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn from_sql_bytes(bytes: Vec<u8>) -> Result<Self, StorageError> {
        bytes
            .as_slice()
            .try_into()
            .map(u64::from_be_bytes)
            .map_err(|_| StorageError::DecodeFailed)
    }
}

impl SqlCodec for u128 {
    fn to_sql_bytes(&self) -> Vec<u8> {
        self.to_be_bytes().to_vec()
    }

    fn from_sql_bytes(bytes: Vec<u8>) -> Result<Self, StorageError> {
        bytes
            .as_slice()
            .try_into()
            .map(u128::from_be_bytes)
            .map_err(|_| StorageError::DecodeFailed)
    }
}

impl<const N: usize> SqlCodec for [u8; N] {
    fn to_sql_bytes(&self) -> Vec<u8> {
        self.to_vec()
    }

    fn from_sql_bytes(bytes: Vec<u8>) -> Result<Self, StorageError> {
        bytes.try_into().map_err(|_| StorageError::DecodeFailed)
    }
}

#[derive(Clone)]
struct SqlEntry<K, V> {
    key: K,
    value: V,
}

#[derive(Clone, Copy)]
struct RevisionedHandle(DbHandle);

impl RevisionedHandle {
    fn query<T, F>(self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&ic_sqlite_vfs::db::connection::Connection) -> Result<T, DbError>,
    {
        self.0.query(f)
    }

    fn update<T, F>(self, f: F) -> Result<T, DbError>
    where
        F: FnOnce(&UpdateConnection<'_>) -> Result<T, DbError>,
    {
        self.0.update(|connection| {
            let result = f(connection)?;
            bump_storage_revision(connection)?;
            Ok(result)
        })
    }

    fn integrity_check(self) -> Result<String, DbError> {
        self.0.integrity_check()
    }

    fn refresh_checksum_chunk(self, max_bytes: u64) -> Result<ChecksumRefresh, DbError> {
        self.0.refresh_checksum_chunk(max_bytes)
    }
}

fn decode_withdrawal_blob(bytes: Vec<u8>) -> Result<WithdrawalRecord, DbError> {
    let blob = StableBlob::new(bytes)
        .map_err(|_| DbError::Constraint("invalid withdrawal change blob".into()))?;
    decode(&blob).map_err(|_| DbError::Constraint("invalid withdrawal record".into()))
}

fn withdrawal_liability_key(record: &WithdrawalRecord) -> Vec<u8> {
    let mut key = Vec::with_capacity(40);
    key.extend_from_slice(&record.observed_at_ns.to_be_bytes());
    key.extend_from_slice(&record.id.bytes());
    key
}

fn adjust_stop_reason_count(
    connection: &UpdateConnection<'_>,
    reason: &str,
    add: bool,
) -> Result<(), DbError> {
    let key = reason.as_bytes().to_vec();
    let current = connection.query_optional_scalar::<Vec<u8>>(
        "SELECT value FROM withdrawal_stop_reason_counts WHERE key = ?1",
        params![key.clone()],
    )?;
    let count = current
        .map(u64::from_sql_bytes)
        .transpose()
        .map_err(|_| DbError::Constraint("invalid withdrawal stop reason count".into()))?
        .unwrap_or(0);
    if add {
        let next = count
            .checked_add(1)
            .ok_or_else(|| DbError::Constraint("withdrawal stop reason count overflow".into()))?;
        connection.execute(
            "INSERT INTO withdrawal_stop_reason_counts(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, next.to_sql_bytes()],
        )?;
        if count == 0 {
            increment_table_count(connection, "withdrawal_stop_reason_counts")?;
        }
    } else {
        let next = count
            .checked_sub(1)
            .ok_or_else(|| DbError::Constraint("withdrawal stop reason count underflow".into()))?;
        if next == 0 {
            connection.execute(
                "DELETE FROM withdrawal_stop_reason_counts WHERE key = ?1",
                params![key],
            )?;
            decrement_table_count(connection, "withdrawal_stop_reason_counts")?;
        } else {
            connection.execute(
                "UPDATE withdrawal_stop_reason_counts SET value = ?1 WHERE key = ?2",
                params![next.to_sql_bytes(), key],
            )?;
        }
    }
    Ok(())
}

fn adjust_withdrawal_liability_record(
    connection: &UpdateConnection<'_>,
    record: &WithdrawalRecord,
    add: bool,
    amount: &mut u128,
) -> Result<(), DbError> {
    if !is_nonterminal_withdrawal(record) {
        return Ok(());
    }
    if add {
        *amount = amount
            .checked_add(record.amount_out.get())
            .ok_or_else(|| DbError::Constraint("withdrawal liability overflow".into()))?;
    } else {
        *amount = amount
            .checked_sub(record.amount_out.get())
            .ok_or_else(|| DbError::Constraint("withdrawal liability underflow".into()))?;
    }
    if let Some(reason) = record.last_settlement_stop_reason.as_deref() {
        adjust_stop_reason_count(connection, reason, add)?;
    }
    Ok(())
}

fn replace_withdrawal_row(
    connection: &UpdateConnection<'_>,
    key: Vec<u8>,
    expected: Option<&StableBlob>,
    next: &StableBlob,
) -> Result<(), DbError> {
    let persisted = connection.query_optional_scalar::<Vec<u8>>(
        "SELECT value FROM withdrawals WHERE key = ?1",
        params![key.clone()],
    )?;
    if persisted.as_deref() != expected.map(StableBlob::as_slice) {
        return Err(DbError::Constraint("stale withdrawal write".into()));
    }
    let raw_amount = connection.query_scalar::<Vec<u8>>(
        "SELECT withdrawal_liability_amount FROM singleton_state WHERE id = 1",
        params![],
    )?;
    let mut amount = u128::from_sql_bytes(raw_amount)
        .map_err(|_| DbError::Constraint("invalid withdrawal liability amount".into()))?;
    let old_record = persisted.map(decode_withdrawal_blob).transpose()?;
    let next_record = decode_withdrawal_blob(next.to_sql_bytes())?;
    let previous_liability_key = old_record
        .as_ref()
        .filter(|record| is_nonterminal_withdrawal(record))
        .map(withdrawal_liability_key);
    let next_liability = is_nonterminal_withdrawal(&next_record).then(|| {
        (
            withdrawal_liability_key(&next_record),
            next_record.id.bytes().to_sql_bytes(),
        )
    });
    transition_tracked_entry(
        connection,
        "withdrawal_liability_index",
        previous_liability_key,
        next_liability,
    )?;
    if let Some(old) = old_record {
        adjust_withdrawal_liability_record(connection, &old, false, &mut amount)?;
        connection.execute(
            "UPDATE withdrawals SET value = ?1 WHERE key = ?2",
            params![next.to_sql_bytes(), key],
        )?;
    } else {
        connection.execute(
            "INSERT INTO withdrawals(key, value) VALUES(?1, ?2)",
            params![key, next.to_sql_bytes()],
        )?;
        increment_table_count(connection, "withdrawals")?;
    }
    adjust_withdrawal_liability_record(connection, &next_record, true, &mut amount)?;
    connection.execute(
        "UPDATE singleton_state SET withdrawal_liability_amount = ?1 WHERE id = 1",
        params![amount.to_sql_bytes()],
    )
}

fn bump_storage_revision(connection: &UpdateConnection<'_>) -> Result<(), DbError> {
    let raw = connection.query_scalar::<Vec<u8>>(
        "SELECT storage_revision FROM singleton_state WHERE id = 1",
        params![],
    )?;
    let revision = u64::from_sql_bytes(raw)
        .map_err(|_| DbError::Constraint("invalid storage revision".into()))?;
    let next = revision
        .checked_add(1)
        .ok_or_else(|| DbError::Constraint("storage revision overflow".into()))?;
    connection.execute(
        "UPDATE singleton_state SET storage_revision = ?1 WHERE id = 1",
        params![next.to_sql_bytes()],
    )
}

impl<K, V> SqlEntry<K, V> {
    fn key(&self) -> &K {
        &self.key
    }

    fn value(&self) -> V
    where
        V: Clone,
    {
        self.value.clone()
    }
}

struct SqlCell<T> {
    handle: RevisionedHandle,
    column: SingletonColumn,
    _value: PhantomData<T>,
}

#[derive(Clone, Copy)]
enum SingletonColumn {
    Accounting,
    Counters,
    ExternalProgress,
    Config,
    AdminState,
    DepositAdmission,
    AuditRetention,
    SettlementAdmission,
    ConfirmationSchedulerHealth,
}

impl SingletonColumn {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Accounting => "accounting",
            Self::Counters => "counters",
            Self::ExternalProgress => "external_progress",
            Self::Config => "config",
            Self::AdminState => "admin_state",
            Self::DepositAdmission => "deposit_admission",
            Self::AuditRetention => "audit_retention",
            Self::SettlementAdmission => "settlement_admission",
            Self::ConfirmationSchedulerHealth => "confirmation_scheduler_health",
        }
    }
}

impl<T: SqlCodec> SqlCell<T> {
    fn load(handle: RevisionedHandle, column: SingletonColumn) -> Result<Self, StorageError> {
        let name = column.as_str();
        let sql = format!("SELECT {name} FROM singleton_state WHERE id = 1");
        let bytes = handle
            .query(|connection| connection.query_optional_scalar::<Vec<u8>>(&sql, params![]))
            .map_err(StorageError::from)?
            .ok_or(StorageError::RecordNotFound)?;
        T::from_sql_bytes(bytes)?;
        Ok(Self {
            handle,
            column,
            _value: PhantomData,
        })
    }

    fn get(&self) -> Result<T, StorageError> {
        let sql = format!(
            "SELECT {} FROM singleton_state WHERE id = 1",
            self.column.as_str()
        );
        let bytes = self
            .handle
            .query(|connection| connection.query_optional_scalar::<Vec<u8>>(&sql, params![]))?
            .ok_or(StorageError::RecordNotFound)?;
        T::from_sql_bytes(bytes)
    }

    fn set(&self, value: T) -> Result<(), StorageError> {
        let bytes = value.to_sql_bytes();
        let sql = format!(
            "UPDATE singleton_state SET {} = ?1 WHERE id = 1",
            self.column.as_str()
        );
        self.handle
            .update(|connection| connection.execute(&sql, params![bytes]))
            .map_err(StorageError::from)
    }
}

struct SqlMap<K, V> {
    handle: RevisionedHandle,
    table: &'static str,
    _types: PhantomData<(K, V)>,
}

impl<K, V> SqlMap<K, V>
where
    K: SqlCodec + Ord,
    V: SqlCodec,
{
    const fn new(handle: RevisionedHandle, table: &'static str) -> Self {
        Self {
            handle,
            table,
            _types: PhantomData,
        }
    }

    fn get(&self, key: &K) -> Option<V> {
        let sql = format!("SELECT value FROM {} WHERE key = ?1", self.table);
        let key = key.to_sql_bytes();
        self.handle
            .query(|connection| connection.query_optional_scalar::<Vec<u8>>(&sql, params![key]))
            .unwrap_or_else(|error| panic!("SQLite map read failed: {error}"))
            .map(|bytes| {
                V::from_sql_bytes(bytes)
                    .unwrap_or_else(|error| panic!("SQLite map value decode failed: {error}"))
            })
    }

    fn insert(&mut self, key: K, value: V) -> Option<V> {
        let select_sql = format!("SELECT value FROM {} WHERE key = ?1", self.table);
        let insert_sql = format!(
            "INSERT INTO {}(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            self.table
        );
        let key_bytes = key.to_sql_bytes();
        let value_bytes = value.to_sql_bytes();
        let table = self.table;
        self.handle
            .update(|connection| {
                let previous = connection
                    .query_optional_scalar::<Vec<u8>>(&select_sql, params![key_bytes.clone()])?;
                connection.execute(&insert_sql, params![key_bytes, value_bytes])?;
                if previous.is_none() {
                    increment_table_count(connection, table)?;
                }
                Ok(previous)
            })
            .unwrap_or_else(|error| panic!("SQLite map insert failed: {error}"))
            .map(|bytes| {
                V::from_sql_bytes(bytes)
                    .unwrap_or_else(|error| panic!("SQLite map value decode failed: {error}"))
            })
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        let select_sql = format!("SELECT value FROM {} WHERE key = ?1", self.table);
        let delete_sql = format!("DELETE FROM {} WHERE key = ?1", self.table);
        let key_bytes = key.to_sql_bytes();
        let table = self.table;
        self.handle
            .update(|connection| {
                let previous = connection
                    .query_optional_scalar::<Vec<u8>>(&select_sql, params![key_bytes.clone()])?;
                if previous.is_some() {
                    connection.execute(&delete_sql, params![key_bytes])?;
                    decrement_table_count(connection, table)?;
                }
                Ok(previous)
            })
            .unwrap_or_else(|error| panic!("SQLite map remove failed: {error}"))
            .map(|bytes| {
                V::from_sql_bytes(bytes)
                    .unwrap_or_else(|error| panic!("SQLite map value decode failed: {error}"))
            })
    }

    fn len(&self) -> u64 {
        let bytes = self
            .handle
            .query(|connection| {
                connection.query_scalar::<Vec<u8>>(
                    "SELECT count FROM table_counts WHERE name = ?1",
                    params![self.table],
                )
            })
            .unwrap_or_else(|error| panic!("SQLite table count read failed: {error}"));
        u64::from_sql_bytes(bytes)
            .unwrap_or_else(|error| panic!("SQLite table count decode failed: {error}"))
    }

    #[cfg(test)]
    fn iter(&self) -> std::vec::IntoIter<SqlEntry<K, V>> {
        let sql = format!("SELECT key, value FROM {} ORDER BY key", self.table);
        self.query_entries(&sql, params![]).into_iter()
    }

    fn range_limited<R: RangeBounds<K>>(
        &self,
        range: R,
        limit: usize,
        descending: bool,
    ) -> Vec<SqlEntry<K, V>> {
        let start = match range.start_bound() {
            RangeBound::Included(key) => Some((key.to_sql_bytes(), true)),
            RangeBound::Excluded(key) => Some((key.to_sql_bytes(), false)),
            RangeBound::Unbounded => None,
        };
        let end = match range.end_bound() {
            RangeBound::Included(key) => Some((key.to_sql_bytes(), true)),
            RangeBound::Excluded(key) => Some((key.to_sql_bytes(), false)),
            RangeBound::Unbounded => None,
        };
        let order = if descending { "DESC" } else { "ASC" };
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let entries = match (start, end) {
            (Some((start, start_inclusive)), Some((end, end_inclusive))) => {
                let sql = format!(
                    "SELECT key, value FROM {} WHERE key {} ?1 AND key {} ?2 ORDER BY key {order} LIMIT ?3",
                    self.table,
                    if start_inclusive { ">=" } else { ">" },
                    if end_inclusive { "<=" } else { "<" }
                );
                self.query_entries(&sql, params![start, end, limit])
            }
            (Some((start, inclusive)), None) => {
                let sql = format!(
                    "SELECT key, value FROM {} WHERE key {} ?1 ORDER BY key {order} LIMIT ?2",
                    self.table,
                    if inclusive { ">=" } else { ">" }
                );
                self.query_entries(&sql, params![start, limit])
            }
            (None, Some((end, inclusive))) => {
                let sql = format!(
                    "SELECT key, value FROM {} WHERE key {} ?1 ORDER BY key {order} LIMIT ?2",
                    self.table,
                    if inclusive { "<=" } else { "<" }
                );
                self.query_entries(&sql, params![end, limit])
            }
            (None, None) => {
                let sql = format!(
                    "SELECT key, value FROM {} ORDER BY key {order} LIMIT ?1",
                    self.table
                );
                self.query_entries(&sql, params![limit])
            }
        };
        entries
    }

    fn first_in_range<R: RangeBounds<K>>(&self, range: R) -> Option<SqlEntry<K, V>> {
        self.range_limited(range, 1, false).into_iter().next()
    }

    fn query_entries(
        &self,
        sql: &str,
        values: &[&dyn ic_sqlite_vfs::db::ToSql],
    ) -> Vec<SqlEntry<K, V>> {
        self.handle
            .query(|connection| {
                connection.query_all(sql, values, |row| {
                    let key = K::from_sql_bytes(row.get::<Vec<u8>>(0)?).map_err(|_| {
                        DbError::TypeMismatch {
                            index: 0,
                            expected: "valid application key",
                            actual: "invalid blob",
                        }
                    })?;
                    let value = V::from_sql_bytes(row.get::<Vec<u8>>(1)?).map_err(|_| {
                        DbError::TypeMismatch {
                            index: 1,
                            expected: "valid application value",
                            actual: "invalid blob",
                        }
                    })?;
                    Ok(SqlEntry { key, value })
                })
            })
            .unwrap_or_else(|error| panic!("SQLite range query failed: {error}"))
    }
}

fn upsert_confirmation_schedule(
    connection: &ic_sqlite_vfs::db::UpdateConnection<'_>,
    kind: SettlementJobKind,
    settlement_id: [u8; 32],
    schedule: ConfirmationSchedule,
) -> Result<(), DbError> {
    connection.execute(
        "INSERT INTO settlement_jobs(
            settlement_kind, settlement_id, operation_id, phase, status, next_run_at_ns,
            confirmation_checks, confirmation_started_at_ns, lease_generation, lease_until_ns,
            last_error_code, last_error_detail, updated_at_ns
         ) VALUES(?1, ?2, ?3, 0, 0, ?6, ?4, ?5, X'0000000000000000', NULL, NULL, NULL, ?5)
         ON CONFLICT(settlement_kind, settlement_id) DO UPDATE SET
            operation_id=excluded.operation_id, phase=0, status=0,
            next_run_at_ns=excluded.next_run_at_ns,
            confirmation_checks=excluded.confirmation_checks,
            confirmation_started_at_ns=excluded.confirmation_started_at_ns,
            lease_until_ns=NULL, last_error_code=NULL, last_error_detail=NULL,
            updated_at_ns=excluded.updated_at_ns",
        params![
            kind.sql(),
            settlement_id.to_sql_bytes(),
            schedule.operation_id.to_sql_bytes(),
            i64::from(schedule.checks_completed),
            schedule.submitted_at_ns.to_sql_bytes(),
            schedule.next_check_at_ns.to_sql_bytes()
        ],
    )
}

fn enqueue_settlement_job(
    connection: &ic_sqlite_vfs::db::UpdateConnection<'_>,
    kind: SettlementJobKind,
    settlement_id: [u8; 32],
    operation_id: Option<u64>,
    now_ns: u64,
) -> Result<(), DbError> {
    let sql = "INSERT INTO settlement_jobs(
            settlement_kind, settlement_id, operation_id, phase, status, next_run_at_ns,
            confirmation_checks, confirmation_started_at_ns, lease_generation, lease_until_ns,
            last_error_code, last_error_detail, updated_at_ns
         ) VALUES(?1, ?2, ?3, 1, 0, ?4, 0, NULL, X'0000000000000000', NULL, NULL, NULL, ?4)
         ON CONFLICT(settlement_kind, settlement_id) DO UPDATE SET
            operation_id=excluded.operation_id, phase=1, status=0,
            next_run_at_ns=excluded.next_run_at_ns, confirmation_checks=0,
            confirmation_started_at_ns=NULL, lease_until_ns=NULL,
            last_error_code=NULL, last_error_detail=NULL,
            updated_at_ns=excluded.updated_at_ns";
    match operation_id {
        Some(operation_id) => connection.execute(
            sql,
            params![
                kind.sql(),
                settlement_id.to_sql_bytes(),
                operation_id.to_sql_bytes(),
                now_ns.to_sql_bytes()
            ],
        ),
        None => connection.execute(
            "INSERT INTO settlement_jobs(
                settlement_kind, settlement_id, operation_id, phase, status, next_run_at_ns,
                confirmation_checks, confirmation_started_at_ns, lease_generation, lease_until_ns,
                last_error_code, last_error_detail, updated_at_ns
             ) VALUES(?1, ?2, NULL, 1, 0, ?3, 0, NULL, X'0000000000000000', NULL, NULL, NULL, ?3)
             ON CONFLICT(settlement_kind, settlement_id) DO UPDATE SET
                operation_id=NULL, phase=1, status=0,
                next_run_at_ns=excluded.next_run_at_ns, confirmation_checks=0,
                confirmation_started_at_ns=NULL, lease_until_ns=NULL,
                last_error_code=NULL, last_error_detail=NULL,
                updated_at_ns=excluded.updated_at_ns",
            params![
                kind.sql(),
                settlement_id.to_sql_bytes(),
                now_ns.to_sql_bytes()
            ],
        ),
    }
}

fn delete_confirmation_schedule(
    connection: &ic_sqlite_vfs::db::UpdateConnection<'_>,
    schedule: ConfirmationSchedule,
) -> Result<(), DbError> {
    connection.execute(
        "DELETE FROM settlement_jobs WHERE operation_id = ?1",
        params![schedule.operation_id.to_sql_bytes()],
    )
}

fn detach_confirmed_operation(
    connection: &ic_sqlite_vfs::db::UpdateConnection<'_>,
    operation_id: u64,
    updated_at_ns: u64,
) -> Result<(), DbError> {
    connection.execute(
        "UPDATE settlement_jobs SET operation_id = NULL, phase = 1,
         confirmation_checks = 0, confirmation_started_at_ns = NULL,
         updated_at_ns = ?1 WHERE operation_id = ?2",
        params![updated_at_ns.to_sql_bytes(), operation_id.to_sql_bytes()],
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterState {
    pub next_evm_operation_id: u64,
    pub next_hold_id: u64,
    pub pending_ledger_operations: u64,
    pub next_audit_sequence: u64,
    pub next_fee_payout_id: u64,
    pub next_deposit_index_sequence: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reserved_deposit_mint_operations: u64,
    pub unresolved_evm_reverts: u64,
    pub awaiting_nonce_evm_operations: u64,
    pub pending_fee_payout_debit: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositCallerQuota {
    pub caller: Vec<u8>,
    pub count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedBaseMintSnapshot {
    pub generation: u64,
    pub observed_at_ns: u64,
    pub snapshot: BaseMintSnapshot,
    pub bridge_signer: [u8; 20],
    pub deposits_paused: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositAdmissionControl {
    pub window_id: u64,
    pub global_count: u16,
    pub caller_counts: Vec<DepositCallerQuota>,
    pub signer_address: Option<[u8; 20]>,
    pub signer_public_key: Option<Vec<u8>>,
    pub governance_operator_address: Option<[u8; 20]>,
    pub governance_operator_public_key: Option<Vec<u8>>,
    pub governance_nonce_initialized: bool,
    pub next_governance_nonce: u64,
    pub next_governance_operation_id: u64,
    pub pending_governance_transaction: Option<GovernanceTransaction>,
    pub pending_timelock_operation: Option<PendingTimelockOperation>,
    pub emergency_pause_deposit_required: bool,
    pub emergency_pause_withdrawal_required: bool,
    pub emergency_cancel_required: bool,
    pub base_snapshot: Option<CachedBaseMintSnapshot>,
    pub refresh_started_at_ns: Option<u64>,
    pub refresh_generation: u64,
    pub refresh_owner: Option<u64>,
    pub next_refresh_allowed_at_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernanceTransactionKind {
    PauseDepositMints,
    PauseWithdrawals,
    SetServiceFee {
        value: u128,
    },
    CancelTimelock {
        operation_id: [u8; 32],
    },
    ScheduleActivation {
        operation_id: [u8; 32],
        salt: [u8; 32],
    },
    ExecuteActivation {
        operation_id: [u8; 32],
        salt: [u8; 32],
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingTimelockOperation {
    pub operation_id: [u8; 32],
    pub salt: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GovernanceTransactionState {
    Prepared,
    Signed,
    Broadcasting {
        transaction_hash: [u8; 32],
    },
    Submitted {
        transaction_hash: [u8; 32],
    },
    NonceConflict {
        transaction_hash: [u8; 32],
    },
    Confirmed {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
    },
    Reverted {
        transaction_hash: [u8; 32],
        receipt_block_number: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GovernanceTransaction {
    pub id: u64,
    pub kind: GovernanceTransactionKind,
    pub envelope: bridge_core::EvmTransactionEnvelope,
    pub state: GovernanceTransactionState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct SettlementAdmissionControl {
    window_id: u64,
    global_count: u16,
    caller_counts: Vec<SettlementCallerQuota>,
    record_counts: Vec<SettlementRecordQuota>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SettlementCallerQuota {
    caller: Principal,
    count: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SettlementRecordQuota {
    key: Vec<u8>,
    count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementJobStatus {
    Scheduled,
    Leased,
    Stopped,
    AwaitingConfirmation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefundJobOutcome {
    KeepLeased,
    RetryAt(u64),
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementJobPhase {
    Confirmation,
    Settlement,
}

impl SettlementJobPhase {
    fn from_sql(value: i64) -> Result<Self, StorageError> {
        match value {
            0 => Ok(Self::Confirmation),
            1 => Ok(Self::Settlement),
            _ => Err(StorageError::DecodeFailed),
        }
    }
}

impl SettlementJobStatus {
    fn from_sql(value: i64) -> Result<Self, StorageError> {
        match value {
            0 => Ok(Self::Scheduled),
            1 => Ok(Self::Leased),
            2 => Ok(Self::Stopped),
            3 => Ok(Self::AwaitingConfirmation),
            _ => Err(StorageError::DecodeFailed),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementJob {
    pub kind: SettlementJobKind,
    pub settlement_id: [u8; 32],
    pub operation_id: Option<u64>,
    pub phase: SettlementJobPhase,
    pub status: SettlementJobStatus,
    pub next_run_at_ns: Option<u64>,
    pub confirmation_checks: u8,
    pub confirmation_started_at_ns: Option<u64>,
    pub lease_generation: u64,
    pub lease_until_ns: Option<u64>,
    pub last_error_code: Option<String>,
    pub last_error_detail: Option<String>,
    pub updated_at_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementCallbackToken {
    kind: SettlementJobKind,
    settlement_id: [u8; 32],
    lease_generation: u64,
    transfer_identity_hash: [u8; 32],
}

impl SettlementCallbackToken {
    pub fn for_deposit(
        job: &SettlementJob,
        transfer: &LedgerTransferIdentity,
    ) -> Result<Self, StorageError> {
        if job.kind != SettlementJobKind::Deposit {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let encoded = encode(transfer)?;
        Ok(Self {
            kind: job.kind,
            settlement_id: job.settlement_id,
            lease_generation: job.lease_generation,
            transfer_identity_hash: Sha256::digest(encoded.as_slice()).into(),
        })
    }

    fn matches_deposit(&self, record: &DepositRecord) -> Result<bool, StorageError> {
        let encoded = encode(&record.transfer)?;
        let transfer_identity_hash: [u8; 32] = Sha256::digest(encoded.as_slice()).into();
        Ok(self.kind == SettlementJobKind::Deposit
            && self.settlement_id == record.id.bytes()
            && self.transfer_identity_hash == transfer_identity_hash)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettlementJobClaim {
    Claimed(SettlementJob),
    ActiveLease { lease_until_ns: u64 },
    None,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManualSettlementClaim {
    Claimed(SettlementJob),
    AutomaticProgressPending { next_run_at_ns: Option<u64> },
    Busy,
}

enum ManualClaimTransaction {
    Claimed(StableBlob),
    AutomaticProgressPending(Option<u64>),
    Busy,
    RateLimited(u64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationSchedulerHealth {
    pub healthy: bool,
    pub last_run_ns: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SettlementJobSummary {
    pub scheduled: u64,
    pub leased: u64,
    pub stopped: u64,
    pub expired: u64,
    pub overdue: u64,
    pub next_wakeup_at_ns: Option<u64>,
}

impl Default for ConfirmationSchedulerHealth {
    fn default() -> Self {
        Self {
            healthy: true,
            last_run_ns: 0,
            last_error: None,
        }
    }
}

#[derive(Clone, Copy)]
enum OperationBundleParent<'a> {
    Deposit {
        previous: &'a DepositRecord,
        next: &'a DepositRecord,
        resolved_hold: Option<(&'a ReconciliationHoldRecord, &'a ReconciliationHoldRecord)>,
    },
}

struct RecoveryStart<'a> {
    previous: &'a EvmOperationRecord,
    next: &'a EvmOperationRecord,
    now_ns: u64,
}

#[derive(Clone, Copy)]
enum HoldBundleParent<'a> {
    Deposit {
        previous: &'a DepositRecord,
        next: &'a DepositRecord,
    },
    Withdrawal {
        previous: &'a WithdrawalRecord,
        next: &'a WithdrawalRecord,
    },
}

#[derive(Clone, Copy)]
enum ResolveHoldBundleParent<'a> {
    Deposit {
        previous: &'a DepositRecord,
        next: &'a DepositRecord,
    },
    Withdrawal {
        previous: &'a WithdrawalRecord,
        next: &'a WithdrawalRecord,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeePayoutTransition {
    Hold,
    Failed,
    Succeeded { block_index: u128 },
}

#[derive(CandidType, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventKind {
    DepositsPaused,
    DepositsPauseRepeated,
    DepositsResumed,
    WithdrawalFeeGuardTripped {
        ledger_fee: u128,
        charged_service_fee: u128,
    },
    WithdrawalFeeGuardCleared,
    DepositRefundRetried {
        deposit_id: Vec<u8>,
        previous_attempt_no: u64,
        previous_fee: u128,
        next_attempt_no: Option<u64>,
        next_fee: u128,
        compensated: bool,
    },
    EvmTransactionRebroadcasted {
        operation_id: u64,
        transaction_hash: Vec<u8>,
        attempt: u8,
    },
    EvmTransactionReplaced {
        operation_id: u64,
        previous_transaction_hash: Vec<u8>,
        transaction_hash: Vec<u8>,
        generation: u8,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
    },
    PausePrincipalRotated,
    FeeRecipientRotated {
        previous_sha256: Vec<u8>,
        current_sha256: Vec<u8>,
    },
    ReserveGateChanged {
        sufficient: bool,
    },
    FeePayoutRequested {
        amount: u128,
    },
    EvmOperationReverted {
        operation_id: u64,
        kind: AuditedEvmOperationKind,
        transaction_hash: Vec<u8>,
        finalized_head_block_number: u64,
    },
    MintRevertRecoveryStarted {
        target_id: Vec<u8>,
        reverted_operation_id: u64,
        replacement_operation_id: u64,
        kind: AuditedEvmOperationKind,
        finalized_block_number: u64,
        finalized_block_hash: Vec<u8>,
        result: String,
    },
    MintRevertRecoveryCompleted {
        target_id: Vec<u8>,
        reverted_operation_id: u64,
        replacement_operation_id: u64,
        kind: AuditedEvmOperationKind,
        finalized_block_number: u64,
        finalized_block_hash: Vec<u8>,
        result: String,
    },
    EvmRpcObservation {
        evm_rpc_canister_id: Principal,
        call_method: String,
        request_digest: Vec<u8>,
        quorum_response_digest: Vec<u8>,
        finalized_block_number: u64,
        finalized_block_hash: Vec<u8>,
        transaction_hash: Option<Vec<u8>>,
    },
    EvmRpcDecision {
        kind: String,
        operation: String,
        configured_provider_count: u8,
        required_threshold: u8,
        stop_reason: Option<String>,
        ledger_call_performed: bool,
        bridge_operation_continued: bool,
        deposits_paused: bool,
        automatically_resigned: bool,
        transaction_hash: Option<Vec<u8>>,
    },
}

#[derive(CandidType, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditedEvmOperationKind {
    MintDeposit,
}

impl From<bridge_core::EvmOperationKind> for AuditedEvmOperationKind {
    fn from(value: bridge_core::EvmOperationKind) -> Self {
        match value {
            bridge_core::EvmOperationKind::MintDeposit => Self::MintDeposit,
        }
    }
}

#[derive(CandidType, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub sequence: u64,
    pub timestamp_ns: u64,
    pub caller: Principal,
    pub kind: AuditEventKind,
}

pub struct RpcAuditBatch {
    pub caller: Principal,
    pub timestamp_ns: u64,
    pub kinds: Vec<AuditEventKind>,
}

struct PreparedAuditBatch {
    events: Vec<(u64, StableBlob)>,
    retention_blob: StableBlob,
    pruned_sequences: Vec<u64>,
}

#[derive(CandidType, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEventPage {
    pub events: Vec<AuditEvent>,
    pub oldest_available_sequence: u64,
    pub next_sequence: Option<u64>,
    pub pruned_count: u64,
    pub pruned_through_sequence: Option<u64>,
    pub pruned_digest: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRetentionState {
    pub pruned_count: u64,
    pub pruned_through_sequence: Option<u64>,
    pub pruned_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvmExecutionPayload {
    AwaitingNonce(EvmCallIntent),
    Prepared(EvmTransactionEnvelope),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageCounts {
    pub deposits: u64,
    pub withdrawals: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
    pub pending_ledger_operations: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reserved_deposit_mint_operations: u64,
    pub unresolved_evm_reverts: u64,
    pub last_finalized_base_block: u64,
    pub active_evm_payloads: u64,
    pub retained_audit_events: u64,
    pub pruned_audit_events: u64,
    pub retained_deposit_index_entries: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WithdrawalLiabilitySummary {
    pub count: u64,
    pub amount_out: u128,
    pub oldest_observed_at_ns: Option<u64>,
    pub stop_reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub enum StorageMaintenanceError {
    Unauthorized,
    InvalidArgument { message: String },
    StateChanged,
    NotStarted,
    StorageFailure,
}

#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct StorageValidationStatus {
    pub complete: bool,
    pub phase: String,
    pub scanned_rows: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, CandidType, Deserialize)]
pub struct ChecksumRefreshStatus {
    pub complete: bool,
    pub checksum: u64,
    pub scanned_bytes: u64,
    pub db_size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StorageValidationProgress {
    expected_revision: u64,
    phase: u16,
    cursor: Option<Vec<u8>>,
    phase_rows: u64,
    scanned_rows: u64,
    pending_ledger_operations: u64,
    nonterminal_withdrawals: u64,
    pending_evm_operations: u64,
    reconciliation_holds: u64,
    reserved_deposit_mint_amount: u128,
    reserved_deposit_mint_operations: u64,
    settlement_job_status_counts: [u64; 4],
}

enum ValidationChunkOutcome {
    Status(StorageValidationStatus),
    StateChanged,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DepositIdPageData {
    pub deposit_ids: Vec<[u8; 32]>,
    pub next_cursor: Option<u64>,
    pub oldest_available_cursor: Option<u64>,
    pub history_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositIntent {
    pub deposit_id: [u8; 32],
    pub caller: Vec<u8>,
    pub owner_sequence: u64,
    pub base_recipient: [u8; 20],
    pub from_subaccount: [u8; 32],
    pub payload_hash: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredDeposit {
    record: DepositRecord,
    owner_sequence: u64,
    base_recipient: [u8; 20],
}

impl StoredDeposit {
    fn intent(&self) -> DepositIntent {
        DepositIntent {
            deposit_id: self.record.id.bytes(),
            caller: self.record.transfer.from.owner().to_vec(),
            owner_sequence: self.owner_sequence,
            base_recipient: self.base_recipient,
            from_subaccount: self.record.transfer.from.subaccount(),
            payload_hash: self.record.payload_hash,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositReserveToken {
    pub nonterminal_withdrawals: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reserved_deposit_mint_operations: u64,
    pub observation_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    EncodeFailed,
    DecodeFailed,
    MissingWireVersion,
    UnsupportedWireVersion(u8),
    UnsupportedSchemaVersion(u16),
    ValueTooLarge { actual: usize, maximum: usize },
    CounterOverflow,
    CounterUnderflow,
    SequenceMismatch { expected: u64 },
    DepositsPaused,
    DepositRateLimited { retry_after_seconds: u64 },
    RecordNotFound,
    DatabaseFailure,
    ReserveUnavailable,
    StaleReserveObservation,
    QuoteSnapshotMismatch,
    Core(CoreError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for StorageError {}

impl From<CoreError> for StorageError {
    fn from(error: CoreError) -> Self {
        Self::Core(error)
    }
}

impl From<DbError> for StorageError {
    fn from(_: DbError) -> Self {
        Self::DatabaseFailure
    }
}

#[cfg(target_arch = "wasm32")]
fn backing_memory_is_empty(_: &DefaultMemoryImpl) -> bool {
    ic_cdk::stable::stable_size() == 0
}

#[cfg(not(target_arch = "wasm32"))]
fn backing_memory_is_empty(memory: &DefaultMemoryImpl) -> bool {
    memory.borrow().is_empty()
}

fn open_database(memory: DefaultMemoryImpl) -> Result<DbHandle, StorageError> {
    let manager = MemoryManager::init_strict(memory).map_err(|_| StorageError::DatabaseFailure)?;
    DbHandle::init(manager.get(SQLITE_MEMORY_ID)).map_err(StorageError::from)
}

fn verify_metadata(handle: DbHandle) -> Result<(), StorageError> {
    let (application_schema, record_wire): (i64, i64) = handle.query(|connection| {
        connection.query_one(
            "SELECT application_schema_version, record_wire_version FROM bridge_metadata WHERE id = 1",
            params![],
            |row| Ok((row.get::<i64>(0)?, row.get::<i64>(1)?)),
        )
    })?;
    if application_schema != i64::from(SCHEMA_VERSION) {
        return Err(StorageError::UnsupportedSchemaVersion(
            u16::try_from(application_schema).unwrap_or(u16::MAX),
        ));
    }
    if record_wire != i64::from(WIRE_VERSION) {
        return Err(StorageError::UnsupportedWireVersion(
            u8::try_from(record_wire).unwrap_or(u8::MAX),
        ));
    }
    Ok(())
}

fn initialize_singleton_state(
    handle: DbHandle,
    config: Option<&BridgeInitArgs>,
) -> Result<(), StorageError> {
    let admin = config.map(|config| AdminState {
        deposits_paused: true,
        withdrawal_fee_guard: None,
        pause_principal: config.pause_principal,
        governance_principal: config.governance_principal,
        fee_recipient: config.fee_recipient.clone(),
    });
    let accounting = encode(&AccountingState::default())?.to_sql_bytes();
    let counters = encode(&CounterState::default())?.to_sql_bytes();
    let external_progress = encode(&ExternalProgress::default())?.to_sql_bytes();
    let config = encode(&config.map(ImmutableBridgeConfig::from_init))?.to_sql_bytes();
    let admin = encode(&admin)?.to_sql_bytes();
    let deposit_admission = encode(&DepositAdmissionControl::default())?.to_sql_bytes();
    let audit_retention = encode(&AuditRetentionState::default())?.to_sql_bytes();
    let settlement_admission = encode(&SettlementAdmissionControl::default())?.to_sql_bytes();
    let confirmation_scheduler_health =
        encode(&ConfirmationSchedulerHealth::default())?.to_sql_bytes();
    handle.update(|connection| {
        connection.execute(
            "INSERT INTO singleton_state(
                id, accounting, counters, external_progress, config, admin_state,
                deposit_admission, audit_retention,
                settlement_admission, confirmation_scheduler_health, storage_revision,
                withdrawal_liability_amount, storage_validation
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL)",
            params![
                accounting,
                counters,
                external_progress,
                config,
                admin,
                deposit_admission,
                audit_retention,
                settlement_admission,
                confirmation_scheduler_health,
                0u64.to_sql_bytes(),
                0u128.to_sql_bytes()
            ],
        )
    })?;
    Ok(())
}

#[cfg(test)]
fn reset_sqlite_test_runtime() {
    ic_sqlite_vfs::test_support::memory::reset_for_tests();
    ic_sqlite_vfs::test_support::lock::reset_for_tests();
}

pub struct StableStore {
    handle: RevisionedHandle,
    accounting: SqlCell<StableBlob>,
    deposits: SqlMap<[u8; 32], StableBlob>,
    withdrawals: SqlMap<[u8; 32], StableBlob>,
    evm_operations: SqlMap<u64, StableBlob>,
    reconciliation_holds: SqlMap<u64, StableBlob>,
    counters: SqlCell<StableBlob>,
    external_progress: SqlCell<StableBlob>,
    evm_execution_payloads: SqlMap<u64, StableBlob>,
    reconciliation_scans: SqlMap<[u8; 9], StableBlob>,
    config: SqlCell<StableBlob>,
    admin_state: SqlCell<StableBlob>,
    audit_events: SqlMap<u64, StableBlob>,
    fee_payouts: SqlMap<u64, StableBlob>,
    deposit_owner_index: SqlMap<StableBlob, [u8; 32]>,
    deposit_admission: SqlCell<StableBlob>,
    operation_owner_index: SqlMap<u64, StableBlob>,
    evm_state_index: SqlMap<StableBlob, u8>,
    pull_pending_deposit_index: SqlMap<[u8; 32], u8>,
    release_pending_withdrawal_index: SqlMap<[u8; 32], u8>,
    withdrawal_notification_index: SqlMap<[u8; 32], [u8; 32]>,
    open_hold_index: SqlMap<u64, u8>,
    owner_deposit_sequences: SqlMap<StableBlob, u64>,
    audit_retention: SqlCell<StableBlob>,
    settlement_admission: SqlCell<StableBlob>,
    confirmation_scheduler_health: SqlCell<StableBlob>,
}

fn read_storage_revision(connection: &UpdateConnection<'_>) -> Result<u64, DbError> {
    let raw = connection.query_scalar::<Vec<u8>>(
        "SELECT storage_revision FROM singleton_state WHERE id = 1",
        params![],
    )?;
    u64::from_sql_bytes(raw).map_err(|_| DbError::Constraint("invalid storage revision".into()))
}

fn decode_with_context<T: DeserializeOwned>(bytes: Vec<u8>, context: &str) -> Result<T, DbError> {
    let blob = StableBlob::new(bytes).map_err(|_| DbError::Constraint(context.into()))?;
    decode(&blob).map_err(|_| DbError::Constraint(context.into()))
}

fn referenced_row_exists(
    connection: &UpdateConnection<'_>,
    table: &str,
    key: &[u8],
) -> Result<bool, DbError> {
    let sql = format!("SELECT 1 FROM {table} WHERE key = ?1");
    Ok(connection
        .query_optional_scalar::<i64>(&sql, params![key])?
        .is_some())
}

fn validate_storage_row(
    connection: &UpdateConnection<'_>,
    table: &str,
    key: &[u8],
    value: &[u8],
    progress: &mut StorageValidationProgress,
) -> Result<(), DbError> {
    match table {
        "deposits" => {
            let stored: StoredDeposit =
                decode_with_context(value.to_vec(), "invalid stored deposit")?;
            let record = &stored.record;
            if key != record.id.bytes() {
                return Err(DbError::Constraint("deposit key mismatch".into()));
            }
            if stored.intent().caller.is_empty() {
                return Err(DbError::Constraint("invalid deposit caller".into()));
            }
            if is_pending_deposit_ledger(record) {
                progress.pending_ledger_operations = progress
                    .pending_ledger_operations
                    .checked_add(1)
                    .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
            }
            if is_deposit_mint_reserved(record) {
                progress.reserved_deposit_mint_operations = progress
                    .reserved_deposit_mint_operations
                    .checked_add(1)
                    .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
                progress.reserved_deposit_mint_amount = progress
                    .reserved_deposit_mint_amount
                    .checked_add(
                        record
                            .reserved_mint_amount()
                            .map_err(|_| DbError::Constraint("invalid deposit quote".into()))?
                            .get(),
                    )
                    .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
            }
        }
        "withdrawals" => {
            let record: WithdrawalRecord =
                decode_with_context(value.to_vec(), "invalid withdrawal")?;
            if key != record.id.bytes() {
                return Err(DbError::Constraint("withdrawal key mismatch".into()));
            }
            if is_pending_withdrawal_ledger(&record) {
                progress.pending_ledger_operations = progress
                    .pending_ledger_operations
                    .checked_add(1)
                    .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
            }
            if is_nonterminal_withdrawal(&record) {
                progress.nonterminal_withdrawals = progress
                    .nonterminal_withdrawals
                    .checked_add(1)
                    .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
                if !referenced_row_exists(
                    connection,
                    "withdrawal_liability_index",
                    &withdrawal_liability_key(&record),
                )? {
                    return Err(DbError::Constraint(
                        "missing withdrawal liability index".into(),
                    ));
                }
            }
        }
        "evm_operations" => {
            let record: EvmOperationRecord =
                decode_with_context(value.to_vec(), "invalid EVM operation")?;
            if key != record.id.get().to_sql_bytes() {
                return Err(DbError::Constraint("EVM operation key mismatch".into()));
            }
            if is_pending_evm(&record) {
                progress.pending_evm_operations = progress
                    .pending_evm_operations
                    .checked_add(1)
                    .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
            }
        }
        "reconciliation_holds" => {
            let record: ReconciliationHoldRecord =
                decode_with_context(value.to_vec(), "invalid reconciliation hold")?;
            if key != record.id.get().to_sql_bytes() {
                return Err(DbError::Constraint(
                    "reconciliation hold key mismatch".into(),
                ));
            }
            if is_open_hold(&record) {
                progress.reconciliation_holds = progress
                    .reconciliation_holds
                    .checked_add(1)
                    .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
            }
        }
        "evm_execution_payloads" => {
            let _: EvmExecutionPayload =
                decode_with_context(value.to_vec(), "invalid EVM execution payload")?;
            if !referenced_row_exists(connection, "evm_operations", key)? {
                return Err(DbError::Constraint("orphan EVM execution payload".into()));
            }
        }
        "reconciliation_scans" => {
            let record: ReconciliationScanProgress =
                decode_with_context(value.to_vec(), "invalid reconciliation scan")?;
            if key != reconciliation_scan_key(&record.target) {
                return Err(DbError::Constraint(
                    "reconciliation scan key mismatch".into(),
                ));
            }
            let target_exists = match record.target {
                ReconciliationTarget::Hold(id) => referenced_row_exists(
                    connection,
                    "reconciliation_holds",
                    &id.get().to_sql_bytes(),
                )?,
                ReconciliationTarget::FeePayout(id) => {
                    referenced_row_exists(connection, "fee_payouts", &id.to_sql_bytes())?
                }
            };
            if !target_exists {
                return Err(DbError::Constraint("orphan reconciliation scan".into()));
            }
        }
        "audit_events" => {
            let record: AuditEvent = decode_with_context(value.to_vec(), "invalid audit event")?;
            if key != record.sequence.to_sql_bytes() {
                return Err(DbError::Constraint("audit event key mismatch".into()));
            }
        }
        "fee_payouts" => {
            let record: crate::admin::FeePayoutRecord =
                decode_with_context(value.to_vec(), "invalid fee payout")?;
            if key != record.id.to_sql_bytes() {
                return Err(DbError::Constraint("fee payout key mismatch".into()));
            }
        }
        "deposit_owner_index" => {
            if value.len() != 32 || !referenced_row_exists(connection, "deposits", value)? {
                return Err(DbError::Constraint("orphan deposit owner index".into()));
            }
        }
        "operation_owner_index" => {
            let owner: OperationOwner =
                decode_with_context(value.to_vec(), "invalid operation owner")?;
            let OperationOwner::Deposit(deposit_id) = owner;
            if !referenced_row_exists(connection, "deposits", &deposit_id)? {
                return Err(DbError::Constraint("orphan operation owner".into()));
            }
        }
        "pull_pending_deposit_index" | "release_pending_withdrawal_index" => {
            let (primary, context) = if table == "pull_pending_deposit_index" {
                ("deposits", "orphan deposit pending index")
            } else {
                ("withdrawals", "orphan withdrawal pending index")
            };
            if !referenced_row_exists(connection, primary, key)? {
                return Err(DbError::Constraint(context.into()));
            }
        }
        "evm_state_index" => {
            expect_row_shape(key, value, 10, 1, "invalid EVM state index")?;
            if value != [0] {
                return Err(DbError::Constraint("invalid EVM state index".into()));
            }
            let id = u64::from_be_bytes(
                key[2..]
                    .try_into()
                    .map_err(|_| DbError::Constraint("invalid EVM state index".into()))?,
            );
            let record = connection
                .query_optional_scalar::<Vec<u8>>(
                    "SELECT value FROM evm_operations WHERE key = ?1",
                    params![id.to_sql_bytes()],
                )?
                .ok_or_else(|| DbError::Constraint("orphan EVM state index".into()))?;
            let record: EvmOperationRecord = decode_with_context(record, "invalid EVM operation")?;
            let expected = evm_state_index_key(&record)
                .map_err(|_| DbError::Constraint("invalid EVM state index".into()))?
                .ok_or_else(|| DbError::Constraint("stale EVM state index".into()))?;
            if key != expected.as_slice() {
                return Err(DbError::Constraint("stale EVM state index".into()));
            }
        }
        "open_hold_index" => {
            expect_row_shape(key, value, 8, 1, "invalid open hold index")?;
            if value != [0] {
                return Err(DbError::Constraint("invalid open hold index".into()));
            }
            let record = connection
                .query_optional_scalar::<Vec<u8>>(
                    "SELECT value FROM reconciliation_holds WHERE key = ?1",
                    params![key],
                )?
                .ok_or_else(|| DbError::Constraint("orphan open hold index".into()))?;
            let record: ReconciliationHoldRecord =
                decode_with_context(record, "invalid reconciliation hold")?;
            if !is_open_hold(&record) || key != record.id.get().to_sql_bytes() {
                return Err(DbError::Constraint("stale open hold index".into()));
            }
        }
        "owner_deposit_sequences" => {
            Principal::try_from_slice(key)
                .map_err(|_| DbError::Constraint("invalid owner sequence principal".into()))?;
            u64::from_sql_bytes(value.to_vec())
                .map_err(|_| DbError::Constraint("invalid owner sequence".into()))?;
        }
        "withdrawal_liability_index" => {
            expect_row_shape(key, value, 40, 32, "invalid withdrawal liability index")?;
            let record = connection
                .query_optional_scalar::<Vec<u8>>(
                    "SELECT value FROM withdrawals WHERE key = ?1",
                    params![value],
                )?
                .ok_or_else(|| DbError::Constraint("orphan withdrawal liability".into()))?;
            let record: WithdrawalRecord = decode_with_context(record, "invalid withdrawal")?;
            if !is_nonterminal_withdrawal(&record) || key != withdrawal_liability_key(&record) {
                return Err(DbError::Constraint("stale withdrawal liability".into()));
            }
        }
        "withdrawal_notification_index" => {
            expect_row_shape(key, value, 32, 32, "invalid withdrawal notification index")?;
            if !referenced_row_exists(connection, "withdrawals", value)? {
                return Err(DbError::Constraint(
                    "orphan withdrawal notification index".into(),
                ));
            }
        }
        "withdrawal_stop_reason_counts" => {
            std::str::from_utf8(key)
                .map_err(|_| DbError::Constraint("invalid withdrawal stop reason".into()))?;
            if u64::from_sql_bytes(value.to_vec())
                .map_err(|_| DbError::Constraint("invalid withdrawal stop reason count".into()))?
                == 0
            {
                return Err(DbError::Constraint(
                    "zero withdrawal stop reason count".into(),
                ));
            }
        }
        "settlement_jobs" => {
            expect_row_shape(key, value, 33, 1, "invalid settlement job validation row")?;
            let settlement_id: [u8; 32] = key[1..]
                .try_into()
                .map_err(|_| DbError::Constraint("invalid settlement job id".into()))?;
            let referenced = match key[0] {
                0 => referenced_row_exists(connection, "deposits", &settlement_id)?,
                1 => referenced_row_exists(connection, "withdrawals", &settlement_id)?,
                2 => {
                    let payout_id = fee_payout_id_from_job(settlement_id)
                        .map_err(|_| DbError::Constraint("invalid fee payout job id".into()))?;
                    referenced_row_exists(connection, "fee_payouts", &payout_id.to_sql_bytes())?
                }
                _ => return Err(DbError::Constraint("invalid settlement job kind".into())),
            };
            if !referenced {
                return Err(DbError::Constraint("orphan settlement job".into()));
            }
            let status = usize::from(value[0]);
            let count = progress
                .settlement_job_status_counts
                .get_mut(status)
                .ok_or_else(|| DbError::Constraint("invalid settlement job status".into()))?;
            *count = count
                .checked_add(1)
                .ok_or_else(|| DbError::Constraint("validation counter overflow".into()))?;
        }
        _ => {
            return Err(DbError::Constraint(format!(
                "unsupported validation table: {table}"
            )));
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for StableStore {
    fn drop(&mut self) {
        // DbHandle caches SQLite connections in thread-local storage. Force the
        // error path to close them before the test thread tears its TLS down.
        let _ = self
            .handle
            .update::<(), _>(|_| Err(DbError::Constraint("close test connection".into())));
    }
}

impl StableStore {
    pub fn deposit_reserve_token(&self) -> Result<DepositReserveToken, StorageError> {
        let counters = self.counters()?;
        let progress = self.external_progress()?;
        Ok(DepositReserveToken {
            nonterminal_withdrawals: self.table_count_value("withdrawal_liability_index")?,
            reserved_deposit_mint_amount: counters.reserved_deposit_mint_amount,
            reserved_deposit_mint_operations: counters.reserved_deposit_mint_operations,
            observation_generation: progress.reserve_observation_generation,
        })
    }

    pub fn init(memory: DefaultMemoryImpl) -> Result<Self, StorageError> {
        Self::init_with_config(memory, None)
    }

    pub fn init_configured(
        memory: DefaultMemoryImpl,
        config: &BridgeInitArgs,
    ) -> Result<Self, StorageError> {
        Self::init_with_config(memory, Some(config))
    }

    fn init_with_config(
        memory: DefaultMemoryImpl,
        config: Option<&BridgeInitArgs>,
    ) -> Result<Self, StorageError> {
        #[cfg(test)]
        reset_sqlite_test_runtime();
        if !backing_memory_is_empty(&memory) {
            return Err(StorageError::DatabaseFailure);
        }
        let handle = open_database(memory)?;
        handle.migrate(MIGRATIONS)?;
        verify_metadata(handle)?;
        initialize_singleton_state(handle, config)?;
        Self::attach_handle(handle)
    }

    fn attach_handle(handle: DbHandle) -> Result<Self, StorageError> {
        let handle = RevisionedHandle(handle);
        Ok(Self {
            handle,
            accounting: SqlCell::load(handle, SingletonColumn::Accounting)?,
            deposits: SqlMap::new(handle, "deposits"),
            withdrawals: SqlMap::new(handle, "withdrawals"),
            evm_operations: SqlMap::new(handle, "evm_operations"),
            reconciliation_holds: SqlMap::new(handle, "reconciliation_holds"),
            counters: SqlCell::load(handle, SingletonColumn::Counters)?,
            external_progress: SqlCell::load(handle, SingletonColumn::ExternalProgress)?,
            evm_execution_payloads: SqlMap::new(handle, "evm_execution_payloads"),
            reconciliation_scans: SqlMap::new(handle, "reconciliation_scans"),
            config: SqlCell::load(handle, SingletonColumn::Config)?,
            admin_state: SqlCell::load(handle, SingletonColumn::AdminState)?,
            audit_events: SqlMap::new(handle, "audit_events"),
            fee_payouts: SqlMap::new(handle, "fee_payouts"),
            deposit_owner_index: SqlMap::new(handle, "deposit_owner_index"),
            deposit_admission: SqlCell::load(handle, SingletonColumn::DepositAdmission)?,
            operation_owner_index: SqlMap::new(handle, "operation_owner_index"),
            evm_state_index: SqlMap::new(handle, "evm_state_index"),
            pull_pending_deposit_index: SqlMap::new(handle, "pull_pending_deposit_index"),
            release_pending_withdrawal_index: SqlMap::new(
                handle,
                "release_pending_withdrawal_index",
            ),
            withdrawal_notification_index: SqlMap::new(handle, "withdrawal_notification_index"),
            open_hold_index: SqlMap::new(handle, "open_hold_index"),
            owner_deposit_sequences: SqlMap::new(handle, "owner_deposit_sequences"),
            audit_retention: SqlCell::load(handle, SingletonColumn::AuditRetention)?,
            settlement_admission: SqlCell::load(handle, SingletonColumn::SettlementAdmission)?,
            confirmation_scheduler_health: SqlCell::load(
                handle,
                SingletonColumn::ConfirmationSchedulerHealth,
            )?,
        })
    }

    pub fn reopen(memory: DefaultMemoryImpl) -> Result<Self, StorageError> {
        #[cfg(test)]
        reset_sqlite_test_runtime();
        let handle = open_database(memory)?;
        Self::reopen_handle(handle)
    }

    fn reopen_handle(handle: DbHandle) -> Result<Self, StorageError> {
        verify_metadata(handle)?;
        let store = Self::attach_handle(handle)?;
        store.validate_singletons()?;
        Ok(store)
    }

    pub fn reopen_after_upgrade(memory: DefaultMemoryImpl) -> Result<Self, StorageError> {
        Self::reopen(memory)
    }

    fn validate_singletons(&self) -> Result<(), StorageError> {
        self.accounting()?;
        self.counters()?;
        self.external_progress()?;
        self.config()?;
        decode::<Option<AdminState>>(&self.admin_state.get()?)?;
        self.deposit_admission()?;
        decode::<AuditRetentionState>(&self.audit_retention.get()?)?;
        decode::<SettlementAdmissionControl>(&self.settlement_admission.get()?)?;
        decode::<ConfirmationSchedulerHealth>(&self.confirmation_scheduler_health.get()?)?;
        let (revision, liability_amount, validation) = self.handle.query(|connection| {
            connection.query_one(
                "SELECT storage_revision, withdrawal_liability_amount, storage_validation
                 FROM singleton_state WHERE id = 1",
                params![],
                |row| {
                    Ok((
                        row.get::<Vec<u8>>(0)?,
                        row.get::<Vec<u8>>(1)?,
                        row.get::<Option<Vec<u8>>>(2)?,
                    ))
                },
            )
        })?;
        u64::from_sql_bytes(revision).map_err(|_| StorageError::DecodeFailed)?;
        u128::from_sql_bytes(liability_amount).map_err(|_| StorageError::DecodeFailed)?;
        if let Some(validation) = validation {
            decode_with_context::<StorageValidationProgress>(
                validation,
                "invalid storage validation progress",
            )
            .map_err(StorageError::from)?;
        }
        Ok(())
    }

    pub fn start_storage_validation(
        &self,
    ) -> Result<StorageValidationStatus, StorageMaintenanceError> {
        self.validate_singletons()
            .map_err(|_| StorageMaintenanceError::StorageFailure)?;
        self.handle
            .update(|connection| {
                let revision = read_storage_revision(connection)?;
                let progress = StorageValidationProgress {
                    expected_revision: revision
                        .checked_add(1)
                        .ok_or_else(|| DbError::Constraint("storage revision overflow".into()))?,
                    phase: 0,
                    cursor: None,
                    phase_rows: 0,
                    scanned_rows: 0,
                    pending_ledger_operations: 0,
                    nonterminal_withdrawals: 0,
                    pending_evm_operations: 0,
                    reconciliation_holds: 0,
                    reserved_deposit_mint_amount: 0,
                    reserved_deposit_mint_operations: 0,
                    settlement_job_status_counts: [0; 4],
                };
                connection.execute(
                    "UPDATE singleton_state SET storage_validation = ?1 WHERE id = 1",
                    params![encode(&progress)
                        .map_err(|_| DbError::Constraint("validation encoding failed".into()))?
                        .to_sql_bytes()],
                )?;
                Ok(StorageValidationStatus {
                    complete: false,
                    phase: VALIDATION_TABLES[0].to_owned(),
                    scanned_rows: 0,
                })
            })
            .map_err(|_| StorageMaintenanceError::StorageFailure)
    }

    pub fn continue_storage_validation(
        &self,
        max_rows: u16,
    ) -> Result<StorageValidationStatus, StorageMaintenanceError> {
        if !(1..=MAX_VALIDATION_ROWS).contains(&max_rows) {
            return Err(StorageMaintenanceError::InvalidArgument {
                message: format!("max_rows must be between 1 and {MAX_VALIDATION_ROWS}"),
            });
        }
        let started = self
            .handle
            .query(|connection| {
                connection.query_optional_scalar::<i64>(
                    "SELECT 1 FROM singleton_state
                     WHERE id = 1 AND storage_validation IS NOT NULL",
                    params![],
                )
            })
            .map_err(|_| StorageMaintenanceError::StorageFailure)?
            .is_some();
        if !started {
            return Err(StorageMaintenanceError::NotStarted);
        }
        let outcome = self
            .handle
            .update(|connection| {
                let stored = connection
                    .query_optional_scalar::<Vec<u8>>(
                        "SELECT storage_validation FROM singleton_state WHERE id = 1",
                        params![],
                    )?
                    .ok_or_else(|| DbError::Constraint("validation not started".into()))?;
                let mut progress: StorageValidationProgress =
                    decode_with_context(stored, "invalid storage validation progress")?;
                let revision = read_storage_revision(connection)?;
                if revision != progress.expected_revision {
                    connection.execute(
                        "UPDATE singleton_state SET storage_validation = NULL WHERE id = 1",
                        params![],
                    )?;
                    return Ok(ValidationChunkOutcome::StateChanged);
                }
                let table = VALIDATION_TABLES
                    .get(usize::from(progress.phase))
                    .ok_or_else(|| DbError::Constraint("invalid validation phase".into()))?;
                let rows = if *table == "settlement_jobs" {
                    let sql = if progress.cursor.is_some() {
                        "SELECT settlement_kind, settlement_id, status FROM settlement_jobs
                         WHERE settlement_kind > ?1
                            OR (settlement_kind = ?1 AND settlement_id > ?2)
                         ORDER BY settlement_kind, settlement_id LIMIT ?3"
                    } else {
                        "SELECT settlement_kind, settlement_id, status FROM settlement_jobs
                         ORDER BY settlement_kind, settlement_id LIMIT ?1"
                    };
                    let decode_job_row = |row: &ic_sqlite_vfs::db::Row| {
                        let kind = row.get::<i64>(0)?;
                        let mut key = vec![u8::try_from(kind).map_err(|_| {
                            DbError::Constraint("invalid settlement job kind".into())
                        })?];
                        key.extend_from_slice(&row.get::<Vec<u8>>(1)?);
                        let status = u8::try_from(row.get::<i64>(2)?).map_err(|_| {
                            DbError::Constraint("invalid settlement job status".into())
                        })?;
                        Ok((key, vec![status]))
                    };
                    if let Some(cursor) = progress.cursor.clone() {
                        let (&kind, id) = cursor.split_first().ok_or_else(|| {
                            DbError::Constraint("invalid settlement job cursor".into())
                        })?;
                        connection.query_all(
                            sql,
                            params![i64::from(kind), id, i64::from(max_rows)],
                            decode_job_row,
                        )?
                    } else {
                        connection.query_all(sql, params![i64::from(max_rows)], decode_job_row)?
                    }
                } else {
                    let sql = if progress.cursor.is_some() {
                        format!(
                            "SELECT key, value FROM {table} WHERE key > ?1 ORDER BY key LIMIT ?2"
                        )
                    } else {
                        format!("SELECT key, value FROM {table} ORDER BY key LIMIT ?1")
                    };
                    if let Some(cursor) = progress.cursor.clone() {
                        connection.query_all(&sql, params![cursor, i64::from(max_rows)], |row| {
                            Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?))
                        })?
                    } else {
                        connection.query_all(&sql, params![i64::from(max_rows)], |row| {
                            Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?))
                        })?
                    }
                };
                for (key, value) in &rows {
                    validate_storage_row(connection, table, key, value, &mut progress)?;
                }
                progress.phase_rows = progress
                    .phase_rows
                    .checked_add(rows.len() as u64)
                    .ok_or_else(|| DbError::Constraint("validation row overflow".into()))?;
                progress.scanned_rows = progress
                    .scanned_rows
                    .checked_add(rows.len() as u64)
                    .ok_or_else(|| DbError::Constraint("validation row overflow".into()))?;
                progress.cursor = rows.last().map(|(key, _)| key.clone());
                if rows.len() < usize::from(max_rows) {
                    if *table == "settlement_jobs" {
                        let recorded = connection.query_all(
                            "SELECT status, count FROM settlement_job_status_counts
                             ORDER BY status",
                            params![],
                            |row| Ok((row.get::<i64>(0)?, row.get::<i64>(1)?)),
                        )?;
                        let mut recorded_counts = [0u64; 4];
                        for (status, count) in recorded {
                            let status = usize::try_from(status).map_err(|_| {
                                DbError::Constraint("invalid settlement job status".into())
                            })?;
                            *recorded_counts.get_mut(status).ok_or_else(|| {
                                DbError::Constraint("invalid settlement job status".into())
                            })? = u64::try_from(count).map_err(|_| {
                                DbError::Constraint("invalid settlement job count".into())
                            })?;
                        }
                        if recorded_counts != progress.settlement_job_status_counts {
                            return Err(DbError::Constraint(
                                "settlement job status count mismatch".into(),
                            ));
                        }
                    } else {
                        let recorded = connection.query_scalar::<Vec<u8>>(
                            "SELECT count FROM table_counts WHERE name = ?1",
                            params![*table],
                        )?;
                        let recorded = u64::from_sql_bytes(recorded)
                            .map_err(|_| DbError::Constraint("invalid table count".into()))?;
                        if recorded != progress.phase_rows {
                            return Err(DbError::Constraint("table count mismatch".into()));
                        }
                    }
                    progress.phase += 1;
                    progress.phase_rows = 0;
                    progress.cursor = None;
                }
                let complete = usize::from(progress.phase) == VALIDATION_TABLES.len();
                if complete {
                    let counters = connection.query_scalar::<Vec<u8>>(
                        "SELECT counters FROM singleton_state WHERE id = 1",
                        params![],
                    )?;
                    let counters: CounterState =
                        decode_with_context(counters, "invalid counters during validation")?;
                    let index_count = |name: &str| -> Result<u64, DbError> {
                        let raw = connection.query_scalar::<Vec<u8>>(
                            "SELECT count FROM table_counts WHERE name = ?1",
                            params![name],
                        )?;
                        u64::from_sql_bytes(raw)
                            .map_err(|_| DbError::Constraint("invalid index count".into()))
                    };
                    if counters.pending_ledger_operations != progress.pending_ledger_operations
                        || index_count("withdrawal_liability_index")?
                            != progress.nonterminal_withdrawals
                        || index_count("evm_state_index")? != progress.pending_evm_operations
                        || index_count("open_hold_index")? != progress.reconciliation_holds
                        || counters.reserved_deposit_mint_amount
                            != progress.reserved_deposit_mint_amount
                        || counters.reserved_deposit_mint_operations
                            != progress.reserved_deposit_mint_operations
                    {
                        return Err(DbError::Constraint("counter mismatch".into()));
                    }
                    connection.execute(
                        "UPDATE singleton_state SET storage_validation = NULL WHERE id = 1",
                        params![],
                    )?;
                } else {
                    progress.expected_revision = revision
                        .checked_add(1)
                        .ok_or_else(|| DbError::Constraint("storage revision overflow".into()))?;
                    connection.execute(
                        "UPDATE singleton_state SET storage_validation = ?1 WHERE id = 1",
                        params![encode(&progress)
                            .map_err(|_| {
                                DbError::Constraint("validation encoding failed".into())
                            })?
                            .to_sql_bytes()],
                    )?;
                }
                Ok(ValidationChunkOutcome::Status(StorageValidationStatus {
                    complete,
                    phase: if complete {
                        "complete".to_owned()
                    } else {
                        VALIDATION_TABLES[usize::from(progress.phase)].to_owned()
                    },
                    scanned_rows: progress.scanned_rows,
                }))
            })
            .map_err(|_| StorageMaintenanceError::StorageFailure)?;
        match outcome {
            ValidationChunkOutcome::Status(status) => Ok(status),
            ValidationChunkOutcome::StateChanged => Err(StorageMaintenanceError::StateChanged),
        }
    }

    pub fn storage_integrity_check(&self) -> Result<String, StorageMaintenanceError> {
        self.handle
            .integrity_check()
            .map_err(|_| StorageMaintenanceError::StorageFailure)
    }

    pub fn refresh_storage_checksum(
        &mut self,
        max_bytes: u64,
    ) -> Result<ChecksumRefreshStatus, StorageMaintenanceError> {
        if !(1..=MAX_CHECKSUM_REFRESH_BYTES).contains(&max_bytes) {
            return Err(StorageMaintenanceError::InvalidArgument {
                message: format!("max_bytes must be between 1 and {MAX_CHECKSUM_REFRESH_BYTES}"),
            });
        }
        self.handle
            .refresh_checksum_chunk(max_bytes)
            .map(|status| ChecksumRefreshStatus {
                complete: status.complete,
                checksum: status.checksum,
                scanned_bytes: status.scanned_bytes,
                db_size: status.db_size,
            })
            .map_err(|_| StorageMaintenanceError::StorageFailure)
    }

    #[cfg(feature = "test-deployment")]
    pub fn seed_storage_test_data(
        &mut self,
        start: u64,
        count: u16,
    ) -> Result<u16, StorageMaintenanceError> {
        if !(1..=100).contains(&count)
            || start
                .checked_add(u64::from(count))
                .is_none_or(|end| end > MAX_AUDIT_EVENTS)
        {
            return Err(StorageMaintenanceError::InvalidArgument {
                message: "seed range must be within 0..10000 and count within 1..100".into(),
            });
        }
        let mut counters = self
            .counters()
            .map_err(|_| StorageMaintenanceError::StorageFailure)?;
        let mut rows = Vec::with_capacity(usize::from(count));
        for offset in 0..u64::from(count) {
            let ordinal = start + offset;
            let mut hasher = Sha256::new();
            hasher.update(b"KINIC_BRIDGE_STORAGE_SEED_V1");
            hasher.update(ordinal.to_be_bytes());
            let id: [u8; 32] = hasher.finalize().into();
            let withdrawal = WithdrawalRecord::observed(
                WithdrawalId::new(id),
                [0; 20],
                vec![1],
                [0; 32],
                id,
                Amount::new(100),
                Amount::new(20),
                Amount::new(10),
                Amount::new(90),
                ordinal,
            )
            .map_err(|_| StorageMaintenanceError::StorageFailure)?;
            let operation = EvmOperationRecord::prepared(
                bridge_core::EvmOperationId::new(ordinal),
                id,
                EvmOperationKind::MintDeposit,
            );
            let mut envelope = EvmCallIntent {
                operation_id: operation.id,
                payload_hash: id,
                chain_id: 8453,
                contract: [1; 20],
                calldata: vec![1, 2, 3, 4],
                gas_limit: 100_000,
                max_fee_per_gas: 10,
                max_priority_fee_per_gas: 1,
            }
            .assign_nonce(ordinal);
            envelope.signed_transaction = Some(vec![1, 2, 3]);
            let sequence = counters.next_audit_sequence;
            counters.next_audit_sequence =
                bridge_core::audit_next(sequence).ok_or(StorageMaintenanceError::StorageFailure)?;
            rows.push((
                id,
                encode(&withdrawal).map_err(|_| StorageMaintenanceError::StorageFailure)?,
                ordinal,
                encode(&operation).map_err(|_| StorageMaintenanceError::StorageFailure)?,
                encode(&EvmExecutionPayload::Prepared(envelope))
                    .map_err(|_| StorageMaintenanceError::StorageFailure)?,
                evm_state_index_key(&operation)
                    .map_err(|_| StorageMaintenanceError::StorageFailure)?
                    .ok_or(StorageMaintenanceError::StorageFailure)?,
                sequence,
                encode(&AuditEvent {
                    sequence,
                    timestamp_ns: ordinal,
                    caller: Principal::anonymous(),
                    kind: AuditEventKind::ReserveGateChanged { sufficient: true },
                })
                .map_err(|_| StorageMaintenanceError::StorageFailure)?,
            ));
        }
        let counters_blob =
            encode(&counters).map_err(|_| StorageMaintenanceError::StorageFailure)?;
        self.handle
            .update(|connection| {
                for (
                    id,
                    withdrawal,
                    operation_id,
                    operation,
                    execution_payload,
                    evm_index,
                    sequence,
                    audit,
                ) in &rows
                {
                    replace_withdrawal_row(connection, id.to_sql_bytes(), None, withdrawal)?;
                    connection.execute(
                        "INSERT INTO settlement_jobs(
                            settlement_kind, settlement_id, operation_id, phase, status,
                            next_run_at_ns, confirmation_checks, confirmation_started_at_ns,
                            lease_generation, lease_until_ns, last_error_code,
                            last_error_detail, updated_at_ns
                         ) VALUES(1, ?1, NULL, 1, 0, ?2, 0, NULL,
                            X'0000000000000000', NULL, NULL, NULL, ?2)",
                        params![id.to_sql_bytes(), u64::MAX.to_sql_bytes()],
                    )?;
                    connection.execute(
                        "INSERT INTO evm_operations(key, value) VALUES(?1, ?2)",
                        params![operation_id.to_sql_bytes(), operation.to_sql_bytes()],
                    )?;
                    increment_table_count(connection, "evm_operations")?;
                    connection.execute(
                        "INSERT INTO evm_execution_payloads(key, value) VALUES(?1, ?2)",
                        params![
                            operation_id.to_sql_bytes(),
                            execution_payload.to_sql_bytes()
                        ],
                    )?;
                    increment_table_count(connection, "evm_execution_payloads")?;
                    transition_tracked_entry(
                        connection,
                        "evm_state_index",
                        None,
                        Some((evm_index.to_sql_bytes(), 0u8.to_sql_bytes())),
                    )?;
                    connection.execute(
                        "INSERT INTO audit_events(key, value) VALUES(?1, ?2)",
                        params![sequence.to_sql_bytes(), audit.to_sql_bytes()],
                    )?;
                    increment_table_count(connection, "audit_events")?;
                }
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                    params![counters_blob.to_sql_bytes()],
                )
            })
            .map_err(|_| StorageMaintenanceError::StorageFailure)?;
        Ok(count)
    }

    #[cfg(feature = "test-deployment")]
    pub fn first_prepared_evm_test_id(&self) -> Result<Option<u64>, StorageMaintenanceError> {
        self.first_prepared_evm()
            .map(|entry| entry.map(|(operation, _)| operation.id.get()))
            .map_err(|_| StorageMaintenanceError::StorageFailure)
    }

    #[cfg(test)]
    fn validate_relations(&self) -> Result<(), StorageError> {
        const COUNTED_TABLES: &[&str] = &[
            "deposits",
            "withdrawals",
            "evm_operations",
            "reconciliation_holds",
            "evm_execution_payloads",
            "reconciliation_scans",
            "audit_events",
            "fee_payouts",
            "deposit_owner_index",
            "operation_owner_index",
            "evm_state_index",
            "pull_pending_deposit_index",
            "release_pending_withdrawal_index",
            "open_hold_index",
            "owner_deposit_sequences",
        ];
        self.handle.query(|connection| {
            for table in COUNTED_TABLES {
                let actual = connection
                    .query_scalar::<i64>(&format!("SELECT COUNT(*) FROM {table}"), params![])?;
                let recorded = connection.query_scalar::<Vec<u8>>(
                    "SELECT count FROM table_counts WHERE name = ?1",
                    params![*table],
                )?;
                let recorded = u64::from_sql_bytes(recorded)
                    .map_err(|_| DbError::Constraint("invalid table count".into()))?;
                if u64::try_from(actual).ok() != Some(recorded) {
                    return Err(DbError::Constraint("table count mismatch".into()));
                }
            }
            Ok(())
        })?;

        let deposits = self.handle.query(|connection| {
            connection.query_all("SELECT key, value FROM deposits", params![], |row| {
                Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?))
            })
        })?;
        let withdrawals = self.handle.query(|connection| {
            connection.query_all("SELECT key, value FROM withdrawals", params![], |row| {
                Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?))
            })
        })?;
        let holds = self.handle.query(|connection| {
            connection.query_all(
                "SELECT key, value FROM reconciliation_holds",
                params![],
                |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?)),
            )
        })?;
        let evm_operations = self.handle.query(|connection| {
            connection.query_all("SELECT key, value FROM evm_operations", params![], |row| {
                Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?))
            })
        })?;
        let evm_execution_payloads = self.handle.query(|connection| {
            connection.query_all(
                "SELECT key, value FROM evm_execution_payloads",
                params![],
                |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?)),
            )
        })?;

        let mut expected_pull = BTreeSet::new();
        let mut expected_release = BTreeSet::new();
        let mut expected_open_holds = BTreeSet::new();
        let mut expected_owners = BTreeSet::new();
        let mut expected_operations = BTreeMap::new();
        let mut pending_ledger_operations = 0u64;
        let mut reserved_deposit_mint_operations = 0u64;
        let mut reserved_deposit_mint_amount = 0u128;
        let mut nonterminal_withdrawals = 0u64;
        let mut reconciliation_holds = 0u64;

        for (key, bytes) in deposits {
            let key: [u8; 32] = key.try_into().map_err(|_| StorageError::DecodeFailed)?;
            let stored: StoredDeposit = decode(&StableBlob::new(bytes)?)?;
            let record = stored.record;
            if key != record.id.bytes() {
                return Err(StorageError::DecodeFailed);
            }
            if is_pending_deposit_ledger(&record) {
                expected_pull.insert(key);
                pending_ledger_operations = pending_ledger_operations
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
            }
            if is_deposit_mint_reserved(&record) {
                reserved_deposit_mint_operations = reserved_deposit_mint_operations
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
                reserved_deposit_mint_amount = reserved_deposit_mint_amount
                    .checked_add(record.reserved_mint_amount()?.get())
                    .ok_or(StorageError::CounterOverflow)?;
            }
            if let Some(operation_id) = deposit_operation_id(&record) {
                let owner = encode(&OperationOwner::Deposit(key))?.to_sql_bytes();
                expected_owners.insert((operation_id, owner.clone()));
                if expected_operations
                    .insert(
                        operation_id,
                        (owner, record.payload_hash, EvmOperationKind::MintDeposit),
                    )
                    .is_some()
                {
                    return Err(StorageError::DatabaseFailure);
                }
            }
        }
        for (key, bytes) in withdrawals {
            let key: [u8; 32] = key.try_into().map_err(|_| StorageError::DecodeFailed)?;
            let record: WithdrawalRecord = decode(&StableBlob::new(bytes)?)?;
            if key != record.id.bytes() {
                return Err(StorageError::DecodeFailed);
            }
            if is_pending_withdrawal_ledger(&record) {
                expected_release.insert(key);
                pending_ledger_operations = pending_ledger_operations
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
            }
            if is_nonterminal_withdrawal(&record) {
                nonterminal_withdrawals = nonterminal_withdrawals
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
            }
        }
        for (key, bytes) in holds {
            let key = u64::from_sql_bytes(key).map_err(|_| StorageError::DecodeFailed)?;
            let record: ReconciliationHoldRecord = decode(&StableBlob::new(bytes)?)?;
            if key != record.id.get() {
                return Err(StorageError::DecodeFailed);
            }
            if is_open_hold(&record) {
                expected_open_holds.insert(key);
                reconciliation_holds = reconciliation_holds
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
            }
        }

        let mut expected_evm_state_index = BTreeSet::new();
        let mut expected_payload_ids = BTreeSet::new();
        let mut pending_evm_operations = 0u64;
        let mut unresolved_evm_reverts = 0u64;
        let mut awaiting_nonce_evm_operations = 0u64;
        let mut decoded_evm_operations = BTreeMap::new();
        for (key, bytes) in evm_operations {
            let key = u64::from_sql_bytes(key).map_err(|_| StorageError::DecodeFailed)?;
            let record: EvmOperationRecord = decode(&StableBlob::new(bytes)?)?;
            if key != record.id.get() {
                return Err(StorageError::DecodeFailed);
            }
            decoded_evm_operations.insert(key, record);
            if let Some(index_key) = evm_state_index_key(&record)? {
                expected_evm_state_index.insert((index_key, 0u8));
            }
            if is_pending_evm(&record) {
                pending_evm_operations = pending_evm_operations
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
            }
            if is_reverted_evm(&record) {
                unresolved_evm_reverts = unresolved_evm_reverts
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
            }
            if matches!(record.state, EvmOperationState::Queued) {
                awaiting_nonce_evm_operations = awaiting_nonce_evm_operations
                    .checked_add(1)
                    .ok_or(StorageError::CounterOverflow)?;
            }

            let parent = expected_operations.get(&key);
            match record.state {
                EvmOperationState::Queued
                | EvmOperationState::Prepared
                | EvmOperationState::Submitted { .. } => {
                    if !parent.is_some_and(|(_, payload_hash, kind)| {
                        *payload_hash == record.payload_hash && *kind == record.kind
                    }) {
                        return Err(StorageError::DatabaseFailure);
                    }
                    if matches!(
                        record.state,
                        EvmOperationState::Queued | EvmOperationState::Prepared
                    ) {
                        expected_payload_ids.insert(key);
                    }
                }
                EvmOperationState::Confirmed { .. }
                | EvmOperationState::Reverted { .. }
                | EvmOperationState::RecoveryPending { .. }
                | EvmOperationState::Recovered { .. } => {
                    if parent.is_some() {
                        return Err(StorageError::DatabaseFailure);
                    }
                }
            }
        }
        for (id, operation) in &decoded_evm_operations {
            match &operation.state {
                EvmOperationState::RecoveryPending {
                    replacement_operation_id,
                    ..
                } => {
                    let replacement = decoded_evm_operations
                        .get(&replacement_operation_id.get())
                        .ok_or(StorageError::DatabaseFailure)?;
                    if replacement.recovery_of != Some(operation.id)
                        || replacement.kind != operation.kind
                        || replacement.payload_hash != operation.payload_hash
                        || !is_pending_evm(replacement)
                    {
                        return Err(StorageError::DatabaseFailure);
                    }
                }
                EvmOperationState::Recovered { resolution, .. } => {
                    if let Some(replacement_id) = resolution.replacement_operation_id() {
                        let replacement = decoded_evm_operations
                            .get(&replacement_id.get())
                            .ok_or(StorageError::DatabaseFailure)?;
                        if replacement.recovery_of != Some(operation.id)
                            || replacement.kind != operation.kind
                            || replacement.payload_hash != operation.payload_hash
                            || is_pending_evm(replacement)
                        {
                            return Err(StorageError::DatabaseFailure);
                        }
                    }
                }
                _ => {}
            }
            if let Some(reverted_id) = operation.recovery_of {
                let predecessor = decoded_evm_operations
                    .get(&reverted_id.get())
                    .ok_or(StorageError::DatabaseFailure)?;
                let linked = match &predecessor.state {
                    EvmOperationState::RecoveryPending {
                        replacement_operation_id,
                        ..
                    } => *replacement_operation_id == operation.id,
                    EvmOperationState::Recovered { resolution, .. } => {
                        resolution.replacement_operation_id() == Some(operation.id)
                    }
                    _ => false,
                };
                if !linked || *id == reverted_id.get() {
                    return Err(StorageError::DatabaseFailure);
                }
            }
        }

        let mut actual_payload_ids = BTreeSet::new();
        for (key, bytes) in evm_execution_payloads {
            let key = u64::from_sql_bytes(key).map_err(|_| StorageError::DecodeFailed)?;
            let payload: EvmExecutionPayload = decode(&StableBlob::new(bytes)?)?;
            let operation = self
                .evm_operation(key)?
                .ok_or(StorageError::DatabaseFailure)?;
            let valid = match (&operation.state, &payload) {
                (EvmOperationState::Queued, EvmExecutionPayload::AwaitingNonce(intent)) => {
                    intent.operation_id == operation.id
                        && intent.payload_hash == operation.payload_hash
                }
                (EvmOperationState::Prepared, EvmExecutionPayload::Prepared(envelope)) => {
                    envelope.operation_id == operation.id
                        && envelope.payload_hash == operation.payload_hash
                }
                (EvmOperationState::Submitted { .. }, EvmExecutionPayload::Prepared(envelope)) => {
                    envelope.operation_id == operation.id
                        && envelope.payload_hash == operation.payload_hash
                        && envelope.signed_transaction.is_some()
                }
                _ => false,
            };
            if !valid {
                return Err(StorageError::DatabaseFailure);
            }
            if matches!(operation.state, EvmOperationState::Submitted { .. }) {
                expected_payload_ids.insert(key);
            }
            actual_payload_ids.insert(key);
        }

        let actual_pull = self
            .pull_pending_deposit_index
            .iter()
            .map(|entry| *entry.key())
            .collect();
        let actual_release = self
            .release_pending_withdrawal_index
            .iter()
            .map(|entry| *entry.key())
            .collect();
        let actual_open_holds = self
            .open_hold_index
            .iter()
            .map(|entry| *entry.key())
            .collect();
        let actual_owners = self
            .operation_owner_index
            .iter()
            .map(|entry| (*entry.key(), entry.value().to_sql_bytes()))
            .collect();
        let actual_evm_state_index = self
            .evm_state_index
            .iter()
            .map(|entry| (entry.key().clone(), entry.value()))
            .collect();
        if expected_pull != actual_pull
            || expected_release != actual_release
            || expected_open_holds != actual_open_holds
            || expected_owners != actual_owners
            || expected_evm_state_index != actual_evm_state_index
            || expected_payload_ids != actual_payload_ids
        {
            return Err(StorageError::DatabaseFailure);
        }
        let counters = self.counters()?;
        if self.table_count_value("withdrawal_liability_index")? != nonterminal_withdrawals
            || self.table_count_value("open_hold_index")? != reconciliation_holds
            || self.table_count_value("evm_state_index")? != pending_evm_operations
            || counters.pending_ledger_operations != pending_ledger_operations
            || counters.reserved_deposit_mint_operations != reserved_deposit_mint_operations
            || counters.reserved_deposit_mint_amount != reserved_deposit_mint_amount
            || counters.unresolved_evm_reverts != unresolved_evm_reverts
            || counters.awaiting_nonce_evm_operations != awaiting_nonce_evm_operations
        {
            return Err(StorageError::DatabaseFailure);
        }
        Ok(())
    }

    pub fn schema_version(&self) -> u16 {
        SCHEMA_VERSION
    }

    pub fn accounting(&self) -> Result<AccountingState, StorageError> {
        decode(&self.accounting.get()?)
    }

    pub fn set_accounting(&mut self, value: &AccountingState) -> Result<(), StorageError> {
        self.accounting.set(encode(value)?)
    }

    pub fn counters(&self) -> Result<CounterState, StorageError> {
        decode(&self.counters.get()?)
    }

    fn deposit_admission(&self) -> Result<DepositAdmissionControl, StorageError> {
        decode(&self.deposit_admission.get()?)
    }

    fn set_deposit_admission(
        &mut self,
        value: &DepositAdmissionControl,
    ) -> Result<(), StorageError> {
        self.deposit_admission.set(encode(value)?)
    }

    pub fn reserve_settlement_quota(
        &mut self,
        caller: Principal,
        record_key: Vec<u8>,
        now_ns: u64,
        limits: SettlementQuotaLimits,
    ) -> Result<(), SettlementAdmissionError> {
        let window_ns = limits.window_seconds.saturating_mul(1_000_000_000);
        let window_id = now_ns / window_ns;
        let admission_blob = self
            .settlement_admission
            .get()
            .map_err(|_| SettlementAdmissionError::Storage)?;
        let mut admission = decode::<SettlementAdmissionControl>(&admission_blob)
            .map_err(|_| SettlementAdmissionError::Storage)?;
        if admission.window_id != window_id {
            admission = SettlementAdmissionControl {
                window_id,
                ..SettlementAdmissionControl::default()
            };
        }
        let caller_count = admission
            .caller_counts
            .iter()
            .find(|entry| entry.caller == caller)
            .map(|entry| entry.count)
            .unwrap_or(0);
        let record_count = admission
            .record_counts
            .iter()
            .find(|entry| entry.key == record_key)
            .map(|entry| entry.count)
            .unwrap_or(0);
        let retry_after_seconds = ((window_id + 1)
            .saturating_mul(window_ns)
            .saturating_sub(now_ns)
            .saturating_add(999_999_999)
            / 1_000_000_000)
            .max(1);
        if admission.global_count >= limits.global
            || caller_count >= limits.per_principal
            || record_count >= limits.per_record
        {
            return Err(SettlementAdmissionError::RateLimited {
                retry_after_seconds,
            });
        }
        admission.global_count = admission
            .global_count
            .checked_add(1)
            .ok_or(SettlementAdmissionError::Storage)?;
        match admission
            .caller_counts
            .iter_mut()
            .find(|entry| entry.caller == caller)
        {
            Some(entry) => {
                entry.count = entry
                    .count
                    .checked_add(1)
                    .ok_or(SettlementAdmissionError::Storage)?
            }
            None => admission
                .caller_counts
                .push(SettlementCallerQuota { caller, count: 1 }),
        }
        match admission
            .record_counts
            .iter_mut()
            .find(|entry| entry.key == record_key)
        {
            Some(entry) => {
                entry.count = entry
                    .count
                    .checked_add(1)
                    .ok_or(SettlementAdmissionError::Storage)?
            }
            None => admission.record_counts.push(SettlementRecordQuota {
                key: record_key,
                count: 1,
            }),
        }
        self.settlement_admission
            .set(encode(&admission).map_err(|_| SettlementAdmissionError::Storage)?)
            .map_err(|_| SettlementAdmissionError::Storage)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_manual_settlement_job(
        &mut self,
        kind: SettlementJobKind,
        settlement_id: [u8; 32],
        caller: Principal,
        now_ns: u64,
        lease_until_ns: u64,
        overdue_after_ns: u64,
        limits: SettlementQuotaLimits,
    ) -> Result<ManualSettlementClaim, SettlementAdmissionError> {
        self.claim_settlement_job_with_mode(
            kind,
            settlement_id,
            caller,
            now_ns,
            lease_until_ns,
            overdue_after_ns,
            limits,
            false,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_confirmation_settlement_job(
        &mut self,
        kind: SettlementJobKind,
        settlement_id: [u8; 32],
        caller: Principal,
        now_ns: u64,
        lease_until_ns: u64,
        overdue_after_ns: u64,
        limits: SettlementQuotaLimits,
    ) -> Result<ManualSettlementClaim, SettlementAdmissionError> {
        self.claim_settlement_job_with_mode(
            kind,
            settlement_id,
            caller,
            now_ns,
            lease_until_ns,
            overdue_after_ns,
            limits,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn claim_settlement_job_with_mode(
        &mut self,
        kind: SettlementJobKind,
        settlement_id: [u8; 32],
        caller: Principal,
        now_ns: u64,
        lease_until_ns: u64,
        overdue_after_ns: u64,
        limits: SettlementQuotaLimits,
        explicit_confirmation: bool,
    ) -> Result<ManualSettlementClaim, SettlementAdmissionError> {
        let admission_blob = self
            .settlement_admission
            .get()
            .map_err(|_| SettlementAdmissionError::Storage)?;
        let mut admission = decode::<SettlementAdmissionControl>(&admission_blob)
            .map_err(|_| SettlementAdmissionError::Storage)?;
        let recovery_operation_id = match kind {
            SettlementJobKind::Deposit => self
                .deposit(settlement_id)
                .map_err(|_| SettlementAdmissionError::Storage)?
                .as_ref()
                .and_then(deposit_operation_id),
            SettlementJobKind::Withdrawal => None,
            SettlementJobKind::FeePayout => None,
        };
        let recovery_operation_id = match recovery_operation_id {
            Some(operation_id)
                if self
                    .evm_operation(operation_id)
                    .map_err(|_| SettlementAdmissionError::Storage)?
                    .is_some_and(|operation| {
                        matches!(operation.state, EvmOperationState::Submitted { .. })
                    }) =>
            {
                Some(operation_id)
            }
            _ => None,
        };
        let record_key = {
            let mut key = Vec::with_capacity(41);
            key.push(kind.sql() as u8);
            key.extend_from_slice(&settlement_id);
            if let Some(operation_id) = recovery_operation_id {
                key.extend_from_slice(&operation_id.to_be_bytes());
            }
            key
        };
        let outcome = self
            .handle
            .update(|connection| {
                let active = connection.query_all(
                    "SELECT settlement_kind, settlement_id, lease_until_ns
                     FROM settlement_jobs WHERE status = 1 AND lease_until_ns > ?1
                     ORDER BY lease_until_ns LIMIT 1",
                    params![now_ns.to_sql_bytes()],
                    |row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?, row.get::<Vec<u8>>(2)?)),
                )?;
                if let Some((active_kind, active_id, _)) = active.into_iter().next() {
                    if active_kind == kind.sql() && active_id == settlement_id {
                        return Ok(ManualClaimTransaction::AutomaticProgressPending(None));
                    }
                    return Ok(ManualClaimTransaction::Busy);
                }

                let target = connection.query_all(
                    "SELECT status, phase, next_run_at_ns, lease_generation, lease_until_ns
                     FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2",
                    params![kind.sql(), settlement_id.to_sql_bytes()],
                    |row| Ok((
                        row.get::<i64>(0)?,
                        row.get::<i64>(1)?,
                        row.get::<Option<Vec<u8>>>(2)?,
                        row.get::<Vec<u8>>(3)?,
                        row.get::<Option<Vec<u8>>>(4)?,
                    )),
                )?;
                if let Some((status, phase, next_raw, _, lease_raw)) = target.first() {
                    let next = next_raw
                        .clone()
                        .map(u64::from_sql_bytes)
                        .transpose()
                        .map_err(|_| DbError::Constraint("invalid next run time".into()))?;
                    let lease_until = lease_raw
                        .clone()
                        .map(u64::from_sql_bytes)
                        .transpose()
                        .map_err(|_| DbError::Constraint("invalid lease deadline".into()))?;
                    let confirmation = *phase == 0 || *status == 3;
                    let scheduled = *status == 0;
                    let active = *status == 1
                        && lease_until.is_some_and(|deadline| deadline > now_ns);
                    let stopped = *status == 2;
                    let overdue = scheduled
                        && next.is_some_and(|deadline| {
                            now_ns >= deadline.saturating_add(overdue_after_ns)
                        });
                    let expired = *status == 1
                        && lease_until.is_some_and(|deadline| deadline <= now_ns);
                    let allowed = if explicit_confirmation {
                        confirmation
                    } else {
                        matches!(
                            bridge_core::manual_claim_decision(
                                confirmation,
                                scheduled,
                                active,
                                stopped,
                                overdue,
                                expired,
                            ),
                            bridge_core::ManualClaimDecision::Allow
                        )
                    };
                    if !allowed {
                        return Ok(ManualClaimTransaction::AutomaticProgressPending(next));
                    }
                }

                let window_ns = limits.window_seconds.saturating_mul(1_000_000_000);
                let window_id = now_ns / window_ns;
                if admission.window_id != window_id {
                    admission = SettlementAdmissionControl { window_id, ..Default::default() };
                }
                let caller_count = admission.caller_counts.iter().find(|entry| entry.caller == caller).map_or(0, |entry| entry.count);
                let record_count = admission.record_counts.iter().find(|entry| entry.key == record_key).map_or(0, |entry| entry.count);
                let retry_after_seconds = ((window_id + 1).saturating_mul(window_ns).saturating_sub(now_ns).saturating_add(999_999_999) / 1_000_000_000).max(1);
                if admission.global_count >= limits.global || caller_count >= limits.per_principal || record_count >= limits.per_record {
                    return Ok(ManualClaimTransaction::RateLimited(retry_after_seconds));
                }
                admission.global_count = admission.global_count.checked_add(1).ok_or_else(|| DbError::Constraint("settlement quota overflow".into()))?;
                match admission.caller_counts.iter_mut().find(|entry| entry.caller == caller) {
                    Some(entry) => entry.count = entry.count.checked_add(1).ok_or_else(|| DbError::Constraint("caller quota overflow".into()))?,
                    None => admission.caller_counts.push(SettlementCallerQuota { caller, count: 1 }),
                }
                match admission.record_counts.iter_mut().find(|entry| entry.key == record_key) {
                    Some(entry) => entry.count = entry.count.checked_add(1).ok_or_else(|| DbError::Constraint("record quota overflow".into()))?,
                    None => admission.record_counts.push(SettlementRecordQuota { key: record_key.clone(), count: 1 }),
                }
                let admission_blob = encode(&admission).map_err(|_| DbError::Constraint("settlement quota encoding failed".into()))?;

                let generation = target
                    .first()
                    .map(|(_, _, _, raw, _)| u64::from_sql_bytes(raw.clone()))
                    .transpose()
                    .map_err(|_| DbError::Constraint("invalid lease generation".into()))?
                    .unwrap_or(0)
                    .checked_add(1)
                    .ok_or_else(|| DbError::Constraint("lease generation overflow".into()))?;
                if target.is_empty() {
                    if let Some(operation_id) = recovery_operation_id {
                        connection.execute(
                            "INSERT INTO settlement_jobs(settlement_kind, settlement_id, operation_id, phase,
                             status, next_run_at_ns, confirmation_checks, confirmation_started_at_ns,
                             lease_generation, lease_until_ns, last_error_code, last_error_detail, updated_at_ns)
                             VALUES(?1, ?2, ?3, 0, 1, NULL, 0, ?4, ?5, ?6, NULL, NULL, ?4)",
                            params![kind.sql(), settlement_id.to_sql_bytes(), operation_id.to_sql_bytes(),
                                now_ns.to_sql_bytes(), generation.to_sql_bytes(), lease_until_ns.to_sql_bytes()],
                        )?;
                    } else {
                        connection.execute(
                            "INSERT INTO settlement_jobs(settlement_kind, settlement_id, operation_id, phase,
                             status, next_run_at_ns, confirmation_checks, confirmation_started_at_ns,
                             lease_generation, lease_until_ns, last_error_code, last_error_detail, updated_at_ns)
                             VALUES(?1, ?2, NULL, 1, 1, NULL, 0, NULL, ?3, ?4, NULL, NULL, ?5)",
                            params![kind.sql(), settlement_id.to_sql_bytes(), generation.to_sql_bytes(),
                                lease_until_ns.to_sql_bytes(), now_ns.to_sql_bytes()],
                        )?;
                    }
                } else {
                    let reset_confirmation =
                        target
                            .first()
                            .is_some_and(|(status, phase, _, _, _)| {
                        *status == 2 && *phase == 0
                    });
                    connection.execute(
                        "UPDATE settlement_jobs SET status = 1, next_run_at_ns = NULL,
                         lease_generation = ?1, lease_until_ns = ?2,
                         confirmation_checks = CASE WHEN ?4 = 1 THEN 0 ELSE confirmation_checks END,
                         confirmation_started_at_ns = CASE WHEN ?4 = 1 THEN ?3 ELSE confirmation_started_at_ns END,
                         last_error_code = NULL, last_error_detail = NULL, updated_at_ns = ?3
                         WHERE settlement_kind = ?5 AND settlement_id = ?6",
                        params![
                            generation.to_sql_bytes(),
                            lease_until_ns.to_sql_bytes(),
                            now_ns.to_sql_bytes(),
                            i64::from(reset_confirmation),
                            kind.sql(),
                            settlement_id.to_sql_bytes(),
                        ],
                    )?;
                }
                connection.execute(
                    "UPDATE singleton_state SET settlement_admission = ?1 WHERE id = 1",
                    params![admission_blob.to_sql_bytes()],
                )?;
                Ok(ManualClaimTransaction::Claimed(admission_blob))
            })
            .map_err(|_| SettlementAdmissionError::Storage)?;

        match outcome {
            ManualClaimTransaction::Claimed(_admission_blob) => {
                let job = self
                    .settlement_job(kind, settlement_id)
                    .map_err(|_| SettlementAdmissionError::Storage)?
                    .ok_or(SettlementAdmissionError::Storage)?;
                Ok(ManualSettlementClaim::Claimed(job))
            }
            ManualClaimTransaction::AutomaticProgressPending(next_run_at_ns) => {
                Ok(ManualSettlementClaim::AutomaticProgressPending { next_run_at_ns })
            }
            ManualClaimTransaction::Busy => Ok(ManualSettlementClaim::Busy),
            ManualClaimTransaction::RateLimited(retry_after_seconds) => {
                Err(SettlementAdmissionError::RateLimited {
                    retry_after_seconds,
                })
            }
        }
    }

    pub fn confirmation_schedule(
        &self,
        operation_id: u64,
    ) -> Result<Option<ConfirmationSchedule>, StorageError> {
        let rows = self.handle.query(|connection| {
            connection.query_all(
                "SELECT operation_id, confirmation_started_at_ns,
                    COALESCE(next_run_at_ns, lease_until_ns), confirmation_checks
             FROM settlement_jobs
             WHERE operation_id = ?1 AND phase = 0 AND status IN (0, 1, 3) LIMIT 1",
                params![operation_id.to_sql_bytes()],
                |row| {
                    Ok((
                        row.get::<Vec<u8>>(0)?,
                        row.get::<Vec<u8>>(1)?,
                        row.get::<Option<Vec<u8>>>(2)?,
                        row.get::<i64>(3)?,
                    ))
                },
            )
        })?;
        rows.into_iter()
            .next()
            .map(|(operation, submitted, next, checks)| {
                Ok(ConfirmationSchedule {
                    operation_id: u64::from_sql_bytes(operation)
                        .map_err(|_| StorageError::DecodeFailed)?,
                    submitted_at_ns: u64::from_sql_bytes(submitted)
                        .map_err(|_| StorageError::DecodeFailed)?,
                    next_check_at_ns: next
                        .map(u64::from_sql_bytes)
                        .transpose()
                        .map_err(|_| StorageError::DecodeFailed)?
                        .unwrap_or(0),
                    checks_completed: u8::try_from(checks)
                        .map_err(|_| StorageError::DecodeFailed)?,
                })
            })
            .transpose()
    }

    pub fn earliest_confirmation_schedule(
        &self,
    ) -> Result<Option<ConfirmationSchedule>, StorageError> {
        let rows = self.handle.query(|connection| {
            connection.query_all(
                "SELECT operation_id FROM settlement_jobs
                 WHERE status = 0 AND phase = 0 AND operation_id IS NOT NULL
             ORDER BY next_run_at_ns, settlement_kind, settlement_id LIMIT 1",
                params![],
                |row| row.get::<Vec<u8>>(0),
            )
        })?;
        let Some(operation) = rows.into_iter().next() else {
            return Ok(None);
        };
        self.confirmation_schedule(
            u64::from_sql_bytes(operation).map_err(|_| StorageError::DecodeFailed)?,
        )
    }

    pub fn confirmation_schedule_count(&self) -> u64 {
        self.handle
            .query(|connection| {
                connection.query_scalar::<i64>(
                    "SELECT COUNT(*) FROM settlement_jobs WHERE status = 0 AND phase = 0",
                    params![],
                )
            })
            .ok()
            .and_then(|count| u64::try_from(count).ok())
            .unwrap_or(0)
    }

    pub fn deposit_confirmation_schedule(
        &self,
        deposit_id: [u8; 32],
    ) -> Result<Option<ConfirmationSchedule>, StorageError> {
        let Some(record) = self.deposit(deposit_id)? else {
            return Ok(None);
        };
        let Some(operation_id) = deposit_operation_id(&record) else {
            return Ok(None);
        };
        self.confirmation_schedule(operation_id)
    }

    pub fn set_confirmation_schedule(
        &mut self,
        schedule: ConfirmationSchedule,
    ) -> Result<(), StorageError> {
        let owner = self
            .operation_owner_index
            .get(&schedule.operation_id)
            .ok_or(StorageError::RecordNotFound)?;
        let OperationOwner::Deposit(id) = decode::<OperationOwner>(&owner)?;
        let kind = SettlementJobKind::Deposit;
        self.handle
            .update(|connection| upsert_confirmation_schedule(connection, kind, id, schedule))?;
        Ok(())
    }

    pub fn remove_confirmation_schedule(&mut self, operation_id: u64) -> Result<(), StorageError> {
        let Some(schedule) = self.confirmation_schedule(operation_id)? else {
            return Ok(());
        };
        self.handle
            .update(|connection| delete_confirmation_schedule(connection, schedule))?;
        Ok(())
    }

    pub fn settlement_job(
        &self,
        kind: SettlementJobKind,
        settlement_id: [u8; 32],
    ) -> Result<Option<SettlementJob>, StorageError> {
        let rows = self.handle.query(|connection| {
            connection.query_all(
                "SELECT operation_id, phase, status, next_run_at_ns, confirmation_checks,
                    confirmation_started_at_ns, lease_generation, lease_until_ns,
                    last_error_code, last_error_detail, updated_at_ns
             FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2",
                params![kind.sql(), settlement_id.to_sql_bytes()],
                |row| {
                    Ok((
                        row.get::<Option<Vec<u8>>>(0)?,
                        row.get::<i64>(1)?,
                        row.get::<i64>(2)?,
                        row.get::<Option<Vec<u8>>>(3)?,
                        row.get::<i64>(4)?,
                        row.get::<Option<Vec<u8>>>(5)?,
                        row.get::<Vec<u8>>(6)?,
                        row.get::<Option<Vec<u8>>>(7)?,
                        row.get::<Option<String>>(8)?,
                        row.get::<Option<String>>(9)?,
                        row.get::<Vec<u8>>(10)?,
                    ))
                },
            )
        })?;
        let Some((
            operation,
            phase,
            status,
            next,
            checks,
            started,
            generation,
            lease,
            error_code,
            error_detail,
            updated,
        )) = rows.into_iter().next()
        else {
            return Ok(None);
        };
        Ok(Some(SettlementJob {
            kind,
            settlement_id,
            operation_id: operation
                .map(u64::from_sql_bytes)
                .transpose()
                .map_err(|_| StorageError::DecodeFailed)?,
            phase: SettlementJobPhase::from_sql(phase)?,
            status: SettlementJobStatus::from_sql(status)?,
            next_run_at_ns: next
                .map(u64::from_sql_bytes)
                .transpose()
                .map_err(|_| StorageError::DecodeFailed)?,
            confirmation_checks: u8::try_from(checks).map_err(|_| StorageError::DecodeFailed)?,
            confirmation_started_at_ns: started
                .map(u64::from_sql_bytes)
                .transpose()
                .map_err(|_| StorageError::DecodeFailed)?,
            lease_generation: u64::from_sql_bytes(generation)
                .map_err(|_| StorageError::DecodeFailed)?,
            lease_until_ns: lease
                .map(u64::from_sql_bytes)
                .transpose()
                .map_err(|_| StorageError::DecodeFailed)?,
            last_error_code: error_code,
            last_error_detail: error_detail,
            updated_at_ns: u64::from_sql_bytes(updated).map_err(|_| StorageError::DecodeFailed)?,
        }))
    }

    pub fn next_settlement_wakeup_ns(&self, _now_ns: u64) -> Result<Option<u64>, StorageError> {
        self.handle
            .query(|connection| {
                connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT next_run_at_ns FROM settlement_jobs WHERE status = 0
                     UNION ALL SELECT lease_until_ns FROM settlement_jobs WHERE status = 1
                     ORDER BY 1 LIMIT 1",
                    params![],
                )
            })?
            .map(u64::from_sql_bytes)
            .transpose()
            .map_err(|_| StorageError::DecodeFailed)
    }

    pub fn park_awaiting_confirmation(
        &mut self,
        job: &SettlementJob,
        now_ns: u64,
    ) -> Result<(), StorageError> {
        self.handle.update(|connection| {
            connection.execute(
                "UPDATE settlement_jobs SET status = 3, next_run_at_ns = NULL,
                 lease_until_ns = NULL, last_error_code = NULL, last_error_detail = NULL,
                 updated_at_ns = ?1 WHERE settlement_kind = ?2 AND settlement_id = ?3
                   AND status = 1 AND lease_generation = ?4",
                params![
                    now_ns.to_sql_bytes(),
                    job.kind.sql(),
                    job.settlement_id.to_sql_bytes(),
                    job.lease_generation.to_sql_bytes()
                ],
            )
        })?;
        Ok(())
    }

    pub fn claim_due_settlement_job(
        &mut self,
        now_ns: u64,
        lease_until_ns: u64,
        max_active_leases: u64,
    ) -> Result<SettlementJobClaim, StorageError> {
        self.handle.update(|connection| {
            if max_active_leases == 0 {
                return Err(DbError::Constraint(
                    "automatic settlement concurrency must be positive".into(),
                ));
            }
            let active_leases = connection.query_scalar::<i64>(
                "SELECT COUNT(*) FROM settlement_jobs WHERE status = 1 AND lease_until_ns > ?1",
                params![now_ns.to_sql_bytes()],
            )?;
            if u64::try_from(active_leases).map_err(|_| DbError::Constraint("invalid active lease count".into()))? >= max_active_leases {
                let lease_until_ns = connection.query_scalar::<Vec<u8>>(
                    "SELECT lease_until_ns FROM settlement_jobs WHERE status = 1 AND lease_until_ns > ?1 ORDER BY lease_until_ns LIMIT 1",
                    params![now_ns.to_sql_bytes()],
                )?;
                return Ok(SettlementJobClaim::ActiveLease {
                    lease_until_ns: u64::from_sql_bytes(lease_until_ns)
                        .map_err(|_| DbError::Constraint("invalid lease deadline".into()))?,
                });
            }
            let rows = connection.query_all(
                "SELECT settlement_kind, settlement_id, operation_id, phase, confirmation_checks,
                        confirmation_started_at_ns, lease_generation
                 FROM settlement_jobs WHERE (status = 0 AND next_run_at_ns <= ?1)
                    OR (status = 1 AND lease_until_ns <= ?1)
                 ORDER BY CASE status WHEN 1 THEN lease_until_ns ELSE next_run_at_ns END,
                          settlement_kind, settlement_id LIMIT 1",
                params![now_ns.to_sql_bytes()],
                |row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?, row.get::<Option<Vec<u8>>>(2)?, row.get::<i64>(3)?, row.get::<i64>(4)?, row.get::<Option<Vec<u8>>>(5)?, row.get::<Vec<u8>>(6)?)),
            )?;
            let Some((kind_raw, id_raw, operation_raw, phase_raw, checks, started_raw, generation_raw)) = rows.into_iter().next() else { return Ok(SettlementJobClaim::None) };
            let kind = match kind_raw { 0 => SettlementJobKind::Deposit, 1 => SettlementJobKind::Withdrawal, 2 => SettlementJobKind::FeePayout, _ => return Err(DbError::Constraint("invalid settlement kind".into())) };
            let settlement_id: [u8; 32] = id_raw.try_into().map_err(|_| DbError::Constraint("invalid settlement id".into()))?;
            let generation = bridge_core::lease_generation_next(
                u64::from_sql_bytes(generation_raw)
                    .map_err(|_| DbError::Constraint("invalid lease generation".into()))?,
            )
            .ok_or_else(|| DbError::Constraint("lease generation overflow".into()))?;
            connection.execute(
                "UPDATE settlement_jobs SET status = 1, next_run_at_ns = NULL,
                 lease_generation = ?1, lease_until_ns = ?2, updated_at_ns = ?3
                 WHERE settlement_kind = ?4 AND settlement_id = ?5",
                params![generation.to_sql_bytes(), lease_until_ns.to_sql_bytes(), now_ns.to_sql_bytes(), kind.sql(), settlement_id.to_sql_bytes()],
            )?;
            Ok(SettlementJobClaim::Claimed(SettlementJob {
                kind, settlement_id,
                operation_id: operation_raw.map(u64::from_sql_bytes).transpose().map_err(|_| DbError::Constraint("invalid operation id".into()))?,
                phase: match phase_raw { 0 => SettlementJobPhase::Confirmation, 1 => SettlementJobPhase::Settlement, _ => return Err(DbError::Constraint("invalid settlement phase".into())) },
                status: SettlementJobStatus::Leased, next_run_at_ns: None,
                confirmation_checks: u8::try_from(checks).map_err(|_| DbError::Constraint("invalid confirmation count".into()))?,
                confirmation_started_at_ns: started_raw.map(u64::from_sql_bytes).transpose().map_err(|_| DbError::Constraint("invalid confirmation start".into()))?,
                lease_generation: generation, lease_until_ns: Some(lease_until_ns),
                last_error_code: None, last_error_detail: None, updated_at_ns: now_ns,
            }))
        }).map_err(Into::into)
    }

    pub fn claim_specific_due_settlement_job(
        &mut self,
        kind: SettlementJobKind,
        settlement_id: [u8; 32],
        now_ns: u64,
        lease_until_ns: u64,
    ) -> Result<SettlementJobClaim, StorageError> {
        self.handle
            .update(|connection| {
                if let Some(active) = connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT lease_until_ns FROM settlement_jobs
                 WHERE settlement_kind = ?1 AND settlement_id = ?2
                   AND status = 1 AND lease_until_ns > ?3",
                    params![
                        kind.sql(),
                        settlement_id.to_sql_bytes(),
                        now_ns.to_sql_bytes()
                    ],
                )? {
                    let lease_until_ns = u64::from_sql_bytes(active)
                        .map_err(|_| DbError::Constraint("invalid lease deadline".into()))?;
                    return Ok(SettlementJobClaim::ActiveLease { lease_until_ns });
                }
                let rows = connection.query_all(
                    "SELECT operation_id, phase, confirmation_checks, confirmation_started_at_ns,
                        lease_generation
                 FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2
                   AND ((status = 0 AND next_run_at_ns <= ?3)
                     OR (status = 1 AND lease_until_ns <= ?3)) LIMIT 1",
                    params![
                        kind.sql(),
                        settlement_id.to_sql_bytes(),
                        now_ns.to_sql_bytes()
                    ],
                    |row| {
                        Ok((
                            row.get::<Option<Vec<u8>>>(0)?,
                            row.get::<i64>(1)?,
                            row.get::<i64>(2)?,
                            row.get::<Option<Vec<u8>>>(3)?,
                            row.get::<Vec<u8>>(4)?,
                        ))
                    },
                )?;
                let Some((operation, phase, checks, started, generation)) = rows.into_iter().next()
                else {
                    return Ok(SettlementJobClaim::None);
                };
                let generation = bridge_core::lease_generation_next(
                    u64::from_sql_bytes(generation)
                        .map_err(|_| DbError::Constraint("invalid lease generation".into()))?,
                )
                .ok_or_else(|| DbError::Constraint("lease generation overflow".into()))?;
                connection.execute(
                    "UPDATE settlement_jobs SET status = 1, next_run_at_ns = NULL,
                 lease_generation = ?1, lease_until_ns = ?2, updated_at_ns = ?3
                 WHERE settlement_kind = ?4 AND settlement_id = ?5",
                    params![
                        generation.to_sql_bytes(),
                        lease_until_ns.to_sql_bytes(),
                        now_ns.to_sql_bytes(),
                        kind.sql(),
                        settlement_id.to_sql_bytes()
                    ],
                )?;
                Ok(SettlementJobClaim::Claimed(SettlementJob {
                    kind,
                    settlement_id,
                    operation_id: operation
                        .map(u64::from_sql_bytes)
                        .transpose()
                        .map_err(|_| DbError::Constraint("invalid operation id".into()))?,
                    phase: match phase {
                        0 => SettlementJobPhase::Confirmation,
                        1 => SettlementJobPhase::Settlement,
                        _ => return Err(DbError::Constraint("invalid settlement phase".into())),
                    },
                    status: SettlementJobStatus::Leased,
                    next_run_at_ns: None,
                    confirmation_checks: u8::try_from(checks)
                        .map_err(|_| DbError::Constraint("invalid confirmation count".into()))?,
                    confirmation_started_at_ns: started
                        .map(u64::from_sql_bytes)
                        .transpose()
                        .map_err(|_| DbError::Constraint("invalid confirmation start".into()))?,
                    lease_generation: generation,
                    lease_until_ns: Some(lease_until_ns),
                    last_error_code: None,
                    last_error_detail: None,
                    updated_at_ns: now_ns,
                }))
            })
            .map_err(Into::into)
    }

    pub fn renew_settlement_lease(
        &mut self,
        job: &mut SettlementJob,
        now_ns: u64,
        lease_until_ns: u64,
    ) -> Result<bool, StorageError> {
        let changed = self.handle.update(|connection| {
            connection.execute(
                "UPDATE settlement_jobs SET lease_until_ns = ?1, updated_at_ns = ?2
                 WHERE settlement_kind = ?3 AND settlement_id = ?4 AND status = 1
                   AND lease_generation = ?5",
                params![
                    lease_until_ns.to_sql_bytes(),
                    now_ns.to_sql_bytes(),
                    job.kind.sql(),
                    job.settlement_id.to_sql_bytes(),
                    job.lease_generation.to_sql_bytes()
                ],
            )?;
            Ok(connection
                .query_optional_scalar::<Vec<u8>>(
                    "SELECT lease_until_ns FROM settlement_jobs
                     WHERE settlement_kind = ?1 AND settlement_id = ?2 AND status = 1
                       AND lease_generation = ?3",
                    params![
                        job.kind.sql(),
                        job.settlement_id.to_sql_bytes(),
                        job.lease_generation.to_sql_bytes()
                    ],
                )?
                .as_deref()
                == Some(lease_until_ns.to_sql_bytes().as_slice()))
        })?;
        if changed {
            job.lease_until_ns = Some(lease_until_ns);
            job.updated_at_ns = now_ns;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn settlement_lease_is_current(&self, job: &SettlementJob) -> Result<bool, StorageError> {
        let generation = self.handle.query(|connection| {
            connection.query_optional_scalar::<Vec<u8>>(
                "SELECT lease_generation FROM settlement_jobs
                 WHERE settlement_kind = ?1 AND settlement_id = ?2 AND status = 1",
                params![job.kind.sql(), job.settlement_id.to_sql_bytes()],
            )
        })?;
        let current = generation
            .map(u64::from_sql_bytes)
            .transpose()
            .map_err(|_| StorageError::DecodeFailed)?;
        Ok(current.is_some_and(|current| {
            matches!(
                bridge_core::lease_outcome_decision(current, job.lease_generation, true),
                bridge_core::LeaseOutcomeDecision::Accept
            )
        }))
    }

    pub fn finish_settlement_job(
        &mut self,
        job: &SettlementJob,
        next_run_at_ns: Option<u64>,
        confirmation_checks: u8,
        stop_error: Option<(&str, &str)>,
        record_stop_reason: Option<String>,
        now_ns: u64,
    ) -> Result<(), StorageError> {
        let (record_table, record_key, previous_record, next_record) = match job.kind {
            SettlementJobKind::Deposit => {
                let mut record = self
                    .deposit(job.settlement_id)?
                    .ok_or(StorageError::RecordNotFound)?;
                let previous_record = record.clone();
                record.last_settlement_stop_reason = record_stop_reason;
                let (previous, next) = self.deposit_record_blobs(&previous_record, &record)?;
                ("deposits", job.settlement_id.to_sql_bytes(), previous, next)
            }
            SettlementJobKind::Withdrawal => {
                let mut record = self
                    .withdrawal(job.settlement_id)?
                    .ok_or(StorageError::RecordNotFound)?;
                let previous = encode(&record)?;
                record.last_settlement_stop_reason = record_stop_reason;
                (
                    "withdrawals",
                    job.settlement_id.to_sql_bytes(),
                    previous,
                    encode(&record)?,
                )
            }
            SettlementJobKind::FeePayout => {
                let id = fee_payout_id_from_job(job.settlement_id)?;
                let record = self.fee_payout(id)?.ok_or(StorageError::RecordNotFound)?;
                let encoded = encode(&record)?;
                ("fee_payouts", id.to_sql_bytes(), encoded.clone(), encoded)
            }
        };
        self.handle.update(|connection| {
            let generation = connection.query_optional_scalar::<Vec<u8>>(
                "SELECT lease_generation FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2 AND status = 1",
                params![job.kind.sql(), job.settlement_id.to_sql_bytes()])?;
            if generation.as_deref() != Some(job.lease_generation.to_sql_bytes().as_slice()) { return Ok(()) }
            let select_record = match record_table {
                "deposits" => "SELECT value FROM deposits WHERE key = ?1",
                "withdrawals" => "SELECT value FROM withdrawals WHERE key = ?1",
                "fee_payouts" => "SELECT value FROM fee_payouts WHERE key = ?1",
                _ => return Err(DbError::Constraint("invalid settlement record table".into())),
            };
            expect_blob(
                connection,
                select_record,
                params![record_key.clone()],
                previous_record.as_slice(),
                "stale settlement record outcome",
            )?;
            match record_table {
                "deposits" => connection.execute(
                    "UPDATE deposits SET value = ?1 WHERE key = ?2",
                    params![next_record.to_sql_bytes(), record_key.clone()],
                )?,
                "withdrawals" => replace_withdrawal_row(
                    connection,
                    record_key.clone(),
                    Some(&previous_record),
                    &next_record,
                )?,
                "fee_payouts" => {},
                _ => return Err(DbError::Constraint("invalid settlement record table".into())),
            }
            if let Some((code, detail)) = stop_error {
                connection.execute(
                    "UPDATE settlement_jobs SET status = 2, next_run_at_ns = NULL, lease_until_ns = NULL,
                     confirmation_checks = ?1, last_error_code = ?2, last_error_detail = ?3,
                     updated_at_ns = ?4 WHERE settlement_kind = ?5 AND settlement_id = ?6",
                    params![i64::from(confirmation_checks), code, detail, now_ns.to_sql_bytes(), job.kind.sql(), job.settlement_id.to_sql_bytes()],
                )
            } else if let Some(next) = next_run_at_ns {
                connection.execute(
                    "UPDATE settlement_jobs SET status = 0, next_run_at_ns = ?1, lease_until_ns = NULL,
                     confirmation_checks = ?2, last_error_code = NULL, last_error_detail = NULL, updated_at_ns = ?3
                     WHERE settlement_kind = ?4 AND settlement_id = ?5",
                    params![next.to_sql_bytes(), i64::from(confirmation_checks), now_ns.to_sql_bytes(), job.kind.sql(), job.settlement_id.to_sql_bytes()],
                )
            } else {
                connection.execute(
                    "DELETE FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2",
                    params![job.kind.sql(), job.settlement_id.to_sql_bytes()],
                )
            }
        })?;
        Ok(())
    }

    pub fn set_settlement_stop_reason_fenced(
        &mut self,
        job: &SettlementJob,
        stop_reason: Option<String>,
    ) -> Result<bool, StorageError> {
        let generation = self.handle.query(|connection| {
            connection.query_optional_scalar::<Vec<u8>>(
                "SELECT lease_generation FROM settlement_jobs
                 WHERE settlement_kind = ?1 AND settlement_id = ?2",
                params![job.kind.sql(), job.settlement_id.to_sql_bytes()],
            )
        })?;
        if generation.as_deref() != Some(job.lease_generation.to_sql_bytes().as_slice()) {
            return Ok(false);
        }
        match job.kind {
            SettlementJobKind::Deposit => {
                let mut record = self
                    .deposit(job.settlement_id)?
                    .ok_or(StorageError::RecordNotFound)?;
                record.last_settlement_stop_reason = stop_reason;
                self.put_deposit(&record)?;
            }
            SettlementJobKind::Withdrawal => {
                let mut record = self
                    .withdrawal(job.settlement_id)?
                    .ok_or(StorageError::RecordNotFound)?;
                record.last_settlement_stop_reason = stop_reason;
                self.put_withdrawal(&record)?;
            }
            SettlementJobKind::FeePayout => {}
        }
        Ok(true)
    }

    pub fn confirmation_scheduler_health(
        &self,
    ) -> Result<ConfirmationSchedulerHealth, StorageError> {
        decode(&self.confirmation_scheduler_health.get()?)
    }

    pub fn settlement_job_summary(
        &self,
        now_ns: u64,
        overdue_after_ns: u64,
    ) -> Result<SettlementJobSummary, StorageError> {
        let (counts, overdue, expired) = self.handle.query(|connection| {
            let counts = connection.query_all(
                "SELECT status, count FROM settlement_job_status_counts ORDER BY status",
                params![],
                |row| Ok((row.get::<i64>(0)?, row.get::<i64>(1)?)),
            )?;
            let overdue = connection
                .query_optional_scalar::<i64>(
                    "SELECT 1 FROM settlement_jobs
                     WHERE status = 0 AND next_run_at_ns <= ?1 LIMIT 1",
                    params![now_ns.saturating_sub(overdue_after_ns).to_sql_bytes()],
                )?
                .is_some();
            let expired = connection
                .query_optional_scalar::<i64>(
                    "SELECT 1 FROM settlement_jobs
                     WHERE status = 1 AND lease_until_ns <= ?1 LIMIT 1",
                    params![now_ns.to_sql_bytes()],
                )?
                .is_some();
            Ok((counts, overdue, expired))
        })?;
        let mut by_status = [0u64; 4];
        for (status, count) in counts {
            let index = usize::try_from(status).map_err(|_| StorageError::DecodeFailed)?;
            let count = u64::try_from(count).map_err(|_| StorageError::DecodeFailed)?;
            *by_status.get_mut(index).ok_or(StorageError::DecodeFailed)? = count;
        }
        let mut summary = SettlementJobSummary {
            scheduled: by_status[0],
            leased: by_status[1].saturating_sub(u64::from(expired)),
            stopped: by_status[2],
            expired: u64::from(expired),
            overdue: u64::from(overdue),
            next_wakeup_at_ns: None,
        };
        summary.next_wakeup_at_ns = self.next_settlement_wakeup_ns(now_ns)?;
        Ok(summary)
    }

    pub fn set_confirmation_scheduler_health(
        &mut self,
        health: &ConfirmationSchedulerHealth,
    ) -> Result<(), StorageError> {
        self.confirmation_scheduler_health.set(encode(health)?)
    }

    pub fn cached_base_mint_snapshot(
        &self,
        now_ns: u64,
        ttl_ns: u64,
        minimum_finalized_block: u64,
    ) -> Result<Option<CachedBaseMintSnapshot>, StorageError> {
        Ok(self.deposit_admission()?.base_snapshot.and_then(|cached| {
            (now_ns.saturating_sub(cached.observed_at_ns) <= ttl_ns
                && cached.snapshot.finalized_head_block_number >= minimum_finalized_block)
                .then_some(cached)
        }))
    }

    pub fn begin_base_snapshot_refresh(
        &mut self,
        now_ns: u64,
        stale_lock_ns: u64,
        cooldown_ns: u64,
    ) -> Result<Option<u64>, StorageError> {
        let mut admission = self.deposit_admission()?;
        let locked = admission
            .refresh_started_at_ns
            .is_some_and(|started| now_ns.saturating_sub(started) < stale_lock_ns);
        if locked || now_ns < admission.next_refresh_allowed_at_ns {
            return Ok(None);
        }
        admission.refresh_generation =
            bridge_core::refresh_generation_next(admission.refresh_generation)
                .ok_or(StorageError::DatabaseFailure)?;
        let owner = admission.refresh_generation;
        admission.refresh_started_at_ns = Some(now_ns);
        admission.refresh_owner = Some(owner);
        admission.next_refresh_allowed_at_ns = now_ns.saturating_add(cooldown_ns);
        self.set_deposit_admission(&admission)?;
        Ok(Some(owner))
    }

    pub fn finish_base_snapshot_refresh(
        &mut self,
        owner: u64,
        observed_at_ns: u64,
        snapshot: BaseMintSnapshot,
        bridge_signer: [u8; 20],
        deposits_paused: bool,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if !bridge_core::refresh_owner_matches(admission.refresh_owner, owner) {
            return Err(StorageError::DatabaseFailure);
        }
        admission.base_snapshot = Some(CachedBaseMintSnapshot {
            generation: owner,
            observed_at_ns,
            snapshot,
            bridge_signer,
            deposits_paused,
        });
        admission.refresh_started_at_ns = None;
        admission.refresh_owner = None;
        self.set_deposit_admission(&admission)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_base_snapshot_refresh_with_rpc_audit(
        &mut self,
        owner: u64,
        observed_at_ns: u64,
        snapshot: BaseMintSnapshot,
        bridge_signer: [u8; 20],
        deposits_paused: bool,
        caller: Principal,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        self.finish_base_snapshot_refresh_with_rpc_audit_and_observation(
            owner,
            observed_at_ns,
            snapshot,
            bridge_signer,
            deposits_paused,
            None,
            caller,
            audit_kinds,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_base_snapshot_refresh_with_rpc_audit_and_observation(
        &mut self,
        owner: u64,
        observed_at_ns: u64,
        snapshot: BaseMintSnapshot,
        bridge_signer: [u8; 20],
        deposits_paused: bool,
        finalized_observation: Option<FinalizedObservationRecord>,
        caller: Principal,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if !bridge_core::refresh_owner_matches(admission.refresh_owner, owner) {
            return Err(StorageError::DatabaseFailure);
        }
        let previous_admission = self.deposit_admission.get()?;
        let mut progress = self.external_progress()?;
        let previous_progress = self.external_progress.get()?;
        if let Some(observation) = finalized_observation {
            progress.observe_finalized(observation)?;
        }
        admission.base_snapshot = Some(CachedBaseMintSnapshot {
            generation: owner,
            observed_at_ns,
            snapshot,
            bridge_signer,
            deposits_paused,
        });
        admission.refresh_started_at_ns = None;
        admission.refresh_owner = None;
        let admission_blob = encode(&admission)?;
        let progress_blob = encode(&progress)?;
        let mut counters = self.counters()?;
        let previous_counters = encode(&counters)?;
        let audit = self.prepare_audit_batch(&mut counters, caller, observed_at_ns, audit_kinds)?;
        let counters_blob = encode(&counters)?;
        self.handle.update(|connection| {
            let persisted_admission = connection.query_scalar::<Vec<u8>>(
                "SELECT deposit_admission FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_progress = connection.query_scalar::<Vec<u8>>(
                "SELECT external_progress FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_admission != previous_admission.to_sql_bytes()
                || persisted_counters != previous_counters.to_sql_bytes()
                || persisted_progress != previous_progress.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale Base snapshot refresh".into()));
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            commit_audit_batch(connection, &audit)?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET deposit_admission = ?1, counters = ?2, external_progress = ?3, audit_retention = ?4 WHERE id = 1",
                params![admission_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes(), audit.retention_blob.to_sql_bytes()],
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(())
    }

    pub fn fail_base_snapshot_refresh(&mut self, owner: u64) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if !bridge_core::refresh_owner_matches(admission.refresh_owner, owner) {
            return Err(StorageError::DatabaseFailure);
        }
        admission.refresh_started_at_ns = None;
        admission.refresh_owner = None;
        self.set_deposit_admission(&admission)
    }

    pub fn fail_base_snapshot_refresh_with_rpc_audit(
        &mut self,
        owner: u64,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if !bridge_core::refresh_owner_matches(admission.refresh_owner, owner) {
            return Err(StorageError::DatabaseFailure);
        }
        let previous_admission = self.deposit_admission.get()?;
        admission.refresh_started_at_ns = None;
        admission.refresh_owner = None;
        let admission_blob = encode(&admission)?;
        let mut counters = self.counters()?;
        let previous_counters = encode(&counters)?;
        let audit = self.prepare_audit_batch(&mut counters, caller, timestamp_ns, audit_kinds)?;
        let counters_blob = encode(&counters)?;
        self.handle.update(|connection| {
            if connection.query_scalar::<Vec<u8>>(
                "SELECT deposit_admission FROM singleton_state WHERE id = 1", params![])?
                != previous_admission.to_sql_bytes()
                || connection.query_scalar::<Vec<u8>>(
                    "SELECT counters FROM singleton_state WHERE id = 1", params![])?
                    != previous_counters.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale failed Base snapshot refresh".into()));
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            commit_audit_batch(connection, &audit)?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET deposit_admission = ?1, counters = ?2, audit_retention = ?3 WHERE id = 1",
                params![admission_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), audit.retention_blob.to_sql_bytes()],
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(())
    }

    pub fn signer_address(&self) -> Result<Option<[u8; 20]>, StorageError> {
        Ok(self.deposit_admission()?.signer_address)
    }

    pub fn signer_public_key(&self) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.deposit_admission()?.signer_public_key)
    }

    pub fn governance_operator_address(&self) -> Result<Option<[u8; 20]>, StorageError> {
        Ok(self.deposit_admission()?.governance_operator_address)
    }

    pub fn initialize_chain_key_addresses(
        &mut self,
        signer_address: [u8; 20],
        governance_operator_address: [u8; 20],
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if admission
            .signer_address
            .is_some_and(|stored| stored != signer_address)
            || admission
                .governance_operator_address
                .is_some_and(|stored| stored != governance_operator_address)
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        admission.signer_address = Some(signer_address);
        admission.governance_operator_address = Some(governance_operator_address);
        self.set_deposit_admission(&admission)
    }

    pub fn governance_operator_public_key(&self) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.deposit_admission()?.governance_operator_public_key)
    }

    pub fn set_governance_operator_public_key_if_absent(
        &mut self,
        public_key: Vec<u8>,
    ) -> Result<Vec<u8>, StorageError> {
        let mut admission = self.deposit_admission()?;
        let selected = admission
            .governance_operator_public_key
            .unwrap_or(public_key);
        admission.governance_operator_public_key = Some(selected.clone());
        self.set_deposit_admission(&admission)?;
        Ok(selected)
    }

    pub fn set_governance_operator_address_if_absent(
        &mut self,
        address: [u8; 20],
    ) -> Result<[u8; 20], StorageError> {
        let mut admission = self.deposit_admission()?;
        let selected = admission.governance_operator_address.unwrap_or(address);
        admission.governance_operator_address = Some(selected);
        self.set_deposit_admission(&admission)?;
        Ok(selected)
    }

    pub fn governance_lane(
        &self,
    ) -> Result<(bool, u64, u64, Option<GovernanceTransaction>), StorageError> {
        let admission = self.deposit_admission()?;
        Ok((
            admission.governance_nonce_initialized,
            admission.next_governance_nonce,
            admission.next_governance_operation_id,
            admission.pending_governance_transaction,
        ))
    }

    pub fn pending_timelock_operation(
        &self,
    ) -> Result<Option<PendingTimelockOperation>, StorageError> {
        Ok(self.deposit_admission()?.pending_timelock_operation)
    }

    pub fn enqueue_emergency_base_actions(&mut self) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        admission.emergency_pause_deposit_required = true;
        admission.emergency_pause_withdrawal_required = true;
        admission.emergency_cancel_required = admission.pending_timelock_operation.is_some();
        self.set_deposit_admission(&admission)
    }

    pub fn next_emergency_base_action(
        &self,
    ) -> Result<Option<GovernanceTransactionKind>, StorageError> {
        let admission = self.deposit_admission()?;
        if admission.emergency_pause_deposit_required {
            Ok(Some(GovernanceTransactionKind::PauseDepositMints))
        } else if admission.emergency_pause_withdrawal_required {
            Ok(Some(GovernanceTransactionKind::PauseWithdrawals))
        } else if admission.emergency_cancel_required {
            admission
                .pending_timelock_operation
                .map(|pending| GovernanceTransactionKind::CancelTimelock {
                    operation_id: pending.operation_id,
                })
                .map(Some)
                .ok_or(StorageError::DecodeFailed)
        } else {
            Ok(None)
        }
    }

    pub fn emergency_base_actions_pending(&self) -> Result<bool, StorageError> {
        let admission = self.deposit_admission()?;
        Ok(admission.emergency_pause_deposit_required
            || admission.emergency_pause_withdrawal_required
            || admission.emergency_cancel_required)
    }

    pub fn initialize_governance_nonce(&mut self, nonce: u64) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if !admission.governance_nonce_initialized {
            admission.governance_nonce_initialized = true;
            admission.next_governance_nonce = nonce;
        } else if admission.next_governance_nonce < nonce {
            return Err(StorageError::DecodeFailed);
        }
        self.set_deposit_admission(&admission)
    }

    pub fn prepare_governance_transaction(
        &mut self,
        transaction: GovernanceTransaction,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if admission.pending_governance_transaction.is_some()
            || transaction.id != admission.next_governance_operation_id
            || transaction.envelope.nonce != admission.next_governance_nonce
        {
            return Err(StorageError::DecodeFailed);
        }
        match transaction.kind {
            GovernanceTransactionKind::ScheduleActivation { operation_id, salt } => {
                if admission.pending_timelock_operation.is_some() {
                    return Err(StorageError::DecodeFailed);
                }
                admission.pending_timelock_operation =
                    Some(PendingTimelockOperation { operation_id, salt });
            }
            GovernanceTransactionKind::ExecuteActivation { operation_id, salt } => {
                if admission.pending_timelock_operation
                    != Some(PendingTimelockOperation { operation_id, salt })
                {
                    return Err(StorageError::DecodeFailed);
                }
            }
            GovernanceTransactionKind::CancelTimelock { operation_id } => {
                if admission
                    .pending_timelock_operation
                    .is_none_or(|pending| pending.operation_id != operation_id)
                {
                    return Err(StorageError::DecodeFailed);
                }
            }
            GovernanceTransactionKind::PauseDepositMints
            | GovernanceTransactionKind::PauseWithdrawals
            | GovernanceTransactionKind::SetServiceFee { .. } => {}
        }
        admission.next_governance_operation_id = admission
            .next_governance_operation_id
            .checked_add(1)
            .ok_or(StorageError::EncodeFailed)?;
        admission.next_governance_nonce = admission
            .next_governance_nonce
            .checked_add(1)
            .ok_or(StorageError::EncodeFailed)?;
        admission.pending_governance_transaction = Some(transaction);
        self.set_deposit_admission(&admission)
    }

    pub fn update_governance_transaction(
        &mut self,
        transaction: GovernanceTransaction,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if admission
            .pending_governance_transaction
            .as_ref()
            .is_none_or(|pending| {
                pending.id != transaction.id || pending.envelope.nonce != transaction.envelope.nonce
            })
        {
            return Err(StorageError::DecodeFailed);
        }
        admission.pending_governance_transaction = Some(transaction);
        self.set_deposit_admission(&admission)
    }

    pub fn abort_unbroadcast_governance_transaction_for_emergency(
        &mut self,
        transaction: &GovernanceTransaction,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if !admission.emergency_pause_deposit_required
            && !admission.emergency_pause_withdrawal_required
            && !admission.emergency_cancel_required
        {
            return Err(StorageError::DecodeFailed);
        }
        if admission.pending_governance_transaction.as_ref() != Some(transaction)
            || !matches!(
                transaction.state,
                GovernanceTransactionState::Prepared | GovernanceTransactionState::Signed
            )
            || !matches!(
                transaction.kind,
                GovernanceTransactionKind::SetServiceFee { .. }
                    | GovernanceTransactionKind::ScheduleActivation { .. }
                    | GovernanceTransactionKind::ExecuteActivation { .. }
            )
            || admission.next_governance_nonce
                != transaction
                    .envelope
                    .nonce
                    .checked_add(1)
                    .ok_or(StorageError::DecodeFailed)?
        {
            return Err(StorageError::DecodeFailed);
        }
        if let GovernanceTransactionKind::ScheduleActivation { operation_id, salt } =
            transaction.kind
        {
            if admission.pending_timelock_operation
                != Some(PendingTimelockOperation { operation_id, salt })
            {
                return Err(StorageError::DecodeFailed);
            }
            admission.pending_timelock_operation = None;
            admission.emergency_cancel_required = false;
        }
        admission.next_governance_nonce = transaction.envelope.nonce;
        admission.pending_governance_transaction = None;
        self.set_deposit_admission(&admission)
    }

    pub fn complete_governance_transaction(
        &mut self,
        transaction: GovernanceTransaction,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        if admission
            .pending_governance_transaction
            .as_ref()
            .is_none_or(|pending| {
                pending.id != transaction.id
                    || pending.kind != transaction.kind
                    || pending.envelope != transaction.envelope
            })
        {
            return Err(StorageError::DecodeFailed);
        }
        let confirmed = matches!(
            transaction.state,
            GovernanceTransactionState::Confirmed { .. }
        );
        let reverted = matches!(
            transaction.state,
            GovernanceTransactionState::Reverted { .. }
        );
        if !confirmed && !reverted {
            return Err(StorageError::DecodeFailed);
        }
        match transaction.kind {
            GovernanceTransactionKind::CancelTimelock { operation_id }
                if confirmed
                    && admission
                        .pending_timelock_operation
                        .is_none_or(|pending| pending.operation_id != operation_id) =>
            {
                return Err(StorageError::DecodeFailed);
            }
            GovernanceTransactionKind::ScheduleActivation { operation_id, salt }
                if reverted
                    && admission.pending_timelock_operation
                        != Some(PendingTimelockOperation { operation_id, salt }) =>
            {
                return Err(StorageError::DecodeFailed);
            }
            GovernanceTransactionKind::ExecuteActivation { operation_id, salt }
                if confirmed
                    && admission.pending_timelock_operation
                        != Some(PendingTimelockOperation { operation_id, salt }) =>
            {
                return Err(StorageError::DecodeFailed);
            }
            _ => {}
        }
        admission.pending_governance_transaction = None;
        match transaction.kind {
            GovernanceTransactionKind::PauseDepositMints if confirmed => {
                admission.emergency_pause_deposit_required = false;
            }
            GovernanceTransactionKind::PauseWithdrawals if confirmed => {
                admission.emergency_pause_withdrawal_required = false;
            }
            GovernanceTransactionKind::CancelTimelock { operation_id }
                if confirmed
                    && admission
                        .pending_timelock_operation
                        .is_some_and(|pending| pending.operation_id == operation_id) =>
            {
                admission.emergency_cancel_required = false;
                admission.pending_timelock_operation = None;
            }
            GovernanceTransactionKind::ScheduleActivation { operation_id, salt }
                if reverted
                    && admission.pending_timelock_operation
                        == Some(PendingTimelockOperation { operation_id, salt }) =>
            {
                admission.pending_timelock_operation = None;
                admission.emergency_cancel_required = false;
            }
            GovernanceTransactionKind::ExecuteActivation { operation_id, salt }
                if confirmed
                    && admission.pending_timelock_operation
                        == Some(PendingTimelockOperation { operation_id, salt }) =>
            {
                admission.pending_timelock_operation = None;
                admission.emergency_cancel_required = false;
            }
            GovernanceTransactionKind::SetServiceFee { .. }
            | GovernanceTransactionKind::ScheduleActivation { .. } => {}
            GovernanceTransactionKind::PauseDepositMints
            | GovernanceTransactionKind::PauseWithdrawals
            | GovernanceTransactionKind::ExecuteActivation { .. }
            | GovernanceTransactionKind::CancelTimelock { .. } => {}
        }
        self.set_deposit_admission(&admission)
    }

    pub fn resolve_governance_nonce_conflict(
        &mut self,
        transaction: &GovernanceTransaction,
        observed_nonce: u64,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        let pending = admission
            .pending_governance_transaction
            .as_ref()
            .ok_or(StorageError::DecodeFailed)?;
        if pending.id != transaction.id
            || pending.kind != transaction.kind
            || pending.envelope != transaction.envelope
            || !matches!(
                transaction.state,
                GovernanceTransactionState::NonceConflict { .. }
                    | GovernanceTransactionState::Broadcasting { .. }
            )
            || observed_nonce <= transaction.envelope.nonce
            || observed_nonce < admission.next_governance_nonce
        {
            return Err(StorageError::DecodeFailed);
        }
        if let GovernanceTransactionKind::ScheduleActivation { operation_id, salt } =
            transaction.kind
        {
            if admission.pending_timelock_operation
                != Some(PendingTimelockOperation { operation_id, salt })
            {
                return Err(StorageError::DecodeFailed);
            }
            admission.pending_timelock_operation = None;
            admission.emergency_cancel_required = false;
        }
        admission.next_governance_nonce = observed_nonce;
        admission.pending_governance_transaction = None;
        self.set_deposit_admission(&admission)
    }

    pub fn set_signer_public_key_if_absent(
        &mut self,
        public_key: Vec<u8>,
    ) -> Result<Vec<u8>, StorageError> {
        let mut admission = self.deposit_admission()?;
        let selected = admission.signer_public_key.unwrap_or(public_key);
        admission.signer_public_key = Some(selected.clone());
        self.set_deposit_admission(&admission)?;
        Ok(selected)
    }

    pub fn set_signer_address_if_absent(
        &mut self,
        address: [u8; 20],
    ) -> Result<[u8; 20], StorageError> {
        let mut admission = self.deposit_admission()?;
        let selected = admission.signer_address.unwrap_or(address);
        admission.signer_address = Some(selected);
        self.set_deposit_admission(&admission)?;
        Ok(selected)
    }

    pub fn external_progress(&self) -> Result<ExternalProgress, StorageError> {
        decode(&self.external_progress.get()?)
    }

    pub fn config(&self) -> Result<Option<BridgeInitArgs>, StorageError> {
        let Some(config) = decode::<Option<ImmutableBridgeConfig>>(&self.config.get()?)? else {
            return Ok(None);
        };
        let admin = self.admin_state()?;
        Ok(Some(config.with_admin(
            admin.governance_principal,
            admin.pause_principal,
            admin.fee_recipient,
        )))
    }

    pub fn set_config_once(&mut self, value: &BridgeInitArgs) -> Result<(), StorageError> {
        let next = ImmutableBridgeConfig::from_init(value);
        match decode::<Option<ImmutableBridgeConfig>>(&self.config.get()?)? {
            None => self.config.set(encode(&Some(next))?),
            Some(previous) if previous == next => Ok(()),
            Some(_) => Err(StorageError::Core(CoreError::ConflictingReplay)),
        }
    }

    pub fn initialize_admin(&mut self, config: &BridgeInitArgs) -> Result<(), StorageError> {
        if decode::<Option<AdminState>>(&self.admin_state.get()?)?.is_some() {
            return Ok(());
        }
        let state = AdminState {
            deposits_paused: true,
            withdrawal_fee_guard: None,
            pause_principal: config.pause_principal,
            governance_principal: config.governance_principal,
            fee_recipient: config.fee_recipient.clone(),
        };
        self.admin_state.set(encode(&Some(state))?)
    }

    pub fn admin_state(&self) -> Result<AdminState, StorageError> {
        decode::<Option<AdminState>>(&self.admin_state.get()?)?.ok_or(StorageError::RecordNotFound)
    }
    pub fn set_admin_state(&mut self, value: &AdminState) -> Result<(), StorageError> {
        self.admin_state.set(encode(&Some(value.clone()))?)
    }

    pub fn rotate_fee_recipient_with_audit(
        &mut self,
        next_recipient: FeeRecipientConfig,
        caller: Principal,
        timestamp_ns: u64,
        previous_sha256: Vec<u8>,
        current_sha256: Vec<u8>,
    ) -> Result<(), StorageError> {
        let mut admin = self.admin_state()?;
        admin.fee_recipient = next_recipient;
        let previous_admin = self.admin_state.get()?;
        let admin_blob = encode(&Some(admin))?;
        let mut counters = self.counters()?;
        let previous_counters = encode(&counters)?;
        let audit = self.prepare_audit_batch(
            &mut counters,
            caller,
            timestamp_ns,
            vec![AuditEventKind::FeeRecipientRotated {
                previous_sha256,
                current_sha256,
            }],
        )?;
        let counters_blob = encode(&counters)?;
        self.handle.update(|connection| {
            let persisted_admin = connection.query_scalar::<Vec<u8>>(
                "SELECT admin_state FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_admin != previous_admin.to_sql_bytes()
                || persisted_counters != previous_counters.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale fee recipient rotation".into()));
            }
            commit_audit_batch(connection, &audit)?;
            connection.execute(
                "UPDATE singleton_state SET admin_state = ?1, counters = ?2, audit_retention = ?3 WHERE id = 1",
                params![
                    admin_blob.to_sql_bytes(),
                    counters_blob.to_sql_bytes(),
                    audit.retention_blob.to_sql_bytes()
                ],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn pause_deposits_with_rpc_audit(
        &mut self,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        let mut admin = self.admin_state()?;
        admin.deposits_paused = true;
        let previous_admin = self.admin_state.get()?;
        let admin_blob = encode(&Some(admin))?;
        let mut counters = self.counters()?;
        let previous_counters = encode(&counters)?;
        let audit = self.prepare_audit_batch(&mut counters, caller, timestamp_ns, audit_kinds)?;
        let counters_blob = encode(&counters)?;
        self.handle.update(|connection| {
            let persisted_admin = connection.query_scalar::<Vec<u8>>(
                "SELECT admin_state FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_admin != previous_admin.to_sql_bytes()
                || persisted_counters != previous_counters.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale nonce-conflict pause".into()));
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            commit_audit_batch(connection, &audit)?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET admin_state = ?1, counters = ?2, audit_retention = ?3 WHERE id = 1",
                params![admin_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), audit.retention_blob.to_sql_bytes()],
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(())
    }
    pub fn append_audit_event(
        &mut self,
        caller: Principal,
        kind: AuditEventKind,
    ) -> Result<AuditEvent, StorageError> {
        self.append_audit_event_at(caller, kind, ic_cdk::api::time())
    }

    pub fn append_audit_events_atomically(
        &mut self,
        caller: Principal,
        kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        let mut counters = self.counters()?;
        let previous_counters = encode(&counters)?;
        let audit = self.prepare_audit_batch(&mut counters, caller, ic_cdk::api::time(), kinds)?;
        let counters_blob = encode(&counters)?;
        self.handle.update(|connection| {
            let persisted = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted != previous_counters.to_sql_bytes() {
                return Err(DbError::Constraint("stale audit batch".into()));
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            commit_audit_batch(connection, &audit)?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, audit_retention = ?2 WHERE id = 1",
                params![
                    counters_blob.to_sql_bytes(),
                    audit.retention_blob.to_sql_bytes()
                ],
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(())
    }

    fn prepare_audit_batch(
        &self,
        counters: &mut CounterState,
        caller: Principal,
        timestamp_ns: u64,
        kinds: Vec<AuditEventKind>,
    ) -> Result<PreparedAuditBatch, StorageError> {
        if kinds.len() > MAX_AUDIT_BATCH {
            return Err(StorageError::ValueTooLarge {
                actual: kinds.len(),
                maximum: MAX_AUDIT_BATCH,
            });
        }
        let mut events = Vec::with_capacity(kinds.len());
        for kind in kinds {
            let sequence = counters.next_audit_sequence;
            counters.next_audit_sequence =
                bridge_core::audit_next(sequence).ok_or(StorageError::CounterOverflow)?;
            events.push((
                sequence,
                encode(&AuditEvent {
                    sequence,
                    timestamp_ns,
                    caller,
                    kind,
                })?,
            ));
        }
        let mut retention: AuditRetentionState = decode(&self.audit_retention.get()?)?;
        let existing =
            usize::try_from(self.audit_events.len()).map_err(|_| StorageError::CounterOverflow)?;
        let prune_count = existing
            .checked_add(events.len())
            .ok_or(StorageError::CounterOverflow)?
            .saturating_sub(MAX_AUDIT_EVENTS as usize);
        let oldest = self
            .audit_events
            .range_limited(.., prune_count, false)
            .into_iter()
            .map(|entry| (*entry.key(), entry.value()))
            .collect::<Vec<_>>();
        if oldest.len() != prune_count {
            return Err(StorageError::RecordNotFound);
        }
        let mut pruned_sequences = Vec::with_capacity(prune_count);
        for (oldest_sequence, oldest_blob) in oldest {
            if oldest_sequence != retention.pruned_count {
                return Err(StorageError::SequenceMismatch {
                    expected: retention.pruned_count,
                });
            }
            let mut digest = Sha256::new();
            digest.update(AUDIT_DIGEST_DOMAIN);
            digest.update(retention.pruned_digest);
            digest.update(oldest_sequence.to_be_bytes());
            digest.update((oldest_blob.as_slice().len() as u64).to_be_bytes());
            digest.update(oldest_blob.as_slice());
            retention.pruned_digest = digest.finalize().into();
            retention.pruned_count = retention
                .pruned_count
                .checked_add(1)
                .ok_or(StorageError::CounterOverflow)?;
            retention.pruned_through_sequence = Some(oldest_sequence);
            pruned_sequences.push(oldest_sequence);
        }
        Ok(PreparedAuditBatch {
            events,
            retention_blob: encode(&retention)?,
            pruned_sequences,
        })
    }

    /// Persists one audit record for a verified EVM RPC transcript.
    ///
    /// Notification retries call this even when the business record already exists. The internal
    /// request digest is the idempotency key, so a prior post-commit audit failure can be repaired
    /// without creating duplicate evidence.
    pub fn append_evm_rpc_observation_once(
        &mut self,
        caller: Principal,
        kind: AuditEventKind,
    ) -> Result<bool, StorageError> {
        self.append_evm_rpc_observation_once_at(caller, kind, ic_cdk::api::time())
    }

    fn append_evm_rpc_observation_once_at(
        &mut self,
        caller: Principal,
        kind: AuditEventKind,
        timestamp_ns: u64,
    ) -> Result<bool, StorageError> {
        let AuditEventKind::EvmRpcObservation {
            evm_rpc_canister_id,
            call_method,
            request_digest,
            quorum_response_digest,
            finalized_block_hash,
            transaction_hash,
            ..
        } = &kind
        else {
            return Err(StorageError::DecodeFailed);
        };
        if call_method.is_empty()
            || request_digest.len() != 32
            || quorum_response_digest.len() != 32
            || finalized_block_hash.len() != 32
            || transaction_hash
                .as_ref()
                .is_some_and(|transaction_hash| transaction_hash.len() != 32)
        {
            return Err(StorageError::DecodeFailed);
        }
        let recent_events = self.handle.query(|connection| {
            connection.query_all(
                "SELECT value FROM audit_events ORDER BY key DESC LIMIT 64",
                params![],
                |row| row.get::<Vec<u8>>(0),
            )
        })?;
        for event_blob in recent_events {
            let event: AuditEvent = decode(&StableBlob::new(event_blob)?)?;
            if let AuditEventKind::EvmRpcObservation {
                evm_rpc_canister_id: previous_canister,
                call_method: previous_method,
                request_digest: previous_request,
                ..
            } = event.kind
            {
                if previous_canister == *evm_rpc_canister_id
                    && previous_method == *call_method
                    && previous_request == *request_digest
                {
                    return Ok(false);
                }
            }
        }
        self.append_audit_event_at(caller, kind, timestamp_ns)?;
        Ok(true)
    }

    fn append_audit_event_at(
        &mut self,
        caller: Principal,
        kind: AuditEventKind,
        timestamp_ns: u64,
    ) -> Result<AuditEvent, StorageError> {
        let mut counters = self.counters()?;
        let sequence = counters.next_audit_sequence;
        counters.next_audit_sequence =
            bridge_core::audit_next(sequence).ok_or(StorageError::CounterOverflow)?;
        let event = AuditEvent {
            sequence,
            timestamp_ns,
            caller,
            kind,
        };
        let event_blob = encode(&event)?;
        let mut retention: AuditRetentionState = decode(&self.audit_retention.get()?)?;
        let pruned = if self.audit_events.len() >= MAX_AUDIT_EVENTS {
            let (oldest_sequence, oldest_blob) = self
                .audit_events
                .first_in_range(..)
                .map(|entry| (*entry.key(), entry.value()))
                .ok_or(StorageError::RecordNotFound)?;
            let expected = retention.pruned_count;
            if oldest_sequence != expected {
                return Err(StorageError::SequenceMismatch { expected });
            }
            let mut digest = Sha256::new();
            digest.update(AUDIT_DIGEST_DOMAIN);
            digest.update(retention.pruned_digest);
            digest.update(oldest_sequence.to_be_bytes());
            digest.update((oldest_blob.as_slice().len() as u64).to_be_bytes());
            digest.update(oldest_blob.as_slice());
            retention.pruned_digest = digest.finalize().into();
            retention.pruned_count = retention
                .pruned_count
                .checked_add(1)
                .ok_or(StorageError::CounterOverflow)?;
            retention.pruned_through_sequence = Some(oldest_sequence);
            Some(oldest_sequence)
        } else {
            None
        };
        let counters_blob = encode(&counters)?;
        let retention_blob = encode(&retention)?;
        self.handle.update(|connection| {
            connection.execute(
                "INSERT INTO audit_events(key, value) VALUES (?1, ?2)",
                params![sequence.to_sql_bytes(), event_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "audit_events")?;
            if let Some(oldest_sequence) = pruned {
                connection.execute(
                    "DELETE FROM audit_events WHERE key = ?1",
                    params![oldest_sequence.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "audit_events")?;
            }
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, audit_retention = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), retention_blob.to_sql_bytes()],
            )
        })?;
        Ok(event)
    }

    pub fn audit_events(
        &self,
        requested_start: u64,
        limit: u16,
    ) -> Result<AuditEventPage, StorageError> {
        let retention: AuditRetentionState = decode(&self.audit_retention.get()?)?;
        let oldest_available_sequence = self
            .audit_events
            .range_limited(.., 1, false)
            .into_iter()
            .next()
            .map(|entry| *entry.key())
            .unwrap_or(retention.pruned_count);
        let start = requested_start.max(oldest_available_sequence);
        let mut entries = self
            .audit_events
            .range_limited(start.., usize::from(limit) + 1, false)
            .into_iter()
            .map(|entry| decode(&entry.value()))
            .collect::<Result<Vec<AuditEvent>, StorageError>>()?;
        let has_more = entries.len() > usize::from(limit);
        if has_more {
            entries.pop();
        }
        let next_sequence = has_more
            .then(|| entries.last().map(|event| event.sequence.saturating_add(1)))
            .flatten();
        Ok(AuditEventPage {
            events: entries,
            oldest_available_sequence,
            next_sequence,
            pruned_count: retention.pruned_count,
            pruned_through_sequence: retention.pruned_through_sequence,
            pruned_digest: retention.pruned_digest.to_vec(),
        })
    }
    pub fn last_audit_sequence(&self) -> Result<Option<u64>, StorageError> {
        Ok(self
            .audit_events
            .range_limited(.., 1, true)
            .into_iter()
            .next()
            .map(|entry| *entry.key()))
    }
    /// Returns the next candidate without reserving it. The request transaction rechecks it.
    pub fn next_fee_payout_id(&self) -> Result<u64, StorageError> {
        Ok(self.counters()?.next_fee_payout_id)
    }

    pub fn commit_fee_payout_request(
        &mut self,
        value: &crate::admin::FeePayoutRecord,
        caller: Principal,
        timestamp_ns: u64,
    ) -> Result<(), StorageError> {
        if value.state != crate::admin::FeePayoutState::Pending {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let previous_counters = self.counters()?;
        if value.id != previous_counters.next_fee_payout_id
            || self.fee_payouts.get(&value.id).is_some()
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let mut counters = previous_counters;
        counters.next_fee_payout_id = value
            .id
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.pending_fee_payout_debit =
            adjust_pending_fee_payout_debit(counters.pending_fee_payout_debit, None, value)?;
        let audit = self.prepare_audit_batch(
            &mut counters,
            caller,
            timestamp_ns,
            vec![AuditEventKind::FeePayoutRequested {
                amount: value.amount,
            }],
        )?;
        let value_blob = encode(value)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let counters_blob = encode(&counters)?;
        fee_payout_bundle_storage_failpoint(FeePayoutBundleFailpoint::Encode)?;
        self.handle.update(|connection| {
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                previous_counters_blob.as_slice(),
                "stale fee payout candidate",
            )?;
            insert_tracked_entry(
                connection,
                "fee_payouts",
                value.id.to_sql_bytes(),
                value_blob.to_sql_bytes(),
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::Record)?;
            enqueue_settlement_job(
                connection,
                SettlementJobKind::FeePayout,
                fee_payout_job_id(value.id),
                None,
                timestamp_ns,
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::Job)?;
            commit_audit_batch(connection, &audit)?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, audit_retention = ?2 WHERE id = 1",
                params![
                    counters_blob.to_sql_bytes(),
                    audit.retention_blob.to_sql_bytes()
                ],
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::SingletonState)
        })?;
        Ok(())
    }
    pub fn fee_payout(
        &self,
        id: u64,
    ) -> Result<Option<crate::admin::FeePayoutRecord>, StorageError> {
        self.fee_payouts
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }
    pub fn pending_fee_payout_amount(&self) -> Result<u128, StorageError> {
        Ok(self.counters()?.pending_fee_payout_debit)
    }
    pub fn complete_fee_payout_success(
        &mut self,
        id: u64,
        block_index: u128,
    ) -> Result<(), StorageError> {
        self.transition_fee_payout(id, FeePayoutTransition::Succeeded { block_index }, None)
    }

    pub fn complete_fee_payout_success_and_scan(
        &mut self,
        id: u64,
        block_index: u128,
        scan_target: &ReconciliationTarget,
    ) -> Result<(), StorageError> {
        self.transition_fee_payout(
            id,
            FeePayoutTransition::Succeeded { block_index },
            Some(scan_target),
        )
    }

    pub fn hold_fee_payout(&mut self, id: u64) -> Result<(), StorageError> {
        self.transition_fee_payout(id, FeePayoutTransition::Hold, None)
    }

    pub fn complete_fee_payout_failure(&mut self, id: u64) -> Result<(), StorageError> {
        self.transition_fee_payout(id, FeePayoutTransition::Failed, None)
    }

    pub fn complete_fee_payout_failure_and_scan(
        &mut self,
        id: u64,
        scan_target: &ReconciliationTarget,
    ) -> Result<(), StorageError> {
        self.transition_fee_payout(id, FeePayoutTransition::Failed, Some(scan_target))
    }

    pub fn commit_fee_payout_scan(
        &mut self,
        progress: &ReconciliationScanProgress,
    ) -> Result<(), StorageError> {
        let ReconciliationTarget::FeePayout(id) = progress.target else {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        };
        let payout = self.fee_payout(id)?.ok_or(StorageError::RecordNotFound)?;
        if payout.state != crate::admin::FeePayoutState::ReconciliationHold
            || payout.transfer != progress.transfer
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        if let Some(previous) = self.reconciliation_scan(&progress.target)? {
            return if previous == *progress {
                Ok(())
            } else {
                Err(StorageError::Core(CoreError::ConflictingReplay))
            };
        }
        let payout_blob = encode(&payout)?;
        let progress_blob = encode(progress)?;
        let scan_key = reconciliation_scan_key(&progress.target);
        fee_payout_bundle_storage_failpoint(FeePayoutBundleFailpoint::Encode)?;
        self.handle.update(|connection| {
            let persisted = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM fee_payouts WHERE key = ?1",
                params![id.to_sql_bytes()],
            )?;
            if persisted != payout_blob.to_sql_bytes() {
                return Err(DbError::Constraint("stale fee payout scan".into()));
            }
            connection.execute(
                "INSERT INTO reconciliation_scans(key, value) VALUES (?1, ?2)",
                params![scan_key.to_sql_bytes(), progress_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "reconciliation_scans")?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::ReconciliationScan)
        })?;
        Ok(())
    }

    pub fn update_fee_payout_scan(
        &mut self,
        previous: &ReconciliationScanProgress,
        next: &ReconciliationScanProgress,
    ) -> Result<(), StorageError> {
        if previous.target != next.target || previous.transfer != next.transfer {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let ReconciliationTarget::FeePayout(id) = previous.target else {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        };
        let payout = self.fee_payout(id)?.ok_or(StorageError::RecordNotFound)?;
        if payout.state != crate::admin::FeePayoutState::ReconciliationHold
            || payout.transfer != previous.transfer
            || self.reconciliation_scan(&previous.target)?.as_ref() != Some(previous)
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let payout_blob = encode(&payout)?;
        let previous_blob = encode(previous)?;
        let next_blob = encode(next)?;
        let scan_key = reconciliation_scan_key(&previous.target);
        fee_payout_bundle_storage_failpoint(FeePayoutBundleFailpoint::Encode)?;
        self.handle.update(|connection| {
            let persisted_payout = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM fee_payouts WHERE key = ?1",
                params![id.to_sql_bytes()],
            )?;
            let persisted_scan = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM reconciliation_scans WHERE key = ?1",
                params![scan_key.to_sql_bytes()],
            )?;
            if persisted_payout != payout_blob.to_sql_bytes()
                || persisted_scan != previous_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale fee payout scan progress".into()));
            }
            connection.execute(
                "UPDATE reconciliation_scans SET value = ?1 WHERE key = ?2",
                params![next_blob.to_sql_bytes(), scan_key.to_sql_bytes()],
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::ReconciliationScan)
        })?;
        Ok(())
    }

    fn transition_fee_payout(
        &mut self,
        id: u64,
        transition: FeePayoutTransition,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<(), StorageError> {
        if scan_target.is_some_and(|target| *target != ReconciliationTarget::FeePayout(id)) {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let previous = self.fee_payout(id)?.ok_or(StorageError::RecordNotFound)?;
        let mut next = previous.clone();
        let terminal_replay = match (transition, &previous.state) {
            (
                FeePayoutTransition::Succeeded { block_index },
                crate::admin::FeePayoutState::Succeeded {
                    block_index: previous,
                },
            ) if block_index == *previous => true,
            (FeePayoutTransition::Failed, crate::admin::FeePayoutState::Failed) => true,
            (FeePayoutTransition::Hold, crate::admin::FeePayoutState::ReconciliationHold) => true,
            _ => false,
        };
        if !terminal_replay {
            next.state = match (transition, &previous.state) {
                (
                    FeePayoutTransition::Succeeded { block_index },
                    crate::admin::FeePayoutState::Pending
                    | crate::admin::FeePayoutState::ReconciliationHold,
                ) => crate::admin::FeePayoutState::Succeeded { block_index },
                (
                    FeePayoutTransition::Failed,
                    crate::admin::FeePayoutState::Pending
                    | crate::admin::FeePayoutState::ReconciliationHold,
                ) => crate::admin::FeePayoutState::Failed,
                (FeePayoutTransition::Hold, crate::admin::FeePayoutState::Pending) => {
                    crate::admin::FeePayoutState::ReconciliationHold
                }
                _ => return Err(StorageError::Core(CoreError::ConflictingReplay)),
            };
        }
        let previous_counters = self.counters()?;
        let mut counters = previous_counters;
        counters.pending_fee_payout_debit = adjust_pending_fee_payout_debit(
            counters.pending_fee_payout_debit,
            Some(&previous),
            &next,
        )?;
        let previous_accounting = self.accounting()?;
        let mut accounting = previous_accounting;
        if !terminal_replay && matches!(transition, FeePayoutTransition::Succeeded { .. }) {
            accounting.spend_fee_reserve(bridge_core::Amount::new(fee_payout_debit(&previous)?))?;
        }
        let scan = scan_target
            .map(|target| self.reconciliation_scan(target))
            .transpose()?
            .flatten();
        if scan_target.is_some() && scan.is_none() && !terminal_replay {
            return Err(StorageError::RecordNotFound);
        }
        if terminal_replay && scan.is_none() {
            return Ok(());
        }
        let previous_blob = encode(&previous)?;
        let next_blob = encode(&next)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let counters_blob = encode(&counters)?;
        let previous_accounting_blob = encode(&previous_accounting)?;
        let accounting_blob = encode(&accounting)?;
        let scan_key = scan_target.map(reconciliation_scan_key);
        let scan_blob = scan.as_ref().map(encode).transpose()?;
        fee_payout_bundle_storage_failpoint(FeePayoutBundleFailpoint::Encode)?;
        self.handle.update(|connection| {
            replace_expected_entry(
                connection,
                "fee_payouts",
                id.to_sql_bytes(),
                previous_blob.as_slice(),
                next_blob.to_sql_bytes(),
                "stale fee payout transition",
            )?;
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                previous_counters_blob.as_slice(),
                "stale fee payout transition",
            )?;
            expect_blob(
                connection,
                "SELECT accounting FROM singleton_state WHERE id = 1",
                params![],
                previous_accounting_blob.as_slice(),
                "stale fee payout transition",
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::Record)?;
            if let (Some(key), Some(expected_blob)) = (&scan_key, &scan_blob) {
                let persisted_scan = connection.query_scalar::<Vec<u8>>(
                    "SELECT value FROM reconciliation_scans WHERE key = ?1",
                    params![key.to_sql_bytes()],
                )?;
                if persisted_scan != expected_blob.to_sql_bytes() {
                    return Err(DbError::Constraint(
                        "stale fee payout reconciliation scan".into(),
                    ));
                }
                connection.execute(
                    "DELETE FROM reconciliation_scans WHERE key = ?1",
                    params![key.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "reconciliation_scans")?;
            }
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::ReconciliationScan)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, accounting = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), accounting_blob.to_sql_bytes()],
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::SingletonState)
        })?;
        Ok(())
    }

    pub fn set_external_progress(&mut self, value: &ExternalProgress) -> Result<(), StorageError> {
        self.external_progress.set(encode(value)?)
    }

    pub fn record_reserve_observation(
        &mut self,
        eth_balance_wei: u128,
        observed_at_ns: u64,
        caller: Principal,
    ) -> Result<(), StorageError> {
        let previous_progress = self.external_progress()?;
        if observed_at_ns < previous_progress.last_reserve_observation_ns {
            return Err(StorageError::StaleReserveObservation);
        }
        let mut progress = previous_progress;
        progress.last_eth_balance_wei = eth_balance_wei;
        progress.reserve_sufficient = true;
        progress.reserve_observation_generation = progress
            .reserve_observation_generation
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        progress.last_reserve_observation_ns = observed_at_ns;

        let previous_counters = self.counters()?;
        let mut counters = previous_counters;
        let audit = (!previous_progress.reserve_sufficient)
            .then(|| {
                self.prepare_audit_batch(
                    &mut counters,
                    caller,
                    observed_at_ns,
                    vec![AuditEventKind::ReserveGateChanged { sufficient: true }],
                )
            })
            .transpose()?;
        let previous_progress_blob = encode(&previous_progress)?;
        let progress_blob = encode(&progress)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let counters_blob = encode(&counters)?;
        let retention_blob = audit.as_ref().map_or_else(
            || self.audit_retention.get(),
            |batch| Ok(batch.retention_blob.clone()),
        )?;
        self.handle.update(|connection| {
            let persisted_progress = connection.query_scalar::<Vec<u8>>(
                "SELECT external_progress FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_progress != previous_progress_blob.to_sql_bytes()
                || persisted_counters != previous_counters_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint(
                    "stale reserve observation state".into(),
                ));
            }
            if let Some(batch) = &audit {
                commit_audit_batch(connection, batch)?;
            }
            connection.execute(
                "UPDATE singleton_state
                 SET external_progress = ?1, counters = ?2, audit_retention = ?3
                 WHERE id = 1",
                params![
                    progress_blob.to_sql_bytes(),
                    counters_blob.to_sql_bytes(),
                    retention_blob.to_sql_bytes()
                ],
            )
        })?;
        Ok(())
    }

    pub fn put_evm_envelope(&mut self, value: &EvmTransactionEnvelope) -> Result<(), StorageError> {
        let id = value.operation_id.get();
        if let Some(previous) = self.evm_execution_payload(id)? {
            match previous {
                EvmExecutionPayload::AwaitingNonce(intent) => {
                    if intent.clone().assign_nonce(value.nonce) != *value {
                        return Err(StorageError::Core(CoreError::ConflictingReplay));
                    }
                }
                EvmExecutionPayload::Prepared(previous) if previous != *value => {
                    let mut expected = value.clone();
                    expected.signed_transaction = previous.signed_transaction.clone();
                    if expected != previous || previous.signed_transaction.is_some() {
                        return Err(StorageError::Core(CoreError::ConflictingReplay));
                    }
                }
                EvmExecutionPayload::Prepared(_) => {}
            }
        } else {
            return Err(StorageError::RecordNotFound);
        }
        self.evm_execution_payloads
            .insert(id, encode(&EvmExecutionPayload::Prepared(value.clone()))?);
        Ok(())
    }

    pub fn replace_submitted_evm_envelope(
        &mut self,
        value: &EvmTransactionEnvelope,
    ) -> Result<(), StorageError> {
        let id = value.operation_id.get();
        let previous = self.evm_envelope(id)?.ok_or(StorageError::RecordNotFound)?;
        if previous.operation_id != value.operation_id
            || previous.payload_hash != value.payload_hash
            || previous.nonce != value.nonce
            || previous.chain_id != value.chain_id
            || previous.contract != value.contract
            || previous.calldata != value.calldata
            || previous.gas_limit != value.gas_limit
            || previous.initial_max_fee_per_gas != value.initial_max_fee_per_gas
            || previous.initial_max_priority_fee_per_gas != value.initial_max_priority_fee_per_gas
            || value.replacement_generation != previous.replacement_generation.saturating_add(1)
            || value.first_broadcast_at_ns != previous.first_broadcast_at_ns
            || value.last_broadcast_at_ns < previous.last_broadcast_at_ns
            || value.rebroadcast_count != previous.rebroadcast_count
            || value.prior_signed_transactions.len()
                != previous.prior_signed_transactions.len().saturating_add(1)
            || value.prior_signed_transactions.last() != previous.signed_transaction.as_ref()
            || !value
                .prior_signed_transactions
                .starts_with(&previous.prior_signed_transactions)
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        self.evm_execution_payloads
            .insert(id, encode(&EvmExecutionPayload::Prepared(value.clone()))?);
        Ok(())
    }

    pub fn promote_submitted_evm_replacement_with_rpc_audit(
        &mut self,
        operation: &EvmOperationRecord,
        envelope: &EvmTransactionEnvelope,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        let id = operation.id.get();
        let previous_operation = self
            .evm_operation(id)?
            .ok_or(StorageError::RecordNotFound)?;
        let previous_envelope = self.evm_envelope(id)?.ok_or(StorageError::RecordNotFound)?;
        let (
            EvmOperationState::Submitted {
                transaction_hash: previous_hash,
            },
            EvmOperationState::Submitted {
                transaction_hash: next_hash,
            },
        ) = (&previous_operation.state, &operation.state)
        else {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        };
        if previous_operation.id != operation.id
            || previous_operation.payload_hash != operation.payload_hash
            || previous_operation.kind != operation.kind
            || previous_operation.recovery_of != operation.recovery_of
            || previous_hash == next_hash
            || operation.id != envelope.operation_id
            || previous_envelope.operation_id != envelope.operation_id
            || previous_envelope.payload_hash != envelope.payload_hash
            || previous_envelope.nonce != envelope.nonce
            || previous_envelope.chain_id != envelope.chain_id
            || previous_envelope.contract != envelope.contract
            || previous_envelope.calldata != envelope.calldata
            || previous_envelope.gas_limit != envelope.gas_limit
            || previous_envelope.max_fee_per_gas != envelope.max_fee_per_gas
            || previous_envelope.max_priority_fee_per_gas != envelope.max_priority_fee_per_gas
            || previous_envelope.signed_transaction != envelope.signed_transaction
            || previous_envelope.initial_max_fee_per_gas != envelope.initial_max_fee_per_gas
            || previous_envelope.initial_max_priority_fee_per_gas
                != envelope.initial_max_priority_fee_per_gas
            || previous_envelope.replacement_generation != envelope.replacement_generation
            || previous_envelope.prior_signed_transactions != envelope.prior_signed_transactions
            || previous_envelope.first_broadcast_at_ns != envelope.first_broadcast_at_ns
            || envelope.last_broadcast_at_ns < previous_envelope.last_broadcast_at_ns
            || previous_envelope.rebroadcast_count != envelope.rebroadcast_count
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }

        let mut counters = self.counters()?;
        let audit = self.prepare_audit_batch(&mut counters, caller, timestamp_ns, audit_kinds)?;
        let operation_blob = encode(operation)?;
        let envelope_blob = encode(&EvmExecutionPayload::Prepared(envelope.clone()))?;
        let previous_operation_blob = encode(&previous_operation)?;
        let previous_envelope_blob = encode(&EvmExecutionPayload::Prepared(previous_envelope))?;
        let counters_blob = encode(&counters)?;
        let key = id.to_sql_bytes();
        self.handle.update(|connection| {
            let persisted_operation = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM evm_operations WHERE key = ?1",
                params![key.clone()],
            )?;
            let persisted_envelope = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM evm_execution_payloads WHERE key = ?1",
                params![key.clone()],
            )?;
            if persisted_operation != previous_operation_blob.to_sql_bytes()
                || persisted_envelope != previous_envelope_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint(
                    "stale EVM replacement promotion".into(),
                ));
            }
            connection.execute(
                "UPDATE evm_operations SET value = ?1 WHERE key = ?2",
                params![operation_blob.to_sql_bytes(), key.clone()],
            )?;
            connection.execute(
                "UPDATE evm_execution_payloads SET value = ?1 WHERE key = ?2",
                params![envelope_blob.to_sql_bytes(), key],
            )?;
            commit_audit_batch(connection, &audit)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, audit_retention = ?2 WHERE id = 1",
                params![
                    counters_blob.to_sql_bytes(),
                    audit.retention_blob.to_sql_bytes()
                ],
            )
        })?;
        Ok(())
    }

    pub fn record_evm_broadcast(
        &mut self,
        value: &EvmTransactionEnvelope,
    ) -> Result<(), StorageError> {
        let id = value.operation_id.get();
        let previous = self.evm_envelope(id)?.ok_or(StorageError::RecordNotFound)?;
        let immutable_matches = previous.operation_id == value.operation_id
            && previous.payload_hash == value.payload_hash
            && previous.nonce == value.nonce
            && previous.chain_id == value.chain_id
            && previous.contract == value.contract
            && previous.calldata == value.calldata
            && previous.gas_limit == value.gas_limit
            && previous.max_fee_per_gas == value.max_fee_per_gas
            && previous.max_priority_fee_per_gas == value.max_priority_fee_per_gas
            && previous.signed_transaction == value.signed_transaction
            && previous.initial_max_fee_per_gas == value.initial_max_fee_per_gas
            && previous.initial_max_priority_fee_per_gas == value.initial_max_priority_fee_per_gas
            && previous.replacement_generation == value.replacement_generation
            && previous.prior_signed_transactions == value.prior_signed_transactions;
        let first_valid = (previous.first_broadcast_at_ns == 0 && value.first_broadcast_at_ns > 0)
            || previous.first_broadcast_at_ns == value.first_broadcast_at_ns;
        let rebroadcast_valid = value.rebroadcast_count == previous.rebroadcast_count
            || value.rebroadcast_count == previous.rebroadcast_count.saturating_add(1);
        if !immutable_matches
            || !first_valid
            || value.last_broadcast_at_ns < value.first_broadcast_at_ns
            || value.last_broadcast_at_ns < previous.last_broadcast_at_ns
            || !rebroadcast_valid
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        self.evm_execution_payloads
            .insert(id, encode(&EvmExecutionPayload::Prepared(value.clone()))?);
        Ok(())
    }

    pub fn prepare_evm_operation(
        &mut self,
        operation: &EvmOperationRecord,
        envelope: &EvmTransactionEnvelope,
        progress: &ExternalProgress,
    ) -> Result<(), StorageError> {
        if !matches!(operation.state, EvmOperationState::Prepared)
            || operation.id != envelope.operation_id
            || operation.payload_hash != envelope.payload_hash
            || progress.next_evm_nonce
                != bridge_core::nonce_next(envelope.nonce).ok_or(StorageError::CounterOverflow)?
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let previous = self
            .evm_operation(operation.id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let intent = self
            .evm_call_intent(operation.id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        if !matches!(previous.state, EvmOperationState::Queued)
            || intent.clone().assign_nonce(envelope.nonce) != *envelope
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }

        let mut counters = self.counters()?;
        counters.awaiting_nonce_evm_operations = counters
            .awaiting_nonce_evm_operations
            .checked_sub(1)
            .ok_or(StorageError::CounterOverflow)?;
        let payload_blob = encode(&EvmExecutionPayload::Prepared(envelope.clone()))?;
        let operation_blob = encode(operation)?;
        let progress_blob = encode(progress)?;
        let counters_blob = encode(&counters)?;
        let previous_index = evm_state_index_key(&previous)?.ok_or(StorageError::RecordNotFound)?;
        let next_index = evm_state_index_key(operation)?.ok_or(StorageError::RecordNotFound)?;
        let operation_key = operation.id.get().to_sql_bytes();
        self.handle.update(|connection| {
            connection.execute(
                "UPDATE evm_execution_payloads SET value = ?1 WHERE key = ?2",
                params![payload_blob.to_sql_bytes(), operation_key.clone()],
            )?;
            connection.execute(
                "UPDATE evm_operations SET value = ?1 WHERE key = ?2",
                params![operation_blob.to_sql_bytes(), operation_key],
            )?;
            transition_tracked_entry(
                connection,
                "evm_state_index",
                Some(previous_index.to_sql_bytes()),
                Some((next_index.to_sql_bytes(), 0u8.to_sql_bytes())),
            )?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, external_progress = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes()],
            )
        })?;
        Ok(())
    }

    pub fn evm_envelope(&self, id: u64) -> Result<Option<EvmTransactionEnvelope>, StorageError> {
        match self.evm_execution_payload(id)? {
            Some(EvmExecutionPayload::Prepared(envelope)) => Ok(Some(envelope)),
            Some(EvmExecutionPayload::AwaitingNonce(_)) | None => Ok(None),
        }
    }

    pub fn first_prepared_evm(
        &self,
    ) -> Result<Option<(EvmOperationRecord, EvmTransactionEnvelope)>, StorageError> {
        let Some(id) = first_evm_index_id(&self.evm_state_index, 1)? else {
            return Ok(None);
        };
        let operation = self
            .evm_operation(id)?
            .ok_or(StorageError::RecordNotFound)?;
        let envelope = self.evm_envelope(id)?.ok_or(StorageError::RecordNotFound)?;
        Ok(Some((operation, envelope)))
    }
    pub fn put_evm_call_intent(&mut self, value: &EvmCallIntent) -> Result<(), StorageError> {
        let id = value.operation_id.get();
        if let Some(previous) = self.evm_execution_payload(id)? {
            if previous != EvmExecutionPayload::AwaitingNonce(value.clone()) {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        self.evm_execution_payloads.insert(
            id,
            encode(&EvmExecutionPayload::AwaitingNonce(value.clone()))?,
        );
        Ok(())
    }
    pub fn evm_call_intent(&self, id: u64) -> Result<Option<EvmCallIntent>, StorageError> {
        match self.evm_execution_payload(id)? {
            Some(EvmExecutionPayload::AwaitingNonce(intent)) => Ok(Some(intent)),
            Some(EvmExecutionPayload::Prepared(_)) | None => Ok(None),
        }
    }
    pub fn evm_execution_payload(
        &self,
        id: u64,
    ) -> Result<Option<EvmExecutionPayload>, StorageError> {
        self.evm_execution_payloads
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }
    pub fn awaiting_nonce_evm_count(&self) -> Result<u64, StorageError> {
        Ok(self.counters()?.awaiting_nonce_evm_operations)
    }

    pub fn put_reconciliation_scan(
        &mut self,
        value: &ReconciliationScanProgress,
    ) -> Result<(), StorageError> {
        if let Some(previous) = self.reconciliation_scan(&value.target)? {
            if previous.transfer != value.transfer {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        self.reconciliation_scans
            .insert(reconciliation_scan_key(&value.target), encode(value)?);
        Ok(())
    }

    pub fn reconciliation_scan(
        &self,
        target: &ReconciliationTarget,
    ) -> Result<Option<ReconciliationScanProgress>, StorageError> {
        self.reconciliation_scans
            .get(&reconciliation_scan_key(target))
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn remove_reconciliation_scan(&mut self, target: &ReconciliationTarget) {
        self.reconciliation_scans
            .remove(&reconciliation_scan_key(target));
    }

    #[cfg(test)]
    fn set_counters(&mut self, value: &CounterState) -> Result<(), StorageError> {
        self.counters.set(encode(value)?)
    }

    #[cfg(test)]
    fn table_count(&self, table: &str) -> u64 {
        self.table_count_value(table).expect("table count")
    }

    fn table_count_value(&self, table: &str) -> Result<u64, StorageError> {
        self.handle
            .query(|connection| {
                let bytes = connection.query_scalar::<Vec<u8>>(
                    "SELECT count FROM table_counts WHERE name = ?1",
                    params![table],
                )?;
                u64::from_sql_bytes(bytes).map_err(|_| DbError::TypeMismatch {
                    index: 0,
                    expected: "u64 big-endian blob",
                    actual: "invalid blob",
                })
            })
            .map_err(Into::into)
    }

    pub fn put_deposit(&mut self, value: &DepositRecord) -> Result<(), StorageError> {
        self.put_deposit_with_audit(value, None, None, None)
    }

    pub fn put_deposit_and_audit(
        &mut self,
        value: &DepositRecord,
        caller: Principal,
        kind: AuditEventKind,
    ) -> Result<(), StorageError> {
        self.put_deposit_with_audit(value, Some((caller, kind)), None, None)
    }

    pub fn put_deposit_refund_retry_bundle(
        &mut self,
        value: &DepositRecord,
        caller: Principal,
        kind: AuditEventKind,
        job: &SettlementJob,
        outcome: RefundJobOutcome,
        now_ns: u64,
    ) -> Result<(), StorageError> {
        if job.kind != SettlementJobKind::Deposit || job.settlement_id != value.id.bytes() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        self.put_deposit_with_audit(
            value,
            Some((caller, kind)),
            Some((job, outcome, now_ns)),
            None,
        )
    }

    pub fn put_deposit_funding_callback(
        &mut self,
        value: &DepositRecord,
        token: &SettlementCallbackToken,
    ) -> Result<(), StorageError> {
        self.put_deposit_with_audit(value, None, None, Some(token))
    }

    fn put_deposit_with_audit(
        &mut self,
        value: &DepositRecord,
        audit_kind: Option<(Principal, AuditEventKind)>,
        refund_job: Option<(&SettlementJob, RefundJobOutcome, u64)>,
        callback_token: Option<&SettlementCallbackToken>,
    ) -> Result<(), StorageError> {
        let previous_stored = self.stored_deposit(value.id.bytes())?;
        let previous = previous_stored.as_ref().map(|stored| stored.record.clone());
        if let Some(token) = callback_token {
            let previous = previous.as_ref().ok_or(StorageError::RecordNotFound)?;
            if !token.matches_deposit(previous)? {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        let next_stored = match previous_stored.as_ref() {
            Some(stored) => StoredDeposit {
                record: value.clone(),
                owner_sequence: stored.owner_sequence,
                base_recipient: stored.base_recipient,
            },
            None if cfg!(test) => StoredDeposit {
                record: value.clone(),
                owner_sequence: 0,
                base_recipient: [0; 20],
            },
            None => return Err(StorageError::RecordNotFound),
        };
        let previous_counters = self.counters()?;
        let mut counters = previous_counters;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous
                .as_ref()
                .map(is_pending_deposit_ledger)
                .unwrap_or(false),
            is_pending_deposit_ledger(value),
        )?;
        counters.reserved_deposit_mint_amount = adjust_reserved_mint_amount(
            counters.reserved_deposit_mint_amount,
            previous.as_ref(),
            value,
        )?;
        counters.reserved_deposit_mint_operations = adjust_reserved_mint_operations(
            counters.reserved_deposit_mint_operations,
            previous.as_ref(),
            value,
        )?;
        let previous_accounting = self.accounting()?;
        let mut accounting = previous_accounting;
        let previous_compensation = previous
            .as_ref()
            .map(refund_compensation_debit)
            .transpose()?
            .unwrap_or(Amount::ZERO);
        let next_compensation = refund_compensation_debit(value)?;
        if next_compensation > previous_compensation {
            accounting.spend_fee_reserve(
                next_compensation
                    .checked_sub(previous_compensation)
                    .map_err(StorageError::Core)?,
            )?;
        } else if previous_compensation > next_compensation {
            accounting.restore_fee_reserve(
                previous_compensation
                    .checked_sub(next_compensation)
                    .map_err(StorageError::Core)?,
            )?;
        }
        let audit = audit_kind
            .map(|(caller, kind)| {
                let audit_time_ns = refund_job
                    .map(|(_, _, now_ns)| now_ns)
                    .unwrap_or_else(ic_cdk::api::time);
                self.prepare_audit_batch(&mut counters, caller, audit_time_ns, vec![kind])
            })
            .transpose()?;
        let value_blob = encode(&next_stored)?;
        let counters_blob = encode(&counters)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let previous_accounting_blob = encode(&previous_accounting)?;
        let accounting_blob = encode(&accounting)?;
        let operation_owner = deposit_operation_id(value)
            .map(|operation_id| {
                encode(&OperationOwner::Deposit(value.id.bytes()))
                    .map(|owner| (operation_id, owner))
            })
            .transpose()?;
        if let Some((operation_id, owner)) = operation_owner.as_ref() {
            if self
                .operation_owner_index
                .get(operation_id)
                .is_some_and(|persisted| persisted != *owner)
            {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        record_write_storage_failpoint(RecordWriteFailpoint::Encode)?;
        let key = value.id.bytes().to_sql_bytes();
        let previous_blob = previous_stored.as_ref().map(encode).transpose()?;
        self.handle.update(|connection| {
            if let Some(token) = callback_token {
                let current = connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT lease_generation FROM settlement_jobs
                     WHERE settlement_kind = ?1 AND settlement_id = ?2 AND status = 1",
                    params![token.kind.sql(), token.settlement_id.to_sql_bytes()],
                )?;
                if current.as_deref()
                    != Some(token.lease_generation.to_sql_bytes().as_slice())
                {
                    return Err(DbError::Constraint("stale settlement callback".into()));
                }
            }
            expect_optional_blob(
                connection,
                "SELECT value FROM deposits WHERE key = ?1",
                params![key.clone()],
                previous_blob.as_ref().map(StableBlob::as_slice),
                "stale deposit write",
            )?;
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                previous_counters_blob.as_slice(),
                "stale deposit counters",
            )?;
            expect_blob(
                connection,
                "SELECT accounting FROM singleton_state WHERE id = 1",
                params![],
                previous_accounting_blob.as_slice(),
                "stale deposit accounting",
            )?;
            if previous.as_ref().is_some_and(is_pending_deposit_ledger) {
                remove_table_entry(connection, "pull_pending_deposit_index", key.clone())?;
            }
            record_write_db_failpoint(RecordWriteFailpoint::RemoveIndex)?;
            if is_pending_deposit_ledger(value) {
                upsert_table_entry(
                    connection,
                    "pull_pending_deposit_index",
                    key.clone(),
                    0u8.to_sql_bytes(),
                )?;
            }
            record_write_db_failpoint(RecordWriteFailpoint::AddIndex)?;
            if let Some((operation_id, owner)) = operation_owner.as_ref() {
                let operation_key = operation_id.to_sql_bytes();
                let persisted_owner = connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT value FROM operation_owner_index WHERE key = ?1",
                    params![operation_key.clone()],
                )?;
                if persisted_owner
                    .as_deref()
                    .is_some_and(|persisted| persisted != owner.as_slice())
                {
                    return Err(DbError::Constraint("conflicting operation owner".into()));
                }
                upsert_table_entry(
                    connection,
                    "operation_owner_index",
                    operation_key,
                    owner.to_sql_bytes(),
                )?;
            }
            record_write_db_failpoint(RecordWriteFailpoint::OperationOwner)?;
            upsert_table_entry(connection, "deposits", key, value_blob.to_sql_bytes())?;
            record_write_db_failpoint(RecordWriteFailpoint::Record)?;
            if let Some(audit) = &audit {
                commit_audit_batch(connection, audit)?;
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, accounting = ?2, audit_retention = ?3 WHERE id = 1",
                    params![
                        counters_blob.to_sql_bytes(),
                        accounting_blob.to_sql_bytes(),
                        audit.retention_blob.to_sql_bytes()
                    ],
                )?;
            } else {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, accounting = ?2 WHERE id = 1",
                    params![counters_blob.to_sql_bytes(), accounting_blob.to_sql_bytes()],
                )?;
            }
            if let Some((job, outcome, now_ns)) = refund_job {
                let current = connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT lease_generation FROM settlement_jobs
                     WHERE settlement_kind = ?1 AND settlement_id = ?2 AND status = 1",
                    params![job.kind.sql(), job.settlement_id.to_sql_bytes()],
                )?;
                if current.as_deref() != Some(job.lease_generation.to_sql_bytes().as_slice()) {
                    return Err(DbError::Constraint("stale refund settlement lease".into()));
                }
                let (status, next_run_at_ns, last_error_code, last_error_detail, checks) =
                    match outcome {
                        RefundJobOutcome::KeepLeased => {
                            return record_write_db_failpoint(
                                RecordWriteFailpoint::SingletonState,
                            );
                        }
                        RefundJobOutcome::RetryAt(next) => (
                            0,
                            Some(next.to_sql_bytes()),
                            None,
                            None,
                            job.confirmation_checks.saturating_add(1),
                        ),
                        RefundJobOutcome::Stop => (
                            2,
                            None,
                            Some("SettlementStopped"),
                            Some("LedgerRejected(\"BadFee\")"),
                            job.confirmation_checks,
                        ),
                    };
                let next_run_at_param = next_run_at_ns
                    .as_deref()
                    .map_or(Value::Null, Value::Blob);
                let last_error_code_param =
                    last_error_code.map_or(Value::Null, Value::Text);
                let last_error_detail_param =
                    last_error_detail.map_or(Value::Null, Value::Text);
                connection.execute(
                    "UPDATE settlement_jobs SET status = ?1, next_run_at_ns = ?2,
                     confirmation_checks = ?3, lease_until_ns = NULL,
                     last_error_code = ?4, last_error_detail = ?5, updated_at_ns = ?6
                     WHERE settlement_kind = ?7 AND settlement_id = ?8 AND status = 1
                       AND lease_generation = ?9",
                    params![
                        i64::from(status),
                        next_run_at_param,
                        i64::from(checks),
                        last_error_code_param,
                        last_error_detail_param,
                        now_ns.to_sql_bytes(),
                        job.kind.sql(),
                        job.settlement_id.to_sql_bytes(),
                        job.lease_generation.to_sql_bytes(),
                    ],
                )?;
            }
            record_write_db_failpoint(RecordWriteFailpoint::SingletonState)
        })?;
        Ok(())
    }

    pub fn admit_deposit(
        &mut self,
        owner: Principal,
        intent: &DepositIntent,
        record: &DepositRecord,
        reserve_admission: Option<DepositReserveAdmission>,
        quota_admission: Option<DepositQuotaAdmission>,
    ) -> Result<DepositAdmissionOutcome, StorageError> {
        if let Some(existing) = self.stored_deposit(record.id.bytes())? {
            if existing.record.payload_hash == record.payload_hash && existing.intent() == *intent {
                return Ok(DepositAdmissionOutcome::Existing);
            }
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        if intent.deposit_id != record.id.bytes()
            || intent.payload_hash != record.payload_hash
            || intent.caller.as_slice() != owner.as_slice()
            || intent.caller.as_slice() != record.transfer.from.owner()
            || intent.from_subaccount != record.transfer.from.subaccount()
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }

        let quota_state = quota_admission
            .map(|quota| -> Result<_, StorageError> {
                let previous_admin_blob = self.admin_state.get()?;
                let admin = decode::<Option<AdminState>>(&previous_admin_blob)?
                    .ok_or(StorageError::RecordNotFound)?;
                if admin.deposits_paused {
                    return Err(StorageError::DepositsPaused);
                }
                let previous_admission_blob = self.deposit_admission.get()?;
                let mut admission: DepositAdmissionControl = decode(&previous_admission_blob)?;
                consume_deposit_quota(&mut admission, owner, quota)?;
                Ok((
                    previous_admin_blob,
                    previous_admission_blob,
                    encode(&admission)?,
                ))
            })
            .transpose()?;

        let owner_sequence_key = owner_sequence_key(owner)?;
        let expected_owner_sequence = self
            .owner_deposit_sequences
            .get(&owner_sequence_key)
            .unwrap_or(0);
        if intent.owner_sequence != expected_owner_sequence {
            return Err(StorageError::SequenceMismatch {
                expected: expected_owner_sequence,
            });
        }
        let next_owner_sequence = expected_owner_sequence
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;

        let previous_counters = self.counters()?;
        let nonterminal_withdrawals = self.table_count_value("withdrawal_liability_index")?;
        let mut counters = previous_counters;
        let previous_progress = self.external_progress()?;
        let mut progress = previous_progress;
        let mut emit_reserve_audit = false;
        if let Some(admission) = reserve_admission {
            if !bridge_core::reserve_token_matches(
                admission.expected_token.nonterminal_withdrawals,
                admission.expected_token.reserved_deposit_mint_amount,
                admission.expected_token.reserved_deposit_mint_operations,
                admission.expected_token.observation_generation,
                nonterminal_withdrawals,
                previous_counters.reserved_deposit_mint_amount,
                previous_counters.reserved_deposit_mint_operations,
                previous_progress.reserve_observation_generation,
            ) || admission.observed_at_ns < previous_progress.last_reserve_observation_ns
            {
                return Err(StorageError::StaleReserveObservation);
            }
            let reserve = admission.reserve_policy.snapshot(
                nonterminal_withdrawals,
                previous_counters.reserved_deposit_mint_operations,
                1,
                admission.eth_balance_wei,
                admission.cycles_balance,
            )?;
            if !reserve.sufficient {
                return Err(StorageError::ReserveUnavailable);
            }
            let quote = record
                .quote
                .ok_or(StorageError::Core(CoreError::InvalidAmount))?;
            let deposit_decision = bridge_core::deposit_admission_decision(
                record.gross_amount.get(),
                quote.service_fee.get(),
                record.max_service_fee.get(),
                admission.mint_snapshot.per_deposit_limit.get(),
                admission.mint_snapshot.effective_minted_in_window().get(),
                previous_counters.reserved_deposit_mint_amount,
                admission.mint_snapshot.mint_window_limit.get(),
            )
            .ok_or(StorageError::Core(CoreError::MintWindowLimitExceeded))?;
            if deposit_decision.net_amount != quote.net_amount.get() {
                return Err(StorageError::Core(CoreError::MintWindowLimitExceeded));
            }
            progress.last_eth_balance_wei = admission.eth_balance_wei;
            progress.reserve_sufficient = true;
            progress.last_reserve_observation_ns = admission.observed_at_ns;
            progress.reserve_observation_generation = progress
                .reserve_observation_generation
                .checked_add(1)
                .ok_or(StorageError::CounterOverflow)?;
            emit_reserve_audit = !previous_progress.reserve_sufficient;
        }
        let sequence = counters.next_deposit_index_sequence;
        counters.next_deposit_index_sequence = sequence
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            false,
            is_pending_deposit_ledger(record),
        )?;
        let reservation_candidate =
            if reserve_admission.is_some() && is_deposit_mint_reserved(record) {
                record.reserved_mint_amount()?.get()
            } else {
                0
            };
        counters.reserved_deposit_mint_amount =
            adjust_reserved_mint_amount(counters.reserved_deposit_mint_amount, None, record)?;
        let reservation = bridge_core::reservation_decision(
            previous_counters.reserved_deposit_mint_amount,
            reservation_candidate,
        )
        .ok_or(StorageError::CounterOverflow)?;
        if reservation.reserved != counters.reserved_deposit_mint_amount
            || reservation.candidate != 0
        {
            return Err(StorageError::CounterOverflow);
        }
        counters.reserved_deposit_mint_operations = adjust_reserved_mint_operations(
            counters.reserved_deposit_mint_operations,
            None,
            record,
        )?;

        let audit = emit_reserve_audit
            .then(|| {
                let admission = reserve_admission.expect("reserve audit requires an observation");
                self.prepare_audit_batch(
                    &mut counters,
                    admission.audit_caller,
                    admission.observed_at_ns,
                    vec![AuditEventKind::ReserveGateChanged { sufficient: true }],
                )
            })
            .transpose()?;

        let record_blob = encode(&StoredDeposit {
            record: record.clone(),
            owner_sequence: intent.owner_sequence,
            base_recipient: intent.base_recipient,
        })?;
        let counters_blob = encode(&counters)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let previous_progress_blob = encode(&previous_progress)?;
        let progress_blob = encode(&progress)?;
        let retention_blob = audit.as_ref().map_or_else(
            || self.audit_retention.get(),
            |audit| Ok(audit.retention_blob.clone()),
        )?;
        let index_key = deposit_owner_index_key(owner, sequence)?;
        let prefix = deposit_owner_index_prefix(owner);
        let range_start = StableBlob::new(deposit_owner_index_bytes(&prefix, 0))?;
        let range_end = StableBlob::new(deposit_owner_index_bytes(&prefix, u64::MAX))?;
        let excess_key = self
            .deposit_owner_index
            .range_limited(
                range_start..=range_end,
                MAX_OWNER_DEPOSIT_INDEX_ENTRIES,
                false,
            )
            .into_iter()
            .nth(MAX_OWNER_DEPOSIT_INDEX_ENTRIES - 1)
            .map(|entry| entry.key().to_sql_bytes());
        let owner_sequence_exists = self
            .owner_deposit_sequences
            .get(&owner_sequence_key)
            .is_some();
        let previous_owner_sequence_blob =
            owner_sequence_exists.then(|| expected_owner_sequence.to_sql_bytes());
        let deposit_key = record.id.bytes().to_sql_bytes();
        self.handle.update(|connection| {
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                previous_counters_blob.as_slice(),
                "stale deposit reserve observation",
            )?;
            expect_blob(
                connection,
                "SELECT external_progress FROM singleton_state WHERE id = 1",
                params![],
                previous_progress_blob.as_slice(),
                "stale deposit reserve observation",
            )?;
            if read_table_count(connection, "withdrawal_liability_index")?
                != nonterminal_withdrawals
            {
                return Err(DbError::Constraint(
                    "stale deposit reserve observation".into(),
                ));
            }
            if let Some((previous_admin, previous_admission, _)) = &quota_state {
                expect_blob(
                    connection,
                    "SELECT admin_state FROM singleton_state WHERE id = 1",
                    params![],
                    previous_admin.as_slice(),
                    "stale deposit admission",
                )?;
                expect_blob(
                    connection,
                    "SELECT deposit_admission FROM singleton_state WHERE id = 1",
                    params![],
                    previous_admission.as_slice(),
                    "stale deposit admission",
                )?;
            }
            expect_optional_blob(
                connection,
                "SELECT value FROM owner_deposit_sequences WHERE key = ?1",
                params![owner_sequence_key.to_sql_bytes()],
                previous_owner_sequence_blob.as_deref(),
                "stale deposit owner sequence",
            )?;
            insert_tracked_entry(
                connection,
                "deposits",
                deposit_key.clone(),
                record_blob.to_sql_bytes(),
            )?;
            insert_tracked_entry(
                connection,
                "deposit_owner_index",
                index_key.to_sql_bytes(),
                deposit_key.clone(),
            )?;
            if let Some(excess_key) = excess_key {
                delete_tracked_entry(connection, "deposit_owner_index", excess_key)?;
            }
            if is_pending_deposit_ledger(record) {
                insert_tracked_entry(
                    connection,
                    "pull_pending_deposit_index",
                    deposit_key,
                    0u8.to_sql_bytes(),
                )?;
            }
            enqueue_settlement_job(
                connection,
                SettlementJobKind::Deposit,
                record.id.bytes(),
                None,
                progress.last_reserve_observation_ns,
            )?;
            if let Some(audit) = &audit {
                commit_audit_batch(connection, audit)?;
            }
            upsert_table_entry(
                connection,
                "owner_deposit_sequences",
                owner_sequence_key.to_sql_bytes(),
                next_owner_sequence.to_sql_bytes(),
            )?;
            if let Some((_, _, admission)) = &quota_state {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, external_progress = ?2,
                        audit_retention = ?3, deposit_admission = ?4 WHERE id = 1",
                    params![
                        counters_blob.to_sql_bytes(),
                        progress_blob.to_sql_bytes(),
                        retention_blob.to_sql_bytes(),
                        admission.to_sql_bytes()
                    ],
                )
            } else {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, external_progress = ?2,
                        audit_retention = ?3 WHERE id = 1",
                    params![
                        counters_blob.to_sql_bytes(),
                        progress_blob.to_sql_bytes(),
                        retention_blob.to_sql_bytes()
                    ],
                )
            }
        })?;
        Ok(DepositAdmissionOutcome::Inserted)
    }

    pub fn next_deposit_sequence(&self, owner: Principal) -> Result<u64, StorageError> {
        Ok(self
            .owner_deposit_sequences
            .get(&owner_sequence_key(owner)?)
            .unwrap_or(0))
    }

    pub fn list_deposit_ids(
        &self,
        owner: Principal,
        before_cursor: Option<u64>,
        limit: u16,
    ) -> Result<DepositIdPageData, StorageError> {
        let prefix = deposit_owner_index_prefix(owner);
        let range_start = StableBlob::new(deposit_owner_index_bytes(&prefix, 0))?;
        let range_end = StableBlob::new(deposit_owner_index_bytes(&prefix, u64::MAX))?;
        let retained_count = self
            .deposit_owner_index
            .range_limited(
                range_start.clone()..=range_end.clone(),
                MAX_OWNER_DEPOSIT_INDEX_ENTRIES,
                false,
            )
            .len() as u64;
        let oldest_available_cursor = self
            .deposit_owner_index
            .range_limited(range_start..=range_end.clone(), 1, true)
            .into_iter()
            .next()
            .map(|entry| deposit_sequence_from_index_key(entry.key()))
            .transpose()?;
        let history_truncated = self.next_deposit_sequence(owner)? > retained_count;
        let start_reverse = match before_cursor {
            Some(0) => {
                return Ok(DepositIdPageData {
                    deposit_ids: Vec::new(),
                    next_cursor: None,
                    oldest_available_cursor,
                    history_truncated,
                });
            }
            Some(sequence) => u64::MAX
                .checked_sub(sequence)
                .and_then(|value| value.checked_add(1))
                .ok_or(StorageError::CounterOverflow)?,
            None => 0,
        };
        let start = StableBlob::new(deposit_owner_index_bytes(&prefix, start_reverse))?;
        let mut entries = self
            .deposit_owner_index
            .range_limited(start..=range_end, usize::from(limit) + 1, false)
            .into_iter()
            .map(|entry| {
                let sequence = deposit_sequence_from_index_key(entry.key())?;
                Ok((sequence, entry.value()))
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        let has_more = entries.len() > usize::from(limit);
        if has_more {
            entries.pop();
        }
        let next = has_more
            .then(|| entries.last().map(|entry| entry.0))
            .flatten();
        Ok(DepositIdPageData {
            deposit_ids: entries.into_iter().map(|entry| entry.1).collect(),
            next_cursor: next,
            oldest_available_cursor,
            history_truncated,
        })
    }

    pub fn deposit_intent(&self, id: [u8; 32]) -> Result<Option<DepositIntent>, StorageError> {
        Ok(self.stored_deposit(id)?.map(|stored| stored.intent()))
    }

    pub fn next_hold_id(&self) -> Result<HoldId, StorageError> {
        let id = self.counters()?.next_hold_id;
        id.checked_add(1).ok_or(StorageError::CounterOverflow)?;
        Ok(HoldId::new(id))
    }

    pub fn commit_deposit_hold_bundle(
        &mut self,
        deposit: &DepositRecord,
        hold: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        self.commit_deposit_hold_bundle_fenced(deposit, hold, None)
    }

    pub fn commit_deposit_funding_hold_bundle(
        &mut self,
        deposit: &DepositRecord,
        hold: &ReconciliationHoldRecord,
        token: &SettlementCallbackToken,
    ) -> Result<(), StorageError> {
        self.commit_deposit_hold_bundle_fenced(deposit, hold, Some(token))
    }

    fn commit_deposit_hold_bundle_fenced(
        &mut self,
        deposit: &DepositRecord,
        hold: &ReconciliationHoldRecord,
        callback_token: Option<&SettlementCallbackToken>,
    ) -> Result<(), StorageError> {
        let previous = self
            .deposit(deposit.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        if let Some(token) = callback_token {
            if !token.matches_deposit(&previous)? {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        let mut expected = previous.clone();
        let (hold_id, request, transfer) = match (&previous.state, &deposit.state) {
            (
                bridge_core::DepositState::FundingPending,
                bridge_core::DepositState::FundingReconciliationHold { hold_id },
            ) => {
                expected
                    .apply(bridge_core::DepositEvent::FundingAmbiguous { hold_id: *hold_id })?;
                (
                    *hold_id,
                    bridge_core::RequestReference::DepositFunding(deposit.id),
                    previous.transfer.clone(),
                )
            }
            (
                bridge_core::DepositState::RefundPending { attempt, .. },
                bridge_core::DepositState::RefundReconciliationHold { hold_id, .. },
            ) => {
                expected.apply(bridge_core::DepositEvent::RefundAmbiguous { hold_id: *hold_id })?;
                (
                    *hold_id,
                    bridge_core::RequestReference::DepositRefund(deposit.id),
                    attempt.identity.clone(),
                )
            }
            _ => return Err(StorageError::Core(CoreError::HoldMismatch)),
        };
        if expected != *deposit
            || *hold != ReconciliationHoldRecord::open(hold_id, request, transfer)
        {
            return Err(StorageError::Core(CoreError::HoldMismatch));
        }
        self.commit_hold_bundle(
            hold,
            HoldBundleParent::Deposit {
                previous: &previous,
                next: deposit,
            },
            callback_token,
        )
    }

    pub fn commit_withdrawal_hold_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        hold: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        let previous = self
            .withdrawal(withdrawal.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let (hold_id, transfer) = match &previous.state {
            WithdrawalState::ReleasePending { attempt, .. } => {
                let hold_id = match withdrawal.state {
                    WithdrawalState::ReconciliationHold { hold_id, .. } => hold_id,
                    _ => return Err(StorageError::Core(CoreError::HoldMismatch)),
                };
                (hold_id, attempt.identity.clone())
            }
            _ => return Err(StorageError::Core(CoreError::HoldMismatch)),
        };
        let mut expected = previous.clone();
        expected.apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })?;
        if expected != *withdrawal
            || *hold
                != ReconciliationHoldRecord::open(
                    hold_id,
                    bridge_core::RequestReference::Withdrawal(withdrawal.id),
                    transfer,
                )
        {
            return Err(StorageError::Core(CoreError::HoldMismatch));
        }
        self.commit_hold_bundle(
            hold,
            HoldBundleParent::Withdrawal {
                previous: &previous,
                next: withdrawal,
            },
            None,
        )
    }

    fn commit_hold_bundle(
        &mut self,
        hold: &ReconciliationHoldRecord,
        parent: HoldBundleParent<'_>,
        callback_token: Option<&SettlementCallbackToken>,
    ) -> Result<(), StorageError> {
        if !is_open_hold(hold) || self.reconciliation_hold(hold.id.get())?.is_some() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let previous_counters = self.counters()?;
        if previous_counters.next_hold_id != hold.id.get() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let mut counters = previous_counters;
        counters.next_hold_id = counters
            .next_hold_id
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        let (table, key, previous_blob, next_blob, previous_index, next_index) = match parent {
            HoldBundleParent::Deposit { previous, next } => {
                counters.pending_ledger_operations = adjust_active_count(
                    counters.pending_ledger_operations,
                    is_pending_deposit_ledger(previous),
                    is_pending_deposit_ledger(next),
                )?;
                let (previous_blob, next_blob) = self.deposit_record_blobs(previous, next)?;
                (
                    "deposits",
                    next.id.bytes(),
                    previous_blob,
                    next_blob,
                    is_pending_deposit_ledger(previous),
                    is_pending_deposit_ledger(next),
                )
            }
            HoldBundleParent::Withdrawal { previous, next } => {
                counters.pending_ledger_operations = adjust_active_count(
                    counters.pending_ledger_operations,
                    is_pending_withdrawal_ledger(previous),
                    is_pending_withdrawal_ledger(next),
                )?;
                (
                    "withdrawals",
                    next.id.bytes(),
                    encode(previous)?,
                    encode(next)?,
                    is_pending_withdrawal_ledger(previous),
                    is_pending_withdrawal_ledger(next),
                )
            }
        };
        if self.open_hold_index.get(&hold.id.get()).is_some() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let parent_index_present = if table == "deposits" {
            self.pull_pending_deposit_index.get(&key).is_some()
        } else {
            self.release_pending_withdrawal_index.get(&key).is_some()
        };
        if parent_index_present != previous_index {
            return Err(StorageError::RecordNotFound);
        }
        hold_bundle_storage_failpoint(HoldBundleFailpoint::Encode)?;
        let hold_blob = encode(hold)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let counters_blob = encode(&counters)?;
        let key = key.to_sql_bytes();
        self.handle.update(|connection| {
            if let Some(token) = callback_token {
                let current = connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT lease_generation FROM settlement_jobs
                     WHERE settlement_kind = ?1 AND settlement_id = ?2 AND status = 1",
                    params![token.kind.sql(), token.settlement_id.to_sql_bytes()],
                )?;
                if current.as_deref() != Some(token.lease_generation.to_sql_bytes().as_slice()) {
                    return Err(DbError::Constraint("stale settlement callback".into()));
                }
            }
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_counters != previous_counters_blob.to_sql_bytes() {
                return Err(DbError::Constraint("stale hold ID candidate".into()));
            }
            let select_parent = if table == "deposits" {
                "SELECT value FROM deposits WHERE key = ?1"
            } else {
                "SELECT value FROM withdrawals WHERE key = ?1"
            };
            if connection.query_scalar::<Vec<u8>>(select_parent, params![key.clone()])?
                != previous_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale hold parent".into()));
            }
            if table == "deposits" {
                connection.execute(
                    "UPDATE deposits SET value = ?1 WHERE key = ?2",
                    params![next_blob.to_sql_bytes(), key.clone()],
                )?;
            } else {
                replace_withdrawal_row(connection, key.clone(), Some(&previous_blob), &next_blob)?;
            }
            hold_bundle_db_failpoint(HoldBundleFailpoint::Parent)?;
            let index_table = if table == "deposits" {
                "pull_pending_deposit_index"
            } else {
                "release_pending_withdrawal_index"
            };
            if previous_index {
                let delete = if table == "deposits" {
                    "DELETE FROM pull_pending_deposit_index WHERE key = ?1"
                } else {
                    "DELETE FROM release_pending_withdrawal_index WHERE key = ?1"
                };
                connection.execute(delete, params![key.clone()])?;
                decrement_table_count(connection, index_table)?;
            }
            if next_index {
                let insert = if table == "deposits" {
                    "INSERT INTO pull_pending_deposit_index(key, value) VALUES (?1, ?2)"
                } else {
                    "INSERT INTO release_pending_withdrawal_index(key, value) VALUES (?1, ?2)"
                };
                connection.execute(insert, params![key, 0u8.to_sql_bytes()])?;
                increment_table_count(connection, index_table)?;
            }
            hold_bundle_db_failpoint(HoldBundleFailpoint::ParentIndex)?;
            connection.execute(
                "INSERT INTO reconciliation_holds(key, value) VALUES (?1, ?2)",
                params![hold.id.get().to_sql_bytes(), hold_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "reconciliation_holds")?;
            hold_bundle_db_failpoint(HoldBundleFailpoint::Hold)?;
            transition_tracked_entry(
                connection,
                "open_hold_index",
                None,
                Some((hold.id.get().to_sql_bytes(), 0u8.to_sql_bytes())),
            )?;
            hold_bundle_db_failpoint(HoldBundleFailpoint::OpenHoldIndex)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                params![counters_blob.to_sql_bytes()],
            )?;
            hold_bundle_db_failpoint(HoldBundleFailpoint::SingletonState)
        })?;
        Ok(())
    }

    /// Returns the operation ID that the next mint or mint-recovery bundle must use.
    ///
    /// This does not reserve or persist the ID. The committing bundle rechecks the candidate
    /// inside the same transaction that advances the counter and persists the operation.
    pub fn next_evm_operation_id(&self) -> Result<bridge_core::EvmOperationId, StorageError> {
        let id = self.counters()?.next_evm_operation_id;
        id.checked_add(1).ok_or(StorageError::CounterOverflow)?;
        Ok(bridge_core::EvmOperationId::new(id))
    }

    pub fn commit_deposit_mint_bundle(
        &mut self,
        deposit: &DepositRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
    ) -> Result<(), StorageError> {
        self.commit_deposit_mint_bundle_and_scan(deposit, operation, intent, None, None)
    }

    pub fn commit_deposit_mint_bundle_and_scan(
        &mut self,
        deposit: &DepositRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
        scan_target: Option<&ReconciliationTarget>,
        reserve_admission: Option<DepositReserveAdmission>,
    ) -> Result<(), StorageError> {
        let previous = self
            .deposit(deposit.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let operation_id = match deposit.state {
            bridge_core::DepositState::MintPending { operation_id, .. } => operation_id,
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        };
        let quote = deposit
            .quote
            .ok_or(StorageError::Core(CoreError::InvalidAmount))?;
        let mut expected = previous.clone();
        expected.apply(bridge_core::DepositEvent::CommitQuote {
            quote,
            operation_id,
        })?;
        if expected != *deposit || operation.kind != EvmOperationKind::MintDeposit {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.commit_operation_bundle(
            operation,
            intent,
            OperationBundleParent::Deposit {
                previous: &previous,
                next: deposit,
                resolved_hold: None,
            },
            reserve_admission,
            None,
            scan_target,
            None,
            None,
        )
    }

    /// Atomically ingests a finalized Base withdrawal and makes its Ledger release runnable.
    pub fn commit_new_withdrawal_release_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        progress: &ExternalProgress,
    ) -> Result<bool, StorageError> {
        self.commit_new_withdrawal_release_bundle_inner(withdrawal, progress, None, None)
    }

    pub fn commit_new_withdrawal_release_bundle_with_rpc_audit(
        &mut self,
        withdrawal: &WithdrawalRecord,
        progress: &ExternalProgress,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
        transaction_hash: [u8; 32],
    ) -> Result<bool, StorageError> {
        self.commit_new_withdrawal_release_bundle_inner(
            withdrawal,
            progress,
            Some((caller, timestamp_ns, audit_kinds)),
            Some(transaction_hash),
        )
    }

    fn commit_new_withdrawal_release_bundle_inner(
        &mut self,
        withdrawal: &WithdrawalRecord,
        progress: &ExternalProgress,
        rpc_audit: Option<(Principal, u64, Vec<AuditEventKind>)>,
        notification_hash: Option<[u8; 32]>,
    ) -> Result<bool, StorageError> {
        if notification_hash
            .is_some_and(|hash| self.withdrawal_notification_index.get(&hash).is_some())
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        if let Some(previous) = self.withdrawal(withdrawal.id.bytes())? {
            if previous == *withdrawal {
                return Ok(false);
            }
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let (attempt, settlement) = match &withdrawal.state {
            WithdrawalState::ReleasePending {
                attempt,
                settlement,
            } => (attempt.clone(), *settlement),
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        };
        let mut expected = withdrawal.clone();
        expected.state = WithdrawalState::Observed;
        expected.apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt),
            settlement,
        })?;
        if expected != *withdrawal {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }

        let mut counters = self.counters()?;
        let previous_counters_blob = encode(&counters)?;
        counters.pending_ledger_operations = counters
            .pending_ledger_operations
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        let prepared_audit = rpc_audit
            .map(|(caller, timestamp_ns, kinds)| {
                self.prepare_audit_batch(&mut counters, caller, timestamp_ns, kinds)
            })
            .transpose()?;
        let withdrawal_blob = encode(withdrawal)?;
        let counters_blob = encode(&counters)?;
        let progress_blob = encode(progress)?;
        let key = withdrawal.id.bytes().to_sql_bytes();

        self.handle.update(|connection| {
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                previous_counters_blob.as_slice(),
                "stale withdrawal ingest",
            )?;
            replace_withdrawal_row(
                connection,
                key.clone(),
                None,
                &withdrawal_blob,
            )?;
            if let Some(transaction_hash) = notification_hash {
                insert_tracked_entry(
                    connection,
                    "withdrawal_notification_index",
                    transaction_hash.to_sql_bytes(),
                    key.clone(),
                )?;
            }
            insert_tracked_entry(
                connection,
                "release_pending_withdrawal_index",
                key.clone(),
                0u8.to_sql_bytes(),
            )?;
            enqueue_settlement_job(
                connection,
                SettlementJobKind::Withdrawal,
                withdrawal.id.bytes(),
                None,
                progress.last_finalized_observation_ns,
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            if let Some(audit) = &prepared_audit {
                commit_audit_batch(connection, audit)?;
                rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, external_progress = ?2, audit_retention = ?3 WHERE id = 1",
                    params![counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes(), audit.retention_blob.to_sql_bytes()],
                )?;
            } else {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, external_progress = ?2 WHERE id = 1",
                    params![counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes()],
                )?;
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(true)
    }

    /// Atomically persists a finalized withdrawal that cannot be released because the
    /// current Ledger fee exceeds the service fee charged to the user.
    #[allow(clippy::too_many_arguments)]
    pub fn commit_withdrawal_fee_guard_trip_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        progress: &ExternalProgress,
        admin: &AdminState,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
        transaction_hash: [u8; 32],
    ) -> Result<(), StorageError> {
        if self
            .withdrawal_notification_index
            .get(&transaction_hash)
            .is_some()
            || self.withdrawal(withdrawal.id.bytes())?.is_some()
            || !matches!(withdrawal.state, WithdrawalState::Observed)
            || withdrawal.last_settlement_stop_reason.as_deref()
                != Some("LedgerFeeExceedsServiceFee")
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let guard = admin
            .withdrawal_fee_guard
            .ok_or(StorageError::Core(CoreError::PayloadConflict))?;
        if guard.ledger_fee <= guard.charged_service_fee
            || guard.charged_service_fee != withdrawal.charged_service_fee.get()
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }

        let mut counters = self.counters()?;
        let previous_counters_blob = encode(&counters)?;
        let audit = self.prepare_audit_batch(&mut counters, caller, timestamp_ns, audit_kinds)?;
        let counters_blob = encode(&counters)?;
        let progress_blob = encode(progress)?;
        let admin_blob = encode(&Some(admin.clone()))?;
        let previous_progress_blob = self.external_progress.get()?;
        let previous_admin_blob = self.admin_state.get()?;
        let withdrawal_blob = encode(withdrawal)?;
        let key = withdrawal.id.bytes().to_sql_bytes();

        self.handle.update(|connection| {
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                previous_counters_blob.as_slice(),
                "stale withdrawal fee guard trip",
            )?;
            expect_blob(
                connection,
                "SELECT external_progress FROM singleton_state WHERE id = 1",
                params![],
                previous_progress_blob.as_slice(),
                "stale withdrawal fee guard trip",
            )?;
            expect_blob(
                connection,
                "SELECT admin_state FROM singleton_state WHERE id = 1",
                params![],
                previous_admin_blob.as_slice(),
                "stale withdrawal fee guard trip",
            )?;
            replace_withdrawal_row(connection, key.clone(), None, &withdrawal_blob)?;
            insert_tracked_entry(
                connection,
                "withdrawal_notification_index",
                transaction_hash.to_sql_bytes(),
                key.clone(),
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            commit_audit_batch(connection, &audit)?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, external_progress = ?2,
                    admin_state = ?3, audit_retention = ?4 WHERE id = 1",
                params![
                    counters_blob.to_sql_bytes(),
                    progress_blob.to_sql_bytes(),
                    admin_blob.to_sql_bytes(),
                    audit.retention_blob.to_sql_bytes()
                ],
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(())
    }

    /// Atomically refreshes an already-tripped fee guard without making the release runnable.
    pub fn commit_withdrawal_fee_guard_continue_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        admin: &AdminState,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        let previous = self
            .withdrawal(withdrawal.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        if !matches!(previous.state, WithdrawalState::Observed)
            || !matches!(withdrawal.state, WithdrawalState::Observed)
            || withdrawal.last_settlement_stop_reason.as_deref()
                != Some("LedgerFeeExceedsServiceFee")
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let mut expected = previous.clone();
        expected.last_settlement_stop_reason = withdrawal.last_settlement_stop_reason.clone();
        if expected != *withdrawal {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let guard = admin
            .withdrawal_fee_guard
            .ok_or(StorageError::Core(CoreError::PayloadConflict))?;
        if guard.ledger_fee <= guard.charged_service_fee
            || guard.charged_service_fee != withdrawal.charged_service_fee.get()
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.commit_withdrawal_fee_guard_update(
            &previous,
            withdrawal,
            admin,
            caller,
            timestamp_ns,
            audit_kinds,
            false,
        )
    }

    /// Atomically clears the fee guard and makes the same withdrawal release runnable.
    pub fn commit_withdrawal_fee_guard_clear_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        admin: &AdminState,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        let previous = self
            .withdrawal(withdrawal.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let (attempt, settlement) = match &withdrawal.state {
            WithdrawalState::ReleasePending {
                attempt,
                settlement,
            } => (attempt.clone(), *settlement),
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        };
        let mut expected = previous.clone();
        expected.apply(WithdrawalEvent::StartRelease {
            attempt: Box::new(attempt),
            settlement,
        })?;
        expected.last_settlement_stop_reason = None;
        if !matches!(previous.state, WithdrawalState::Observed)
            || expected != *withdrawal
            || admin.withdrawal_fee_guard.is_some()
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.commit_withdrawal_fee_guard_update(
            &previous,
            withdrawal,
            admin,
            caller,
            timestamp_ns,
            audit_kinds,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_withdrawal_fee_guard_update(
        &mut self,
        previous: &WithdrawalRecord,
        withdrawal: &WithdrawalRecord,
        admin: &AdminState,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
        make_release_runnable: bool,
    ) -> Result<(), StorageError> {
        let mut counters = self.counters()?;
        let previous_counters_blob = encode(&counters)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            is_pending_withdrawal_ledger(previous),
            is_pending_withdrawal_ledger(withdrawal),
        )?;
        let audit = self.prepare_audit_batch(&mut counters, caller, timestamp_ns, audit_kinds)?;
        let counters_blob = encode(&counters)?;
        let previous_admin_blob = self.admin_state.get()?;
        let admin_blob = encode(&Some(admin.clone()))?;
        let previous_blob = encode(previous)?;
        let withdrawal_blob = encode(withdrawal)?;
        let key = withdrawal.id.bytes().to_sql_bytes();

        self.handle.update(|connection| {
            let persisted = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM withdrawals WHERE key = ?1",
                params![key.clone()],
            )?;
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_admin = connection.query_scalar::<Vec<u8>>(
                "SELECT admin_state FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted != previous_blob.to_sql_bytes()
                || persisted_counters != previous_counters_blob.to_sql_bytes()
                || persisted_admin != previous_admin_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale withdrawal fee guard update".into()));
            }
            replace_withdrawal_row(
                connection,
                key.clone(),
                Some(&previous_blob),
                &withdrawal_blob,
            )?;
            if make_release_runnable {
                connection.execute(
                    "INSERT INTO release_pending_withdrawal_index(key, value) VALUES (?1, ?2)",
                    params![key.clone(), 0u8.to_sql_bytes()],
                )?;
                increment_table_count(connection, "release_pending_withdrawal_index")?;
                let existing_job = connection.query_optional_scalar::<i64>(
                    "SELECT 1 FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2",
                    params![SettlementJobKind::Withdrawal.sql(), key.clone()],
                )?;
                if existing_job.is_none() {
                    enqueue_settlement_job(
                        connection,
                        SettlementJobKind::Withdrawal,
                        withdrawal.id.bytes(),
                        None,
                        timestamp_ns,
                    )?;
                }
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            commit_audit_batch(connection, &audit)?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, admin_state = ?2,
                    audit_retention = ?3 WHERE id = 1",
                params![
                    counters_blob.to_sql_bytes(),
                    admin_blob.to_sql_bytes(),
                    audit.retention_blob.to_sql_bytes()
                ],
            )?;
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_deposit_recovery_bundle(
        &mut self,
        previous: &DepositRecord,
        next: &DepositRecord,
        reverted: &EvmOperationRecord,
        recovery_pending: &EvmOperationRecord,
        replacement: &EvmOperationRecord,
        intent: &EvmCallIntent,
        admission: DepositRecoveryAdmission,
        rpc_audit: RpcAuditBatch,
    ) -> Result<(), StorageError> {
        let mut expected = previous.clone();
        expected.apply(bridge_core::DepositEvent::RetryMint {
            reverted_operation_id: reverted.id,
            replacement_operation_id: replacement.id,
        })?;
        if expected != *next || replacement.kind != EvmOperationKind::MintDeposit {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let mut progress = self.external_progress()?;
        progress.observe_finalized(admission.finalized_observation)?;
        self.commit_operation_bundle(
            replacement,
            intent,
            OperationBundleParent::Deposit {
                previous,
                next,
                resolved_hold: None,
            },
            Some(admission.reserve),
            Some(&progress),
            None,
            Some((rpc_audit.caller, rpc_audit.timestamp_ns, rpc_audit.kinds)),
            Some(RecoveryStart {
                previous: reverted,
                next: recovery_pending,
                now_ns: progress.last_finalized_observation_ns,
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_operation_bundle(
        &mut self,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
        parent: OperationBundleParent<'_>,
        reserve_admission: Option<DepositReserveAdmission>,
        progress: Option<&ExternalProgress>,
        scan_target: Option<&ReconciliationTarget>,
        rpc_audit: Option<(Principal, u64, Vec<AuditEventKind>)>,
        recovery: Option<RecoveryStart<'_>>,
    ) -> Result<(), StorageError> {
        if !matches!(operation.state, EvmOperationState::Queued)
            || intent.operation_id != operation.id
            || intent.payload_hash != operation.payload_hash
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        if operation.recovery_of != recovery.as_ref().map(|value| value.previous.id) {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        if let Some(recovery) = &recovery {
            let mut expected = *recovery.previous;
            expected.apply(EvmOperationEvent::StartRecovery {
                replacement_operation_id: operation.id,
            })?;
            if expected != *recovery.next
                || recovery.previous.id == operation.id
                || recovery.previous.kind != operation.kind
                || recovery.previous.payload_hash != operation.payload_hash
                || self.evm_operation(recovery.previous.id.get())?.as_ref()
                    != Some(recovery.previous)
            {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        let mut counters = self.counters()?;
        let previous_counters = counters;
        let nonterminal_withdrawals = self.table_count_value("withdrawal_liability_index")?;
        let previous_progress = self.external_progress()?;
        let mut admitted_progress = None;
        if let Some(admission) = reserve_admission {
            let next = match parent {
                OperationBundleParent::Deposit { next, .. } => next,
            };
            let quote = next
                .quote
                .ok_or(StorageError::Core(CoreError::InvalidAmount))?;
            let expected_net = admission
                .mint_snapshot
                .quote(next.gross_amount, next.max_service_fee)
                .map_err(|_| StorageError::QuoteSnapshotMismatch)?;
            let snapshot_matches = if recovery.is_some() {
                true
            } else {
                self.deposit_admission()?
                    .base_snapshot
                    .is_some_and(|current| {
                        current.generation == admission.snapshot_generation
                            && current.snapshot == admission.mint_snapshot
                    })
            };
            if !snapshot_matches
                || quote.service_fee != admission.mint_snapshot.service_fee
                || quote.net_amount != expected_net
            {
                return Err(StorageError::QuoteSnapshotMismatch);
            }
            if !bridge_core::reserve_token_matches(
                admission.expected_token.nonterminal_withdrawals,
                admission.expected_token.reserved_deposit_mint_amount,
                admission.expected_token.reserved_deposit_mint_operations,
                admission.expected_token.observation_generation,
                nonterminal_withdrawals,
                previous_counters.reserved_deposit_mint_amount,
                previous_counters.reserved_deposit_mint_operations,
                previous_progress.reserve_observation_generation,
            ) || admission.observed_at_ns < previous_progress.last_reserve_observation_ns
            {
                return Err(StorageError::StaleReserveObservation);
            }
            let reserve = admission.reserve_policy.snapshot(
                nonterminal_withdrawals,
                previous_counters.reserved_deposit_mint_operations,
                1,
                admission.eth_balance_wei,
                admission.cycles_balance,
            )?;
            if !reserve.sufficient {
                return Err(StorageError::ReserveUnavailable);
            }
            let deposit_decision = bridge_core::deposit_admission_decision(
                next.gross_amount.get(),
                quote.service_fee.get(),
                next.max_service_fee.get(),
                admission.mint_snapshot.per_deposit_limit.get(),
                admission.mint_snapshot.effective_minted_in_window().get(),
                previous_counters.reserved_deposit_mint_amount,
                admission.mint_snapshot.mint_window_limit.get(),
            )
            .ok_or(StorageError::Core(CoreError::MintWindowLimitExceeded))?;
            if deposit_decision.net_amount != quote.net_amount.get() {
                return Err(StorageError::Core(CoreError::MintWindowLimitExceeded));
            }
            let mut next_progress = previous_progress;
            next_progress.last_eth_balance_wei = admission.eth_balance_wei;
            next_progress.reserve_sufficient = true;
            next_progress.last_reserve_observation_ns = admission.observed_at_ns;
            next_progress.reserve_observation_generation = next_progress
                .reserve_observation_generation
                .checked_add(1)
                .ok_or(StorageError::CounterOverflow)?;
            admitted_progress = Some(next_progress);
        }
        let mut merged_progress = progress.copied();
        if let Some(admitted) = admitted_progress {
            match &mut merged_progress {
                Some(progress) => {
                    progress.last_eth_balance_wei = admitted.last_eth_balance_wei;
                    progress.reserve_sufficient = admitted.reserve_sufficient;
                    progress.last_reserve_observation_ns = admitted.last_reserve_observation_ns;
                    progress.reserve_observation_generation =
                        admitted.reserve_observation_generation;
                }
                None => merged_progress = Some(admitted),
            }
        }
        let progress = merged_progress.as_ref();
        if counters.next_evm_operation_id != operation.id.get() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        counters.next_evm_operation_id = counters
            .next_evm_operation_id
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.awaiting_nonce_evm_operations = counters
            .awaiting_nonce_evm_operations
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;

        let (
            parent_table,
            parent_key,
            previous_parent_blob,
            parent_blob,
            owner,
            parent_was_present,
            previous_parent_index,
            next_parent_index,
        ) = match parent {
            OperationBundleParent::Deposit {
                previous,
                next,
                resolved_hold: _,
            } => {
                let before_reserved = counters.reserved_deposit_mint_amount;
                counters.pending_ledger_operations = adjust_active_count(
                    counters.pending_ledger_operations,
                    is_pending_deposit_ledger(previous),
                    is_pending_deposit_ledger(next),
                )?;
                counters.reserved_deposit_mint_amount = adjust_reserved_mint_amount(
                    counters.reserved_deposit_mint_amount,
                    Some(previous),
                    next,
                )?;
                if admitted_progress.is_some()
                    && !is_deposit_mint_reserved(previous)
                    && is_deposit_mint_reserved(next)
                {
                    let reservation = bridge_core::reservation_decision(
                        before_reserved,
                        next.reserved_mint_amount()?.get(),
                    )
                    .ok_or(StorageError::CounterOverflow)?;
                    if reservation.reserved != counters.reserved_deposit_mint_amount
                        || reservation.candidate != 0
                    {
                        return Err(StorageError::CounterOverflow);
                    }
                }
                counters.reserved_deposit_mint_operations = adjust_reserved_mint_operations(
                    counters.reserved_deposit_mint_operations,
                    Some(previous),
                    next,
                )?;
                let (previous_blob, next_blob) = self.deposit_record_blobs(previous, next)?;
                (
                    "deposits",
                    next.id.bytes(),
                    Some(previous_blob),
                    next_blob,
                    OperationOwner::Deposit(next.id.bytes()),
                    true,
                    is_pending_deposit_ledger(previous),
                    is_pending_deposit_ledger(next),
                )
            }
        };
        if self.evm_execution_payload(operation.id.get())?.is_some()
            || self.evm_operation(operation.id.get())?.is_some()
            || self
                .operation_owner_index
                .get(&operation.id.get())
                .is_some()
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let evm_index_key = evm_state_index_key(operation)?.ok_or(StorageError::RecordNotFound)?;
        if self.evm_state_index.get(&evm_index_key).is_some() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        if let OperationBundleParent::Deposit {
            resolved_hold: Some((previous_hold, _)),
            ..
        } = parent
        {
            if self.open_hold_index.get(&previous_hold.id.get()).is_none() {
                return Err(StorageError::RecordNotFound);
            }
        }

        operation_bundle_storage_failpoint(OperationBundleFailpoint::Encode)?;
        let prepared_audit = rpc_audit
            .map(|(caller, timestamp_ns, kinds)| {
                self.prepare_audit_batch(&mut counters, caller, timestamp_ns, kinds)
            })
            .transpose()?;
        let payload_blob = encode(&EvmExecutionPayload::AwaitingNonce(intent.clone()))?;
        let operation_blob = encode(operation)?;
        let owner_blob = encode(&owner)?;
        let initial_job = if parent_was_present && recovery.is_none() {
            None
        } else {
            let OperationOwner::Deposit(settlement_id) = &owner;
            let (kind, settlement_id) = (SettlementJobKind::Deposit, *settlement_id);
            Some((
                kind,
                settlement_id,
                progress.map_or(0, |value| value.last_finalized_observation_ns),
            ))
        };
        let initial_job = initial_job.or_else(|| {
            recovery.as_ref().map(|recovery| {
                let OperationOwner::Deposit(settlement_id) = &owner;
                let (kind, settlement_id) = (SettlementJobKind::Deposit, *settlement_id);
                (kind, settlement_id, recovery.now_ns)
            })
        });
        let recovery_blobs = recovery
            .as_ref()
            .map(|recovery| {
                Ok::<_, StorageError>((
                    recovery.previous.id.get(),
                    encode(recovery.previous)?,
                    encode(recovery.next)?,
                ))
            })
            .transpose()?;
        let previous_counters_blob = encode(&previous_counters)?;
        let previous_progress_blob = reserve_admission
            .map(|_| encode(&previous_progress))
            .transpose()?;
        let counters_blob = encode(&counters)?;
        let progress_blob = progress.map(encode).transpose()?;
        let scan_blob = scan_target
            .map(|target| {
                self.reconciliation_scan(target)?
                    .map(|scan| encode(&scan))
                    .transpose()?
                    .ok_or(StorageError::RecordNotFound)
            })
            .transpose()?;
        let scan_key = scan_target.map(reconciliation_scan_key);
        let resolved_hold_blobs = match parent {
            OperationBundleParent::Deposit { resolved_hold, .. } => resolved_hold
                .map(|(previous, next)| -> Result<_, StorageError> {
                    Ok((previous.id.get(), encode(previous)?, encode(next)?))
                })
                .transpose()?,
        };
        let operation_key = operation.id.get().to_sql_bytes();
        let parent_key = parent_key.to_sql_bytes();

        self.handle.update(|connection| {
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                previous_counters_blob.as_slice(),
                "stale operation ID candidate",
            )?;
            if read_table_count(connection, "withdrawal_liability_index")?
                != nonterminal_withdrawals
            {
                return Err(DbError::Constraint(
                    "stale deposit reserve observation".into(),
                ));
            }
            if let Some(expected_progress) = &previous_progress_blob {
                expect_blob(
                    connection,
                    "SELECT external_progress FROM singleton_state WHERE id = 1",
                    params![],
                    expected_progress.as_slice(),
                    "stale deposit reserve observation",
                )?;
            }
            if parent_was_present {
                if parent_table == "deposits" {
                    replace_expected_entry(
                        connection,
                        "deposits",
                        parent_key.clone(),
                        previous_parent_blob
                            .as_ref()
                            .expect("present parent")
                            .as_slice(),
                        parent_blob.to_sql_bytes(),
                        "stale operation parent",
                    )?;
                } else {
                    replace_withdrawal_row(
                        connection,
                        parent_key.clone(),
                        previous_parent_blob.as_ref(),
                        &parent_blob,
                    )?;
                }
            } else {
                replace_withdrawal_row(
                    connection,
                    parent_key.clone(),
                    None,
                    &parent_blob,
                )?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::Parent)?;

            let parent_index_table = if parent_table == "deposits" {
                "pull_pending_deposit_index"
            } else {
                "release_pending_withdrawal_index"
            };
            transition_tracked_entry(
                connection,
                parent_index_table,
                previous_parent_index.then(|| parent_key.clone()),
                next_parent_index.then(|| (parent_key, 0u8.to_sql_bytes())),
            )?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::ParentIndex)?;

            if let Some((reverted_id, previous_blob, next_blob)) = &recovery_blobs {
                replace_expected_entry(
                    connection,
                    "evm_operations",
                    reverted_id.to_sql_bytes(),
                    previous_blob.as_slice(),
                    next_blob.to_sql_bytes(),
                    "stale recovery predecessor",
                )?;
            }

            if let Some((hold_id, previous_hold_blob, next_hold_blob)) = &resolved_hold_blobs {
                replace_expected_entry(
                    connection,
                    "reconciliation_holds",
                    hold_id.to_sql_bytes(),
                    previous_hold_blob.as_slice(),
                    next_hold_blob.to_sql_bytes(),
                    "stale reconciliation hold",
                )?;
                operation_bundle_db_failpoint(OperationBundleFailpoint::ReconciliationHold)?;
                delete_tracked_entry(connection, "open_hold_index", hold_id.to_sql_bytes())?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::OpenHoldIndex)?;

            if let (Some(scan_key), Some(scan_blob)) = (scan_key, &scan_blob) {
                expect_blob(
                    connection,
                    "SELECT value FROM reconciliation_scans WHERE key = ?1",
                    params![scan_key.to_sql_bytes()],
                    scan_blob.as_slice(),
                    "stale reconciliation scan",
                )?;
                delete_tracked_entry(
                    connection,
                    "reconciliation_scans",
                    scan_key.to_sql_bytes(),
                )?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::ReconciliationScan)?;

            insert_tracked_entry(
                connection,
                "evm_execution_payloads",
                operation_key.clone(),
                payload_blob.to_sql_bytes(),
            )?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::ExecutionPayload)?;
            insert_tracked_entry(
                connection,
                "evm_operations",
                operation_key.clone(),
                operation_blob.to_sql_bytes(),
            )?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::EvmOperation)?;
            insert_tracked_entry(
                connection,
                "evm_state_index",
                evm_index_key.to_sql_bytes(),
                0u8.to_sql_bytes(),
            )?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::EvmStateIndex)?;
            insert_tracked_entry(
                connection,
                "operation_owner_index",
                operation_key,
                owner_blob.to_sql_bytes(),
            )?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::OperationOwnerIndex)?;
            if let Some((kind, settlement_id, now_ns)) = initial_job {
                enqueue_settlement_job(
                    connection,
                    kind,
                    settlement_id,
                    Some(operation.id.get()),
                    now_ns,
                )?;
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;

            if let Some(audit) = &prepared_audit {
                commit_audit_batch(connection, audit)?;
                rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
            }

            match (&progress_blob, &prepared_audit) {
                (Some(progress), Some(audit)) => connection.execute(
                    "UPDATE singleton_state SET counters = ?1, external_progress = ?2, audit_retention = ?3 WHERE id = 1",
                    params![counters_blob.to_sql_bytes(), progress.to_sql_bytes(), audit.retention_blob.to_sql_bytes()],
                )?,
                (Some(progress), None) => connection.execute(
                    "UPDATE singleton_state SET counters = ?1, external_progress = ?2 WHERE id = 1",
                    params![counters_blob.to_sql_bytes(), progress.to_sql_bytes()],
                )?,
                (None, Some(audit)) => connection.execute(
                    "UPDATE singleton_state SET counters = ?1, audit_retention = ?2 WHERE id = 1",
                    params![counters_blob.to_sql_bytes(), audit.retention_blob.to_sql_bytes()],
                )?,
                (None, None) => connection.execute(
                    "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                    params![counters_blob.to_sql_bytes()],
                )?,
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::SingletonState)
        })?;
        Ok(())
    }

    pub fn commit_evm_terminal_bundle(
        &mut self,
        operation: &EvmOperationRecord,
        progress: &ExternalProgress,
        revert_audit: Option<(Principal, u64, u64)>,
    ) -> Result<(), StorageError> {
        self.commit_evm_terminal_bundle_inner(operation, progress, revert_audit, None)
    }

    pub fn commit_evm_terminal_bundle_with_rpc_audit(
        &mut self,
        operation: &EvmOperationRecord,
        progress: &ExternalProgress,
        revert_audit: Option<(Principal, u64, u64)>,
        caller: Principal,
        timestamp_ns: u64,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        self.commit_evm_terminal_bundle_inner(
            operation,
            progress,
            revert_audit,
            Some((caller, timestamp_ns, audit_kinds)),
        )
    }

    fn commit_evm_terminal_bundle_inner(
        &mut self,
        operation: &EvmOperationRecord,
        progress: &ExternalProgress,
        revert_audit: Option<(Principal, u64, u64)>,
        rpc_audit: Option<(Principal, u64, Vec<AuditEventKind>)>,
    ) -> Result<(), StorageError> {
        let previous_operation = self
            .evm_operation(operation.id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let transition = match operation.state {
            EvmOperationState::Confirmed {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
            } => EvmOperationEvent::Confirmed {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
            },
            EvmOperationState::Reverted {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
            } => EvmOperationEvent::Reverted {
                transaction_hash,
                receipt_block_number,
                finalized_head_block_number,
            },
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        };
        let mut expected_operation = previous_operation;
        expected_operation.apply(transition)?;
        if expected_operation != *operation {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let owner_blob = self
            .operation_owner_index
            .get(&operation.id.get())
            .ok_or(StorageError::RecordNotFound)?;
        let owner: OperationOwner = decode(&owner_blob)?;
        let is_confirmed = matches!(operation.state, EvmOperationState::Confirmed { .. });
        if is_confirmed != revert_audit.is_none() {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let mut deposit_admission = self.deposit_admission()?;
        if is_confirmed
            && operation.kind == EvmOperationKind::MintDeposit
            && deposit_admission.base_snapshot.is_none_or(|snapshot| {
                snapshot.snapshot.finalized_head_block_number < progress.last_finalized_mint_block
            })
        {
            // Advancing finalized mint progress invalidates the cached admission
            // snapshot. Permit exactly the next refresh attempt; that attempt
            // reinstates the cooldown before making its outcall.
            deposit_admission.next_refresh_allowed_at_ns = 0;
        }

        let mut accounting = self.accounting()?;
        let mut counters = self.counters()?;
        if !is_confirmed {
            counters.unresolved_evm_reverts = counters
                .unresolved_evm_reverts
                .checked_add(1)
                .ok_or(StorageError::CounterOverflow)?;
        }
        let recovered_predecessor = operation
            .recovery_of
            .map(|reverted_id| -> Result<_, StorageError> {
                let previous = self
                    .evm_operation(reverted_id.get())?
                    .ok_or(StorageError::RecordNotFound)?;
                let mut next = previous;
                let resolution = if is_confirmed {
                    bridge_core::EvmRecoveryResolution::ReplacementConfirmed {
                        replacement_operation_id: operation.id,
                    }
                } else {
                    bridge_core::EvmRecoveryResolution::ReplacementReverted {
                        replacement_operation_id: operation.id,
                    }
                };
                next.apply(EvmOperationEvent::ResolveRecovery { resolution })?;
                counters.unresolved_evm_reverts = counters
                    .unresolved_evm_reverts
                    .checked_sub(1)
                    .ok_or(StorageError::CounterUnderflow)?;
                Ok((previous, next))
            })
            .transpose()?;

        let (
            parent_table,
            parent_key,
            previous_parent_blob,
            parent_blob,
            previous_parent_index,
            next_parent_index,
        ) = match owner {
            OperationOwner::Deposit(id) => {
                if operation.kind != EvmOperationKind::MintDeposit {
                    return Err(StorageError::Core(CoreError::PayloadConflict));
                }
                let previous = self.deposit(id)?.ok_or(StorageError::RecordNotFound)?;
                let mut next = previous.clone();
                let result = if is_confirmed {
                    next.apply(bridge_core::DepositEvent::MintConfirmed {
                        operation_id: operation.id,
                    })?
                } else {
                    next.apply(bridge_core::DepositEvent::MintReverted {
                        operation_id: operation.id,
                    })?
                };
                if is_confirmed {
                    accounting.confirm_fee(FeeKind::Deposit, result.fee_delta)?;
                }
                counters.pending_ledger_operations = adjust_active_count(
                    counters.pending_ledger_operations,
                    is_pending_deposit_ledger(&previous),
                    is_pending_deposit_ledger(&next),
                )?;
                counters.reserved_deposit_mint_amount = adjust_reserved_mint_amount(
                    counters.reserved_deposit_mint_amount,
                    Some(&previous),
                    &next,
                )?;
                counters.reserved_deposit_mint_operations = adjust_reserved_mint_operations(
                    counters.reserved_deposit_mint_operations,
                    Some(&previous),
                    &next,
                )?;
                let (previous_blob, next_blob) = self.deposit_record_blobs(&previous, &next)?;
                (
                    "deposits",
                    id,
                    previous_blob,
                    next_blob,
                    is_pending_deposit_ledger(&previous),
                    is_pending_deposit_ledger(&next),
                )
            }
        };

        let previous_schedule = self
            .confirmation_schedule(operation.id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let previous_evm_index =
            evm_state_index_key(&previous_operation)?.ok_or(StorageError::RecordNotFound)?;
        if self.evm_state_index.get(&previous_evm_index).is_none() {
            return Err(StorageError::RecordNotFound);
        }
        let operation_blob = encode(operation)?;
        let accounting_blob = encode(&accounting)?;
        let progress_blob = encode(progress)?;
        let parent_key_sql = parent_key.to_sql_bytes();
        let operation_key = operation.id.get().to_sql_bytes();
        let execution_payload_present = self
            .evm_execution_payloads
            .get(&operation.id.get())
            .is_some();

        let mut audit_caller = rpc_audit.as_ref().map(|(caller, _, _)| *caller);
        let mut audit_timestamp = rpc_audit.as_ref().map(|(_, timestamp, _)| *timestamp);
        let mut audit_kinds = rpc_audit.map(|(_, _, kinds)| kinds).unwrap_or_default();
        if let Some((previous, _)) = &recovered_predecessor {
            let OperationOwner::Deposit(id) = owner;
            let target_id = id.to_vec();
            let (finalized_block_number, finalized_block_hash) = audit_kinds
                .iter()
                .rev()
                .find_map(|kind| match kind {
                    AuditEventKind::EvmRpcObservation {
                        finalized_block_number,
                        finalized_block_hash,
                        ..
                    } => Some((*finalized_block_number, finalized_block_hash.clone())),
                    _ => None,
                })
                .ok_or(StorageError::DecodeFailed)?;
            audit_kinds.push(AuditEventKind::MintRevertRecoveryCompleted {
                target_id,
                reverted_operation_id: previous.id.get(),
                replacement_operation_id: operation.id.get(),
                kind: operation.kind.into(),
                finalized_block_number,
                finalized_block_hash,
                result: if is_confirmed {
                    "replacement_confirmed".into()
                } else {
                    "replacement_reverted".into()
                },
            });
        }
        let admin_blob =
            if let Some((caller, timestamp_ns, finalized_head_block_number)) = revert_audit {
                audit_caller.get_or_insert(caller);
                audit_timestamp.get_or_insert(timestamp_ns);
                let mut admin = self.admin_state()?;
                admin.deposits_paused = true;
                let transaction_hash = match operation.state {
                    EvmOperationState::Reverted {
                        transaction_hash, ..
                    } => transaction_hash,
                    _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
                };
                audit_kinds.push(AuditEventKind::EvmOperationReverted {
                    operation_id: operation.id.get(),
                    kind: operation.kind.into(),
                    transaction_hash: transaction_hash.to_vec(),
                    finalized_head_block_number,
                });
                Some(encode(&Some(admin))?)
            } else {
                None
            };
        let prepared_audit = if audit_kinds.is_empty() {
            None
        } else {
            Some(self.prepare_audit_batch(
                &mut counters,
                audit_caller.ok_or(StorageError::DecodeFailed)?,
                audit_timestamp.ok_or(StorageError::DecodeFailed)?,
                audit_kinds,
            )?)
        };
        let counters_blob = encode(&counters)?;
        let deposit_admission_blob = encode(&deposit_admission)?;
        let recovered_predecessor_blobs = recovered_predecessor
            .as_ref()
            .map(|(previous, next)| -> Result<_, StorageError> {
                Ok((previous.id.get(), encode(previous)?, encode(next)?))
            })
            .transpose()?;

        self.handle.update(|connection| {
            let persisted_operation = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM evm_operations WHERE key = ?1",
                params![operation_key.clone()],
            )?;
            if persisted_operation != encode(&previous_operation)
                .map_err(|_| DbError::Constraint("operation encoding failed".into()))?
                .to_sql_bytes()
            {
                return Err(DbError::Constraint("stale terminal operation".into()));
            }
            let select_parent = if parent_table == "deposits" {
                "SELECT value FROM deposits WHERE key = ?1"
            } else {
                "SELECT value FROM withdrawals WHERE key = ?1"
            };
            if connection.query_scalar::<Vec<u8>>(select_parent, params![parent_key_sql.clone()])?
                != previous_parent_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale terminal parent".into()));
            }
            if parent_table == "deposits" {
                connection.execute(
                    "UPDATE deposits SET value = ?1 WHERE key = ?2",
                    params![parent_blob.to_sql_bytes(), parent_key_sql.clone()],
                )?;
            } else {
                replace_withdrawal_row(
                    connection,
                    parent_key_sql.clone(),
                    Some(&previous_parent_blob),
                    &parent_blob,
                )?;
            }
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::Parent)?;
            let parent_index_table = if parent_table == "deposits" { "pull_pending_deposit_index" } else { "release_pending_withdrawal_index" };
            if previous_parent_index {
                let sql = if parent_table == "deposits" { "DELETE FROM pull_pending_deposit_index WHERE key = ?1" } else { "DELETE FROM release_pending_withdrawal_index WHERE key = ?1" };
                connection.execute(sql, params![parent_key_sql.clone()])?;
                decrement_table_count(connection, parent_index_table)?;
            }
            if next_parent_index {
                let sql = if parent_table == "deposits" { "INSERT INTO pull_pending_deposit_index(key, value) VALUES (?1, ?2)" } else { "INSERT INTO release_pending_withdrawal_index(key, value) VALUES (?1, ?2)" };
                connection.execute(sql, params![parent_key_sql, 0u8.to_sql_bytes()])?;
                increment_table_count(connection, parent_index_table)?;
            }
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::ParentIndex)?;
            connection.execute(
                "UPDATE evm_operations SET value = ?1 WHERE key = ?2",
                params![operation_blob.to_sql_bytes(), operation_key.clone()],
            )?;
            if execution_payload_present {
                connection.execute(
                    "DELETE FROM evm_execution_payloads WHERE key = ?1",
                    params![operation_key.clone()],
                )?;
                decrement_table_count(connection, "evm_execution_payloads")?;
            }
            if let Some((predecessor_id, previous_blob, next_blob)) =
                &recovered_predecessor_blobs
            {
                let predecessor_key = predecessor_id.to_sql_bytes();
                if connection.query_scalar::<Vec<u8>>(
                    "SELECT value FROM evm_operations WHERE key = ?1",
                    params![predecessor_key.clone()],
                )? != previous_blob.to_sql_bytes()
                {
                    return Err(DbError::Constraint("stale recovery predecessor".into()));
                }
                connection.execute(
                    "UPDATE evm_operations SET value = ?1 WHERE key = ?2",
                    params![next_blob.to_sql_bytes(), predecessor_key],
                )?;
            }
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::EvmOperation)?;
            transition_tracked_entry(
                connection,
                "evm_state_index",
                Some(previous_evm_index.to_sql_bytes()),
                None,
            )?;
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::EvmStateIndex)?;
            connection.execute("DELETE FROM operation_owner_index WHERE key = ?1", params![operation_key])?;
            decrement_table_count(connection, "operation_owner_index")?;
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::OperationOwnerIndex)?;
            detach_confirmed_operation(
                connection,
                previous_schedule.operation_id,
                progress.last_finalized_observation_ns,
            )?;
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::ConfirmationSchedule)?;
            if let Some(audit) = &prepared_audit {
                commit_audit_batch(connection, audit)?;
            }
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::Audit)?;
            if let (Some(admin_blob), Some(audit)) = (&admin_blob, &prepared_audit) {
                connection.execute(
                    "UPDATE singleton_state SET accounting = ?1, counters = ?2, external_progress = ?3,
                        admin_state = ?4, audit_retention = ?5, deposit_admission = ?6 WHERE id = 1",
                    params![accounting_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes(), admin_blob.to_sql_bytes(), audit.retention_blob.to_sql_bytes(), deposit_admission_blob.to_sql_bytes()],
                )?;
            } else if let Some(audit) = &prepared_audit {
                connection.execute(
                    "UPDATE singleton_state SET accounting = ?1, counters = ?2, external_progress = ?3, audit_retention = ?4, deposit_admission = ?5 WHERE id = 1",
                    params![accounting_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes(), audit.retention_blob.to_sql_bytes(), deposit_admission_blob.to_sql_bytes()],
                )?;
            } else {
                connection.execute(
                    "UPDATE singleton_state SET accounting = ?1, counters = ?2, external_progress = ?3, deposit_admission = ?4 WHERE id = 1",
                    params![accounting_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes(), deposit_admission_blob.to_sql_bytes()],
                )?;
            }
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::SingletonState)
        })?;
        Ok(())
    }

    pub fn deposit(&self, id: [u8; 32]) -> Result<Option<DepositRecord>, StorageError> {
        Ok(self.stored_deposit(id)?.map(|stored| stored.record))
    }

    fn stored_deposit(&self, id: [u8; 32]) -> Result<Option<StoredDeposit>, StorageError> {
        self.deposits.get(&id).map(|blob| decode(&blob)).transpose()
    }

    fn deposit_record_blobs(
        &self,
        previous: &DepositRecord,
        next: &DepositRecord,
    ) -> Result<(StableBlob, StableBlob), StorageError> {
        if previous.id != next.id {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let stored = self
            .stored_deposit(previous.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        if stored.record != *previous {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let next_stored = StoredDeposit {
            record: next.clone(),
            owner_sequence: stored.owner_sequence,
            base_recipient: stored.base_recipient,
        };
        Ok((encode(&stored)?, encode(&next_stored)?))
    }

    pub fn put_withdrawal(&mut self, value: &WithdrawalRecord) -> Result<(), StorageError> {
        let previous = self.withdrawal(value.id.bytes())?;
        let fee_delta = match (previous.as_ref().map(|record| &record.state), &value.state) {
            (
                Some(WithdrawalState::ReleasePending { .. }),
                WithdrawalState::Paid { settlement, .. },
            ) => settlement.net_service_fee()?,
            _ => Amount::ZERO,
        };
        let mut accounting = self.accounting()?;
        if fee_delta != Amount::ZERO {
            accounting.confirm_fee(FeeKind::Withdrawal, fee_delta)?;
        }
        let mut counters = self.counters()?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous
                .as_ref()
                .map(is_pending_withdrawal_ledger)
                .unwrap_or(false),
            is_pending_withdrawal_ledger(value),
        )?;
        let value_blob = encode(value)?;
        let counters_blob = encode(&counters)?;
        let accounting_blob = encode(&accounting)?;
        record_write_storage_failpoint(RecordWriteFailpoint::Encode)?;
        let key = value.id.bytes().to_sql_bytes();
        let previous_blob = previous.as_ref().map(encode).transpose()?;
        self.handle.update(|connection| {
            expect_optional_blob(
                connection,
                "SELECT value FROM withdrawals WHERE key = ?1",
                params![key.clone()],
                previous_blob.as_ref().map(StableBlob::as_slice),
                "stale withdrawal write",
            )?;
            if previous.as_ref().is_some_and(is_pending_withdrawal_ledger) {
                remove_table_entry(connection, "release_pending_withdrawal_index", key.clone())?;
            }
            record_write_db_failpoint(RecordWriteFailpoint::RemoveIndex)?;
            if is_pending_withdrawal_ledger(value) {
                upsert_table_entry(
                    connection,
                    "release_pending_withdrawal_index",
                    key.clone(),
                    0u8.to_sql_bytes(),
                )?;
            }
            record_write_db_failpoint(RecordWriteFailpoint::AddIndex)?;
            record_write_db_failpoint(RecordWriteFailpoint::OperationOwner)?;
            replace_withdrawal_row(connection, key, previous_blob.as_ref(), &value_blob)?;
            record_write_db_failpoint(RecordWriteFailpoint::Record)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, accounting = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), accounting_blob.to_sql_bytes()],
            )?;
            record_write_db_failpoint(RecordWriteFailpoint::SingletonState)
        })?;
        Ok(())
    }

    pub fn withdrawal(&self, id: [u8; 32]) -> Result<Option<WithdrawalRecord>, StorageError> {
        self.withdrawals
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn notified_withdrawal_id(
        &self,
        transaction_hash: [u8; 32],
    ) -> Result<Option<[u8; 32]>, StorageError> {
        Ok(self.withdrawal_notification_index.get(&transaction_hash))
    }

    pub fn nonterminal_withdrawal_count(&self) -> Result<u64, StorageError> {
        self.table_count_value("withdrawal_liability_index")
    }

    pub fn withdrawal_liability_summary(&self) -> Result<WithdrawalLiabilitySummary, StorageError> {
        self.handle
            .query(|connection| {
                let count = connection.query_scalar::<Vec<u8>>(
                    "SELECT count FROM table_counts WHERE name = 'withdrawal_liability_index'",
                    params![],
                )?;
                let amount = connection.query_scalar::<Vec<u8>>(
                    "SELECT withdrawal_liability_amount FROM singleton_state WHERE id = 1",
                    params![],
                )?;
                let oldest = connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT key FROM withdrawal_liability_index ORDER BY key LIMIT 1",
                    params![],
                )?;
                let reasons = connection.query_all(
                    "SELECT key FROM withdrawal_stop_reason_counts ORDER BY key",
                    params![],
                    |row| row.get::<Vec<u8>>(0),
                )?;
                Ok(WithdrawalLiabilitySummary {
                    count: u64::from_sql_bytes(count)
                        .map_err(|_| DbError::Constraint("invalid liability count".into()))?,
                    amount_out: u128::from_sql_bytes(amount)
                        .map_err(|_| DbError::Constraint("invalid liability amount".into()))?,
                    oldest_observed_at_ns: oldest
                        .map(|key| {
                            key.get(..8)
                                .and_then(|bytes| bytes.try_into().ok())
                                .map(u64::from_be_bytes)
                                .ok_or_else(|| DbError::Constraint("invalid liability key".into()))
                        })
                        .transpose()?,
                    stop_reasons: reasons
                        .into_iter()
                        .map(|reason| {
                            String::from_utf8(reason).map_err(|_| {
                                DbError::Constraint("invalid withdrawal stop reason".into())
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                })
            })
            .map_err(Into::into)
    }

    pub fn deposit_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<DepositRecord>, StorageError> {
        let Some(owner) = self.operation_owner_index.get(&operation_id.get()) else {
            return Ok(None);
        };
        let OperationOwner::Deposit(id) = decode::<OperationOwner>(&owner)?;
        self.deposit(id)
    }

    pub fn withdrawal_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<WithdrawalRecord>, StorageError> {
        let _ = operation_id;
        Ok(None)
    }

    pub fn put_evm_operation(&mut self, value: &EvmOperationRecord) -> Result<(), StorageError> {
        self.put_evm_operation_inner(value, None, None)
    }

    pub fn put_submitted_evm_operation(
        &mut self,
        value: &EvmOperationRecord,
        submitted_at_ns: u64,
    ) -> Result<(), StorageError> {
        self.put_evm_operation_inner(
            value,
            Some(ConfirmationSchedule {
                operation_id: value.id.get(),
                submitted_at_ns,
                next_check_at_ns: submitted_at_ns.saturating_add(60 * 1_000_000_000),
                checks_completed: 0,
            }),
            None,
        )
    }

    pub fn put_submitted_evm_operation_with_rpc_audit(
        &mut self,
        value: &EvmOperationRecord,
        submitted_at_ns: u64,
        caller: Principal,
        audit_kinds: Vec<AuditEventKind>,
    ) -> Result<(), StorageError> {
        self.put_evm_operation_inner(
            value,
            Some(ConfirmationSchedule {
                operation_id: value.id.get(),
                submitted_at_ns,
                next_check_at_ns: submitted_at_ns.saturating_add(60 * 1_000_000_000),
                checks_completed: 0,
            }),
            Some((caller, submitted_at_ns, audit_kinds)),
        )
    }

    fn put_evm_operation_inner(
        &mut self,
        value: &EvmOperationRecord,
        submission_schedule: Option<ConfirmationSchedule>,
        rpc_audit: Option<(Principal, u64, Vec<AuditEventKind>)>,
    ) -> Result<(), StorageError> {
        let payload = self.evm_execution_payload(value.id.get())?;
        match (&value.state, &payload) {
            (EvmOperationState::Queued, Some(EvmExecutionPayload::AwaitingNonce(intent)))
                if intent.operation_id == value.id && intent.payload_hash == value.payload_hash => {
            }
            (EvmOperationState::Prepared, Some(EvmExecutionPayload::Prepared(envelope)))
                if envelope.operation_id == value.id
                    && envelope.payload_hash == value.payload_hash => {}
            (
                EvmOperationState::Submitted { .. }
                | EvmOperationState::Confirmed { .. }
                | EvmOperationState::Reverted { .. }
                | EvmOperationState::RecoveryPending { .. }
                | EvmOperationState::Recovered { .. },
                _,
            ) => {}
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        }
        let encoded_value = encode(value)?;
        let previous = self
            .evm_operations
            .get(&value.id.get())
            .map(|blob| decode::<EvmOperationRecord>(&blob))
            .transpose()?;
        let mut counters = self.counters()?;
        counters.unresolved_evm_reverts = adjust_active_count(
            counters.unresolved_evm_reverts,
            previous.as_ref().map(is_reverted_evm).unwrap_or(false),
            is_reverted_evm(value),
        )?;
        counters.awaiting_nonce_evm_operations = adjust_active_count(
            counters.awaiting_nonce_evm_operations,
            previous
                .as_ref()
                .is_some_and(|operation| matches!(operation.state, EvmOperationState::Queued)),
            matches!(value.state, EvmOperationState::Queued),
        )?;
        let prepared_audit = rpc_audit
            .map(|(caller, timestamp_ns, kinds)| {
                self.prepare_audit_batch(&mut counters, caller, timestamp_ns, kinds)
            })
            .transpose()?;
        let encoded_counters = encode(&counters)?;
        let previous_key = previous
            .as_ref()
            .map(evm_state_index_key)
            .transpose()?
            .flatten();
        let next_key = evm_state_index_key(value)?;
        let removes_payload = matches!(
            value.state,
            EvmOperationState::Confirmed { .. }
                | EvmOperationState::Reverted { .. }
                | EvmOperationState::RecoveryPending { .. }
                | EvmOperationState::Recovered { .. }
        );
        let removes_owner = matches!(
            value.state,
            EvmOperationState::Confirmed { .. }
                | EvmOperationState::Reverted { .. }
                | EvmOperationState::RecoveryPending { .. }
                | EvmOperationState::Recovered { .. }
        );
        let payload_present = payload.is_some();
        let owner_present =
            removes_owner && self.operation_owner_index.get(&value.id.get()).is_some();
        let previous_schedule = self.confirmation_schedule(value.id.get())?;
        let submission_owner = if submission_schedule.is_some() {
            let owner = self
                .operation_owner_index
                .get(&value.id.get())
                .ok_or(StorageError::RecordNotFound)?;
            let OperationOwner::Deposit(id) = decode::<OperationOwner>(&owner)?;
            Some((SettlementJobKind::Deposit, id))
        } else {
            None
        };
        let removes_schedule = matches!(
            value.state,
            EvmOperationState::Confirmed { .. }
                | EvmOperationState::Reverted { .. }
                | EvmOperationState::RecoveryPending { .. }
                | EvmOperationState::Recovered { .. }
        );
        let operation_key = value.id.get().to_sql_bytes();
        self.handle.update(|connection| {
            transition_tracked_entry(
                connection,
                "evm_state_index",
                previous_key.map(|key| key.to_sql_bytes()),
                next_key.map(|key| (key.to_sql_bytes(), 0u8.to_sql_bytes())),
            )?;
            upsert_table_entry(
                connection,
                "evm_operations",
                operation_key.clone(),
                encoded_value.to_sql_bytes(),
            )?;
            if removes_payload && payload_present {
                delete_tracked_entry(connection, "evm_execution_payloads", operation_key.clone())?;
            }
            if owner_present {
                delete_tracked_entry(connection, "operation_owner_index", operation_key)?;
            }
            if removes_schedule {
                if let Some(schedule) = previous_schedule {
                    detach_confirmed_operation(
                        connection,
                        schedule.operation_id,
                        schedule.next_check_at_ns,
                    )?;
                }
            } else if let (Some(schedule), Some((kind, id))) =
                (submission_schedule, submission_owner)
            {
                upsert_confirmation_schedule(connection, kind, id, schedule)?;
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Business)?;
            if let Some(audit) = &prepared_audit {
                commit_audit_batch(connection, audit)?;
                rpc_atomic_db_failpoint(RpcAtomicFailpoint::Audit)?;
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, audit_retention = ?2 WHERE id = 1",
                    params![
                        encoded_counters.to_sql_bytes(),
                        audit.retention_blob.to_sql_bytes()
                    ],
                )?;
            } else {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                    params![encoded_counters.to_sql_bytes()],
                )?;
            }
            rpc_atomic_db_failpoint(RpcAtomicFailpoint::Singleton)
        })?;
        Ok(())
    }

    pub fn evm_operation(&self, id: u64) -> Result<Option<EvmOperationRecord>, StorageError> {
        self.evm_operations
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    fn put_reconciliation_hold(
        &mut self,
        value: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        let encoded_value = encode(value)?;
        let previous = self
            .reconciliation_holds
            .get(&value.id.get())
            .map(|blob| decode::<ReconciliationHoldRecord>(&blob))
            .transpose()?;
        record_write_storage_failpoint(RecordWriteFailpoint::Encode)?;
        let key = value.id.get().to_sql_bytes();
        let previous_blob = previous.as_ref().map(encode).transpose()?;
        self.handle.update(|connection| {
            expect_optional_blob(
                connection,
                "SELECT value FROM reconciliation_holds WHERE key = ?1",
                params![key.clone()],
                previous_blob.as_ref().map(StableBlob::as_slice),
                "stale reconciliation hold write",
            )?;
            let previous_open = previous
                .as_ref()
                .is_some_and(is_open_hold)
                .then(|| key.clone());
            let next_open = is_open_hold(value).then(|| (key.clone(), 0u8.to_sql_bytes()));
            transition_tracked_entry(connection, "open_hold_index", previous_open, next_open)?;
            record_write_db_failpoint(RecordWriteFailpoint::RemoveIndex)?;
            record_write_db_failpoint(RecordWriteFailpoint::AddIndex)?;
            record_write_db_failpoint(RecordWriteFailpoint::OperationOwner)?;
            upsert_table_entry(
                connection,
                "reconciliation_holds",
                key,
                encoded_value.to_sql_bytes(),
            )?;
            record_write_db_failpoint(RecordWriteFailpoint::Record)?;
            record_write_db_failpoint(RecordWriteFailpoint::SingletonState)
        })?;
        Ok(())
    }

    pub fn put_open_reconciliation_hold(
        &mut self,
        value: &ReconciliationHoldRecord,
    ) -> Result<(), StorageError> {
        if !is_open_hold(value) {
            return Err(StorageError::Core(CoreError::HoldMismatch));
        }
        if let Some(previous) = self.reconciliation_hold(value.id.get())? {
            if previous == *value {
                return Ok(());
            }
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        self.put_reconciliation_hold(value)
    }

    pub fn reconciliation_hold(
        &self,
        id: u64,
    ) -> Result<Option<ReconciliationHoldRecord>, StorageError> {
        self.reconciliation_holds
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn resolve_deposit_hold(
        &mut self,
        deposit_id: DepositId,
        hold_id: HoldId,
        resolution: DepositHoldResolution,
    ) -> Result<ApplyResult, StorageError> {
        self.resolve_deposit_hold_and_scan(deposit_id, hold_id, resolution, None)
    }

    pub fn resolve_deposit_hold_and_scan(
        &mut self,
        deposit_id: DepositId,
        hold_id: HoldId,
        resolution: DepositHoldResolution,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<ApplyResult, StorageError> {
        let mut deposit = self
            .deposit(deposit_id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let mut hold = self
            .reconciliation_hold(hold_id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let result = resolve_deposit_hold(&mut deposit, &mut hold, resolution)?;
        self.persist_resolved_deposit_and_hold(&deposit, &hold, scan_target)?;
        Ok(result)
    }

    pub fn resolve_withdrawal_hold(
        &mut self,
        withdrawal_id: WithdrawalId,
        hold_id: HoldId,
        resolution: WithdrawalHoldResolution,
    ) -> Result<ApplyResult, StorageError> {
        self.resolve_withdrawal_hold_and_scan(withdrawal_id, hold_id, resolution, None)
    }

    pub fn resolve_withdrawal_hold_and_scan(
        &mut self,
        withdrawal_id: WithdrawalId,
        hold_id: HoldId,
        resolution: WithdrawalHoldResolution,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<ApplyResult, StorageError> {
        let mut withdrawal = self
            .withdrawal(withdrawal_id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let mut hold = self
            .reconciliation_hold(hold_id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let result = resolve_withdrawal_hold(&mut withdrawal, &mut hold, resolution)?;
        self.persist_resolved_withdrawal_and_hold(&withdrawal, &hold, scan_target)?;
        Ok(result)
    }

    fn persist_resolved_deposit_and_hold(
        &mut self,
        deposit: &DepositRecord,
        hold: &ReconciliationHoldRecord,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<(), StorageError> {
        let previous = self
            .deposit(deposit.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        self.persist_resolved_hold_bundle(
            hold,
            ResolveHoldBundleParent::Deposit {
                previous: &previous,
                next: deposit,
            },
            scan_target,
        )
    }

    fn persist_resolved_withdrawal_and_hold(
        &mut self,
        withdrawal: &WithdrawalRecord,
        hold: &ReconciliationHoldRecord,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<(), StorageError> {
        let previous = self
            .withdrawal(withdrawal.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        self.persist_resolved_hold_bundle(
            hold,
            ResolveHoldBundleParent::Withdrawal {
                previous: &previous,
                next: withdrawal,
            },
            scan_target,
        )
    }

    fn persist_resolved_hold_bundle(
        &mut self,
        hold: &ReconciliationHoldRecord,
        parent: ResolveHoldBundleParent<'_>,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<(), StorageError> {
        let previous_hold = self
            .reconciliation_hold(hold.id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let parent_is_replay = match parent {
            ResolveHoldBundleParent::Deposit { previous, next } => previous == next,
            ResolveHoldBundleParent::Withdrawal { previous, next } => previous == next,
        };
        if previous_hold == *hold && parent_is_replay && scan_target.is_none() {
            return Ok(());
        }
        if !is_open_hold(&previous_hold) || is_open_hold(hold) {
            return Err(StorageError::Core(CoreError::HoldMismatch));
        }
        let fee_delta = match parent {
            ResolveHoldBundleParent::Withdrawal { previous, next }
                if matches!(previous.state, WithdrawalState::ReconciliationHold { .. }) =>
            {
                match &next.state {
                    WithdrawalState::Paid { settlement, .. } => settlement.net_service_fee()?,
                    _ => Amount::ZERO,
                }
            }
            _ => Amount::ZERO,
        };
        let mut accounting = self.accounting()?;
        if fee_delta != Amount::ZERO {
            accounting.confirm_fee(FeeKind::Withdrawal, fee_delta)?;
        }
        let previous_counters = self.counters()?;
        let mut counters = previous_counters;
        let (table, key, previous_parent_blob, parent_blob, previous_index, next_index) =
            match parent {
                ResolveHoldBundleParent::Deposit { previous, next } => {
                    counters.pending_ledger_operations = adjust_active_count(
                        counters.pending_ledger_operations,
                        is_pending_deposit_ledger(previous),
                        is_pending_deposit_ledger(next),
                    )?;
                    counters.reserved_deposit_mint_amount = adjust_reserved_mint_amount(
                        counters.reserved_deposit_mint_amount,
                        Some(previous),
                        next,
                    )?;
                    counters.reserved_deposit_mint_operations = adjust_reserved_mint_operations(
                        counters.reserved_deposit_mint_operations,
                        Some(previous),
                        next,
                    )?;
                    let (previous_blob, next_blob) = self.deposit_record_blobs(previous, next)?;
                    (
                        "deposits",
                        next.id.bytes(),
                        previous_blob,
                        next_blob,
                        is_pending_deposit_ledger(previous),
                        is_pending_deposit_ledger(next),
                    )
                }
                ResolveHoldBundleParent::Withdrawal { previous, next } => {
                    counters.pending_ledger_operations = adjust_active_count(
                        counters.pending_ledger_operations,
                        is_pending_withdrawal_ledger(previous),
                        is_pending_withdrawal_ledger(next),
                    )?;
                    (
                        "withdrawals",
                        next.id.bytes(),
                        encode(previous)?,
                        encode(next)?,
                        is_pending_withdrawal_ledger(previous),
                        is_pending_withdrawal_ledger(next),
                    )
                }
            };
        if self.open_hold_index.get(&hold.id.get()).is_none() {
            return Err(StorageError::RecordNotFound);
        }
        let parent_index_present = if table == "deposits" {
            self.pull_pending_deposit_index.get(&key).is_some()
        } else {
            self.release_pending_withdrawal_index.get(&key).is_some()
        };
        if parent_index_present != previous_index {
            return Err(StorageError::RecordNotFound);
        }
        resolve_hold_bundle_storage_failpoint(ResolveHoldBundleFailpoint::Encode)?;
        let previous_hold_blob = encode(&previous_hold)?;
        let hold_blob = encode(hold)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let counters_blob = encode(&counters)?;
        let accounting_blob = encode(&accounting)?;
        let scan_blob = scan_target
            .map(|target| {
                self.reconciliation_scan(target)?
                    .map(|scan| encode(&scan))
                    .transpose()?
                    .ok_or(StorageError::RecordNotFound)
            })
            .transpose()?;
        let scan_key = scan_target.map(reconciliation_scan_key);
        let key = key.to_sql_bytes();
        self.handle.update(|connection| {
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_counters != previous_counters_blob.to_sql_bytes() {
                return Err(DbError::Constraint("stale resolved hold counters".into()));
            }
            let select_parent = if table == "deposits" {
                "SELECT value FROM deposits WHERE key = ?1"
            } else {
                "SELECT value FROM withdrawals WHERE key = ?1"
            };
            if connection.query_scalar::<Vec<u8>>(select_parent, params![key.clone()])?
                != previous_parent_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale resolved hold parent".into()));
            }
            if connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM reconciliation_holds WHERE key = ?1",
                params![hold.id.get().to_sql_bytes()],
            )? != previous_hold_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale resolved hold".into()));
            }
            if table == "deposits" {
                connection.execute(
                    "UPDATE deposits SET value = ?1 WHERE key = ?2",
                    params![parent_blob.to_sql_bytes(), key.clone()],
                )?;
            } else {
                replace_withdrawal_row(
                    connection,
                    key.clone(),
                    Some(&previous_parent_blob),
                    &parent_blob,
                )?;
            }
            resolve_hold_bundle_db_failpoint(ResolveHoldBundleFailpoint::Parent)?;
            let index_table = if table == "deposits" {
                "pull_pending_deposit_index"
            } else {
                "release_pending_withdrawal_index"
            };
            if previous_index {
                let delete = if table == "deposits" {
                    "DELETE FROM pull_pending_deposit_index WHERE key = ?1"
                } else {
                    "DELETE FROM release_pending_withdrawal_index WHERE key = ?1"
                };
                connection.execute(delete, params![key.clone()])?;
                decrement_table_count(connection, index_table)?;
            }
            if next_index {
                let insert = if table == "deposits" {
                    "INSERT INTO pull_pending_deposit_index(key, value) VALUES (?1, ?2)"
                } else {
                    "INSERT INTO release_pending_withdrawal_index(key, value) VALUES (?1, ?2)"
                };
                connection.execute(insert, params![key, 0u8.to_sql_bytes()])?;
                increment_table_count(connection, index_table)?;
            }
            resolve_hold_bundle_db_failpoint(ResolveHoldBundleFailpoint::ParentIndex)?;
            connection.execute(
                "UPDATE reconciliation_holds SET value = ?1 WHERE key = ?2",
                params![hold_blob.to_sql_bytes(), hold.id.get().to_sql_bytes()],
            )?;
            resolve_hold_bundle_db_failpoint(ResolveHoldBundleFailpoint::Hold)?;
            transition_tracked_entry(
                connection,
                "open_hold_index",
                Some(hold.id.get().to_sql_bytes()),
                None,
            )?;
            resolve_hold_bundle_db_failpoint(ResolveHoldBundleFailpoint::OpenHoldIndex)?;
            if let (Some(scan_key), Some(scan_blob)) = (scan_key, &scan_blob) {
                if connection.query_scalar::<Vec<u8>>(
                    "SELECT value FROM reconciliation_scans WHERE key = ?1",
                    params![scan_key.to_sql_bytes()],
                )? != scan_blob.to_sql_bytes()
                {
                    return Err(DbError::Constraint("stale reconciliation scan".into()));
                }
                connection.execute(
                    "DELETE FROM reconciliation_scans WHERE key = ?1",
                    params![scan_key.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "reconciliation_scans")?;
            }
            resolve_hold_bundle_db_failpoint(ResolveHoldBundleFailpoint::ReconciliationScan)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, accounting = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), accounting_blob.to_sql_bytes()],
            )?;
            resolve_hold_bundle_db_failpoint(ResolveHoldBundleFailpoint::SingletonState)
        })?;
        Ok(())
    }

    pub fn status_counts(&self) -> Result<StorageCounts, StorageError> {
        let counters = self.counters()?;
        let audit_retention: AuditRetentionState = decode(&self.audit_retention.get()?)?;

        Ok(StorageCounts {
            deposits: self.deposits.len(),
            withdrawals: self.withdrawals.len(),
            pending_evm_operations: self.table_count_value("evm_state_index")?,
            reconciliation_holds: self.table_count_value("open_hold_index")?,
            pending_ledger_operations: counters.pending_ledger_operations,
            reserved_deposit_mint_amount: counters.reserved_deposit_mint_amount,
            reserved_deposit_mint_operations: counters.reserved_deposit_mint_operations,
            unresolved_evm_reverts: counters.unresolved_evm_reverts,
            last_finalized_base_block: self.external_progress()?.last_finalized_base_block,
            active_evm_payloads: self.evm_execution_payloads.len(),
            retained_audit_events: self.audit_events.len(),
            pruned_audit_events: audit_retention.pruned_count,
            retained_deposit_index_entries: self.deposit_owner_index.len(),
        })
    }
}

fn is_open_hold(value: &ReconciliationHoldRecord) -> bool {
    let state = match value.state {
        ReconciliationHoldState::Open => 0,
        ReconciliationHoldState::ResolvedSucceeded { .. } => 1,
        ReconciliationHoldState::ResolvedAbsent { .. } => 2,
    };
    bridge_core::reconciliation_hold_indexed(state)
}

fn is_pending_deposit_ledger(value: &DepositRecord) -> bool {
    matches!(
        value.state,
        bridge_core::DepositState::FundingPending | bridge_core::DepositState::RefundPending { .. }
    )
}

fn refund_compensation_debit(value: &DepositRecord) -> Result<Amount, StorageError> {
    let attempt = match &value.state {
        bridge_core::DepositState::RefundPending { attempt, .. }
        | bridge_core::DepositState::RefundReconciliationHold { attempt, .. }
        | bridge_core::DepositState::Refunded { attempt, .. } => attempt,
        _ => return Ok(Amount::ZERO),
    };
    let total = attempt
        .identity
        .amount
        .checked_add(attempt.identity.fee)
        .map_err(StorageError::Core)?;
    Ok(if total > value.gross_amount {
        total
            .checked_sub(value.gross_amount)
            .map_err(StorageError::Core)?
    } else {
        Amount::ZERO
    })
}

fn is_deposit_mint_reserved(value: &DepositRecord) -> bool {
    value.reserves_mint_resources()
}

fn adjust_reserved_mint_amount(
    current: u128,
    previous: Option<&DepositRecord>,
    next: &DepositRecord,
) -> Result<u128, StorageError> {
    let without_previous = if previous.is_some_and(is_deposit_mint_reserved) {
        current
            .checked_sub(
                previous
                    .expect("checked previous")
                    .reserved_mint_amount()
                    .map_err(StorageError::Core)?
                    .get(),
            )
            .ok_or(StorageError::CounterUnderflow)?
    } else {
        current
    };
    if is_deposit_mint_reserved(next) {
        without_previous
            .checked_add(
                next.reserved_mint_amount()
                    .map_err(StorageError::Core)?
                    .get(),
            )
            .ok_or(StorageError::CounterOverflow)
    } else {
        Ok(without_previous)
    }
}

fn adjust_reserved_mint_operations(
    current: u64,
    previous: Option<&DepositRecord>,
    next: &DepositRecord,
) -> Result<u64, StorageError> {
    adjust_active_count(
        current,
        previous.is_some_and(is_deposit_mint_reserved),
        is_deposit_mint_reserved(next),
    )
}

fn is_pending_withdrawal_ledger(value: &WithdrawalRecord) -> bool {
    matches!(value.state, WithdrawalState::ReleasePending { .. })
}

fn is_nonterminal_withdrawal(value: &WithdrawalRecord) -> bool {
    let state = match value.state {
        WithdrawalState::Observed => 0,
        WithdrawalState::ReleasePending { .. } => 1,
        WithdrawalState::Paid { .. } => 2,
        WithdrawalState::ReconciliationHold { .. } => 3,
    };
    bridge_core::withdrawal_liability_indexed(state)
}

fn is_pending_fee_payout(value: &crate::admin::FeePayoutRecord) -> bool {
    matches!(
        value.state,
        crate::admin::FeePayoutState::Pending | crate::admin::FeePayoutState::ReconciliationHold
    )
}

fn fee_payout_debit(value: &crate::admin::FeePayoutRecord) -> Result<u128, StorageError> {
    bridge_core::payout_debit(true, value.amount, value.transfer.fee.get())
        .ok_or(StorageError::CounterOverflow)
}

fn adjust_pending_fee_payout_debit(
    current: u128,
    previous: Option<&crate::admin::FeePayoutRecord>,
    next: &crate::admin::FeePayoutRecord,
) -> Result<u128, StorageError> {
    let without_previous = if previous.is_some_and(is_pending_fee_payout) {
        current
            .checked_sub(fee_payout_debit(previous.expect("checked previous"))?)
            .ok_or(StorageError::CounterUnderflow)?
    } else {
        current
    };
    if is_pending_fee_payout(next) {
        without_previous
            .checked_add(fee_payout_debit(next)?)
            .ok_or(StorageError::CounterOverflow)
    } else {
        Ok(without_previous)
    }
}

fn adjust_active_count(
    current: u64,
    was_active: bool,
    is_active: bool,
) -> Result<u64, StorageError> {
    bridge_core::checked_counter_transition(current, was_active, is_active).ok_or(if is_active {
        StorageError::CounterOverflow
    } else {
        StorageError::CounterUnderflow
    })
}

fn encode<T: Serialize>(value: &T) -> Result<StableBlob, StorageError> {
    let mut bytes = vec![WIRE_VERSION];
    ciborium::into_writer(value, &mut bytes).map_err(|_| StorageError::EncodeFailed)?;
    StableBlob::new(bytes)
}

fn decode<T: DeserializeOwned>(blob: &StableBlob) -> Result<T, StorageError> {
    let (version, payload) = blob
        .as_slice()
        .split_first()
        .ok_or(StorageError::MissingWireVersion)?;
    if *version != WIRE_VERSION {
        return Err(StorageError::UnsupportedWireVersion(*version));
    }
    let mut cursor = Cursor::new(payload);
    let value = ciborium::from_reader(&mut cursor).map_err(|_| StorageError::DecodeFailed)?;
    if cursor.position() != payload.len() as u64 {
        return Err(StorageError::DecodeFailed);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{
        Account, Amount, ApplyOutcome, BaseMintSnapshot, DepositEvent, DepositHoldResolution,
        DepositId, DepositQuote, DepositRequest, DepositState, EvmCallIntent, EvmOperationEvent,
        EvmOperationId, EvmOperationKind, HoldId, LedgerOperation, LedgerTransferIdentity,
        ReconciliationArchiveRange, ReconciliationHoldRecord, ReconciliationHoldState,
        ReconciliationLedgerPage, ReconciliationScanPhase, ReconciliationScanProgress,
        ReconciliationTarget, RequestReference, Settlement, TransferAttempt, WithdrawalEvent,
        WithdrawalHoldResolution, WithdrawalId,
    };
    use ic_sqlite_vfs::DefaultMemoryImpl as VectorMemory;
    use serial_test::serial;

    fn account(tag: u8) -> Account {
        Account::new(vec![tag], [tag; 32]).expect("valid account")
    }

    fn transfer(operation: LedgerOperation, amount: u128, tag: u8) -> LedgerTransferIdentity {
        LedgerTransferIdentity {
            operation,
            created_at_time_ns: u64::from(tag),
            memo: [tag; 32],
            amount: Amount::new(amount),
            fee: Amount::new(1),
            from: account(tag),
            to: account(tag + 1),
            spender: None,
        }
    }

    fn withdrawal_transfer(amount: u128, tag: u8) -> LedgerTransferIdentity {
        let mut identity = transfer(LedgerOperation::ReleaseWithdrawal, amount, tag);
        identity.to = Account::new(vec![1], [0; 32]).expect("valid withdrawal destination");
        identity
    }

    fn deposit() -> DepositRecord {
        let mut deposit = DepositRecord::accept(DepositRequest {
            id: DepositId::new([1; 32]),
            payload_hash: [2; 32],
            gross_amount: Amount::new(110),
            user_max_service_fee: Amount::new(10),
            transfer: transfer(LedgerOperation::PullDeposit, 110, 10),
        })
        .expect("valid deposit");
        deposit
            .apply(DepositEvent::FundingSucceeded {
                ledger_block_index: 4,
            })
            .expect("escrowed");
        deposit
    }

    fn funding_pending_deposit() -> DepositRecord {
        DepositRecord::accept(DepositRequest {
            id: DepositId::new([31; 32]),
            payload_hash: [32; 32],
            gross_amount: Amount::new(110),
            user_max_service_fee: Amount::new(10),
            transfer: transfer(LedgerOperation::PullDeposit, 110, 30),
        })
        .expect("valid funding deposit")
    }

    fn deposit_for(owner: Principal) -> DepositRecord {
        let mut deposit = deposit();
        deposit.transfer.from =
            Account::new(owner.as_slice().to_vec(), [0; 32]).expect("valid deposit owner");
        deposit
    }

    fn test_deposit_quote() -> DepositQuote {
        DepositQuote {
            service_fee: Amount::new(10),
            net_amount: Amount::new(100),
        }
    }

    fn mint_snapshot() -> BaseMintSnapshot {
        BaseMintSnapshot {
            finalized_head_block_number: 1,
            confirmed_block_timestamp: 1,
            service_fee: Amount::new(10),
            max_service_fee: Amount::new(20),
            per_deposit_limit: Amount::new(1_000),
            mint_window_limit: Amount::new(10_000),
            mint_window_started_at: 0,
            mint_window_duration: 100,
            minted_in_window: Amount::ZERO,
        }
    }

    fn withdrawal() -> WithdrawalRecord {
        let mut withdrawal = WithdrawalRecord::observed(
            WithdrawalId::new([3; 32]),
            [0; 20],
            vec![1],
            [0; 32],
            [4; 32],
            Amount::new(100),
            Amount::new(20),
            Amount::new(10),
            Amount::new(90),
            1,
        )
        .expect("valid withdrawal");
        withdrawal
            .apply(WithdrawalEvent::StartRelease {
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity: withdrawal_transfer(90, 20),
                }),
                settlement: Settlement {
                    amount_out: Amount::new(90),
                    service_fee: Amount::new(10),
                    ledger_fee: Amount::new(1),
                },
            })
            .expect("release pending");
        withdrawal
    }

    fn fee_guard_withdrawal(tag: u8) -> WithdrawalRecord {
        let mut withdrawal = WithdrawalRecord::observed(
            WithdrawalId::new([tag; 32]),
            [0; 20],
            vec![1],
            [0; 32],
            [tag.saturating_add(1); 32],
            Amount::new(100),
            Amount::new(20),
            Amount::new(10),
            Amount::new(90),
            1,
        )
        .expect("valid guarded withdrawal");
        withdrawal.last_settlement_stop_reason = Some("LedgerFeeExceedsServiceFee".to_owned());
        withdrawal
    }

    fn fee_guard_admin(store: &StableStore, ledger_fee: u128, tripped_at_ns: u64) -> AdminState {
        let mut admin = store.admin_state().expect("admin state");
        admin.withdrawal_fee_guard = Some(crate::admin::WithdrawalFeeGuard {
            ledger_fee,
            charged_service_fee: 10,
            tripped_at_ns,
        });
        admin
    }

    fn release_guarded_withdrawal(mut withdrawal: WithdrawalRecord) -> WithdrawalRecord {
        withdrawal
            .apply(WithdrawalEvent::StartRelease {
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity: withdrawal_transfer(90, 21),
                }),
                settlement: Settlement {
                    amount_out: Amount::new(90),
                    service_fee: Amount::new(10),
                    ledger_fee: Amount::new(1),
                },
            })
            .expect("start guarded release");
        withdrawal.last_settlement_stop_reason = None;
        withdrawal
    }

    fn config() -> BridgeInitArgs {
        let principal = Principal::self_authenticating([7; 32]);
        BridgeInitArgs {
            ledger_canister_id: principal,
            index_canister_id: principal,
            evm_rpc_canister_id: principal,
            custom_evm_rpc_urls: vec![],
            base_chain_id: 8453,
            bridge_contract: vec![1; 20],
            timelock_contract: vec![2; 20],
            ecdsa_key_name: "test_key".into(),
            ecdsa_derivation_path: vec![],
            governance_ecdsa_derivation_path: vec![b"governance-operator".to_vec()],
            deposit_rate_limit_window_seconds: 60,
            deposit_rate_limit_global: 30,
            deposit_rate_limit_per_principal: 3,
            settlement_rate_limit_window_seconds: 600,
            settlement_rate_limit_global: 60,
            settlement_rate_limit_per_principal: 6,
            settlement_rate_limit_per_record: 3,
            transaction_gas_limit: 500_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 1,
            evm_liveness: crate::config::EvmLivenessPolicy::default(),
            eth_floor_wei: 1,
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: principal,
            pause_principal: Principal::from_slice(&[2]),
            fee_recipient: FeeRecipientConfig {
                owner: Principal::from_slice(&[3]),
                subaccount: vec![],
            },
        }
    }

    fn intent(id: [u8; 32], owner: Principal) -> DepositIntent {
        DepositIntent {
            deposit_id: id,
            caller: owner.as_slice().to_vec(),
            owner_sequence: 0,
            base_recipient: [9; 20],
            from_subaccount: [0; 32],
            payload_hash: [2; 32],
        }
    }

    fn evm_intent(operation_id: EvmOperationId, payload_hash: [u8; 32]) -> EvmCallIntent {
        EvmCallIntent {
            operation_id,
            payload_hash,
            chain_id: 8453,
            contract: [7; 20],
            calldata: vec![1, 2, 3, 4],
            gas_limit: 100_000,
            max_fee_per_gas: 2,
            max_priority_fee_per_gas: 1,
        }
    }

    fn rpc_audit_kind(tag: u8) -> AuditEventKind {
        AuditEventKind::EvmRpcObservation {
            evm_rpc_canister_id: Principal::self_authenticating([tag; 32]),
            call_method: "multi_request".into(),
            request_digest: vec![tag; 32],
            quorum_response_digest: vec![tag.wrapping_add(1); 32],
            finalized_block_number: u64::from(tag),
            finalized_block_hash: vec![tag.wrapping_add(2); 32],
            transaction_hash: Some(vec![tag.wrapping_add(3); 32]),
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct RpcAtomicSnapshot {
        counters: CounterState,
        progress: ExternalProgress,
        admin: StableBlob,
        admission: StableBlob,
        retention: StableBlob,
        withdrawals: Vec<([u8; 32], StableBlob)>,
        operations: Vec<(u64, StableBlob)>,
        payloads: Vec<(u64, StableBlob)>,
        owners: Vec<(u64, StableBlob)>,
        evm_indexes: Vec<(StableBlob, u8)>,
        release_indexes: Vec<([u8; 32], u8)>,
        notification_indexes: Vec<([u8; 32], [u8; 32])>,
        audits: Vec<(u64, StableBlob)>,
        schedule: Option<ConfirmationSchedule>,
        settlement_jobs: Vec<(i64, Vec<u8>, i64, u64)>,
    }

    fn rpc_atomic_snapshot(store: &StableStore, operation_id: Option<u64>) -> RpcAtomicSnapshot {
        RpcAtomicSnapshot {
            counters: store.counters().expect("counters"),
            progress: store.external_progress().expect("progress"),
            admin: store.admin_state.get().expect("admin blob"),
            admission: store.deposit_admission.get().expect("admission blob"),
            retention: store.audit_retention.get().expect("retention blob"),
            withdrawals: store
                .withdrawals
                .iter()
                .map(|entry| (*entry.key(), entry.value()))
                .collect(),
            operations: store
                .evm_operations
                .iter()
                .map(|entry| (*entry.key(), entry.value()))
                .collect(),
            payloads: store
                .evm_execution_payloads
                .iter()
                .map(|entry| (*entry.key(), entry.value()))
                .collect(),
            owners: store
                .operation_owner_index
                .iter()
                .map(|entry| (*entry.key(), entry.value()))
                .collect(),
            evm_indexes: store
                .evm_state_index
                .iter()
                .map(|entry| (entry.key().clone(), entry.value()))
                .collect(),
            release_indexes: store
                .release_pending_withdrawal_index
                .iter()
                .map(|entry| (*entry.key(), entry.value()))
                .collect(),
            notification_indexes: store
                .withdrawal_notification_index
                .iter()
                .map(|entry| (*entry.key(), entry.value()))
                .collect(),
            audits: store
                .audit_events
                .iter()
                .map(|entry| (*entry.key(), entry.value()))
                .collect(),
            schedule: operation_id
                .and_then(|id| store.confirmation_schedule(id).expect("schedule")),
            settlement_jobs: store
                .handle
                .query(|connection| {
                    connection.query_all(
                        "SELECT settlement_kind, settlement_id, status, lease_generation
                         FROM settlement_jobs ORDER BY settlement_kind, settlement_id",
                        params![],
                        |row| {
                            Ok((
                                row.get::<i64>(0)?,
                                row.get::<Vec<u8>>(1)?,
                                row.get::<i64>(2)?,
                                u64::from_sql_bytes(row.get::<Vec<u8>>(3)?).map_err(|_| {
                                    DbError::Constraint("invalid test lease generation".into())
                                })?,
                            ))
                        },
                    )
                })
                .expect("settlement jobs"),
        }
    }

    fn seed_queued_evm_operation(store: &mut StableStore) -> (EvmOperationRecord, EvmCallIntent) {
        let operation_id = EvmOperationId::new(77);
        let mut parent = deposit();
        parent
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare deposit mint");
        store
            .put_deposit(&parent)
            .expect("persist operation parent");
        let operation = EvmOperationRecord::queued(
            operation_id,
            parent.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        let intent = evm_intent(operation_id, parent.payload_hash);
        store
            .put_evm_call_intent(&intent)
            .expect("persist execution payload");
        store
            .put_evm_operation(&operation)
            .expect("persist EVM operation");
        (operation, intent)
    }

    #[test]
    #[serial]
    fn configured_store_starts_with_new_deposits_paused() {
        let initial = config();
        let store = StableStore::init_configured(VectorMemory::default(), &initial)
            .expect("initialize configured store");
        assert!(
            store
                .admin_state()
                .expect("read administrator state")
                .deposits_paused
        );
        assert_eq!(store.config().expect("read config"), Some(initial));
    }

    #[test]
    #[serial]
    fn chain_key_addresses_initialize_atomically_and_survive_reopen() {
        let memory = VectorMemory::default();
        let mut store =
            StableStore::init_configured(memory.clone(), &config()).expect("initialize store");
        store
            .initialize_chain_key_addresses([1; 20], [2; 20])
            .expect("initialize chain-key addresses");
        store
            .initialize_chain_key_addresses([1; 20], [2; 20])
            .expect("exact replay is idempotent");
        assert_eq!(
            store.signer_address().expect("signer address"),
            Some([1; 20])
        );
        assert_eq!(
            store
                .governance_operator_address()
                .expect("governance operator address"),
            Some([2; 20])
        );
        drop(store);

        let mut reopened = StableStore::reopen(memory).expect("reopen store");
        assert_eq!(
            reopened.signer_address().expect("reopened signer address"),
            Some([1; 20])
        );
        assert_eq!(
            reopened
                .governance_operator_address()
                .expect("reopened governance operator address"),
            Some([2; 20])
        );
        assert!(matches!(
            reopened.initialize_chain_key_addresses([3; 20], [2; 20]),
            Err(StorageError::Core(CoreError::ConflictingReplay))
        ));
        assert_eq!(
            reopened.signer_address().expect("unchanged signer address"),
            Some([1; 20])
        );
        assert_eq!(
            reopened
                .governance_operator_address()
                .expect("unchanged governance operator address"),
            Some([2; 20])
        );
    }

    #[test]
    #[serial]
    fn governance_nonce_lane_is_stable_and_independent_from_mint_lane() {
        let memory = VectorMemory::default();
        let mut store =
            StableStore::init_configured(memory.clone(), &config()).expect("initialize store");
        let mint_before = store.external_progress().expect("mint progress");
        store
            .initialize_governance_nonce(7)
            .expect("initialize operator nonce");
        let intent = evm_intent(EvmOperationId::new(0), [9; 32]);
        let mut transaction = GovernanceTransaction {
            id: 0,
            kind: GovernanceTransactionKind::PauseDepositMints,
            envelope: intent.assign_nonce(7),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(transaction.clone())
            .expect("prepare operator transaction");
        assert_eq!(
            store.external_progress().expect("mint progress"),
            mint_before
        );
        assert_eq!(
            store.governance_lane().expect("operator lane"),
            (true, 8, 1, Some(transaction.clone()))
        );
        transaction.state = GovernanceTransactionState::Broadcasting {
            transaction_hash: [0x71; 32],
        };
        store
            .update_governance_transaction(transaction.clone())
            .expect("journal broadcasting state");

        drop(store);
        let reopened = StableStore::reopen(memory).expect("reopen current schema");
        assert_eq!(
            reopened.external_progress().expect("mint progress"),
            mint_before
        );
        assert!(reopened.governance_lane().expect("operator lane").0);
        assert_eq!(reopened.governance_lane().expect("operator lane").1, 8);
        assert_eq!(
            reopened.governance_lane().expect("operator lane").3,
            Some(transaction)
        );
    }

    #[test]
    #[serial]
    fn reverted_execute_keeps_the_recorded_timelock_operation_for_retry() {
        let memory = VectorMemory::default();
        let mut store =
            StableStore::init_configured(memory.clone(), &config()).expect("initialize store");
        let operation_id = [0xabu8; 32];
        let salt = [0xcdu8; 32];
        store
            .initialize_governance_nonce(7)
            .expect("initialize operator nonce");

        let mut scheduled = GovernanceTransaction {
            id: 0,
            kind: GovernanceTransactionKind::ScheduleActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(0), [7; 32]).assign_nonce(7),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(scheduled.clone())
            .expect("prepare activation schedule");
        scheduled.state = GovernanceTransactionState::Confirmed {
            transaction_hash: [3; 32],
            receipt_block_number: 5,
        };
        store
            .complete_governance_transaction(scheduled)
            .expect("complete activation schedule");
        drop(store);
        let mut store = StableStore::reopen(memory).expect("reopen scheduled activation");
        assert_eq!(
            store
                .pending_timelock_operation()
                .expect("pending activation"),
            Some(PendingTimelockOperation { operation_id, salt })
        );

        let mut reverted = GovernanceTransaction {
            id: 1,
            kind: GovernanceTransactionKind::ExecuteActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(1), [9; 32]).assign_nonce(8),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(reverted.clone())
            .expect("prepare early execute");
        reverted.state = GovernanceTransactionState::Reverted {
            transaction_hash: [1; 32],
            receipt_block_number: 10,
        };
        store
            .update_governance_transaction(reverted.clone())
            .expect("record early revert");
        store
            .complete_governance_transaction(reverted)
            .expect("complete early revert");
        assert_eq!(
            store
                .pending_timelock_operation()
                .expect("pending activation"),
            Some(PendingTimelockOperation { operation_id, salt })
        );

        let mut confirmed = GovernanceTransaction {
            id: 2,
            kind: GovernanceTransactionKind::ExecuteActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(2), [8; 32]).assign_nonce(9),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(confirmed.clone())
            .expect("prepare mature execute");
        confirmed.state = GovernanceTransactionState::Confirmed {
            transaction_hash: [2; 32],
            receipt_block_number: 20,
        };
        store
            .update_governance_transaction(confirmed.clone())
            .expect("record confirmed execute");
        store
            .complete_governance_transaction(confirmed)
            .expect("complete confirmed execute");
        assert_eq!(
            store
                .pending_timelock_operation()
                .expect("cleared activation"),
            None
        );
    }

    #[test]
    #[serial]
    fn reverted_schedule_releases_activation_and_nonce_conflict_resyncs_monotonically() {
        let mut store =
            StableStore::init_configured(VectorMemory::default(), &config()).expect("store");
        store
            .initialize_governance_nonce(4)
            .expect("initialize operator nonce");
        let operation_id = [0x11; 32];
        let salt = [0x22; 32];
        let mut schedule = GovernanceTransaction {
            id: 0,
            kind: GovernanceTransactionKind::ScheduleActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(0), [5; 32]).assign_nonce(4),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(schedule.clone())
            .expect("prepare schedule");
        schedule.state = GovernanceTransactionState::Reverted {
            transaction_hash: [3; 32],
            receipt_block_number: 9,
        };
        store
            .complete_governance_transaction(schedule)
            .expect("complete reverted schedule");
        assert_eq!(
            store
                .pending_timelock_operation()
                .expect("pending activation"),
            None
        );

        let mut conflict = GovernanceTransaction {
            id: 1,
            kind: GovernanceTransactionKind::PauseWithdrawals,
            envelope: evm_intent(EvmOperationId::new(1), [6; 32]).assign_nonce(5),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(conflict.clone())
            .expect("prepare conflicting transaction");
        conflict.state = GovernanceTransactionState::NonceConflict {
            transaction_hash: [4; 32],
        };
        store
            .update_governance_transaction(conflict.clone())
            .expect("persist conflict");
        assert!(store
            .resolve_governance_nonce_conflict(&conflict, 5)
            .is_err());
        store
            .resolve_governance_nonce_conflict(&conflict, 8)
            .expect("resync advanced nonce");
        assert_eq!(store.governance_lane().expect("lane"), (true, 8, 2, None));
    }

    #[test]
    #[serial]
    fn emergency_aborts_only_unbroadcast_dangerous_transactions_and_reuses_the_nonce() {
        let mut store =
            StableStore::init_configured(VectorMemory::default(), &config()).expect("store");
        store.initialize_governance_nonce(4).expect("nonce");
        let operation_id = [0x41; 32];
        let salt = [0x42; 32];
        let schedule = GovernanceTransaction {
            id: 0,
            kind: GovernanceTransactionKind::ScheduleActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(0), [0x43; 32]).assign_nonce(4),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(schedule.clone())
            .expect("prepare schedule");
        store
            .enqueue_emergency_base_actions()
            .expect("enqueue emergency");
        store
            .abort_unbroadcast_governance_transaction_for_emergency(&schedule)
            .expect("abort unbroadcast schedule");

        assert_eq!(store.governance_lane().expect("lane"), (true, 4, 1, None));
        assert_eq!(store.pending_timelock_operation().expect("timelock"), None);
        assert_eq!(
            store
                .next_emergency_base_action()
                .expect("emergency action"),
            Some(GovernanceTransactionKind::PauseDepositMints)
        );

        let broadcasting = GovernanceTransaction {
            id: 1,
            kind: GovernanceTransactionKind::SetServiceFee { value: 7 },
            envelope: evm_intent(EvmOperationId::new(1), [0x44; 32]).assign_nonce(4),
            state: GovernanceTransactionState::Broadcasting {
                transaction_hash: [0x45; 32],
            },
        };
        store
            .prepare_governance_transaction(broadcasting.clone())
            .expect("prepare fee transaction");
        store
            .update_governance_transaction(broadcasting.clone())
            .expect("record broadcasting");
        assert!(store
            .abort_unbroadcast_governance_transaction_for_emergency(&broadcasting)
            .is_err());
        assert_eq!(
            store.governance_lane().expect("lane").3,
            Some(broadcasting.clone())
        );
        store
            .resolve_governance_nonce_conflict(&broadcasting, 6)
            .expect("resync broadcasting transaction after nonce advancement");
        assert_eq!(store.governance_lane().expect("lane"), (true, 6, 2, None));
    }

    #[test]
    #[serial]
    fn emergency_abort_of_signed_execute_retains_the_scheduled_timelock() {
        let mut store =
            StableStore::init_configured(VectorMemory::default(), &config()).expect("store");
        store.initialize_governance_nonce(10).expect("nonce");
        let operation_id = [0x51; 32];
        let salt = [0x52; 32];
        let mut schedule = GovernanceTransaction {
            id: 0,
            kind: GovernanceTransactionKind::ScheduleActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(0), [0x53; 32]).assign_nonce(10),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(schedule.clone())
            .expect("prepare schedule");
        schedule.state = GovernanceTransactionState::Confirmed {
            transaction_hash: [0x54; 32],
            receipt_block_number: 1,
        };
        store
            .complete_governance_transaction(schedule)
            .expect("confirm schedule");

        let mut execute = GovernanceTransaction {
            id: 1,
            kind: GovernanceTransactionKind::ExecuteActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(1), [0x55; 32]).assign_nonce(11),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(execute.clone())
            .expect("prepare execute");
        execute.state = GovernanceTransactionState::Signed;
        store
            .update_governance_transaction(execute.clone())
            .expect("record signed execute");
        store
            .enqueue_emergency_base_actions()
            .expect("enqueue emergency");
        store
            .abort_unbroadcast_governance_transaction_for_emergency(&execute)
            .expect("abort execute");

        assert_eq!(store.governance_lane().expect("lane"), (true, 11, 2, None));
        assert_eq!(
            store.pending_timelock_operation().expect("timelock"),
            Some(PendingTimelockOperation { operation_id, salt })
        );
        assert!(store.emergency_base_actions_pending().expect("emergency"));
    }

    #[test]
    #[serial]
    fn confirmed_cancel_clears_only_the_matching_timelock_operation() {
        let mut store =
            StableStore::init_configured(VectorMemory::default(), &config()).expect("store");
        store
            .initialize_governance_nonce(10)
            .expect("initialize nonce");
        let operation_id = [0x31; 32];
        let salt = [0x32; 32];
        let mut schedule = GovernanceTransaction {
            id: 0,
            kind: GovernanceTransactionKind::ScheduleActivation { operation_id, salt },
            envelope: evm_intent(EvmOperationId::new(0), [0x33; 32]).assign_nonce(10),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(schedule.clone())
            .expect("prepare schedule");
        schedule.state = GovernanceTransactionState::Confirmed {
            transaction_hash: [0x34; 32],
            receipt_block_number: 1,
        };
        store
            .complete_governance_transaction(schedule)
            .expect("confirm schedule");

        let wrong = GovernanceTransaction {
            id: 1,
            kind: GovernanceTransactionKind::CancelTimelock {
                operation_id: [0x35; 32],
            },
            envelope: evm_intent(EvmOperationId::new(1), [0x36; 32]).assign_nonce(11),
            state: GovernanceTransactionState::Prepared,
        };
        assert!(store.prepare_governance_transaction(wrong).is_err());

        let mut cancel = GovernanceTransaction {
            id: 1,
            kind: GovernanceTransactionKind::CancelTimelock { operation_id },
            envelope: evm_intent(EvmOperationId::new(1), [0x37; 32]).assign_nonce(11),
            state: GovernanceTransactionState::Prepared,
        };
        store
            .prepare_governance_transaction(cancel.clone())
            .expect("prepare matching cancel");
        cancel.state = GovernanceTransactionState::Confirmed {
            transaction_hash: [0x38; 32],
            receipt_block_number: 2,
        };
        store
            .complete_governance_transaction(cancel)
            .expect("confirm cancel");
        assert_eq!(
            store
                .pending_timelock_operation()
                .expect("pending activation"),
            None
        );
    }

    #[test]
    #[serial]
    fn deposit_owner_index_is_newest_first_paginated_and_owner_scoped() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize stable store");
        let owner = Principal::self_authenticating([1; 32]);
        let other = Principal::self_authenticating([2; 32]);

        for (tag, principal) in [(1u8, owner), (2, other), (3, owner), (4, owner)] {
            let mut record = deposit_for(principal);
            record.id = DepositId::new([tag; 32]);
            record.payload_hash = [2; 32];
            let mut deposit_intent = intent([tag; 32], principal);
            deposit_intent.owner_sequence = store
                .next_deposit_sequence(principal)
                .expect("read owner sequence");
            store
                .admit_deposit(principal, &deposit_intent, &record, None, None)
                .expect("admit deposit");
        }

        let first = store
            .list_deposit_ids(owner, None, 2)
            .expect("list first page");
        assert_eq!(first.deposit_ids, vec![[4; 32], [3; 32]]);
        assert_eq!(first.next_cursor, Some(2));

        let second = store
            .list_deposit_ids(owner, first.next_cursor, 2)
            .expect("list second page");
        assert_eq!(second.deposit_ids, vec![[1; 32]]);
        assert_eq!(second.next_cursor, None);
        assert_eq!(
            store
                .list_deposit_ids(other, None, 100)
                .expect("list other owner")
                .deposit_ids,
            vec![[2; 32]]
        );
    }

    #[test]
    #[serial]
    fn deposit_owner_index_retains_only_latest_hundred_without_deleting_records() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize stable store");
        let owner = Principal::self_authenticating([11; 32]);
        let other = Principal::self_authenticating([12; 32]);

        for tag in 0u8..=100 {
            let mut record = deposit_for(owner);
            record.id = DepositId::new([tag; 32]);
            let mut deposit_intent = intent([tag; 32], owner);
            deposit_intent.owner_sequence = store
                .next_deposit_sequence(owner)
                .expect("read owner sequence");
            store
                .admit_deposit(owner, &deposit_intent, &record, None, None)
                .expect("admit deposit");
        }
        let mut other_record = deposit_for(other);
        other_record.id = DepositId::new([200; 32]);
        let other_intent = intent([200; 32], other);
        store
            .admit_deposit(other, &other_intent, &other_record, None, None)
            .expect("admit other owner deposit");

        let page = store
            .list_deposit_ids(owner, None, 100)
            .expect("list retained history");
        assert_eq!(page.deposit_ids.len(), 100);
        assert_eq!(page.deposit_ids.first(), Some(&[100; 32]));
        assert_eq!(page.deposit_ids.last(), Some(&[1; 32]));
        assert_eq!(page.oldest_available_cursor, Some(1));
        assert!(page.history_truncated);
        assert!(store.deposit([0; 32]).expect("read old deposit").is_some());
        assert!(store
            .deposit_intent([0; 32])
            .expect("read old intent")
            .is_some());
        assert_eq!(
            store
                .list_deposit_ids(other, None, 100)
                .expect("list other owner")
                .deposit_ids,
            vec![[200; 32]]
        );
    }

    #[test]
    #[serial]
    fn audit_retention_prunes_one_event_and_commits_the_wire_blob() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize stable store");
        let caller = Principal::self_authenticating([13; 32]);
        let first = AuditEvent {
            sequence: 0,
            timestamp_ns: 1_000,
            caller,
            kind: AuditEventKind::DepositsPaused,
        };
        let first_blob = encode(&first).expect("encode first event");

        for sequence in 0..=MAX_AUDIT_EVENTS {
            store
                .append_audit_event_at(caller, AuditEventKind::DepositsPaused, 1_000 + sequence)
                .expect("append audit event");
        }

        let page = store.audit_events(0, 100).expect("read audit page");
        assert_eq!(page.events.len(), 100);
        assert_eq!(page.events.first().map(|event| event.sequence), Some(1));
        assert_eq!(page.next_sequence, Some(101));
        assert_eq!(page.oldest_available_sequence, 1);
        assert_eq!(page.pruned_count, 1);
        assert_eq!(page.pruned_through_sequence, Some(0));
        assert_eq!(store.audit_events.len(), MAX_AUDIT_EVENTS);

        let mut expected = Sha256::new();
        expected.update(AUDIT_DIGEST_DOMAIN);
        expected.update([0; 32]);
        expected.update(0u64.to_be_bytes());
        expected.update((first_blob.as_slice().len() as u64).to_be_bytes());
        expected.update(first_blob.as_slice());
        assert_eq!(page.pruned_digest, expected.finalize().to_vec());
    }

    #[test]
    #[serial]
    fn evm_rpc_audit_observation_is_validated_and_idempotent() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize");
        let caller = Principal::self_authenticating([71; 32]);
        let kind = AuditEventKind::EvmRpcObservation {
            evm_rpc_canister_id: Principal::self_authenticating([72; 32]),
            call_method: "request".into(),
            request_digest: vec![73; 32],
            quorum_response_digest: vec![74; 32],
            finalized_block_number: 75,
            finalized_block_hash: vec![76; 32],
            transaction_hash: Some(vec![77; 32]),
        };
        assert!(store
            .append_evm_rpc_observation_once_at(caller, kind.clone(), 1_000)
            .expect("append evidence"));
        assert!(!store
            .append_evm_rpc_observation_once_at(caller, kind, 1_001)
            .expect("deduplicate evidence"));
        assert_eq!(store.audit_events.len(), 1);
        assert_eq!(store.counters().expect("counters").next_audit_sequence, 1);

        let malformed = AuditEventKind::EvmRpcObservation {
            evm_rpc_canister_id: Principal::self_authenticating([72; 32]),
            call_method: "request".into(),
            request_digest: vec![73; 31],
            quorum_response_digest: vec![74; 32],
            finalized_block_number: 75,
            finalized_block_hash: vec![76; 32],
            transaction_hash: None,
        };
        assert_eq!(
            store.append_evm_rpc_observation_once_at(caller, malformed, 1_002),
            Err(StorageError::DecodeFailed)
        );
        assert_eq!(store.audit_events.len(), 1);
        drop(store);
        let reopened = StableStore::reopen(memory).expect("reopen audit evidence");
        assert_eq!(reopened.audit_events.len(), 1);
        let page = reopened.audit_events(0, 10).expect("read audit evidence");
        assert!(matches!(
            page.events.first().map(|event| &event.kind),
            Some(AuditEventKind::EvmRpcObservation { .. })
        ));
    }

    #[test]
    #[serial]
    fn every_rpc_atomic_path_rolls_back_business_audit_cache_and_reopen() {
        let failpoints = [
            RpcAtomicFailpoint::Business,
            RpcAtomicFailpoint::Audit,
            RpcAtomicFailpoint::Singleton,
        ];
        let caller = Principal::self_authenticating([80; 32]);

        for failpoint in failpoints {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("release init");
            let withdrawal = withdrawal();
            let before = rpc_atomic_snapshot(&store, None);
            set_rpc_atomic_failpoint(Some(failpoint));
            assert!(store
                .commit_new_withdrawal_release_bundle_with_rpc_audit(
                    &withdrawal,
                    &ExternalProgress::default(),
                    caller,
                    100,
                    vec![rpc_audit_kind(80)],
                    [80; 32],
                )
                .is_err());
            set_rpc_atomic_failpoint(None);
            assert_eq!(
                rpc_atomic_snapshot(&store, None),
                before,
                "release {failpoint:?}"
            );
            drop(store);
            assert_eq!(
                rpc_atomic_snapshot(&StableStore::reopen(memory).expect("release reopen"), None),
                before,
                "release reopen {failpoint:?}"
            );
        }

        for failpoint in failpoints {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("submitted init");
            let operation_id = EvmOperationId::new(83);
            let mut parent = deposit();
            parent
                .apply(DepositEvent::CommitQuote {
                    quote: test_deposit_quote(),
                    operation_id,
                })
                .expect("mint parent");
            store.put_deposit(&parent).expect("parent");
            let mut operation = EvmOperationRecord::queued(
                operation_id,
                parent.payload_hash,
                EvmOperationKind::MintDeposit,
            );
            let intent = evm_intent(operation_id, parent.payload_hash);
            store.put_evm_call_intent(&intent).expect("intent");
            store.put_evm_operation(&operation).expect("queued");
            let envelope = intent.assign_nonce(0);
            operation
                .apply(EvmOperationEvent::Prepared)
                .expect("prepared");
            let progress = ExternalProgress {
                next_evm_nonce: 1,
                ..ExternalProgress::default()
            };
            store
                .prepare_evm_operation(&operation, &envelope, &progress)
                .expect("persist prepared");
            operation
                .apply(EvmOperationEvent::Submitted {
                    transaction_hash: [84; 32],
                })
                .expect("submitted");
            let before = rpc_atomic_snapshot(&store, Some(operation_id.get()));
            set_rpc_atomic_failpoint(Some(failpoint));
            assert!(store
                .put_submitted_evm_operation_with_rpc_audit(
                    &operation,
                    102,
                    caller,
                    vec![rpc_audit_kind(82)],
                )
                .is_err());
            set_rpc_atomic_failpoint(None);
            assert_eq!(
                rpc_atomic_snapshot(&store, Some(operation_id.get())),
                before,
                "submitted {failpoint:?}"
            );
            drop(store);
            assert_eq!(
                rpc_atomic_snapshot(
                    &StableStore::reopen(memory).expect("submitted reopen"),
                    Some(operation_id.get()),
                ),
                before,
                "submitted reopen {failpoint:?}"
            );
        }

        for failpoint in failpoints {
            let memory = VectorMemory::default();
            let mut store =
                StableStore::init_configured(memory.clone(), &config()).expect("pause init");
            let mut admin = store.admin_state().expect("admin");
            admin.deposits_paused = false;
            store.set_admin_state(&admin).expect("unpause fixture");
            let before = rpc_atomic_snapshot(&store, None);
            set_rpc_atomic_failpoint(Some(failpoint));
            assert!(store
                .pause_deposits_with_rpc_audit(caller, 104, vec![rpc_audit_kind(83)])
                .is_err());
            set_rpc_atomic_failpoint(None);
            assert_eq!(
                rpc_atomic_snapshot(&store, None),
                before,
                "pause {failpoint:?}"
            );
            drop(store);
            assert_eq!(
                rpc_atomic_snapshot(&StableStore::reopen(memory).expect("pause reopen"), None),
                before,
                "pause reopen {failpoint:?}"
            );
        }

        for (success, tag) in [(true, 84u8), (false, 85u8)] {
            for failpoint in failpoints {
                let memory = VectorMemory::default();
                let mut store = StableStore::init(memory.clone()).expect("snapshot init");
                let refresh_owner = store
                    .begin_base_snapshot_refresh(105, 1_000, 1)
                    .expect("begin refresh")
                    .expect("refresh owner");
                let before = rpc_atomic_snapshot(&store, None);
                set_rpc_atomic_failpoint(Some(failpoint));
                let result = if success {
                    store.finish_base_snapshot_refresh_with_rpc_audit_and_observation(
                        refresh_owner,
                        106,
                        mint_snapshot(),
                        [86; 20],
                        false,
                        Some(FinalizedObservationRecord {
                            chain_id: 8453,
                            block_number: 86,
                            block_hash: [87; 32],
                            observed_at_ns: 106,
                            bridge_signer: [86; 20],
                            runtime_sha256: [88; 32],
                        }),
                        caller,
                        vec![rpc_audit_kind(tag)],
                    )
                } else {
                    store.fail_base_snapshot_refresh_with_rpc_audit(
                        refresh_owner,
                        caller,
                        106,
                        vec![rpc_audit_kind(tag)],
                    )
                };
                assert!(result.is_err());
                set_rpc_atomic_failpoint(None);
                assert_eq!(
                    rpc_atomic_snapshot(&store, None),
                    before,
                    "snapshot success={success} {failpoint:?}"
                );
                drop(store);
                assert_eq!(
                    rpc_atomic_snapshot(
                        &StableStore::reopen(memory).expect("snapshot reopen"),
                        None,
                    ),
                    before,
                    "snapshot reopen success={success} {failpoint:?}"
                );
            }
        }

        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("successful snapshot init");
        let refresh_owner = store
            .begin_base_snapshot_refresh(105, 1_000, 1)
            .expect("begin successful refresh")
            .expect("refresh owner");
        let observation = FinalizedObservationRecord {
            chain_id: 8453,
            block_number: 86,
            block_hash: [87; 32],
            observed_at_ns: 106,
            bridge_signer: [86; 20],
            runtime_sha256: [88; 32],
        };
        store
            .finish_base_snapshot_refresh_with_rpc_audit_and_observation(
                refresh_owner,
                106,
                mint_snapshot(),
                [86; 20],
                false,
                Some(observation),
                caller,
                vec![rpc_audit_kind(89)],
            )
            .expect("persist successful snapshot");
        assert_eq!(
            store
                .external_progress()
                .expect("progress")
                .finalized_observation,
            Some(observation)
        );
        drop(store);
        assert_eq!(
            StableStore::reopen(memory)
                .expect("successful snapshot reopen")
                .external_progress()
                .expect("reopened progress")
                .finalized_observation,
            Some(observation)
        );
    }

    #[test]
    #[serial]
    fn deposit_admission_returns_existing_without_duplicate_index_entry() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize stable store");
        let owner = Principal::self_authenticating([3; 32]);
        let record = deposit_for(owner);
        let intent = intent(record.id.bytes(), owner);
        store
            .admit_deposit(owner, &intent, &record, None, None)
            .expect("first admission");
        assert_eq!(
            store
                .admit_deposit(owner, &intent, &record, None, None)
                .expect("idempotent admission"),
            DepositAdmissionOutcome::Existing
        );
        assert_eq!(
            store
                .list_deposit_ids(owner, None, 100)
                .expect("list owner deposits")
                .deposit_ids,
            vec![record.id.bytes()]
        );
    }

    #[test]
    #[serial]
    fn deposit_quota_is_committed_only_with_a_new_record() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init_configured(memory, &config()).expect("initialize");
        let make = |owner: Principal, id: [u8; 32], payload_hash: [u8; 32]| {
            let mut transfer = transfer(LedgerOperation::PullDeposit, 110, id[0]);
            transfer.from =
                Account::new(owner.as_slice().to_vec(), [0; 32]).expect("deposit owner");
            let record = DepositRecord::accept(DepositRequest {
                id: DepositId::new(id),
                payload_hash,
                gross_amount: Amount::new(110),
                user_max_service_fee: Amount::new(10),
                transfer,
            })
            .expect("deposit record");
            let intent = DepositIntent {
                deposit_id: id,
                caller: owner.as_slice().to_vec(),
                owner_sequence: 0,
                base_recipient: [9; 20],
                from_subaccount: [0; 32],
                payload_hash,
            };
            (record, intent)
        };
        let owner = Principal::self_authenticating([31; 32]);
        let other = Principal::self_authenticating([32; 32]);
        let (record, intent) = make(owner, [31; 32], [41; 32]);
        let quota = DepositQuotaAdmission {
            now_ns: 1,
            window_seconds: 60,
            global_limit: 1,
            per_principal_limit: 1,
        };
        let before = store.deposit_admission.get().expect("quota before");
        assert_eq!(
            store.admit_deposit(owner, &intent, &record, None, Some(quota)),
            Err(StorageError::DepositsPaused)
        );
        assert_eq!(store.deposit_admission.get().expect("paused quota"), before);

        let mut admin = store.admin_state().expect("admin");
        admin.deposits_paused = false;
        store.set_admin_state(&admin).expect("resume deposits");
        assert_eq!(
            store
                .admit_deposit(owner, &intent, &record, None, Some(quota))
                .expect("admit deposit"),
            DepositAdmissionOutcome::Inserted
        );
        let consumed = store.deposit_admission.get().expect("consumed quota");
        assert_eq!(
            store
                .admit_deposit(owner, &intent, &record, None, Some(quota))
                .expect("idempotent retry"),
            DepositAdmissionOutcome::Existing
        );
        assert_eq!(
            store.deposit_admission.get().expect("retry quota"),
            consumed
        );

        let (other_record, other_intent) = make(other, [32; 32], [42; 32]);
        assert_eq!(
            store.admit_deposit(other, &other_intent, &other_record, None, Some(quota)),
            Err(StorageError::DepositRateLimited {
                retry_after_seconds: 60
            })
        );
        assert_eq!(
            store.deposit_admission.get().expect("limited quota"),
            consumed
        );
        assert_eq!(
            store
                .deposit(other_record.id.bytes())
                .expect("limited deposit"),
            None
        );
    }

    #[test]
    #[serial]
    fn snapshot_refresh_rejects_stale_and_conflicting_finalized_observations_atomically() {
        let caller = Principal::self_authenticating([90; 32]);
        let current = FinalizedObservationRecord {
            chain_id: 8453,
            block_number: 100,
            block_hash: [100; 32],
            observed_at_ns: 1_000,
            bridge_signer: [7; 20],
            runtime_sha256: [8; 32],
        };
        for (candidate, expected) in [
            (
                FinalizedObservationRecord {
                    block_number: 99,
                    block_hash: [99; 32],
                    observed_at_ns: 1_100,
                    ..current
                },
                CoreError::StaleFinalizedObservation,
            ),
            (
                FinalizedObservationRecord {
                    block_hash: [9; 32],
                    observed_at_ns: 1_100,
                    ..current
                },
                CoreError::ConflictingFinalizedObservation,
            ),
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize snapshot store");
            let initial_owner = store
                .begin_base_snapshot_refresh(900, 1_000, 1)
                .expect("begin initial refresh")
                .expect("initial owner");
            store
                .finish_base_snapshot_refresh_with_rpc_audit_and_observation(
                    initial_owner,
                    1_000,
                    mint_snapshot(),
                    current.bridge_signer,
                    false,
                    Some(current),
                    caller,
                    vec![rpc_audit_kind(90)],
                )
                .expect("persist current observation");
            let rejected_owner = store
                .begin_base_snapshot_refresh(1_050, 1_000, 1)
                .expect("begin rejected refresh")
                .expect("rejected owner");
            let before = rpc_atomic_snapshot(&store, None);

            assert_eq!(
                store.finish_base_snapshot_refresh_with_rpc_audit_and_observation(
                    rejected_owner,
                    1_100,
                    mint_snapshot(),
                    candidate.bridge_signer,
                    false,
                    Some(candidate),
                    caller,
                    vec![rpc_audit_kind(91)],
                ),
                Err(StorageError::Core(expected))
            );
            assert_eq!(rpc_atomic_snapshot(&store, None), before);
            drop(store);
            assert_eq!(
                rpc_atomic_snapshot(
                    &StableStore::reopen(memory).expect("reopen rejected snapshot"),
                    None,
                ),
                before
            );
        }
    }

    #[test]
    #[serial]
    fn observed_withdrawal_cannot_be_persisted_without_a_release_job() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize store");
        let record = WithdrawalRecord::observed(
            WithdrawalId::new([92; 32]),
            [1; 20],
            vec![1],
            [0; 32],
            [2; 32],
            Amount::new(100),
            Amount::new(10),
            Amount::new(10),
            Amount::new(90),
            100,
        )
        .expect("observed withdrawal");

        assert_eq!(
            store.commit_new_withdrawal_release_bundle(&record, &ExternalProgress::default()),
            Err(StorageError::Core(CoreError::PayloadConflict))
        );
        assert_eq!(
            store.withdrawal(record.id.bytes()).expect("withdrawal"),
            None
        );
        assert_eq!(
            store
                .settlement_job(SettlementJobKind::Withdrawal, record.id.bytes())
                .expect("settlement job"),
            None
        );
    }

    #[test]
    #[serial]
    fn fee_guard_observed_withdrawal_persists_without_release_index_or_job() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize store");
        let mut record = WithdrawalRecord::observed(
            WithdrawalId::new([93; 32]),
            [1; 20],
            vec![1],
            [0; 32],
            [2; 32],
            Amount::new(100),
            Amount::new(10),
            Amount::new(10),
            Amount::new(90),
            100,
        )
        .expect("observed withdrawal");
        record.last_settlement_stop_reason = Some("LedgerFeeExceedsServiceFee".into());

        store
            .put_withdrawal(&record)
            .expect("persist fee guard record");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .pending_ledger_operations,
            0
        );
        assert!(store
            .settlement_job(SettlementJobKind::Withdrawal, record.id.bytes())
            .expect("settlement job")
            .is_none());
        drop(store);
        let reopened = StableStore::reopen(memory).expect("reopen fee guard record");
        assert_eq!(
            reopened.withdrawal(record.id.bytes()).expect("withdrawal"),
            Some(record)
        );
    }

    #[test]
    #[serial]
    fn fee_guard_bundles_roll_back_business_guard_audit_and_jobs() {
        let failpoints = [
            RpcAtomicFailpoint::Business,
            RpcAtomicFailpoint::Audit,
            RpcAtomicFailpoint::Singleton,
        ];
        let caller = Principal::self_authenticating([94; 32]);

        for failpoint in failpoints {
            let memory = VectorMemory::default();
            let mut store =
                StableStore::init_configured(memory.clone(), &config()).expect("trip init");
            let record = fee_guard_withdrawal(94);
            let admin = fee_guard_admin(&store, 11, 100);
            let before = rpc_atomic_snapshot(&store, None);
            set_rpc_atomic_failpoint(Some(failpoint));
            assert!(store
                .commit_withdrawal_fee_guard_trip_bundle(
                    &record,
                    &ExternalProgress::default(),
                    &admin,
                    caller,
                    100,
                    vec![AuditEventKind::WithdrawalFeeGuardTripped {
                        ledger_fee: 11,
                        charged_service_fee: 10,
                    }],
                    [94; 32],
                )
                .is_err());
            set_rpc_atomic_failpoint(None);
            assert_eq!(
                rpc_atomic_snapshot(&store, None),
                before,
                "trip {failpoint:?}"
            );
            drop(store);
            assert_eq!(
                rpc_atomic_snapshot(&StableStore::reopen(memory).expect("trip reopen"), None),
                before,
                "trip reopen {failpoint:?}"
            );
        }

        for (clear, tag) in [(false, 95u8), (true, 96u8)] {
            for failpoint in failpoints {
                let memory = VectorMemory::default();
                let mut store =
                    StableStore::init_configured(memory.clone(), &config()).expect("update init");
                let record = fee_guard_withdrawal(tag);
                let initial_admin = fee_guard_admin(&store, 11, 100);
                store
                    .commit_withdrawal_fee_guard_trip_bundle(
                        &record,
                        &ExternalProgress::default(),
                        &initial_admin,
                        caller,
                        100,
                        vec![AuditEventKind::WithdrawalFeeGuardTripped {
                            ledger_fee: 11,
                            charged_service_fee: 10,
                        }],
                        [tag; 32],
                    )
                    .expect("trip fixture");
                let before = rpc_atomic_snapshot(&store, None);
                set_rpc_atomic_failpoint(Some(failpoint));
                let result = if clear {
                    let next = release_guarded_withdrawal(record.clone());
                    let mut admin = initial_admin.clone();
                    admin.withdrawal_fee_guard = None;
                    store.commit_withdrawal_fee_guard_clear_bundle(
                        &next,
                        &admin,
                        caller,
                        200,
                        vec![AuditEventKind::WithdrawalFeeGuardCleared],
                    )
                } else {
                    let admin = fee_guard_admin(&store, 12, 200);
                    store.commit_withdrawal_fee_guard_continue_bundle(
                        &record,
                        &admin,
                        caller,
                        200,
                        vec![AuditEventKind::WithdrawalFeeGuardTripped {
                            ledger_fee: 12,
                            charged_service_fee: 10,
                        }],
                    )
                };
                assert!(result.is_err());
                set_rpc_atomic_failpoint(None);
                assert_eq!(
                    rpc_atomic_snapshot(&store, None),
                    before,
                    "clear={clear} {failpoint:?}"
                );
                drop(store);
                assert_eq!(
                    rpc_atomic_snapshot(&StableStore::reopen(memory).expect("update reopen"), None),
                    before,
                    "clear={clear} reopen {failpoint:?}"
                );
            }
        }
    }

    #[test]
    #[serial]
    fn fee_guard_clear_atomically_resumes_the_same_withdrawal_once() {
        let memory = VectorMemory::default();
        let mut store =
            StableStore::init_configured(memory.clone(), &config()).expect("initialize");
        let caller = Principal::self_authenticating([97; 32]);
        let record = fee_guard_withdrawal(97);
        let admin = fee_guard_admin(&store, 11, 100);
        let accounting_before = store.accounting().expect("accounting before");
        store
            .commit_withdrawal_fee_guard_trip_bundle(
                &record,
                &ExternalProgress::default(),
                &admin,
                caller,
                100,
                vec![AuditEventKind::WithdrawalFeeGuardTripped {
                    ledger_fee: 11,
                    charged_service_fee: 10,
                }],
                [97; 32],
            )
            .expect("trip fee guard");
        assert!(store
            .settlement_job(SettlementJobKind::Withdrawal, record.id.bytes())
            .expect("guard job")
            .is_none());
        assert!(store
            .release_pending_withdrawal_index
            .get(&record.id.bytes())
            .is_none());

        store
            .commit_withdrawal_fee_guard_continue_bundle(&record, &admin, caller, 150, vec![])
            .expect("unchanged guard");
        assert_eq!(store.audit_events.len(), 1);

        let changed_admin = fee_guard_admin(&store, 12, 200);
        store
            .commit_withdrawal_fee_guard_continue_bundle(
                &record,
                &changed_admin,
                caller,
                200,
                vec![AuditEventKind::WithdrawalFeeGuardTripped {
                    ledger_fee: 12,
                    charged_service_fee: 10,
                }],
            )
            .expect("changed guard");
        assert_eq!(store.audit_events.len(), 2);

        let next = release_guarded_withdrawal(record.clone());
        let mut cleared_admin = changed_admin;
        cleared_admin.withdrawal_fee_guard = None;
        store
            .commit_withdrawal_fee_guard_clear_bundle(
                &next,
                &cleared_admin,
                caller,
                300,
                vec![AuditEventKind::WithdrawalFeeGuardCleared],
            )
            .expect("clear fee guard");
        assert_eq!(
            store.withdrawal(record.id.bytes()).expect("withdrawal"),
            Some(next)
        );
        assert!(store
            .admin_state()
            .expect("admin")
            .withdrawal_fee_guard
            .is_none());
        assert!(store
            .release_pending_withdrawal_index
            .get(&record.id.bytes())
            .is_some());
        assert!(store
            .settlement_job(SettlementJobKind::Withdrawal, record.id.bytes())
            .expect("release job")
            .is_some());
        let counters = store.counters().expect("counters");
        assert_eq!(counters.pending_ledger_operations, 1);
        assert_eq!(store.nonterminal_withdrawal_count().expect("count"), 1);
        assert_eq!(store.audit_events.len(), 3);
        assert_eq!(
            store.accounting().expect("accounting after"),
            accounting_before
        );

        let snapshot = rpc_atomic_snapshot(&store, None);
        drop(store);
        assert_eq!(
            rpc_atomic_snapshot(&StableStore::reopen(memory).expect("reopen"), None),
            snapshot
        );
    }

    #[test]
    #[serial]
    fn deposit_admission_constraint_failure_rolls_back_records_indexes_and_counters() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize rollback fixture");
        let owner = Principal::self_authenticating([31; 32]);
        let record = deposit_for(owner);
        let intent = intent(record.id.bytes(), owner);
        let conflicting_index = deposit_owner_index_key(owner, 0).expect("index key");
        store
            .deposit_owner_index
            .insert(conflicting_index, [99; 32]);
        let before = store.counters().expect("counters before");

        assert_eq!(
            store.admit_deposit(owner, &intent, &record, None, None),
            Err(StorageError::DatabaseFailure)
        );
        assert_eq!(store.deposit(record.id.bytes()).expect("deposit"), None);
        assert_eq!(
            store.deposit_intent(record.id.bytes()).expect("intent"),
            None
        );
        assert_eq!(store.counters().expect("counters after"), before);
        assert_eq!(
            store
                .owner_deposit_sequences
                .get(&owner_sequence_key(owner).expect("owner key")),
            None
        );
    }

    #[test]
    #[serial]
    fn deposit_admission_rejects_balance_observed_before_counter_interrupt() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize race fixture");
        let owner = Principal::self_authenticating([32; 32]);
        let record = deposit_for(owner);
        let intent = intent(record.id.bytes(), owner);

        // Model one nonterminal withdrawal immediately before the final ETH balance await.
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "INSERT INTO withdrawal_liability_index(key, value) VALUES(?1, ?2)",
                    params![vec![0; 40], vec![0; 32]],
                )?;
                increment_table_count(connection, "withdrawal_liability_index")
            })
            .expect("seed withdrawal liability");
        let before_observation = store.counters().expect("counters before observation");
        store
            .set_counters(&before_observation)
            .expect("seed withdrawal reservation");
        let progress_before = store
            .external_progress()
            .expect("progress before observation");

        // The ETH response was obtained, but a competing finalization message ran while the
        // caller was suspended and released the withdrawal reservation. The old implementation
        // could combine that pre-finalization ETH balance with this newer, smaller counter.
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "DELETE FROM withdrawal_liability_index WHERE key = ?1",
                    params![vec![0; 40]],
                )?;
                decrement_table_count(connection, "withdrawal_liability_index")
            })
            .expect("model competing finalization");

        let result = store.admit_deposit(
            owner,
            &intent,
            &record,
            Some(DepositReserveAdmission {
                audit_caller: owner,
                expected_token: DepositReserveToken {
                    nonterminal_withdrawals: 1,
                    reserved_deposit_mint_amount: before_observation.reserved_deposit_mint_amount,
                    reserved_deposit_mint_operations: before_observation
                        .reserved_deposit_mint_operations,
                    observation_generation: progress_before.reserve_observation_generation,
                },
                observed_at_ns: 10,
                eth_balance_wei: 20_000_000,
                cycles_balance: 20_000_000,
                reserve_policy: config().reserve_policy(),
                mint_snapshot: mint_snapshot(),
                snapshot_generation: 0,
            }),
            None,
        );

        assert_eq!(result, Err(StorageError::StaleReserveObservation));
        assert_eq!(
            store.counters().expect("counters after rejection"),
            before_observation
        );
        assert_eq!(
            store.external_progress().expect("progress after rejection"),
            progress_before
        );
        assert_eq!(store.deposit(record.id.bytes()).expect("deposit"), None);
        assert_eq!(
            store.deposit_intent(record.id.bytes()).expect("intent"),
            None
        );
    }

    #[test]
    #[serial]
    fn deposit_admission_does_not_reserve_mint_resources_before_quote() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize admission fixture");
        let owner = Principal::self_authenticating([33; 32]);
        let record = deposit_for(owner);
        let intent = intent(record.id.bytes(), owner);
        store
            .admit_deposit(owner, &intent, &record, None, None)
            .expect("admit unquoted deposit");

        let progress = store.external_progress().expect("progress after admission");
        assert_eq!(progress.reserve_observation_generation, 0);
        assert_eq!(
            store
                .counters()
                .expect("counters after admission")
                .reserved_deposit_mint_operations,
            0
        );
        assert!(store
            .audit_events(0, 10)
            .expect("audit events")
            .events
            .is_empty());

        drop(store);
        let reopened = StableStore::reopen(memory).expect("reopen admission fixture");
        assert_eq!(
            reopened.external_progress().expect("reopened progress"),
            progress
        );
        assert_eq!(
            reopened
                .counters()
                .expect("reopened counters")
                .reserved_deposit_mint_operations,
            0
        );
        assert!(reopened
            .deposit(record.id.bytes())
            .expect("reopened deposit")
            .is_some());
    }

    #[test]
    #[serial]
    fn reserve_observation_is_atomic_audited_and_reopenable() {
        let memory = VectorMemory::default();
        let caller = Principal::self_authenticating([34; 32]);
        let mut store = StableStore::init(memory.clone()).expect("initialize reserve fixture");

        store
            .record_reserve_observation(50, 100, caller)
            .expect("record reserve observation");
        let first = store.external_progress().expect("first reserve progress");
        assert_eq!(first.last_eth_balance_wei, 50);
        assert!(first.reserve_sufficient);
        assert_eq!(first.reserve_observation_generation, 1);
        assert_eq!(first.last_reserve_observation_ns, 100);
        assert!(matches!(
            store
                .audit_events(0, 10)
                .expect("reserve audit")
                .events
                .as_slice(),
            [AuditEvent {
                caller: event_caller,
                kind: AuditEventKind::ReserveGateChanged { sufficient: true },
                ..
            }] if *event_caller == caller
        ));

        assert_eq!(
            store.record_reserve_observation(60, 99, caller),
            Err(StorageError::StaleReserveObservation)
        );
        assert_eq!(
            store.external_progress().expect("progress after stale"),
            first
        );
        store
            .record_reserve_observation(60, 101, caller)
            .expect("refresh sufficient reserve");
        assert_eq!(
            store
                .audit_events(0, 10)
                .expect("deduplicated reserve audit")
                .events
                .len(),
            1
        );

        drop(store);
        let reopened = StableStore::reopen(memory).expect("reopen reserve fixture");
        let progress = reopened.external_progress().expect("reopened reserve");
        assert_eq!(progress.last_eth_balance_wei, 60);
        assert_eq!(progress.reserve_observation_generation, 2);
        assert_eq!(progress.last_reserve_observation_ns, 101);
    }

    #[test]
    #[serial]
    fn owner_sequence_advances_only_on_admission_and_survives_reopen() {
        let memory = VectorMemory::default();
        let owner = Principal::self_authenticating([21; 32]);
        let mut store = StableStore::init(memory.clone()).expect("initialize stable store");
        assert_eq!(
            store
                .next_deposit_sequence(owner)
                .expect("initial sequence"),
            0
        );

        let mut record = deposit_for(owner);
        record.id = DepositId::new([21; 32]);
        let mut gap = intent(record.id.bytes(), owner);
        gap.owner_sequence = 1;
        assert!(matches!(
            store.admit_deposit(owner, &gap, &record, None, None),
            Err(StorageError::SequenceMismatch { expected: 0 })
        ));
        assert_eq!(
            store
                .next_deposit_sequence(owner)
                .expect("unchanged sequence"),
            0
        );

        let accepted = intent(record.id.bytes(), owner);
        store
            .admit_deposit(owner, &accepted, &record, None, None)
            .expect("accept sequence zero");
        record.last_settlement_stop_reason = Some("metadata envelope regression".to_owned());
        store.put_deposit(&record).expect("update deposit record");
        drop(store);

        let reopened = StableStore::reopen(memory).expect("reopen stable store");
        assert_eq!(
            reopened
                .next_deposit_sequence(owner)
                .expect("reopened sequence"),
            1
        );
        assert_eq!(
            reopened
                .deposit_intent(record.id.bytes())
                .expect("reopened deposit metadata"),
            Some(accepted)
        );
        assert_eq!(
            reopened
                .deposit(record.id.bytes())
                .expect("reopened deposit"),
            Some(record)
        );
    }

    fn held_deposit() -> (DepositRecord, ReconciliationHoldRecord) {
        let mut deposit = DepositRecord::accept(DepositRequest {
            id: DepositId::new([11; 32]),
            payload_hash: [12; 32],
            gross_amount: Amount::new(110),
            user_max_service_fee: Amount::new(10),
            transfer: transfer(LedgerOperation::PullDeposit, 110, 40),
        })
        .expect("valid deposit");
        let hold_id = HoldId::new(12);
        deposit
            .apply(DepositEvent::FundingAmbiguous { hold_id })
            .expect("hold deposit");
        let hold = ReconciliationHoldRecord::open(
            hold_id,
            RequestReference::DepositFunding(deposit.id),
            deposit.transfer.clone(),
        );
        (deposit, hold)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct HoldBundleSnapshot {
        counters: CounterState,
        deposit: Option<DepositRecord>,
        withdrawal: Option<WithdrawalRecord>,
        hold: Option<ReconciliationHoldRecord>,
        pull_index: bool,
        release_index: bool,
        open_index: bool,
        scan: Option<ReconciliationScanProgress>,
        scan_table_count: u64,
        counts: StorageCounts,
    }

    fn hold_bundle_snapshot(
        store: &StableStore,
        deposit_id: Option<DepositId>,
        withdrawal_id: Option<WithdrawalId>,
        hold_id: HoldId,
    ) -> HoldBundleSnapshot {
        HoldBundleSnapshot {
            counters: store.counters().expect("counters"),
            deposit: deposit_id.and_then(|id| store.deposit(id.bytes()).expect("deposit")),
            withdrawal: withdrawal_id
                .and_then(|id| store.withdrawal(id.bytes()).expect("withdrawal")),
            hold: store.reconciliation_hold(hold_id.get()).expect("hold"),
            pull_index: deposit_id
                .is_some_and(|id| store.pull_pending_deposit_index.get(&id.bytes()).is_some()),
            release_index: withdrawal_id.is_some_and(|id| {
                store
                    .release_pending_withdrawal_index
                    .get(&id.bytes())
                    .is_some()
            }),
            open_index: store.open_hold_index.get(&hold_id.get()).is_some(),
            scan: store
                .reconciliation_scan(&ReconciliationTarget::Hold(hold_id))
                .expect("scan"),
            scan_table_count: store.table_count("reconciliation_scans"),
            counts: store.status_counts().expect("counts"),
        }
    }

    #[test]
    #[serial]
    fn deposit_hold_creation_rolls_back_every_write_failpoint() {
        for failpoint in [
            HoldBundleFailpoint::Encode,
            HoldBundleFailpoint::Parent,
            HoldBundleFailpoint::ParentIndex,
            HoldBundleFailpoint::Hold,
            HoldBundleFailpoint::OpenHoldIndex,
            HoldBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("init");
            let mut previous = deposit();
            previous.state = DepositState::FundingPending;
            store.put_deposit(&previous).expect("seed parent");
            let hold_id = store.next_hold_id().expect("candidate");
            let mut next = previous.clone();
            next.apply(DepositEvent::FundingAmbiguous { hold_id })
                .expect("ambiguous");
            let hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::DepositFunding(next.id),
                next.transfer.clone(),
            );
            let before = hold_bundle_snapshot(&store, Some(next.id), None, hold_id);
            set_hold_bundle_failpoint(Some(failpoint));
            assert!(store.commit_deposit_hold_bundle(&next, &hold).is_err());
            set_hold_bundle_failpoint(None);
            assert_eq!(
                hold_bundle_snapshot(&store, Some(next.id), None, hold_id),
                before,
                "{failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                hold_bundle_snapshot(&reopened, Some(next.id), None, hold_id),
                before,
                "reopen {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn withdrawal_hold_creation_rolls_back_every_write_failpoint() {
        for failpoint in [
            HoldBundleFailpoint::Encode,
            HoldBundleFailpoint::Parent,
            HoldBundleFailpoint::ParentIndex,
            HoldBundleFailpoint::Hold,
            HoldBundleFailpoint::OpenHoldIndex,
            HoldBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("init");
            let previous = withdrawal();
            store.put_withdrawal(&previous).expect("seed parent");
            let hold_id = store.next_hold_id().expect("candidate");
            let transfer = match &previous.state {
                WithdrawalState::ReleasePending { attempt, .. } => attempt.identity.clone(),
                _ => unreachable!(),
            };
            let mut next = previous.clone();
            next.apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
                .expect("ambiguous");
            let hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::Withdrawal(next.id),
                transfer,
            );
            let before = hold_bundle_snapshot(&store, None, Some(next.id), hold_id);
            set_hold_bundle_failpoint(Some(failpoint));
            assert!(store.commit_withdrawal_hold_bundle(&next, &hold).is_err());
            set_hold_bundle_failpoint(None);
            assert_eq!(
                hold_bundle_snapshot(&store, None, Some(next.id), hold_id),
                before,
                "{failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                hold_bundle_snapshot(&reopened, None, Some(next.id), hold_id),
                before,
                "reopen {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn deposit_hold_absent_resolution_rolls_back_every_write_failpoint() {
        for failpoint in [
            ResolveHoldBundleFailpoint::Encode,
            ResolveHoldBundleFailpoint::Parent,
            ResolveHoldBundleFailpoint::ParentIndex,
            ResolveHoldBundleFailpoint::Hold,
            ResolveHoldBundleFailpoint::OpenHoldIndex,
            ResolveHoldBundleFailpoint::ReconciliationScan,
            ResolveHoldBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("init");
            let mut previous = deposit();
            previous.state = DepositState::FundingPending;
            store.put_deposit(&previous).expect("seed");
            let hold_id = store.next_hold_id().expect("candidate");
            let mut held = previous.clone();
            held.apply(DepositEvent::FundingAmbiguous { hold_id })
                .expect("held");
            let hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::DepositFunding(held.id),
                held.transfer.clone(),
            );
            store
                .commit_deposit_hold_bundle(&held, &hold)
                .expect("commit hold");
            let scan_target = ReconciliationTarget::Hold(hold_id);
            store
                .put_reconciliation_scan(&ReconciliationScanProgress::new(
                    scan_target.clone(),
                    hold.transfer.clone(),
                ))
                .expect("scan");
            let before = hold_bundle_snapshot(&store, Some(held.id), None, hold_id);
            set_resolve_hold_bundle_failpoint(Some(failpoint));
            assert!(store
                .resolve_deposit_hold_and_scan(
                    held.id,
                    hold_id,
                    DepositHoldResolution::FundingAbsent {
                        history_watermark: 100
                    },
                    Some(&scan_target),
                )
                .is_err());
            set_resolve_hold_bundle_failpoint(None);
            assert_eq!(
                hold_bundle_snapshot(&store, Some(held.id), None, hold_id),
                before,
                "{failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                hold_bundle_snapshot(&reopened, Some(held.id), None, hold_id),
                before,
                "reopen {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn withdrawal_hold_absent_resolution_rolls_back_every_write_failpoint() {
        for failpoint in [
            ResolveHoldBundleFailpoint::Encode,
            ResolveHoldBundleFailpoint::Parent,
            ResolveHoldBundleFailpoint::ParentIndex,
            ResolveHoldBundleFailpoint::Hold,
            ResolveHoldBundleFailpoint::OpenHoldIndex,
            ResolveHoldBundleFailpoint::ReconciliationScan,
            ResolveHoldBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("init");
            let previous = withdrawal();
            store.put_withdrawal(&previous).expect("seed");
            let hold_id = store.next_hold_id().expect("candidate");
            let transfer = match &previous.state {
                WithdrawalState::ReleasePending { attempt, .. } => attempt.identity.clone(),
                _ => unreachable!(),
            };
            let mut held = previous.clone();
            held.apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
                .expect("held");
            let hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::Withdrawal(held.id),
                transfer.clone(),
            );
            store
                .commit_withdrawal_hold_bundle(&held, &hold)
                .expect("commit hold");
            let scan_target = ReconciliationTarget::Hold(hold_id);
            store
                .put_reconciliation_scan(&ReconciliationScanProgress::new(
                    scan_target.clone(),
                    hold.transfer.clone(),
                ))
                .expect("scan");
            let mut next_identity = transfer;
            next_identity.created_at_time_ns += 1;
            next_identity.memo = [99; 32];
            let before = hold_bundle_snapshot(&store, None, Some(held.id), hold_id);
            set_resolve_hold_bundle_failpoint(Some(failpoint));
            assert!(store
                .resolve_withdrawal_hold_and_scan(
                    held.id,
                    hold_id,
                    WithdrawalHoldResolution::Absent {
                        history_watermark: 100,
                        next_identity: Box::new(next_identity)
                    },
                    Some(&scan_target),
                )
                .is_err());
            set_resolve_hold_bundle_failpoint(None);
            assert_eq!(
                hold_bundle_snapshot(&store, None, Some(held.id), hold_id),
                before,
                "{failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                hold_bundle_snapshot(&reopened, None, Some(held.id), hold_id),
                before,
                "reopen {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn reconciliation_scans_survive_reopen_and_are_isolated_by_target() {
        let memory = VectorMemory::default();
        let (_, hold) = held_deposit();
        let mut scan = ReconciliationScanProgress::new(
            ReconciliationTarget::Hold(hold.id),
            hold.transfer.clone(),
        );
        scan.phase = ReconciliationScanPhase::Ledger {
            next_block: 1_000,
            ledger_tip: Some(10_000),
            pending_page: Some(Box::new(ReconciliationLedgerPage {
                end: 2_000,
                archives: vec![ReconciliationArchiveRange {
                    canister_id: Principal::management_canister().as_slice().to_vec(),
                    method: "get_transactions".into(),
                    start: 1_000,
                    length: 1_000,
                }],
                next_archive: 0,
            })),
        };
        {
            let mut store = StableStore::init(memory.clone()).expect("initialize stable store");
            store
                .put_reconciliation_scan(&scan)
                .expect("persist active scan");
            let fee_scan = ReconciliationScanProgress::new(
                ReconciliationTarget::FeePayout(7),
                scan.transfer.clone(),
            );
            store
                .put_reconciliation_scan(&fee_scan)
                .expect("persist independent fee payout scan");
        }
        let mut reopened = StableStore::reopen(memory).expect("reopen stable store");
        assert_eq!(
            reopened
                .reconciliation_scan(&scan.target)
                .expect("read hold scan"),
            Some(scan.clone())
        );
        let fee_target = ReconciliationTarget::FeePayout(7);
        assert_eq!(
            reopened
                .reconciliation_scan(&fee_target)
                .expect("read fee payout scan")
                .map(|progress| progress.target),
            Some(fee_target.clone())
        );
        let mut conflicting_transfer = scan.transfer.clone();
        conflicting_transfer.created_at_time_ns += 1;
        let conflicting =
            ReconciliationScanProgress::new(scan.target.clone(), conflicting_transfer);
        assert!(matches!(
            reopened.put_reconciliation_scan(&conflicting),
            Err(StorageError::Core(CoreError::ConflictingReplay))
        ));
        reopened.remove_reconciliation_scan(&scan.target);
        assert_eq!(
            reopened
                .reconciliation_scan(&scan.target)
                .expect("read removed hold scan"),
            None
        );
        assert!(reopened
            .reconciliation_scan(&fee_target)
            .expect("read retained fee payout scan")
            .is_some());
    }

    #[test]
    #[serial]
    fn every_incomplete_record_survives_reopen() {
        let memory = VectorMemory::default();
        let mut deposit = deposit();
        let withdrawal = withdrawal();
        deposit
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id: EvmOperationId::new(6),
            })
            .expect("prepare mint");
        let mut evm = EvmOperationRecord::prepared(
            EvmOperationId::new(6),
            deposit.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        evm.apply(EvmOperationEvent::Submitted {
            transaction_hash: [8; 32],
        })
        .expect("submit evm");
        let hold = ReconciliationHoldRecord::open(
            HoldId::new(7),
            RequestReference::Withdrawal(withdrawal.id),
            transfer(LedgerOperation::ReleaseWithdrawal, 85, 30),
        );
        let accounting = AccountingState {
            fee_reserve: Amount::new(11),
            confirmed_deposit_fees: Amount::new(5),
            confirmed_withdrawal_fees: Amount::new(6),
        };
        let counters = CounterState {
            next_evm_operation_id: 8,
            next_hold_id: 9,
            pending_ledger_operations: 1,
            reserved_deposit_mint_amount: 100,
            reserved_deposit_mint_operations: 1,
            ..CounterState::default()
        };

        {
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            store.put_deposit(&deposit).expect("write deposit");
            store.put_withdrawal(&withdrawal).expect("write withdrawal");
            store.put_evm_operation(&evm).expect("write evm");
            store.put_reconciliation_hold(&hold).expect("write hold");
            store.set_accounting(&accounting).expect("write accounting");
            store.set_counters(&counters).expect("write counters");
        }

        let reopened = StableStore::reopen(memory).expect("reopen");
        assert_eq!(reopened.schema_version(), SCHEMA_VERSION);
        assert_eq!(
            reopened.deposit(deposit.id.bytes()).expect("read deposit"),
            Some(deposit)
        );
        assert_eq!(
            reopened
                .withdrawal(withdrawal.id.bytes())
                .expect("read withdrawal"),
            Some(withdrawal)
        );
        assert_eq!(
            reopened.evm_operation(evm.id.get()).expect("read evm"),
            Some(evm)
        );
        assert_eq!(
            reopened
                .reconciliation_hold(hold.id.get())
                .expect("read hold"),
            Some(hold)
        );
        assert_eq!(reopened.accounting().expect("read accounting"), accounting);
        assert_eq!(reopened.counters().expect("read counters"), counters);
        assert_eq!(
            reopened.status_counts().expect("counts"),
            StorageCounts {
                deposits: 1,
                withdrawals: 1,
                pending_evm_operations: 1,
                reconciliation_holds: 1,
                pending_ledger_operations: 1,
                reserved_deposit_mint_amount: 100,
                reserved_deposit_mint_operations: 1,
                unresolved_evm_reverts: 0,
                last_finalized_base_block: 0,
                active_evm_payloads: 0,
                retained_audit_events: 0,
                pruned_audit_events: 0,
                retained_deposit_index_entries: 0,
            }
        );
    }

    #[test]
    #[serial]
    fn schema_wire_corruption_and_size_are_rejected() {
        #[derive(Serialize)]
        struct IncompleteCounterState {
            next_evm_operation_id: u64,
            next_hold_id: u64,
            pending_evm_operations: u64,
            reconciliation_holds: u64,
        }

        assert_eq!(
            StableBlob::new(vec![0; MAX_STABLE_VALUE_BYTES + 1]),
            Err(StorageError::ValueTooLarge {
                actual: MAX_STABLE_VALUE_BYTES + 1,
                maximum: MAX_STABLE_VALUE_BYTES,
            })
        );
        assert_eq!(
            decode::<CounterState>(&StableBlob::new(vec![1, 0]).expect("bounded")),
            Err(StorageError::UnsupportedWireVersion(1))
        );
        assert_eq!(
            decode::<CounterState>(&StableBlob::new(vec![WIRE_VERSION, 0xff]).expect("bounded")),
            Err(StorageError::DecodeFailed)
        );
        assert_eq!(
            decode::<CounterState>(
                &encode(&IncompleteCounterState {
                    next_evm_operation_id: 0,
                    next_hold_id: 0,
                    pending_evm_operations: 0,
                    reconciliation_holds: 0,
                })
                .expect("encode incomplete value")
            ),
            Err(StorageError::DecodeFailed)
        );

        let store = StableStore::init(VectorMemory::default()).expect("initialize current schema");
        assert_eq!(store.schema_version(), SCHEMA_VERSION);
    }

    #[test]
    #[serial]
    fn non_current_schema_is_rejected_without_migration() {
        assert_ne!(SCHEMA_VERSION, 2);
        assert_eq!(SCHEMA_VERSION, 22);
        assert_eq!(WIRE_VERSION, 18);
    }

    #[test]
    #[serial]
    fn current_schema_has_one_schema_authority_and_no_legacy_deposit_or_withdrawal_log() {
        let store = StableStore::init(VectorMemory::default()).expect("initialize");
        let (legacy_tables, legacy_triggers, singleton_schema_columns) = store
            .handle
            .query(|connection| {
                Ok((
                    connection.query_scalar::<i64>(
                        "SELECT COUNT(*) FROM sqlite_master
                         WHERE type = 'table' AND name IN (
                            'deposit_intents',
                            'withdrawal_change_log',
                            'fee_payout_state_index'
                         )",
                        params![],
                    )?,
                    connection.query_scalar::<i64>(
                        "SELECT COUNT(*) FROM sqlite_master
                         WHERE type = 'trigger' AND name LIKE 'withdrawals_liability_%'",
                        params![],
                    )?,
                    connection.query_scalar::<i64>(
                        "SELECT COUNT(*) FROM pragma_table_info('singleton_state') WHERE name = 'schema'",
                        params![],
                    )?,
                ))
            })
            .expect("inspect schema");
        assert_eq!(
            (legacy_tables, legacy_triggers, singleton_schema_columns),
            (0, 0, 0)
        );
    }

    #[test]
    #[serial]
    fn singleton_reads_sqlite_as_the_only_authority() {
        let store = StableStore::init(VectorMemory::default()).expect("initialize");
        let expected = AccountingState {
            fee_reserve: Amount::new(9),
            confirmed_deposit_fees: Amount::new(4),
            confirmed_withdrawal_fees: Amount::new(5),
        };
        let blob = encode(&expected).expect("encode accounting");
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE singleton_state SET accounting = ?1 WHERE id = 1",
                    params![blob.to_sql_bytes()],
                )
            })
            .expect("update accounting");
        assert_eq!(store.accounting().expect("read accounting"), expected);
    }

    fn mark_stored_schema(store: &StableStore, version: u16) {
        store
            .handle
            .0
            .update(|connection| {
                connection.execute(
                    "UPDATE bridge_metadata SET application_schema_version = ?1 WHERE id = 1",
                    params![i64::from(version)],
                )
            })
            .expect("mark stored schema");
    }

    #[test]
    #[serial]
    fn obsolete_schema_fails_closed_even_when_empty() {
        let obsolete_version = SCHEMA_VERSION
            .checked_sub(1)
            .expect("current schema has a predecessor");
        let memory = VectorMemory::default();
        let store = StableStore::init(memory.clone()).expect("initialize current schema");
        mark_stored_schema(&store, obsolete_version);
        drop(store);

        assert!(matches!(
            StableStore::reopen_after_upgrade(memory),
            Err(StorageError::UnsupportedSchemaVersion(version))
                if version == obsolete_version
        ));
    }

    #[test]
    #[serial]
    fn old_wire_version_fails_closed() {
        let memory = VectorMemory::default();
        let store = StableStore::init(memory.clone()).expect("initialize current schema");
        store
            .handle
            .0
            .update(|connection| {
                connection.execute(
                    "UPDATE bridge_metadata SET record_wire_version = 17 WHERE id = 1",
                    params![],
                )
            })
            .expect("mark old wire");
        drop(store);

        assert!(matches!(
            StableStore::reopen_after_upgrade(memory),
            Err(StorageError::UnsupportedWireVersion(17))
        ));
    }

    #[test]
    #[serial]
    fn unknown_schema_still_fails_closed_during_upgrade() {
        let memory = VectorMemory::default();
        let store = StableStore::init(memory.clone()).expect("initialize current schema");
        mark_stored_schema(&store, 16);
        drop(store);

        assert!(matches!(
            StableStore::reopen_after_upgrade(memory),
            Err(StorageError::UnsupportedSchemaVersion(16))
        ));
    }

    #[test]
    #[serial]
    fn submitted_evm_confirmation_wait_is_scheduled_and_removed_atomically() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = EvmOperationId::new(41);
        let mut owner = deposit();
        owner
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare mint owner");
        store.put_deposit(&owner).expect("persist operation owner");
        let mut operation = EvmOperationRecord::prepared(
            operation_id,
            owner.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [4; 32],
            })
            .expect("submit operation");

        store
            .put_submitted_evm_operation(&operation, 10)
            .expect("persist operation and schedule");
        assert_eq!(
            store.evm_operation(operation_id.get()).expect("operation"),
            Some(operation)
        );
        assert_eq!(
            store
                .confirmation_schedule(operation_id.get())
                .expect("waiting confirmation"),
            Some(ConfirmationSchedule {
                operation_id: operation_id.get(),
                submitted_at_ns: 10,
                next_check_at_ns: 60_000_000_010,
                checks_completed: 0,
            })
        );
        assert_eq!(
            store.next_settlement_wakeup_ns(10).expect("wakeup"),
            Some(60_000_000_010)
        );
        let mut earlier = EvmOperationRecord::prepared(
            EvmOperationId::new(99),
            [9; 32],
            EvmOperationKind::MintDeposit,
        );
        let mut earlier_owner = deposit();
        earlier_owner.id = DepositId::new([99; 32]);
        earlier_owner.payload_hash = [9; 32];
        earlier_owner
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id: EvmOperationId::new(99),
            })
            .expect("prepare earlier owner");
        store
            .put_deposit(&earlier_owner)
            .expect("persist earlier owner");
        earlier
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [9; 32],
            })
            .expect("submit earlier operation");
        store
            .put_submitted_evm_operation(&earlier, 20)
            .expect("insert earlier schedule");
        assert!(store
            .confirmation_schedule(99)
            .expect("second wait")
            .is_some());
        assert_eq!(
            store.next_settlement_wakeup_ns(20).expect("wakeup"),
            Some(60_000_000_010)
        );
        store
            .remove_confirmation_schedule(99)
            .expect("remove earlier");

        operation.state = EvmOperationState::Confirmed {
            transaction_hash: [4; 32],
            receipt_block_number: 7,
            finalized_head_block_number: 8,
        };
        store
            .put_evm_operation(&operation)
            .expect("persist confirmation and remove schedule");
        assert_eq!(
            store.evm_operation(operation_id.get()).expect("confirmed"),
            Some(operation)
        );
        assert_eq!(
            store
                .confirmation_schedule(operation_id.get())
                .expect("schedule"),
            None
        );
    }

    #[test]
    #[serial]
    fn settlement_phase_operation_is_not_decoded_as_a_confirmation_schedule() {
        let store = StableStore::init(VectorMemory::default()).expect("initialize");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Withdrawal,
                    [7; 32],
                    Some(77),
                    10,
                )
            })
            .expect("enqueue settlement phase job");

        assert_eq!(
            store.confirmation_schedule(77).expect("schedule lookup"),
            None
        );
    }

    #[test]
    #[serial]
    fn manual_claim_cannot_bypass_an_awaiting_confirmation() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let operation_id = EvmOperationId::new(42);
        let mut owner = deposit();
        owner
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare mint owner");
        store.put_deposit(&owner).expect("persist operation owner");
        let mut operation = EvmOperationRecord::prepared(
            operation_id,
            owner.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [4; 32],
            })
            .expect("submit operation");
        store
            .put_submitted_evm_operation(&operation, 10)
            .expect("persist operation and schedule");

        assert!(matches!(
            store
                .claim_due_settlement_job(200, 300, u64::MAX)
                .expect("no due confirmation"),
            SettlementJobClaim::None
        ));
        let limits = SettlementQuotaLimits {
            window_seconds: 60,
            global: 10,
            per_principal: 10,
            per_record: 10,
        };
        let job_before = store
            .settlement_job(SettlementJobKind::Deposit, owner.id.bytes())
            .expect("job before manual claim");
        assert!(matches!(
            store
                .claim_manual_settlement_job(
                    SettlementJobKind::Deposit,
                    owner.id.bytes(),
                    Principal::self_authenticating([42; 32]),
                    200,
                    300,
                    0,
                    limits,
                )
                .expect("confirmation claim decision"),
            ManualSettlementClaim::AutomaticProgressPending { .. }
        ));
        assert_eq!(
            store
                .settlement_job(SettlementJobKind::Deposit, owner.id.bytes())
                .expect("job after manual claim"),
            job_before
        );
        assert_eq!(
            store
                .confirmation_schedule(operation_id.get())
                .expect("confirmation schedule"),
            Some(ConfirmationSchedule {
                operation_id: operation_id.get(),
                submitted_at_ns: 10,
                next_check_at_ns: 60_000_000_010,
                checks_completed: 0,
            })
        );
    }

    #[test]
    #[serial]
    fn manual_claim_propagates_corrupt_evm_operation_without_mutating_job_or_quota() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let operation_id = EvmOperationId::new(42);
        let mut owner = deposit();
        owner
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare mint owner");
        store.put_deposit(&owner).expect("persist operation owner");
        let mut operation = EvmOperationRecord::prepared(
            operation_id,
            owner.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [4; 32],
            })
            .expect("submit operation");
        store
            .put_submitted_evm_operation(&operation, 10)
            .expect("persist submitted operation");
        store.evm_operations.insert(
            operation_id.get(),
            StableBlob::new(vec![WIRE_VERSION, 0xff]).expect("bounded corruption"),
        );
        let job_before = store
            .settlement_job(SettlementJobKind::Deposit, owner.id.bytes())
            .expect("job before claim");
        let quota_before = store.settlement_admission.get().expect("quota blob");

        assert_eq!(
            store.claim_manual_settlement_job(
                SettlementJobKind::Deposit,
                owner.id.bytes(),
                Principal::self_authenticating([42; 32]),
                200,
                300,
                0,
                SettlementQuotaLimits {
                    window_seconds: 60,
                    global: 10,
                    per_principal: 10,
                    per_record: 10,
                },
            ),
            Err(SettlementAdmissionError::Storage)
        );
        assert_eq!(
            store
                .settlement_job(SettlementJobKind::Deposit, owner.id.bytes())
                .expect("job after claim"),
            job_before
        );
        assert_eq!(
            store.settlement_admission.get().expect("quota blob"),
            quota_before
        );
    }

    #[test]
    #[serial]
    fn active_lease_does_not_block_an_overdue_job_for_another_record() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let first = deposit();
        let mut second = deposit();
        second.id = DepositId::new([2; 32]);
        second.payload_hash = [3; 32];
        store.put_deposit(&first).expect("first deposit");
        store.put_deposit(&second).expect("second deposit");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    first.id.bytes(),
                    None,
                    100,
                )
            })
            .expect("first job");
        let SettlementJobClaim::Claimed(_) = store
            .claim_due_settlement_job(100, 500, 2)
            .expect("claim first")
        else {
            panic!("first job was not claimed")
        };
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    second.id.bytes(),
                    None,
                    1,
                )
            })
            .expect("overdue second job");

        assert_eq!(
            store.next_settlement_wakeup_ns(200).expect("wakeup"),
            Some(1)
        );
        assert!(matches!(
            store
                .claim_due_settlement_job(200, 600, 1)
                .expect("bounded claim"),
            SettlementJobClaim::ActiveLease {
                lease_until_ns: 500
            }
        ));
        let SettlementJobClaim::Claimed(claimed) = store
            .claim_due_settlement_job(200, 600, 2)
            .expect("second claim")
        else {
            panic!("active lease on the first record blocked the second record")
        };
        assert_eq!(claimed.settlement_id, second.id.bytes());
        assert!(matches!(
            store
                .claim_specific_due_settlement_job(
                    SettlementJobKind::Deposit,
                    first.id.bytes(),
                    200,
                    600,
                )
                .expect("specific active lease"),
            SettlementJobClaim::ActiveLease {
                lease_until_ns: 500
            }
        ));
    }

    #[test]
    #[serial]
    fn lease_renewal_uses_real_update_time_and_stale_generation_cannot_finish() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let record = deposit();
        store.put_deposit(&record).expect("deposit");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    None,
                    100,
                )
            })
            .expect("job");
        let SettlementJobClaim::Claimed(mut first) = store
            .claim_due_settlement_job(100, 220, u64::MAX)
            .expect("first claim")
        else {
            panic!("job was not claimed")
        };
        assert!(store
            .renew_settlement_lease(&mut first, 150, 270)
            .expect("renew"));
        assert_eq!(first.updated_at_ns, 150);
        assert_eq!(first.lease_until_ns, Some(270));

        let SettlementJobClaim::Claimed(second) = store
            .claim_due_settlement_job(270, 390, u64::MAX)
            .expect("expired recovery")
        else {
            panic!("expired lease was not recovered")
        };
        assert!(second.lease_generation > first.lease_generation);
        store
            .finish_settlement_job(&first, None, 0, None, None, 300)
            .expect("stale outcome is ignored");
        assert_eq!(
            store
                .settlement_job(SettlementJobKind::Deposit, record.id.bytes())
                .expect("job")
                .expect("present")
                .lease_generation,
            second.lease_generation
        );
    }

    #[test]
    #[serial]
    fn stopped_job_and_record_stop_reason_commit_together() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let record = deposit();
        store.put_deposit(&record).expect("deposit");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    None,
                    10,
                )
            })
            .expect("job");
        let SettlementJobClaim::Claimed(job) = store
            .claim_due_settlement_job(10, 130, u64::MAX)
            .expect("claim")
        else {
            panic!("job was not claimed")
        };
        store
            .finish_settlement_job(
                &job,
                None,
                0,
                Some(("RpcInconsistent", "RPC quorum disagreed")),
                Some("RPC quorum disagreed".into()),
                20,
            )
            .expect("atomic stop");

        assert_eq!(
            store
                .deposit(record.id.bytes())
                .expect("deposit")
                .expect("present")
                .last_settlement_stop_reason
                .as_deref(),
            Some("RPC quorum disagreed")
        );
        let stopped = store
            .settlement_job(SettlementJobKind::Deposit, record.id.bytes())
            .expect("job")
            .expect("present");
        assert_eq!(stopped.status, SettlementJobStatus::Stopped);
        assert_eq!(stopped.last_error_code.as_deref(), Some("RpcInconsistent"));
        assert_eq!(stopped.updated_at_ns, 20);
    }

    #[test]
    #[serial]
    fn manual_claim_consumes_quota_only_when_the_lease_is_acquired() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let caller = Principal::self_authenticating([55; 32]);
        let limits = SettlementQuotaLimits {
            window_seconds: 60,
            global: 1,
            per_principal: 1,
            per_record: 1,
        };
        let record = deposit();
        store.put_deposit(&record).expect("deposit");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    None,
                    1_000,
                )
            })
            .expect("scheduled job");
        assert!(matches!(
            store
                .claim_manual_settlement_job(
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    caller,
                    0,
                    120,
                    300,
                    limits,
                )
                .expect("scheduled manual claim decision"),
            ManualSettlementClaim::AutomaticProgressPending {
                next_run_at_ns: Some(1_000)
            }
        ));

        store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE settlement_jobs SET status = 2, next_run_at_ns = NULL
                     WHERE settlement_kind = 0 AND settlement_id = ?1",
                    params![record.id.bytes().to_sql_bytes()],
                )
            })
            .expect("second stopped job");
        let ManualSettlementClaim::Claimed(_) = store
            .claim_manual_settlement_job(
                SettlementJobKind::Deposit,
                record.id.bytes(),
                caller,
                2,
                122,
                300,
                limits,
            )
            .expect("stopped manual claim")
        else {
            panic!("stopped job was not claimed")
        };
        assert!(matches!(
            store.claim_manual_settlement_job(
                SettlementJobKind::Deposit,
                record.id.bytes(),
                caller,
                123,
                243,
                300,
                limits,
            ),
            Err(SettlementAdmissionError::RateLimited { .. })
        ));
    }

    #[test]
    #[serial]
    fn funding_callback_requires_current_generation_and_exact_transfer_identity() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let pending = funding_pending_deposit();
        store.put_deposit(&pending).expect("seed funding deposit");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    pending.id.bytes(),
                    None,
                    0,
                )
            })
            .expect("schedule funding job");
        let SettlementJobClaim::Claimed(job) = store
            .claim_due_settlement_job(0, 100, 1)
            .expect("claim funding job")
        else {
            panic!("funding job was not claimed")
        };
        let token =
            SettlementCallbackToken::for_deposit(&job, &pending.transfer).expect("callback token");
        let mut succeeded = pending.clone();
        succeeded
            .apply(DepositEvent::FundingSucceeded {
                ledger_block_index: 9,
            })
            .expect("funding succeeds");

        let mut stale = token;
        stale.lease_generation += 1;
        assert!(store
            .put_deposit_funding_callback(&succeeded, &stale)
            .is_err());
        assert_eq!(
            store
                .deposit(pending.id.bytes())
                .expect("read pending")
                .expect("present"),
            pending
        );

        let mut foreign_transfer = pending.transfer.clone();
        foreign_transfer.memo = [99; 32];
        let foreign =
            SettlementCallbackToken::for_deposit(&job, &foreign_transfer).expect("foreign token");
        assert!(store
            .put_deposit_funding_callback(&succeeded, &foreign)
            .is_err());
        assert_eq!(
            store
                .deposit(pending.id.bytes())
                .expect("read pending")
                .expect("present"),
            pending
        );

        store
            .put_deposit_funding_callback(&succeeded, &token)
            .expect("current callback");
        assert_eq!(
            store
                .deposit(pending.id.bytes())
                .expect("read succeeded")
                .expect("present"),
            succeeded
        );
    }

    #[test]
    #[serial]
    fn funding_callback_rolls_back_every_record_write_failpoint_and_reopens() {
        for failpoint in [
            RecordWriteFailpoint::Encode,
            RecordWriteFailpoint::RemoveIndex,
            RecordWriteFailpoint::AddIndex,
            RecordWriteFailpoint::OperationOwner,
            RecordWriteFailpoint::Record,
            RecordWriteFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let pending = funding_pending_deposit();
            store.put_deposit(&pending).expect("seed funding deposit");
            store
                .handle
                .update(|connection| {
                    enqueue_settlement_job(
                        connection,
                        SettlementJobKind::Deposit,
                        pending.id.bytes(),
                        None,
                        0,
                    )
                })
                .expect("schedule funding job");
            let SettlementJobClaim::Claimed(job) = store
                .claim_due_settlement_job(0, 100, 1)
                .expect("claim funding job")
            else {
                panic!("funding job was not claimed")
            };
            let token = SettlementCallbackToken::for_deposit(&job, &pending.transfer)
                .expect("callback token");
            let mut succeeded = pending.clone();
            succeeded
                .apply(DepositEvent::FundingSucceeded {
                    ledger_block_index: 9,
                })
                .expect("funding succeeds");
            let counters = store.counters().expect("counters before");

            set_record_write_failpoint(Some(failpoint));
            assert!(
                store
                    .put_deposit_funding_callback(&succeeded, &token)
                    .is_err(),
                "{failpoint:?}"
            );
            set_record_write_failpoint(None);
            assert_eq!(
                store
                    .deposit(pending.id.bytes())
                    .expect("read pending")
                    .expect("present"),
                pending,
                "{failpoint:?}"
            );
            assert_eq!(store.counters().expect("counters after"), counters);
            assert_eq!(
                store
                    .settlement_job(SettlementJobKind::Deposit, pending.id.bytes())
                    .expect("job after")
                    .expect("present")
                    .lease_generation,
                job.lease_generation
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                reopened
                    .deposit(pending.id.bytes())
                    .expect("reopened pending")
                    .expect("present"),
                pending,
                "{failpoint:?}"
            );
            assert_eq!(reopened.counters().expect("reopened counters"), counters);
        }
    }

    #[test]
    #[serial]
    fn manual_claim_uses_overdue_boundary_from_shared_decision() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let record = deposit();
        store.put_deposit(&record).expect("deposit");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    None,
                    100,
                )
            })
            .expect("scheduled job");
        let limits = SettlementQuotaLimits {
            window_seconds: 60,
            global: 10,
            per_principal: 10,
            per_record: 10,
        };
        assert!(matches!(
            store
                .claim_manual_settlement_job(
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    Principal::self_authenticating([61; 32]),
                    399,
                    500,
                    300,
                    limits,
                )
                .expect("pre-overdue decision"),
            ManualSettlementClaim::AutomaticProgressPending {
                next_run_at_ns: Some(100)
            }
        ));
        assert!(matches!(
            store
                .claim_manual_settlement_job(
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    Principal::self_authenticating([61; 32]),
                    400,
                    500,
                    300,
                    limits,
                )
                .expect("overdue decision"),
            ManualSettlementClaim::Claimed(_)
        ));
        assert!(matches!(
            store
                .claim_manual_settlement_job(
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    Principal::self_authenticating([62; 32]),
                    401,
                    500,
                    300,
                    limits,
                )
                .expect("active overdue decision"),
            ManualSettlementClaim::AutomaticProgressPending {
                next_run_at_ns: None
            }
        ));
    }

    #[test]
    #[serial]
    fn settlement_quota_enforces_each_boundary_and_resets_window() {
        let caller = Principal::self_authenticating([41; 32]);
        let other = Principal::self_authenticating([42; 32]);
        let limits = |global, per_principal, per_record| SettlementQuotaLimits {
            window_seconds: 60,
            global,
            per_principal,
            per_record,
        };

        let mut per_record = StableStore::init(VectorMemory::default()).expect("record store");
        per_record
            .reserve_settlement_quota(caller, vec![1], 1, limits(10, 10, 1))
            .expect("first record attempt");
        assert!(matches!(
            per_record.reserve_settlement_quota(caller, vec![1], 2, limits(10, 10, 1)),
            Err(SettlementAdmissionError::RateLimited { .. })
        ));

        let mut per_caller = StableStore::init(VectorMemory::default()).expect("caller store");
        per_caller
            .reserve_settlement_quota(caller, vec![1], 1, limits(10, 1, 10))
            .expect("first caller attempt");
        assert!(matches!(
            per_caller.reserve_settlement_quota(caller, vec![2], 2, limits(10, 1, 10)),
            Err(SettlementAdmissionError::RateLimited { .. })
        ));

        let mut global = StableStore::init(VectorMemory::default()).expect("global store");
        global
            .reserve_settlement_quota(caller, vec![1], 1, limits(1, 1, 1))
            .expect("first global attempt");
        assert!(matches!(
            global.reserve_settlement_quota(other, vec![2], 2, limits(1, 1, 1)),
            Err(SettlementAdmissionError::RateLimited { .. })
        ));
        global
            .reserve_settlement_quota(other, vec![2], 60_000_000_000, limits(1, 1, 1))
            .expect("new window resets all quotas");
    }

    #[test]
    #[serial]
    fn current_schema_reopen_preserves_confirmation_wait_quota_and_scheduler_health() {
        let memory = VectorMemory::default();
        let caller = Principal::self_authenticating([43; 32]);
        let schedule = ConfirmationSchedule {
            operation_id: 43,
            submitted_at_ns: 10,
            next_check_at_ns: 20,
            checks_completed: 2,
        };
        let health = ConfirmationSchedulerHealth {
            healthy: false,
            last_run_ns: 30,
            last_error: Some("test fault".into()),
        };
        {
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let mut owner = deposit();
            owner.payload_hash = [4; 32];
            owner
                .apply(DepositEvent::CommitQuote {
                    quote: test_deposit_quote(),
                    operation_id: EvmOperationId::new(43),
                })
                .expect("prepare scheduled owner");
            store.put_deposit(&owner).expect("persist scheduled owner");
            let mut operation = EvmOperationRecord::prepared(
                EvmOperationId::new(43),
                [4; 32],
                EvmOperationKind::MintDeposit,
            );
            operation
                .apply(EvmOperationEvent::Submitted {
                    transaction_hash: [4; 32],
                })
                .expect("submit scheduled operation");
            store
                .put_submitted_evm_operation(&operation, schedule.submitted_at_ns)
                .expect("schedule");
            store
                .set_confirmation_schedule(schedule)
                .expect("update checks completed");
            store
                .reserve_settlement_quota(
                    caller,
                    vec![1],
                    1,
                    SettlementQuotaLimits {
                        window_seconds: 60,
                        global: 2,
                        per_principal: 2,
                        per_record: 2,
                    },
                )
                .expect("reserve quota");
            store
                .set_confirmation_scheduler_health(&health)
                .expect("scheduler health");
        }

        let mut reopened = StableStore::reopen(memory).expect("reopen v7");
        assert_eq!(reopened.schema_version(), SCHEMA_VERSION);
        assert_eq!(
            reopened.confirmation_schedule(43).expect("schedule"),
            Some(ConfirmationSchedule { ..schedule })
        );
        let reopened_job = reopened
            .settlement_job(SettlementJobKind::Deposit, [1; 32])
            .expect("job")
            .expect("scheduled job");
        assert_eq!(reopened_job.phase, SettlementJobPhase::Confirmation);
        assert_eq!(reopened_job.confirmation_started_at_ns, Some(10));
        assert_eq!(reopened_job.confirmation_checks, 2);
        assert_eq!(reopened_job.status, SettlementJobStatus::Scheduled);
        assert_eq!(
            reopened.confirmation_scheduler_health().expect("health"),
            health
        );
        reopened
            .reserve_settlement_quota(
                caller,
                vec![2],
                2,
                SettlementQuotaLimits {
                    window_seconds: 60,
                    global: 2,
                    per_principal: 1,
                    per_record: 2,
                },
            )
            .expect_err("reopened caller quota is retained");
    }

    #[test]
    #[serial]
    fn active_counters_follow_insert_replay_and_terminal_updates() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut evm = EvmOperationRecord::queued(
            EvmOperationId::new(1),
            [1; 32],
            EvmOperationKind::MintDeposit,
        );
        store
            .put_evm_call_intent(&evm_intent(evm.id, evm.payload_hash))
            .expect("insert EVM intent");
        store.put_evm_operation(&evm).expect("insert pending EVM");
        store.put_evm_operation(&evm).expect("replay pending EVM");
        assert_eq!(store.table_count("evm_state_index"), 1);
        evm.state = EvmOperationState::Confirmed {
            transaction_hash: [2; 32],
            receipt_block_number: 2,
            finalized_head_block_number: 3,
        };
        store.put_evm_operation(&evm).expect("confirm EVM");
        store.put_evm_operation(&evm).expect("replay confirmed EVM");
        assert_eq!(store.table_count("evm_state_index"), 0);

        let (_, mut hold) = held_deposit();
        store
            .put_open_reconciliation_hold(&hold)
            .expect("insert open hold");
        store
            .put_open_reconciliation_hold(&hold)
            .expect("replay open hold");
        assert_eq!(store.table_count("open_hold_index"), 1);
        hold.state = ReconciliationHoldState::ResolvedAbsent {
            history_watermark: 9,
        };
        store
            .put_reconciliation_hold(&hold)
            .expect("resolve hold internally");
        store
            .put_reconciliation_hold(&hold)
            .expect("replay resolved hold internally");
        assert_eq!(store.table_count("open_hold_index"), 0);
    }

    #[test]
    #[serial]
    fn withdrawal_liability_summary_tracks_fixed_amount_age_and_stop_reason() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let mut record = withdrawal();
        record.observed_at_ns = 42;
        record.last_settlement_stop_reason = Some("LedgerUnavailable".into());
        store
            .put_withdrawal(&record)
            .expect("persist unpaid withdrawal");
        assert_eq!(
            store.withdrawal_liability_summary().expect("summary"),
            WithdrawalLiabilitySummary {
                count: 1,
                amount_out: 90,
                oldest_observed_at_ns: Some(42),
                stop_reasons: vec!["LedgerUnavailable".into()],
            }
        );
        record
            .apply(WithdrawalEvent::ReleaseSucceeded {
                ledger_block_index: 7,
            })
            .expect("mark paid");
        store
            .put_withdrawal(&record)
            .expect("persist paid withdrawal");
        assert_eq!(
            store
                .withdrawal_liability_summary()
                .expect("terminal summary"),
            WithdrawalLiabilitySummary::default()
        );
    }

    #[test]
    #[serial]
    fn chunked_validation_detects_state_changes_and_maintenance_is_bounded() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        store
            .put_withdrawal(&withdrawal())
            .expect("seed withdrawal");

        assert_eq!(
            store.continue_storage_validation(1),
            Err(StorageMaintenanceError::NotStarted)
        );
        assert_eq!(
            store.continue_storage_validation(0),
            Err(StorageMaintenanceError::InvalidArgument {
                message: "max_rows must be between 1 and 100".into(),
            })
        );
        store.start_storage_validation().expect("start validation");
        store.put_deposit(&deposit()).expect("interrupt validation");
        assert_eq!(
            store.continue_storage_validation(1),
            Err(StorageMaintenanceError::StateChanged)
        );

        store
            .start_storage_validation()
            .expect("restart validation");
        loop {
            let status = store
                .continue_storage_validation(MAX_VALIDATION_ROWS)
                .expect("continue validation");
            if status.complete {
                assert!(status.scanned_rows >= 4);
                break;
            }
        }
        assert_eq!(store.storage_integrity_check().expect("integrity"), "ok");
        assert_eq!(
            store.refresh_storage_checksum(0),
            Err(StorageMaintenanceError::InvalidArgument {
                message: "max_bytes must be between 1 and 4194304".into(),
            })
        );
        loop {
            let status = store
                .refresh_storage_checksum(MAX_CHECKSUM_REFRESH_BYTES)
                .expect("refresh checksum");
            if status.complete {
                assert!(status.db_size > 0);
                break;
            }
        }
    }

    #[test]
    #[serial]
    fn chunked_validation_rejects_malformed_rows_in_every_validation_table() {
        let previously_unchecked_tables = [
            "reconciliation_scans",
            "audit_events",
            "fee_payouts",
            "evm_state_index",
            "open_hold_index",
            "owner_deposit_sequences",
        ];

        for table in previously_unchecked_tables {
            let store = StableStore::init(VectorMemory::default()).expect("initialize");
            store
                .handle
                .update(|connection| {
                    connection.execute(
                        &format!("INSERT INTO {table}(key, value) VALUES (?1, ?2)"),
                        params![vec![0u8], vec![0u8]],
                    )?;
                    increment_table_count(connection, table)
                })
                .expect("inject malformed validation row");

            store.start_storage_validation().expect("start validation");
            let mut rejected = false;
            for _ in 0..=VALIDATION_TABLES.len() {
                match store.continue_storage_validation(MAX_VALIDATION_ROWS) {
                    Err(StorageMaintenanceError::StorageFailure) => {
                        rejected = true;
                        break;
                    }
                    Ok(status) if !status.complete => {}
                    result => panic!("{table} unexpectedly passed validation: {result:?}"),
                }
            }
            assert!(rejected, "{table} malformed row was not rejected");
        }
    }

    #[test]
    #[serial]
    fn primary_record_writes_roll_back_every_write_failpoint_and_after_reopen() {
        let failpoints = [
            RecordWriteFailpoint::Encode,
            RecordWriteFailpoint::RemoveIndex,
            RecordWriteFailpoint::AddIndex,
            RecordWriteFailpoint::OperationOwner,
            RecordWriteFailpoint::Record,
            RecordWriteFailpoint::SingletonState,
        ];

        for failpoint in failpoints {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("init deposit store");
            let previous = deposit();
            store.put_deposit(&previous).expect("seed deposit");
            let mut next = previous.clone();
            next.apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id: EvmOperationId::new(91),
            })
            .expect("prepare mint");
            let before = (
                store.counters().expect("counters"),
                store.deposit(previous.id.bytes()).expect("deposit"),
                store.operation_owner_index.get(&91),
                store.table_count("deposits"),
                store.table_count("operation_owner_index"),
            );
            set_record_write_failpoint(Some(failpoint));
            assert!(store.put_deposit(&next).is_err(), "{failpoint:?}");
            set_record_write_failpoint(None);
            assert_eq!(
                (
                    store.counters().expect("counters"),
                    store.deposit(previous.id.bytes()).expect("deposit"),
                    store.operation_owner_index.get(&91),
                    store.table_count("deposits"),
                    store.table_count("operation_owner_index"),
                ),
                before,
                "deposit {failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen deposit store");
            assert_eq!(
                (
                    reopened.counters().expect("counters"),
                    reopened.deposit(previous.id.bytes()).expect("deposit"),
                    reopened.operation_owner_index.get(&91),
                    reopened.table_count("deposits"),
                    reopened.table_count("operation_owner_index"),
                ),
                before,
                "reopened deposit {failpoint:?}"
            );
        }

        for failpoint in failpoints {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("init withdrawal store");
            let mut previous = WithdrawalRecord::observed(
                WithdrawalId::new([33; 32]),
                [0; 20],
                vec![1],
                [0; 32],
                [34; 32],
                Amount::new(100),
                Amount::new(20),
                Amount::new(10),
                Amount::new(90),
                1,
            )
            .expect("valid withdrawal");
            previous
                .apply(WithdrawalEvent::StartRelease {
                    attempt: Box::new(TransferAttempt {
                        attempt_no: 0,
                        identity: withdrawal_transfer(90, 30),
                    }),
                    settlement: Settlement {
                        amount_out: Amount::new(90),
                        service_fee: Amount::new(10),
                        ledger_fee: Amount::new(1),
                    },
                })
                .expect("release pending");
            store.put_withdrawal(&previous).expect("seed withdrawal");
            let mut next = previous.clone();
            next.apply(WithdrawalEvent::ReleaseSucceeded {
                ledger_block_index: 72,
            })
            .expect("release transferred");
            let before = (
                store.counters().expect("counters"),
                store.withdrawal(previous.id.bytes()).expect("withdrawal"),
                store
                    .release_pending_withdrawal_index
                    .get(&previous.id.bytes()),
                store.table_count("withdrawals"),
                store.table_count("release_pending_withdrawal_index"),
            );
            set_record_write_failpoint(Some(failpoint));
            assert!(store.put_withdrawal(&next).is_err(), "{failpoint:?}");
            set_record_write_failpoint(None);
            assert_eq!(
                (
                    store.counters().expect("counters"),
                    store.withdrawal(previous.id.bytes()).expect("withdrawal"),
                    store
                        .release_pending_withdrawal_index
                        .get(&previous.id.bytes()),
                    store.table_count("withdrawals"),
                    store.table_count("release_pending_withdrawal_index"),
                ),
                before,
                "withdrawal {failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen withdrawal store");
            assert_eq!(
                (
                    reopened.counters().expect("counters"),
                    reopened
                        .withdrawal(previous.id.bytes())
                        .expect("withdrawal"),
                    reopened
                        .release_pending_withdrawal_index
                        .get(&previous.id.bytes()),
                    reopened.table_count("withdrawals"),
                    reopened.table_count("release_pending_withdrawal_index"),
                ),
                before,
                "reopened withdrawal {failpoint:?}"
            );
        }

        for failpoint in failpoints {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("init hold store");
            let (_, mut next) = held_deposit();
            store
                .put_open_reconciliation_hold(&next)
                .expect("seed hold");
            next.state = ReconciliationHoldState::ResolvedAbsent {
                history_watermark: 55,
            };
            let before = (
                store.counters().expect("counters"),
                store.reconciliation_hold(next.id.get()).expect("hold"),
                store.open_hold_index.get(&next.id.get()),
                store.table_count("reconciliation_holds"),
                store.table_count("open_hold_index"),
            );
            set_record_write_failpoint(Some(failpoint));
            assert!(
                store.put_reconciliation_hold(&next).is_err(),
                "{failpoint:?}"
            );
            set_record_write_failpoint(None);
            assert_eq!(
                (
                    store.counters().expect("counters"),
                    store.reconciliation_hold(next.id.get()).expect("hold"),
                    store.open_hold_index.get(&next.id.get()),
                    store.table_count("reconciliation_holds"),
                    store.table_count("open_hold_index"),
                ),
                before,
                "hold {failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen hold store");
            assert_eq!(
                (
                    reopened.counters().expect("counters"),
                    reopened.reconciliation_hold(next.id.get()).expect("hold"),
                    reopened.open_hold_index.get(&next.id.get()),
                    reopened.table_count("reconciliation_holds"),
                    reopened.table_count("open_hold_index"),
                ),
                before,
                "reopened hold {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn evm_payload_is_exclusive_retained_through_submission_and_removed_at_terminal() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = EvmOperationId::new(77);
        let mut deposit = deposit();
        deposit
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare deposit mint");
        store.put_deposit(&deposit).expect("index operation owner");

        let mut operation = EvmOperationRecord::queued(
            operation_id,
            deposit.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        let intent = evm_intent(operation_id, deposit.payload_hash);
        store
            .put_evm_call_intent(&intent)
            .expect("store awaiting nonce payload");
        store
            .put_evm_operation(&operation)
            .expect("store queued operation");
        assert!(matches!(
            store
                .evm_execution_payload(operation_id.get())
                .expect("read payload"),
            Some(EvmExecutionPayload::AwaitingNonce(_))
        ));

        let envelope = intent.assign_nonce(9);
        operation
            .apply(EvmOperationEvent::Prepared)
            .expect("prepare operation");
        let progress = ExternalProgress {
            nonce_initialized: true,
            next_evm_nonce: 10,
            ..ExternalProgress::default()
        };
        store
            .prepare_evm_operation(&operation, &envelope, &progress)
            .expect("atomically prepare operation");
        assert!(store
            .evm_call_intent(operation_id.get())
            .expect("read intent")
            .is_none());
        assert!(store
            .evm_envelope(operation_id.get())
            .expect("read envelope")
            .is_some());

        let mut signed = envelope;
        signed.signed_transaction = Some(vec![1, 2, 3, 4]);
        store
            .put_evm_envelope(&signed)
            .expect("persist signed transaction before broadcast");
        assert_eq!(
            store
                .evm_envelope(operation_id.get())
                .expect("read signed envelope")
                .and_then(|envelope| envelope.signed_transaction),
            Some(vec![1, 2, 3, 4])
        );

        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [8; 32],
            })
            .expect("submit operation");
        store
            .put_evm_operation(&operation)
            .expect("retain payload with submitted state");
        assert!(store
            .evm_execution_payload(operation_id.get())
            .expect("read submitted payload")
            .is_some());
        assert!(store
            .deposit_for_operation(operation_id)
            .expect("read nonterminal owner")
            .is_some());

        operation
            .apply(EvmOperationEvent::Confirmed {
                transaction_hash: [8; 32],
                receipt_block_number: 20,
                finalized_head_block_number: 21,
            })
            .expect("confirm operation");
        store
            .put_evm_operation(&operation)
            .expect("remove terminal owner index");
        assert!(store
            .evm_execution_payload(operation_id.get())
            .expect("read terminal payload")
            .is_none());
        assert!(store
            .deposit_for_operation(operation_id)
            .expect("read terminal owner")
            .is_none());
        assert_eq!(
            store
                .evm_operation(operation_id.get())
                .expect("read terminal operation")
                .map(|operation| operation.state),
            Some(EvmOperationState::Confirmed {
                transaction_hash: [8; 32],
                receipt_block_number: 20,
                finalized_head_block_number: 21,
            })
        );
    }

    #[test]
    #[serial]
    fn pending_evm_replacement_survives_reopen_and_promotes_with_audit_atomically() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize");
        let operation_id = EvmOperationId::new(78);
        let mut deposit = deposit();
        deposit
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare deposit mint");
        store.put_deposit(&deposit).expect("index operation owner");

        let mut operation = EvmOperationRecord::queued(
            operation_id,
            deposit.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        let intent = evm_intent(operation_id, deposit.payload_hash);
        store
            .put_evm_call_intent(&intent)
            .expect("store awaiting nonce payload");
        store
            .put_evm_operation(&operation)
            .expect("store queued operation");
        let mut envelope = intent.assign_nonce(9);
        operation
            .apply(EvmOperationEvent::Prepared)
            .expect("prepare operation");
        store
            .prepare_evm_operation(
                &operation,
                &envelope,
                &ExternalProgress {
                    nonce_initialized: true,
                    next_evm_nonce: 10,
                    ..ExternalProgress::default()
                },
            )
            .expect("atomically prepare operation");
        envelope.signed_transaction = Some(vec![1, 2, 3, 4]);
        store
            .put_evm_envelope(&envelope)
            .expect("persist original signed transaction");
        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [8; 32],
            })
            .expect("submit original transaction");
        store
            .put_evm_operation(&operation)
            .expect("persist submitted operation");

        let original_raw = envelope
            .signed_transaction
            .take()
            .expect("original signed transaction");
        envelope.prior_signed_transactions.push(original_raw);
        envelope.signed_transaction = Some(vec![5, 6, 7, 8]);
        envelope.max_fee_per_gas += 1;
        envelope.max_priority_fee_per_gas += 1;
        envelope.replacement_generation = 1;
        store
            .replace_submitted_evm_envelope(&envelope)
            .expect("write replacement before broadcast");
        drop(store);

        let mut reopened = StableStore::reopen(memory).expect("reopen pending replacement");
        assert_eq!(
            reopened
                .evm_operation(operation_id.get())
                .expect("read operation")
                .expect("operation")
                .state,
            EvmOperationState::Submitted {
                transaction_hash: [8; 32]
            }
        );
        assert_eq!(
            reopened
                .evm_envelope(operation_id.get())
                .expect("read pending envelope"),
            Some(envelope.clone())
        );

        operation.state = EvmOperationState::Submitted {
            transaction_hash: [9; 32],
        };
        envelope.last_broadcast_at_ns = 200;
        reopened
            .promote_submitted_evm_replacement_with_rpc_audit(
                &operation,
                &envelope,
                Principal::self_authenticating([14; 32]),
                200,
                vec![AuditEventKind::EvmTransactionReplaced {
                    operation_id: operation_id.get(),
                    previous_transaction_hash: vec![8; 32],
                    transaction_hash: vec![9; 32],
                    generation: 1,
                    max_fee_per_gas: envelope.max_fee_per_gas,
                    max_priority_fee_per_gas: envelope.max_priority_fee_per_gas,
                }],
            )
            .expect("promote replacement");
        assert_eq!(
            reopened
                .evm_operation(operation_id.get())
                .expect("read promoted operation"),
            Some(operation)
        );
        assert_eq!(
            reopened
                .evm_envelope(operation_id.get())
                .expect("read promoted envelope"),
            Some(envelope)
        );
        assert!(matches!(
            reopened
                .audit_events(0, 10)
                .expect("read replacement audit")
                .events
                .as_slice(),
            [AuditEvent {
                kind: AuditEventKind::EvmTransactionReplaced { .. },
                ..
            }]
        ));
    }

    #[test]
    #[serial]
    fn mint_reservation_is_released_by_final_revert_or_cancel() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut pending = deposit();
        store
            .put_deposit(&pending)
            .expect("reserve escrowed deposit");
        store.put_deposit(&pending).expect("replay reservation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            0
        );
        let operation_id = EvmOperationId::new(41);
        pending
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare mint");
        store
            .put_deposit(&pending)
            .expect("retain pending reservation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            100
        );
        pending
            .apply(DepositEvent::MintReverted { operation_id })
            .expect("terminal revert");
        store
            .put_deposit(&pending)
            .expect("release reverted reservation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            0
        );

        let mut minted = deposit();
        minted
            .apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare replacement fixture");
        minted
            .apply(DepositEvent::MintConfirmed { operation_id })
            .expect("confirm mint");
        store
            .put_deposit(&minted)
            .expect("release minted reservation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            0
        );
    }

    #[test]
    #[serial]
    fn definitive_pull_failure_releases_reservation_and_pending_counter() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut pending = deposit();
        pending.state = DepositState::FundingPending;
        store
            .put_deposit(&pending)
            .expect("reserve pending deposit");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            0
        );
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .pending_ledger_operations,
            1
        );
        pending
            .apply(DepositEvent::FundingFailed {
                code: bridge_core::LedgerFailure::BadFee {
                    expected_fee: Amount::new(2),
                },
            })
            .expect("cancel deposit");
        store.put_deposit(&pending).expect("persist cancellation");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            0
        );
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .pending_ledger_operations,
            0
        );
    }

    #[test]
    #[serial]
    fn compensated_refund_reserves_and_restores_fee_reserve_atomically() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        store
            .set_accounting(&AccountingState {
                fee_reserve: Amount::new(200),
                ..AccountingState::default()
            })
            .expect("seed reserve");
        let mut record = deposit();
        let mut identity = transfer(LedgerOperation::RefundDeposit, 100, 30);
        identity.fee = Amount::new(10);
        identity.from = record.transfer.to.clone();
        identity.to = record.transfer.from.clone();
        record
            .apply(DepositEvent::StartRefund {
                reason: bridge_core::DepositRefundReason::ReserveInsufficient,
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity: identity.clone(),
                }),
            })
            .expect("start refund");
        store.put_deposit(&record).expect("seed refund");
        record
            .apply(DepositEvent::RefundBadFee {
                expected_fee: Amount::new(110),
                next_identity: None,
            })
            .expect("recovery required");
        store.put_deposit(&record).expect("persist recovery");
        identity.created_at_time_ns += 1;
        identity.memo = [31; 32];
        identity.amount = record.gross_amount;
        identity.fee = Amount::new(110);
        record
            .apply(DepositEvent::ResumeRefund {
                identity: Box::new(identity),
            })
            .expect("compensate");
        store.put_deposit(&record).expect("reserve compensation");
        assert_eq!(
            store.accounting().expect("accounting").fee_reserve,
            Amount::new(90)
        );
        record
            .apply(DepositEvent::RefundBadFee {
                expected_fee: Amount::new(111),
                next_identity: None,
            })
            .expect("definitive failure");
        store.put_deposit(&record).expect("restore reserve");
        assert_eq!(
            store.accounting().expect("accounting").fee_reserve,
            Amount::new(200)
        );
    }

    #[test]
    #[serial]
    fn refund_retry_record_audit_and_job_are_fenced_in_one_transaction() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut record = deposit();
        let mut identity = transfer(LedgerOperation::RefundDeposit, 100, 30);
        identity.fee = Amount::new(10);
        identity.from = record.transfer.to.clone();
        identity.to = record.transfer.from.clone();
        record
            .apply(DepositEvent::StartRefund {
                reason: bridge_core::DepositRefundReason::ReserveInsufficient,
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity: identity.clone(),
                }),
            })
            .expect("start refund");
        store.put_deposit(&record).expect("seed refund");
        store
            .handle
            .update(|connection| {
                enqueue_settlement_job(
                    connection,
                    SettlementJobKind::Deposit,
                    record.id.bytes(),
                    None,
                    0,
                )
            })
            .expect("schedule refund");
        let SettlementJobClaim::Claimed(job) = store
            .claim_specific_due_settlement_job(
                SettlementJobKind::Deposit,
                record.id.bytes(),
                0,
                100,
            )
            .expect("claim refund")
        else {
            panic!("refund job was not claimed")
        };

        let expected_fee = Amount::new(11);
        identity.created_at_time_ns += 1;
        identity.memo = [31; 32];
        identity.amount = record
            .gross_amount
            .checked_sub(expected_fee)
            .expect("positive refund");
        identity.fee = expected_fee;
        let mut retry = record.clone();
        retry
            .apply(DepositEvent::RefundBadFee {
                expected_fee,
                next_identity: Some(Box::new(identity)),
            })
            .expect("build retry");
        let audit = AuditEventKind::DepositRefundRetried {
            deposit_id: retry.id.bytes().to_vec(),
            previous_attempt_no: 0,
            previous_fee: 10,
            next_attempt_no: Some(1),
            next_fee: expected_fee.get(),
            compensated: false,
        };

        let mut stale_job = job.clone();
        stale_job.lease_generation += 1;
        assert_eq!(
            store.put_deposit_refund_retry_bundle(
                &retry,
                Principal::anonymous(),
                audit.clone(),
                &stale_job,
                RefundJobOutcome::RetryAt(50),
                40,
            ),
            Err(StorageError::DatabaseFailure)
        );
        assert_eq!(
            store.deposit(record.id.bytes()).expect("record"),
            Some(record.clone())
        );
        assert_eq!(
            store
                .settlement_job(SettlementJobKind::Deposit, record.id.bytes())
                .expect("job")
                .expect("present")
                .status,
            SettlementJobStatus::Leased
        );

        store
            .put_deposit_refund_retry_bundle(
                &retry,
                Principal::anonymous(),
                audit,
                &job,
                RefundJobOutcome::RetryAt(50),
                40,
            )
            .expect("commit retry bundle");
        assert_eq!(
            store.deposit(record.id.bytes()).expect("record"),
            Some(retry)
        );
        let pending = store
            .settlement_job(SettlementJobKind::Deposit, record.id.bytes())
            .expect("job")
            .expect("present");
        assert_eq!(pending.status, SettlementJobStatus::Scheduled);
        assert_eq!(pending.next_run_at_ns, Some(50));
        assert_eq!(pending.confirmation_checks, 1);
    }

    #[test]
    #[serial]
    fn reverted_evm_counter_is_constant_time_and_idempotent() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut evm = EvmOperationRecord::queued(
            EvmOperationId::new(42),
            [4; 32],
            EvmOperationKind::MintDeposit,
        );
        store
            .put_evm_call_intent(&evm_intent(evm.id, evm.payload_hash))
            .expect("insert EVM intent");
        store.put_evm_operation(&evm).expect("insert pending");
        evm.state = EvmOperationState::Reverted {
            transaction_hash: [5; 32],
            receipt_block_number: 98,
            finalized_head_block_number: 99,
        };
        store.put_evm_operation(&evm).expect("mark reverted");
        store.put_evm_operation(&evm).expect("replay reverted");
        let counts = store.status_counts().expect("status");
        assert_eq!(counts.pending_evm_operations, 0);
        assert_eq!(counts.unresolved_evm_reverts, 1);
    }

    #[derive(Debug, PartialEq, Eq)]
    struct OperationBundleSnapshot {
        counters: CounterState,
        deposit: Option<DepositRecord>,
        operation: Option<EvmOperationRecord>,
        payload: Option<EvmExecutionPayload>,
        owner: Option<StableBlob>,
        evm_index_present: bool,
        pull_index_present: bool,
        counts: StorageCounts,
    }

    fn operation_bundle_snapshot(
        store: &StableStore,
        deposit_id: DepositId,
        operation: &EvmOperationRecord,
    ) -> OperationBundleSnapshot {
        let index_key = evm_state_index_key(operation)
            .expect("encode state index")
            .expect("queued operation is indexed");
        OperationBundleSnapshot {
            counters: store.counters().expect("read counters"),
            deposit: store.deposit(deposit_id.bytes()).expect("read deposit"),
            operation: store
                .evm_operation(operation.id.get())
                .expect("read operation"),
            payload: store
                .evm_execution_payload(operation.id.get())
                .expect("read payload"),
            owner: store.operation_owner_index.get(&operation.id.get()),
            evm_index_present: store.evm_state_index.get(&index_key).is_some(),
            pull_index_present: store
                .pull_pending_deposit_index
                .get(&deposit_id.bytes())
                .is_some(),
            counts: store.status_counts().expect("read counts"),
        }
    }

    #[test]
    #[serial]
    fn deposit_mint_bundle_rolls_back_every_write_failpoint() {
        for failpoint in [
            OperationBundleFailpoint::Encode,
            OperationBundleFailpoint::Parent,
            OperationBundleFailpoint::ParentIndex,
            OperationBundleFailpoint::ExecutionPayload,
            OperationBundleFailpoint::EvmOperation,
            OperationBundleFailpoint::EvmStateIndex,
            OperationBundleFailpoint::OperationOwnerIndex,
            OperationBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let previous = deposit();
            store.put_deposit(&previous).expect("seed escrowed deposit");
            let operation_id = store.next_evm_operation_id().expect("candidate");
            let mut next = previous.clone();
            next.apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare mint");
            let operation = EvmOperationRecord::queued(
                operation_id,
                next.payload_hash,
                EvmOperationKind::MintDeposit,
            );
            let intent = evm_intent(operation_id, next.payload_hash);
            let before = operation_bundle_snapshot(&store, next.id, &operation);

            set_operation_bundle_failpoint(Some(failpoint));
            assert!(store
                .commit_deposit_mint_bundle(&next, &operation, &intent)
                .is_err());
            set_operation_bundle_failpoint(None);
            assert_eq!(
                operation_bundle_snapshot(&store, next.id, &operation),
                before,
                "failpoint {failpoint:?} changed stable state"
            );

            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen after rollback");
            assert_eq!(
                operation_bundle_snapshot(&reopened, next.id, &operation),
                before,
                "failpoint {failpoint:?} changed reopened state"
            );
        }
    }

    #[test]
    #[serial]
    fn deposit_quote_is_recomputed_from_the_exact_cached_snapshot() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let refresh_generation = store
            .begin_base_snapshot_refresh(1, 10, 1)
            .expect("begin refresh")
            .expect("refresh owner");
        let snapshot = mint_snapshot();
        store
            .finish_base_snapshot_refresh(refresh_generation, 1, snapshot, [7; 20], false)
            .expect("cache snapshot");
        let previous = deposit();
        store.put_deposit(&previous).expect("seed deposit");
        let operation_id = store.next_evm_operation_id().expect("operation id");
        let operation = EvmOperationRecord::queued(
            operation_id,
            previous.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        let intent = evm_intent(operation_id, previous.payload_hash);
        let admission = DepositReserveAdmission {
            audit_caller: Principal::anonymous(),
            expected_token: store.deposit_reserve_token().expect("reserve token"),
            observed_at_ns: 2,
            eth_balance_wei: u128::MAX,
            cycles_balance: u128::MAX,
            reserve_policy: config().reserve_policy(),
            mint_snapshot: snapshot,
            snapshot_generation: refresh_generation,
        };

        for (quote, generation) in [
            (
                DepositQuote {
                    service_fee: Amount::new(9),
                    net_amount: Amount::new(101),
                },
                refresh_generation,
            ),
            (test_deposit_quote(), refresh_generation.saturating_add(1)),
        ] {
            let mut next = previous.clone();
            next.apply(DepositEvent::CommitQuote {
                quote,
                operation_id,
            })
            .expect("arithmetically valid quote");
            let before = operation_bundle_snapshot(&store, next.id, &operation);
            let mut candidate = admission;
            candidate.snapshot_generation = generation;
            assert_eq!(
                store.commit_deposit_mint_bundle_and_scan(
                    &next,
                    &operation,
                    &intent,
                    None,
                    Some(candidate),
                ),
                Err(StorageError::QuoteSnapshotMismatch)
            );
            assert_eq!(
                operation_bundle_snapshot(&store, next.id, &operation),
                before
            );
        }
    }

    #[test]
    #[serial]
    fn held_deposit_mint_bundle_rolls_back_hold_and_operation_together() {
        for failpoint in [
            OperationBundleFailpoint::Encode,
            OperationBundleFailpoint::Parent,
            OperationBundleFailpoint::ParentIndex,
            OperationBundleFailpoint::ReconciliationHold,
            OperationBundleFailpoint::OpenHoldIndex,
            OperationBundleFailpoint::ReconciliationScan,
            OperationBundleFailpoint::ExecutionPayload,
            OperationBundleFailpoint::EvmOperation,
            OperationBundleFailpoint::EvmStateIndex,
            OperationBundleFailpoint::OperationOwnerIndex,
            OperationBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let hold_id = HoldId::new(56);
            let mut previous = deposit();
            previous.state = DepositState::FundingPending;
            previous
                .apply(DepositEvent::FundingAmbiguous { hold_id })
                .expect("hold deposit");
            let previous_hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::DepositFunding(previous.id),
                previous.transfer.clone(),
            );
            store.put_deposit(&previous).expect("persist held deposit");
            store
                .put_open_reconciliation_hold(&previous_hold)
                .expect("persist open hold");
            let scan_target = ReconciliationTarget::Hold(hold_id);
            store
                .put_reconciliation_scan(&ReconciliationScanProgress::new(
                    scan_target.clone(),
                    previous_hold.transfer.clone(),
                ))
                .expect("scan");
            let operation_id = store.next_evm_operation_id().expect("candidate");
            let mut next = previous.clone();
            let mut next_hold = previous_hold.clone();
            resolve_deposit_hold(
                &mut next,
                &mut next_hold,
                DepositHoldResolution::FundingSucceeded {
                    ledger_block_index: 92,
                },
            )
            .expect("resolve hold in memory");
            next.apply(DepositEvent::CommitQuote {
                quote: test_deposit_quote(),
                operation_id,
            })
            .expect("prepare mint");
            let operation = EvmOperationRecord::queued(
                operation_id,
                next.payload_hash,
                EvmOperationKind::MintDeposit,
            );
            let intent = evm_intent(operation_id, next.payload_hash);
            let before = (
                operation_bundle_snapshot(&store, next.id, &operation),
                store.reconciliation_hold(hold_id.get()).expect("hold"),
                store.open_hold_index.get(&hold_id.get()).is_some(),
                store.reconciliation_scan(&scan_target).expect("scan"),
            );
            set_operation_bundle_failpoint(Some(failpoint));
            assert!(store
                .commit_deposit_mint_bundle_and_scan(
                    &next,
                    &operation,
                    &intent,
                    Some(&scan_target),
                    None,
                )
                .is_err());
            set_operation_bundle_failpoint(None);
            let after = (
                operation_bundle_snapshot(&store, next.id, &operation),
                store.reconciliation_hold(hold_id.get()).expect("hold"),
                store.open_hold_index.get(&hold_id.get()).is_some(),
                store.reconciliation_scan(&scan_target).expect("scan"),
            );
            assert_eq!(after, before, "failpoint {failpoint:?}");
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            let reopened_snapshot = (
                operation_bundle_snapshot(&reopened, next.id, &operation),
                reopened.reconciliation_hold(hold_id.get()).expect("hold"),
                reopened.open_hold_index.get(&hold_id.get()).is_some(),
                reopened.reconciliation_scan(&scan_target).expect("scan"),
            );
            assert_eq!(reopened_snapshot, before, "reopen failpoint {failpoint:?}");
        }
    }

    #[test]
    #[serial]
    fn operation_bundle_rejects_stale_candidate_without_writes() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let previous = deposit();
        store.put_deposit(&previous).expect("seed escrowed deposit");
        let stale_id = EvmOperationId::new(1);
        let mut next = previous.clone();
        next.apply(DepositEvent::CommitQuote {
            quote: test_deposit_quote(),
            operation_id: stale_id,
        })
        .expect("prepare mint");
        let operation =
            EvmOperationRecord::queued(stale_id, next.payload_hash, EvmOperationKind::MintDeposit);
        let intent = evm_intent(stale_id, next.payload_hash);
        let before = operation_bundle_snapshot(&store, next.id, &operation);
        assert!(store
            .commit_deposit_mint_bundle(&next, &operation, &intent)
            .is_err());
        assert_eq!(
            operation_bundle_snapshot(&store, next.id, &operation),
            before
        );
    }

    #[test]
    #[serial]
    fn recovery_bundle_rejects_a_reserve_token_changed_during_eth_observation() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init_configured(memory, &config()).expect("initialize");
        let (deposit_id, submitted) = submitted_mint_fixture(&mut store);
        let mut reverted = submitted;
        reverted
            .apply(EvmOperationEvent::Reverted {
                transaction_hash: [9; 32],
                receipt_block_number: 30,
                finalized_head_block_number: 40,
            })
            .expect("revert operation");
        let observation = FinalizedObservationRecord {
            chain_id: 8453,
            block_number: 40,
            block_hash: [40; 32],
            observed_at_ns: 40,
            bridge_signer: [7; 20],
            runtime_sha256: [8; 32],
        };
        let mut terminal_progress = store.external_progress().expect("progress");
        terminal_progress
            .observe_finalized(observation)
            .expect("observe revert");
        store
            .commit_evm_terminal_bundle(
                &reverted,
                &terminal_progress,
                Some((Principal::self_authenticating([8; 32]), 40, 40)),
            )
            .expect("commit revert");

        let previous = store
            .deposit(deposit_id.bytes())
            .expect("deposit")
            .expect("reverted deposit");
        let reverted = store
            .evm_operation(reverted.id.get())
            .expect("operation")
            .expect("reverted operation");
        let replacement_id = store.next_evm_operation_id().expect("replacement id");
        let mut next = previous.clone();
        next.apply(DepositEvent::RetryMint {
            reverted_operation_id: reverted.id,
            replacement_operation_id: replacement_id,
        })
        .expect("retry mint");
        let mut recovery_pending = reverted;
        recovery_pending
            .apply(EvmOperationEvent::StartRecovery {
                replacement_operation_id: replacement_id,
            })
            .expect("start recovery");
        let replacement = EvmOperationRecord::queued_recovery(
            replacement_id,
            next.payload_hash,
            EvmOperationKind::MintDeposit,
            reverted.id,
        );
        let intent = evm_intent(replacement_id, next.payload_hash);
        let expected_token = store.deposit_reserve_token().expect("reserve token");
        let mut interrupted = store.counters().expect("counters");
        interrupted.reserved_deposit_mint_operations += 1;
        store
            .set_counters(&interrupted)
            .expect("model competing reservation");
        let before = (
            store.counters().expect("before counters"),
            store.external_progress().expect("before progress"),
            store.deposit(deposit_id.bytes()).expect("before deposit"),
            store
                .evm_operation(replacement_id.get())
                .expect("before replacement"),
        );
        let result = store.commit_deposit_recovery_bundle(
            &previous,
            &next,
            &reverted,
            &recovery_pending,
            &replacement,
            &intent,
            DepositRecoveryAdmission {
                reserve: DepositReserveAdmission {
                    audit_caller: Principal::anonymous(),
                    expected_token,
                    observed_at_ns: 41,
                    eth_balance_wei: u128::MAX,
                    cycles_balance: u128::MAX,
                    reserve_policy: config().reserve_policy(),
                    mint_snapshot: mint_snapshot(),
                    snapshot_generation: 0,
                },
                finalized_observation: FinalizedObservationRecord {
                    block_number: 41,
                    block_hash: [41; 32],
                    observed_at_ns: 41,
                    ..observation
                },
            },
            RpcAuditBatch {
                caller: Principal::anonymous(),
                timestamp_ns: 41,
                kinds: vec![],
            },
        );
        assert_eq!(result, Err(StorageError::StaleReserveObservation));
        assert_eq!(
            (
                store.counters().expect("after counters"),
                store.external_progress().expect("after progress"),
                store.deposit(deposit_id.bytes()).expect("after deposit"),
                store
                    .evm_operation(replacement_id.get())
                    .expect("after replacement"),
            ),
            before
        );
    }

    #[derive(Debug, PartialEq, Eq)]
    struct TerminalBundleSnapshot {
        counters: CounterState,
        accounting: AccountingState,
        progress: ExternalProgress,
        deposit_admission: DepositAdmissionControl,
        deposit: Option<DepositRecord>,
        operation: Option<EvmOperationRecord>,
        owner_present: bool,
        evm_index_present: bool,
        schedule: Option<ConfirmationSchedule>,
        admin: AdminState,
        audit_count: u64,
        counts: StorageCounts,
    }

    fn terminal_bundle_snapshot(
        store: &StableStore,
        deposit_id: DepositId,
        operation_id: EvmOperationId,
    ) -> TerminalBundleSnapshot {
        let operation = store.evm_operation(operation_id.get()).expect("operation");
        let evm_index_present = operation
            .as_ref()
            .and_then(|operation| evm_state_index_key(operation).ok().flatten())
            .is_some_and(|key| store.evm_state_index.get(&key).is_some());
        TerminalBundleSnapshot {
            counters: store.counters().expect("counters"),
            accounting: store.accounting().expect("accounting"),
            progress: store.external_progress().expect("progress"),
            deposit_admission: store.deposit_admission().expect("deposit admission"),
            deposit: store.deposit(deposit_id.bytes()).expect("deposit"),
            operation,
            owner_present: store
                .operation_owner_index
                .get(&operation_id.get())
                .is_some(),
            evm_index_present,
            schedule: store
                .confirmation_schedule(operation_id.get())
                .expect("schedule"),
            admin: store.admin_state().expect("admin"),
            audit_count: store.audit_events.len(),
            counts: store.status_counts().expect("counts"),
        }
    }

    fn submitted_mint_fixture(store: &mut StableStore) -> (DepositId, EvmOperationRecord) {
        let previous = deposit();
        let deposit_id = previous.id;
        store.put_deposit(&previous).expect("seed deposit");
        let operation_id = store.next_evm_operation_id().expect("candidate");
        let mut next = previous;
        next.apply(DepositEvent::CommitQuote {
            quote: test_deposit_quote(),
            operation_id,
        })
        .expect("mint pending");
        let mut operation = EvmOperationRecord::queued(
            operation_id,
            next.payload_hash,
            EvmOperationKind::MintDeposit,
        );
        let intent = evm_intent(operation_id, next.payload_hash);
        store
            .commit_deposit_mint_bundle(&next, &operation, &intent)
            .expect("prepare bundle");
        let mut envelope = intent.assign_nonce(0);
        operation
            .apply(EvmOperationEvent::Prepared)
            .expect("prepared");
        let mut progress = store.external_progress().expect("progress");
        progress.next_evm_nonce = 1;
        store
            .prepare_evm_operation(&operation, &envelope, &progress)
            .expect("persist prepared");
        envelope.signed_transaction = Some(vec![1, 2, 3, 4]);
        store
            .put_evm_envelope(&envelope)
            .expect("persist signed transaction");
        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [9; 32],
            })
            .expect("submitted");
        store
            .put_submitted_evm_operation(&operation, 10)
            .expect("persist submitted");
        (deposit_id, operation)
    }

    #[test]
    #[serial]
    fn terminal_bundle_rolls_back_every_write_failpoint() {
        for failpoint in [
            TerminalBundleFailpoint::Parent,
            TerminalBundleFailpoint::ParentIndex,
            TerminalBundleFailpoint::EvmOperation,
            TerminalBundleFailpoint::EvmStateIndex,
            TerminalBundleFailpoint::OperationOwnerIndex,
            TerminalBundleFailpoint::ConfirmationSchedule,
            TerminalBundleFailpoint::Audit,
            TerminalBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            store.initialize_admin(&config()).expect("admin");
            let (deposit_id, submitted) = submitted_mint_fixture(&mut store);
            let mut terminal = submitted;
            terminal
                .apply(EvmOperationEvent::Reverted {
                    transaction_hash: [9; 32],
                    receipt_block_number: 30,
                    finalized_head_block_number: 40,
                })
                .expect("reverted");
            let mut progress = store.external_progress().expect("progress");
            progress.last_finalized_base_block = 40;
            let before = terminal_bundle_snapshot(&store, deposit_id, submitted.id);
            set_terminal_bundle_failpoint(Some(failpoint));
            assert!(store
                .commit_evm_terminal_bundle_with_rpc_audit(
                    &terminal,
                    &progress,
                    Some((Principal::self_authenticating([5; 32]), 50, 40)),
                    Principal::self_authenticating([5; 32]),
                    50,
                    vec![AuditEventKind::EvmRpcObservation {
                        evm_rpc_canister_id: Principal::self_authenticating([6; 32]),
                        call_method: "multi_request".into(),
                        request_digest: vec![7; 32],
                        quorum_response_digest: vec![8; 32],
                        finalized_block_number: 40,
                        finalized_block_hash: vec![9; 32],
                        transaction_hash: Some(vec![9; 32]),
                    }],
                )
                .is_err());
            set_terminal_bundle_failpoint(None);
            assert_eq!(
                terminal_bundle_snapshot(&store, deposit_id, submitted.id),
                before,
                "failpoint {failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                terminal_bundle_snapshot(&reopened, deposit_id, submitted.id),
                before,
                "reopen failpoint {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn confirmed_mint_reopens_snapshot_refresh_after_finalized_progress_advances() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize");
        store.initialize_admin(&config()).expect("admin");
        let refresh_owner = store
            .begin_base_snapshot_refresh(100, 300, 1_000)
            .expect("begin refresh")
            .expect("refresh owner");
        let mut snapshot = mint_snapshot();
        snapshot.finalized_head_block_number = 10;
        store
            .finish_base_snapshot_refresh(refresh_owner, 110, snapshot, [7; 20], false)
            .expect("finish refresh");
        assert!(store
            .begin_base_snapshot_refresh(120, 300, 1_000)
            .expect("cooldown check")
            .is_none());

        let (_, submitted) = submitted_mint_fixture(&mut store);
        let mut terminal = submitted;
        terminal
            .apply(EvmOperationEvent::Confirmed {
                transaction_hash: [9; 32],
                receipt_block_number: 30,
                finalized_head_block_number: 40,
            })
            .expect("confirm mint");
        let mut progress = store.external_progress().expect("progress");
        progress.last_finalized_base_block = 40;
        progress.last_finalized_mint_block = 40;
        store
            .commit_evm_terminal_bundle(&terminal, &progress, None)
            .expect("commit confirmation");

        assert!(store
            .begin_base_snapshot_refresh(120, 300, 1_000)
            .expect("refresh after finalized progress")
            .is_some());
        drop(store);
        let reopened = StableStore::reopen(memory).expect("reopen");
        assert_eq!(
            reopened
                .deposit_admission()
                .expect("reopened admission")
                .refresh_started_at_ns,
            Some(120)
        );
    }

    #[test]
    #[serial]
    fn resolved_deposit_and_hold_survive_reopen_and_retry_together() {
        let memory = VectorMemory::default();
        let (deposit, hold) = held_deposit();
        {
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            store.put_deposit(&deposit).expect("write deposit");
            store
                .put_open_reconciliation_hold(&hold)
                .expect("write hold");
            assert_eq!(
                store
                    .resolve_deposit_hold(
                        deposit.id,
                        hold.id,
                        DepositHoldResolution::FundingSucceeded {
                            ledger_block_index: 88,
                        },
                    )
                    .expect("resolve")
                    .outcome,
                ApplyOutcome::Applied
            );
            assert_eq!(store.table_count("open_hold_index"), 0);
        }

        let mut reopened = StableStore::reopen(memory).expect("reopen");
        assert!(matches!(
            reopened
                .deposit(deposit.id.bytes())
                .expect("read deposit")
                .expect("deposit exists")
                .state,
            DepositState::EscrowedUnquoted {
                ledger_block_index: 88
            }
        ));
        assert_eq!(
            reopened
                .reconciliation_hold(hold.id.get())
                .expect("read hold")
                .expect("hold exists")
                .state,
            ReconciliationHoldState::ResolvedSucceeded {
                ledger_block_index: 88
            }
        );
        assert_eq!(
            reopened
                .resolve_deposit_hold(
                    deposit.id,
                    hold.id,
                    DepositHoldResolution::FundingSucceeded {
                        ledger_block_index: 88,
                    },
                )
                .expect("retry resolution")
                .outcome,
            ApplyOutcome::Idempotent
        );
        assert_eq!(reopened.table_count("open_hold_index"), 0);
    }

    #[test]
    #[serial]
    fn withdrawal_and_hold_absence_resolution_is_persisted_together() {
        let memory = VectorMemory::default();
        let mut withdrawal = withdrawal();
        let hold_id = HoldId::new(50);
        withdrawal
            .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
            .expect("hold withdrawal");
        let hold_transfer = match &withdrawal.state {
            bridge_core::WithdrawalState::ReconciliationHold { attempt, .. } => {
                attempt.identity.clone()
            }
            _ => panic!("withdrawal must be held"),
        };
        let hold = ReconciliationHoldRecord::open(
            hold_id,
            RequestReference::Withdrawal(withdrawal.id),
            hold_transfer,
        );
        let mut store = StableStore::init(memory).expect("initialize");
        store.put_withdrawal(&withdrawal).expect("write withdrawal");
        store
            .put_open_reconciliation_hold(&hold)
            .expect("write hold");
        let mut next_identity = hold.transfer.clone();
        next_identity.created_at_time_ns += 1;
        next_identity.memo = [99; 32];
        let resolution = WithdrawalHoldResolution::Absent {
            history_watermark: 500,
            next_identity: Box::new(next_identity),
        };
        assert_eq!(
            store
                .resolve_withdrawal_hold(withdrawal.id, hold.id, resolution.clone())
                .expect("resolve absent")
                .outcome,
            ApplyOutcome::Applied
        );
        assert_eq!(
            store
                .resolve_withdrawal_hold(withdrawal.id, hold.id, resolution)
                .expect("retry absent")
                .outcome,
            ApplyOutcome::Idempotent
        );
        assert_eq!(store.table_count("open_hold_index"), 0);
    }

    #[test]
    #[serial]
    fn status_counts_do_not_decode_historical_records() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        for id in 0..100 {
            let mut evm = EvmOperationRecord::prepared(
                EvmOperationId::new(id),
                [1; 32],
                EvmOperationKind::MintDeposit,
            );
            evm.state = EvmOperationState::Confirmed {
                transaction_hash: [2; 32],
                receipt_block_number: id,
                finalized_head_block_number: id,
            };
            store.put_evm_operation(&evm).expect("write terminal EVM");
        }
        store.evm_operations.insert(
            200,
            StableBlob::new(vec![WIRE_VERSION, 0xff]).expect("bounded corruption"),
        );
        assert_eq!(
            store.status_counts().expect("constant-time counts"),
            StorageCounts {
                deposits: 0,
                withdrawals: 0,
                pending_evm_operations: 0,
                reconciliation_holds: 0,
                pending_ledger_operations: 0,
                reserved_deposit_mint_amount: 0,
                reserved_deposit_mint_operations: 0,
                unresolved_evm_reverts: 0,
                last_finalized_base_block: 0,
                active_evm_payloads: 0,
                retained_audit_events: 0,
                pruned_audit_events: 0,
                retained_deposit_index_entries: 0,
            }
        );
    }

    #[test]
    #[serial]
    fn base_snapshot_cache_is_bounded_by_ttl_progress_and_singleflight() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let snapshot = BaseMintSnapshot {
            finalized_head_block_number: 10,
            confirmed_block_timestamp: 10,
            service_fee: Amount::new(1),
            max_service_fee: Amount::new(2),
            per_deposit_limit: Amount::new(100),
            mint_window_limit: Amount::new(1_000),
            mint_window_started_at: 0,
            mint_window_duration: 100,
            minted_in_window: Amount::ZERO,
        };
        let refresh_owner = store
            .begin_base_snapshot_refresh(100, 300, 60)
            .expect("begin refresh")
            .expect("refresh owner");
        assert!(store
            .begin_base_snapshot_refresh(101, 300, 60)
            .expect("singleflight rejects overlap")
            .is_none());
        store
            .finish_base_snapshot_refresh(refresh_owner, 110, snapshot, [7; 20], false)
            .expect("cache snapshot");
        let cached = store
            .cached_base_mint_snapshot(160, 60, 10)
            .expect("fresh cache")
            .expect("cached snapshot");
        assert_eq!(cached.snapshot, snapshot);
        assert_eq!(cached.bridge_signer, [7; 20]);
        assert!(!cached.deposits_paused);
        assert_eq!(
            store
                .cached_base_mint_snapshot(171, 60, 10)
                .expect("expired cache"),
            None
        );
        assert_eq!(
            store
                .cached_base_mint_snapshot(120, 60, 11)
                .expect("progress-invalid cache"),
            None
        );
    }

    #[test]
    #[serial]
    fn stale_snapshot_worker_cannot_finish_or_release_a_new_owner() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let first = store
            .begin_base_snapshot_refresh(100, 300, 60)
            .expect("first begin")
            .expect("first owner");
        let second = store
            .begin_base_snapshot_refresh(500, 300, 60)
            .expect("take over stale lock")
            .expect("second owner");
        assert_ne!(first, second);
        assert_eq!(
            store.fail_base_snapshot_refresh(first),
            Err(StorageError::DatabaseFailure)
        );
        assert!(store
            .begin_base_snapshot_refresh(501, 300, 60)
            .expect("new owner remains locked")
            .is_none());
        store
            .fail_base_snapshot_refresh(second)
            .expect("current owner releases lock");
    }

    #[allow(clippy::type_complexity)]
    fn fee_payout_bundle_snapshot(
        store: &StableStore,
        payout_id: u64,
        target: &ReconciliationTarget,
    ) -> (
        Option<crate::admin::FeePayoutRecord>,
        CounterState,
        AccountingState,
        Option<ReconciliationScanProgress>,
        Option<u64>,
        AuditRetentionState,
        Vec<AuditEvent>,
        Option<SettlementJob>,
        [u64; 3],
    ) {
        (
            store.fee_payout(payout_id).expect("payout"),
            store.counters().expect("counters"),
            store.accounting().expect("accounting"),
            store.reconciliation_scan(target).expect("scan"),
            store.last_audit_sequence().expect("audit sequence"),
            decode(&store.audit_retention.get().expect("audit retention blob"))
                .expect("audit retention"),
            store.audit_events(0, 100).expect("audit events").events,
            store
                .settlement_job(SettlementJobKind::FeePayout, fee_payout_job_id(payout_id))
                .expect("fee payout job"),
            [
                store.table_count("fee_payouts"),
                store.table_count("reconciliation_scans"),
                store.table_count("audit_events"),
            ],
        )
    }

    fn fee_payout_fixture(store: &StableStore) -> crate::admin::FeePayoutRecord {
        crate::admin::FeePayoutRecord {
            id: store.next_fee_payout_id().expect("candidate payout id"),
            amount: 100,
            recipient: FeeRecipientConfig {
                owner: Principal::self_authenticating([7; 32]),
                subaccount: vec![],
            },
            transfer: transfer(LedgerOperation::FeePayout, 100, 30),
            state: crate::admin::FeePayoutState::Pending,
        }
    }

    #[test]
    #[serial]
    fn withdrawal_notification_index_commits_and_reopens_with_the_withdrawal() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize");
        let withdrawal = withdrawal();
        let transaction_hash = [0x5a; 32];
        store
            .commit_new_withdrawal_release_bundle_with_rpc_audit(
                &withdrawal,
                &ExternalProgress::default(),
                Principal::anonymous(),
                1,
                vec![],
                transaction_hash,
            )
            .expect("commit notification");
        assert_eq!(
            store
                .notified_withdrawal_id(transaction_hash)
                .expect("notification index"),
            Some(withdrawal.id.bytes())
        );
        drop(store);
        assert_eq!(
            StableStore::reopen(memory)
                .expect("reopen")
                .notified_withdrawal_id(transaction_hash)
                .expect("reopened notification index"),
            Some(withdrawal.id.bytes())
        );
    }

    #[test]
    #[serial]
    fn fee_payout_request_rolls_back_every_write_failpoint() {
        for failpoint in [
            FeePayoutBundleFailpoint::Encode,
            FeePayoutBundleFailpoint::Record,
            FeePayoutBundleFailpoint::Job,
            FeePayoutBundleFailpoint::Audit,
            FeePayoutBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let payout = fee_payout_fixture(&store);
            let target = ReconciliationTarget::FeePayout(payout.id);
            let before = fee_payout_bundle_snapshot(&store, payout.id, &target);
            set_fee_payout_bundle_failpoint(Some(failpoint));
            assert!(store
                .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
                .is_err());
            set_fee_payout_bundle_failpoint(None);
            assert_eq!(
                fee_payout_bundle_snapshot(&store, payout.id, &target),
                before,
                "request failpoint {failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                fee_payout_bundle_snapshot(&reopened, payout.id, &target),
                before,
                "reopened request failpoint {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn fee_payout_request_persists_a_canonical_durable_job() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize");
        let payout = fee_payout_fixture(&store);
        store
            .commit_fee_payout_request(&payout, Principal::anonymous(), 7)
            .expect("commit payout request");
        let key = fee_payout_job_id(payout.id);
        assert_eq!(fee_payout_id_from_job(key), Ok(payout.id));
        assert_eq!(
            store
                .settlement_job(SettlementJobKind::FeePayout, key)
                .expect("read payout job")
                .expect("payout job")
                .status,
            SettlementJobStatus::Scheduled
        );
        drop(store);
        assert!(StableStore::reopen(memory)
            .expect("reopen")
            .settlement_job(SettlementJobKind::FeePayout, key)
            .expect("reopened payout job")
            .is_some());
    }

    #[test]
    #[serial]
    fn terminal_fee_payout_cleanup_deletes_the_job_and_fences_stale_leases() {
        for start_in_hold in [false, true] {
            let mut store = StableStore::init(VectorMemory::default()).expect("initialize storage");
            let payout = fee_payout_fixture(&store);
            let key = fee_payout_job_id(payout.id);
            store
                .commit_fee_payout_request(&payout, Principal::anonymous(), 7)
                .expect("commit payout request");
            if start_in_hold {
                store.hold_fee_payout(payout.id).expect("hold payout");
            }
            let SettlementJobClaim::Claimed(first) = store
                .claim_due_settlement_job(7, 10, u64::MAX)
                .expect("claim payout job")
            else {
                panic!("payout job was not claimed")
            };
            let current = if start_in_hold {
                first.clone()
            } else {
                let SettlementJobClaim::Claimed(reclaimed) = store
                    .claim_due_settlement_job(10, 20, u64::MAX)
                    .expect("reclaim expired payout lease")
                else {
                    panic!("expired payout job was not reclaimed")
                };
                assert!(reclaimed.lease_generation > first.lease_generation);
                reclaimed
            };

            store
                .complete_fee_payout_failure(payout.id)
                .expect("fail payout");
            if !start_in_hold {
                store
                    .finish_settlement_job(&first, None, 0, None, None, 11)
                    .expect("ignore stale terminal cleanup");
                assert_eq!(
                    store
                        .settlement_job(SettlementJobKind::FeePayout, key)
                        .expect("read current job")
                        .expect("current job retained")
                        .lease_generation,
                    current.lease_generation
                );
            }
            store
                .finish_settlement_job(&current, None, 0, None, None, 12)
                .expect("delete terminal payout job");

            assert!(matches!(
                store
                    .fee_payout(payout.id)
                    .expect("read payout")
                    .expect("payout record")
                    .state,
                crate::admin::FeePayoutState::Failed
            ));
            assert!(store
                .settlement_job(SettlementJobKind::FeePayout, key)
                .expect("read deleted job")
                .is_none());
            assert_eq!(
                store
                    .settlement_job_summary(12, 0)
                    .expect("job summary")
                    .stopped,
                0
            );
        }
    }

    #[test]
    #[serial]
    fn fee_payout_success_and_scan_roll_back_every_write_failpoint() {
        for failpoint in [
            FeePayoutBundleFailpoint::Encode,
            FeePayoutBundleFailpoint::Record,
            FeePayoutBundleFailpoint::ReconciliationScan,
            FeePayoutBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            store
                .set_accounting(&AccountingState {
                    fee_reserve: Amount::new(101),
                    ..AccountingState::default()
                })
                .expect("seed reserve");
            let payout = fee_payout_fixture(&store);
            store
                .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
                .expect("request");
            store.hold_fee_payout(payout.id).expect("hold");
            let target = ReconciliationTarget::FeePayout(payout.id);
            let progress = ReconciliationScanProgress::new(target.clone(), payout.transfer.clone());
            store.commit_fee_payout_scan(&progress).expect("scan");
            let before = fee_payout_bundle_snapshot(&store, payout.id, &target);
            set_fee_payout_bundle_failpoint(Some(failpoint));
            assert!(store
                .complete_fee_payout_success_and_scan(payout.id, 8, &target)
                .is_err());
            set_fee_payout_bundle_failpoint(None);
            assert_eq!(
                fee_payout_bundle_snapshot(&store, payout.id, &target),
                before,
                "success failpoint {failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                fee_payout_bundle_snapshot(&reopened, payout.id, &target),
                before,
                "reopened success failpoint {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn fee_payout_failure_and_scan_roll_back_every_write_failpoint() {
        for failpoint in [
            FeePayoutBundleFailpoint::Encode,
            FeePayoutBundleFailpoint::Record,
            FeePayoutBundleFailpoint::ReconciliationScan,
            FeePayoutBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let payout = fee_payout_fixture(&store);
            store
                .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
                .expect("request");
            store.hold_fee_payout(payout.id).expect("hold");
            let target = ReconciliationTarget::FeePayout(payout.id);
            store
                .commit_fee_payout_scan(&ReconciliationScanProgress::new(
                    target.clone(),
                    payout.transfer.clone(),
                ))
                .expect("scan");
            let before = fee_payout_bundle_snapshot(&store, payout.id, &target);
            set_fee_payout_bundle_failpoint(Some(failpoint));
            assert!(store
                .complete_fee_payout_failure_and_scan(payout.id, &target)
                .is_err());
            set_fee_payout_bundle_failpoint(None);
            assert_eq!(
                fee_payout_bundle_snapshot(&store, payout.id, &target),
                before
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                fee_payout_bundle_snapshot(&reopened, payout.id, &target),
                before,
                "reopened failure failpoint {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn fee_payout_scan_creation_rolls_back_before_and_after_write() {
        for failpoint in [
            FeePayoutBundleFailpoint::Encode,
            FeePayoutBundleFailpoint::ReconciliationScan,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let payout = fee_payout_fixture(&store);
            store
                .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
                .expect("request");
            store.hold_fee_payout(payout.id).expect("hold");
            let target = ReconciliationTarget::FeePayout(payout.id);
            let progress = ReconciliationScanProgress::new(target.clone(), payout.transfer.clone());
            let before = fee_payout_bundle_snapshot(&store, payout.id, &target);
            set_fee_payout_bundle_failpoint(Some(failpoint));
            assert!(store.commit_fee_payout_scan(&progress).is_err());
            set_fee_payout_bundle_failpoint(None);
            assert_eq!(
                fee_payout_bundle_snapshot(&store, payout.id, &target),
                before,
                "scan failpoint {failpoint:?}"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                fee_payout_bundle_snapshot(&reopened, payout.id, &target),
                before,
                "reopened scan failpoint {failpoint:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn every_fee_payout_transition_rolls_back_every_write_failpoint() {
        for transition in [
            FeePayoutTransition::Hold,
            FeePayoutTransition::Failed,
            FeePayoutTransition::Succeeded { block_index: 8 },
        ] {
            for failpoint in [
                FeePayoutBundleFailpoint::Encode,
                FeePayoutBundleFailpoint::Record,
                FeePayoutBundleFailpoint::ReconciliationScan,
                FeePayoutBundleFailpoint::SingletonState,
            ] {
                let memory = VectorMemory::default();
                let mut store = StableStore::init(memory.clone()).expect("initialize");
                store
                    .set_accounting(&AccountingState {
                        fee_reserve: Amount::new(101),
                        ..AccountingState::default()
                    })
                    .expect("seed reserve");
                let payout = fee_payout_fixture(&store);
                store
                    .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
                    .expect("request");
                let target = ReconciliationTarget::FeePayout(payout.id);
                let before = fee_payout_bundle_snapshot(&store, payout.id, &target);
                set_fee_payout_bundle_failpoint(Some(failpoint));
                assert!(store
                    .transition_fee_payout(payout.id, transition, None)
                    .is_err());
                set_fee_payout_bundle_failpoint(None);
                assert_eq!(
                    fee_payout_bundle_snapshot(&store, payout.id, &target),
                    before,
                    "transition {transition:?} failpoint {failpoint:?}"
                );
                drop(store);
                let reopened = StableStore::reopen(memory).expect("reopen");
                assert_eq!(
                    fee_payout_bundle_snapshot(&reopened, payout.id, &target),
                    before,
                    "reopened transition {transition:?} failpoint {failpoint:?}"
                );
            }
        }
    }

    #[test]
    #[serial]
    fn fee_payout_success_debits_once_and_removes_reconciliation_reservation() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        store
            .set_accounting(&AccountingState {
                fee_reserve: Amount::new(101),
                ..AccountingState::default()
            })
            .expect("seed reserve");
        let payout = fee_payout_fixture(&store);
        store
            .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
            .expect("insert payout");
        assert_eq!(store.pending_fee_payout_amount().expect("pending"), 101);
        store.hold_fee_payout(payout.id).expect("hold payout");
        let target = ReconciliationTarget::FeePayout(payout.id);
        store
            .commit_fee_payout_scan(&ReconciliationScanProgress::new(
                target.clone(),
                payout.transfer.clone(),
            ))
            .expect("persist reconciliation scan");
        store
            .complete_fee_payout_success_and_scan(payout.id, 8, &target)
            .expect("complete payout");
        store
            .complete_fee_payout_success(payout.id, 8)
            .expect("idempotent replay");
        assert_eq!(
            store.accounting().expect("accounting").fee_reserve,
            Amount::ZERO
        );
        assert_eq!(store.pending_fee_payout_amount().expect("pending"), 0);
        assert_eq!(store.reconciliation_scan(&target).expect("scan"), None);
        assert_eq!(
            store.complete_fee_payout_success(payout.id, 9),
            Err(StorageError::Core(CoreError::ConflictingReplay))
        );
    }

    #[test]
    #[serial]
    fn fee_payout_failure_releases_pending_debit_and_scan_without_spending_reserve() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        store
            .set_accounting(&AccountingState {
                fee_reserve: Amount::new(101),
                ..AccountingState::default()
            })
            .expect("seed reserve");
        let payout = fee_payout_fixture(&store);
        store
            .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
            .expect("request");
        store.hold_fee_payout(payout.id).expect("hold payout");
        let target = ReconciliationTarget::FeePayout(payout.id);
        store
            .commit_fee_payout_scan(&ReconciliationScanProgress::new(
                target.clone(),
                payout.transfer.clone(),
            ))
            .expect("scan");
        store
            .complete_fee_payout_failure_and_scan(payout.id, &target)
            .expect("fail payout");
        assert_eq!(store.pending_fee_payout_amount().expect("pending"), 0);
        assert_eq!(
            store.accounting().expect("accounting").fee_reserve,
            Amount::new(101)
        );
        assert_eq!(store.reconciliation_scan(&target).expect("scan"), None);
        assert_eq!(
            store
                .fee_payout(payout.id)
                .expect("payout")
                .expect("record")
                .state,
            crate::admin::FeePayoutState::Failed
        );
    }

    #[test]
    #[serial]
    fn stale_fee_payout_scan_progress_cannot_recreate_scan_after_terminal_commit() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        store
            .set_accounting(&AccountingState {
                fee_reserve: Amount::new(101),
                ..AccountingState::default()
            })
            .expect("seed reserve");
        let payout = fee_payout_fixture(&store);
        store
            .commit_fee_payout_request(&payout, Principal::anonymous(), 1)
            .expect("request");
        store.hold_fee_payout(payout.id).expect("hold payout");
        let target = ReconciliationTarget::FeePayout(payout.id);
        let previous = ReconciliationScanProgress::new(target.clone(), payout.transfer.clone());
        store.commit_fee_payout_scan(&previous).expect("scan");
        let mut stale_progress = previous.clone();
        stale_progress.phase = ReconciliationScanPhase::Index {
            ledger_watermark: 10,
            index_watermark: Some(9),
            next_start: Some(4),
        };
        store
            .complete_fee_payout_success_and_scan(payout.id, 8, &target)
            .expect("terminal commit");
        assert_eq!(
            store.update_fee_payout_scan(&previous, &stale_progress),
            Err(StorageError::Core(CoreError::ConflictingReplay))
        );
        assert_eq!(store.reconciliation_scan(&target).expect("scan"), None);
        assert_eq!(store.table_count("reconciliation_scans"), 0);
    }

    #[test]
    #[serial]
    fn reopen_fails_closed_for_empty_legacy_unknown_and_missing_state() {
        let empty = VectorMemory::default();
        assert_eq!(
            StableStore::reopen(empty).err(),
            Some(StorageError::DatabaseFailure)
        );

        let legacy = VectorMemory::default();
        reset_sqlite_test_runtime();
        let manager = MemoryManager::init_strict(legacy.clone()).expect("legacy memory manager");
        DbHandle::init(manager.get(MemoryId::new(0))).expect("legacy slot");
        assert_eq!(
            StableStore::reopen(legacy).err(),
            Some(StorageError::DatabaseFailure)
        );

        let unknown = VectorMemory::default();
        let store = StableStore::init(unknown.clone()).expect("initialize unknown fixture");
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE bridge_metadata SET application_schema_version = ?1 WHERE id = 1",
                    params![i64::from(SCHEMA_VERSION.saturating_add(1))],
                )
            })
            .expect("corrupt schema");
        drop(store);
        assert_eq!(
            StableStore::reopen(unknown).err(),
            Some(StorageError::UnsupportedSchemaVersion(
                SCHEMA_VERSION.saturating_add(1)
            ))
        );

        let missing = VectorMemory::default();
        let store = StableStore::init(missing.clone()).expect("initialize missing fixture");
        store
            .handle
            .update(|connection| connection.execute("DELETE FROM bridge_metadata", params![]))
            .expect("delete metadata");
        drop(store);
        assert_eq!(
            StableStore::reopen(missing).err(),
            Some(StorageError::DatabaseFailure)
        );
    }

    #[test]
    #[serial]
    fn reopen_rejects_malformed_singleton_cbor() {
        let memory = VectorMemory::default();
        let store = StableStore::init(memory.clone()).expect("initialize malformed fixture");
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                    params![vec![WIRE_VERSION, 0xff]],
                )
            })
            .expect("corrupt counters");
        drop(store);
        assert_eq!(
            StableStore::reopen(memory).err(),
            Some(StorageError::DecodeFailed)
        );
    }

    #[test]
    #[serial]
    fn reopen_rejects_cross_table_count_index_and_counter_drift() {
        let count_memory = VectorMemory::default();
        let mut count_store = StableStore::init(count_memory.clone()).expect("initialize count");
        count_store.put_deposit(&deposit()).expect("seed deposit");
        count_store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE table_counts SET count = ?1 WHERE name = 'deposits'",
                    params![0u64.to_sql_bytes()],
                )
            })
            .expect("corrupt count");
        drop(count_store);
        let reopened = StableStore::reopen(count_memory).expect("bounded reopen ignores rows");
        assert_eq!(
            reopened.validate_relations().err(),
            Some(StorageError::DatabaseFailure)
        );

        let index_memory = VectorMemory::default();
        let mut index_store = StableStore::init(index_memory.clone()).expect("initialize index");
        let mut pending = deposit();
        pending.state = DepositState::FundingPending;
        index_store
            .put_deposit(&pending)
            .expect("seed pending deposit");
        index_store
            .handle
            .update(|connection| {
                connection.execute(
                    "DELETE FROM pull_pending_deposit_index WHERE key = ?1",
                    params![pending.id.bytes().to_sql_bytes()],
                )?;
                decrement_table_count(connection, "pull_pending_deposit_index")
            })
            .expect("corrupt index");
        drop(index_store);
        let reopened = StableStore::reopen(index_memory).expect("bounded reopen ignores rows");
        assert_eq!(
            reopened.validate_relations().err(),
            Some(StorageError::DatabaseFailure)
        );

        let counter_memory = VectorMemory::default();
        let mut counter_store =
            StableStore::init(counter_memory.clone()).expect("initialize counter");
        counter_store.put_deposit(&deposit()).expect("seed deposit");
        let mut counters = counter_store.counters().expect("counters");
        counters.reserved_deposit_mint_operations = 1;
        let corrupt = encode(&counters).expect("encode corrupt counters");
        counter_store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                    params![corrupt.to_sql_bytes()],
                )
            })
            .expect("corrupt counter");
        drop(counter_store);
        let reopened = StableStore::reopen(counter_memory).expect("bounded reopen ignores rows");
        assert_eq!(
            reopened.validate_relations().err(),
            Some(StorageError::DatabaseFailure)
        );
    }

    #[test]
    #[serial]
    fn reopen_rejects_same_count_but_wrong_evm_state_index() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize EVM index fixture");
        let (operation, _) = seed_queued_evm_operation(&mut store);
        let correct = evm_state_index_key(&operation)
            .expect("derive queued index")
            .expect("queued operation is indexed");
        let wrong = evm_state_index_key(&EvmOperationRecord::prepared(
            operation.id,
            operation.payload_hash,
            operation.kind,
        ))
        .expect("derive prepared index")
        .expect("prepared operation is indexed");
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "DELETE FROM evm_state_index WHERE key = ?1",
                    params![correct.to_sql_bytes()],
                )?;
                connection.execute(
                    "INSERT INTO evm_state_index(key, value) VALUES (?1, ?2)",
                    params![wrong.to_sql_bytes(), 0u8.to_sql_bytes()],
                )
            })
            .expect("replace index without changing its count");
        drop(store);

        let reopened = StableStore::reopen(memory).expect("bounded reopen ignores rows");
        assert_eq!(
            reopened.validate_relations().err(),
            Some(StorageError::DatabaseFailure)
        );
    }

    #[test]
    #[serial]
    fn reopen_rejects_orphan_execution_payload_with_consistent_table_count() {
        let memory = VectorMemory::default();
        let store = StableStore::init(memory.clone()).expect("initialize orphan payload fixture");
        let orphan_id = EvmOperationId::new(91);
        let payload = encode(&EvmExecutionPayload::AwaitingNonce(evm_intent(
            orphan_id, [92; 32],
        )))
        .expect("encode orphan payload");
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "INSERT INTO evm_execution_payloads(key, value) VALUES (?1, ?2)",
                    params![orphan_id.get().to_sql_bytes(), payload.to_sql_bytes()],
                )?;
                increment_table_count(connection, "evm_execution_payloads")
            })
            .expect("insert count-consistent orphan payload");
        drop(store);

        let reopened = StableStore::reopen(memory).expect("bounded reopen ignores rows");
        assert_eq!(
            reopened.validate_relations().err(),
            Some(StorageError::DatabaseFailure)
        );
    }

    #[test]
    #[serial]
    fn reopen_rejects_execution_payload_hash_mismatch() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize payload hash fixture");
        let (operation, mut intent) = seed_queued_evm_operation(&mut store);
        intent.payload_hash = [93; 32];
        let mismatched =
            encode(&EvmExecutionPayload::AwaitingNonce(intent)).expect("encode mismatched payload");
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE evm_execution_payloads SET value = ?1 WHERE key = ?2",
                    params![mismatched.to_sql_bytes(), operation.id.get().to_sql_bytes()],
                )
            })
            .expect("corrupt payload hash");
        drop(store);

        let reopened = StableStore::reopen(memory).expect("bounded reopen ignores rows");
        assert_eq!(
            reopened.validate_relations().err(),
            Some(StorageError::DatabaseFailure)
        );
    }

    #[test]
    #[serial]
    fn reopen_rejects_execution_payload_retained_after_submission() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize submitted fixture");
        let (mut operation, _) = seed_queued_evm_operation(&mut store);
        let previous_index = evm_state_index_key(&operation)
            .expect("derive queued index")
            .expect("queued operation is indexed");
        operation
            .apply(EvmOperationEvent::Prepared)
            .expect("prepare operation in memory");
        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [94; 32],
            })
            .expect("submit operation in memory");
        let submitted_index = evm_state_index_key(&operation)
            .expect("derive submitted index")
            .expect("submitted operation is indexed");
        let operation_blob = encode(&operation).expect("encode submitted operation");
        let mut counters = store.counters().expect("read counters");
        counters.awaiting_nonce_evm_operations = 0;
        let counters_blob = encode(&counters).expect("encode counters");
        store
            .handle
            .update(|connection| {
                connection.execute(
                    "UPDATE evm_operations SET value = ?1 WHERE key = ?2",
                    params![
                        operation_blob.to_sql_bytes(),
                        operation.id.get().to_sql_bytes()
                    ],
                )?;
                connection.execute(
                    "DELETE FROM evm_state_index WHERE key = ?1",
                    params![previous_index.to_sql_bytes()],
                )?;
                connection.execute(
                    "INSERT INTO evm_state_index(key, value) VALUES (?1, ?2)",
                    params![submitted_index.to_sql_bytes(), 0u8.to_sql_bytes()],
                )?;
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                    params![counters_blob.to_sql_bytes()],
                )
            })
            .expect("retain payload while otherwise committing submitted state");
        drop(store);

        let reopened = StableStore::reopen(memory).expect("bounded reopen ignores rows");
        assert_eq!(
            reopened.validate_relations().err(),
            Some(StorageError::DatabaseFailure)
        );
    }

    #[test]
    #[serial]
    fn lookup_queries_use_primary_key_indexes() {
        let memory = VectorMemory::default();
        let store = StableStore::init(memory).expect("initialize query plan fixture");
        let plans = [
            (
                "EXPLAIN QUERY PLAN SELECT key, value FROM deposit_owner_index \
                 WHERE key >= ?1 AND key < ?2 ORDER BY key",
                vec![vec![0], vec![255]],
            ),
            (
                "EXPLAIN QUERY PLAN SELECT value FROM evm_state_index \
                 WHERE key >= ?1 AND key < ?2 ORDER BY key LIMIT 1",
                vec![vec![0], vec![1]],
            ),
            (
                "EXPLAIN QUERY PLAN SELECT value FROM pull_pending_deposit_index WHERE key = ?1",
                vec![vec![0; 32]],
            ),
            (
                "EXPLAIN QUERY PLAN SELECT value FROM operation_owner_index WHERE key = ?1",
                vec![0u64.to_be_bytes().to_vec()],
            ),
        ];
        for (sql, values) in plans {
            let parameters: Vec<&dyn ic_sqlite_vfs::db::ToSql> = values
                .iter()
                .map(|value| value as &dyn ic_sqlite_vfs::db::ToSql)
                .collect();
            let details = store
                .handle
                .query(|connection| {
                    connection.query_all(sql, &parameters, |row| row.get::<String>(3))
                })
                .expect("explain query plan");
            assert!(
                details.iter().any(|detail| {
                    let detail = detail.to_ascii_uppercase();
                    detail.contains("PRIMARY KEY") || detail.contains("INDEX")
                }),
                "query must not scan the full table: {details:?}"
            );
        }
    }

    #[test]
    #[serial]
    fn fee_recipient_rotation_preserves_accounting_audit_and_reopen() {
        let memory = VectorMemory::default();
        let initial = config();
        let mut store =
            StableStore::init_configured(memory.clone(), &initial).expect("initialize configured");
        let before = store.accounting().expect("accounting");
        let next = FeeRecipientConfig {
            owner: Principal::self_authenticating([42; 32]),
            subaccount: vec![9; 32],
        };
        store
            .rotate_fee_recipient_with_audit(
                next.clone(),
                Principal::self_authenticating([7; 32]),
                99,
                vec![1; 32],
                vec![2; 32],
            )
            .expect("rotate recipient");

        assert_eq!(store.admin_state().expect("admin").fee_recipient, next);
        assert_eq!(
            store
                .config()
                .expect("materialized config")
                .unwrap()
                .fee_recipient,
            next
        );
        assert_eq!(store.accounting().expect("accounting"), before);
        assert!(matches!(
            store.audit_events(0, 10).expect("audit").events[0].kind,
            AuditEventKind::FeeRecipientRotated { .. }
        ));
        drop(store);

        let reopened = StableStore::reopen(memory).expect("reopen");
        assert_eq!(reopened.admin_state().expect("admin").fee_recipient, next);
        assert_eq!(
            reopened
                .config()
                .expect("reopened materialized config")
                .unwrap()
                .fee_recipient,
            next
        );
        assert_eq!(reopened.accounting().expect("accounting"), before);
    }

    #[test]
    #[serial]
    fn reserved_memory_ids_are_never_reassigned() {
        assert_eq!(RETIRED_STABLE_STRUCTURE_MEMORY_IDS, 0..=32);
        assert_eq!(SQLITE_MEMORY_ID, MemoryId::new(120));
    }
}
