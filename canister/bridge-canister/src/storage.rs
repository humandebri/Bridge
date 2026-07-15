use crate::config::BridgeInitArgs;
use crate::{admin::AdminState, config::FeeRecipientConfig};
use bridge_core::{
    resolve_deposit_hold, resolve_withdrawal_hold, AccountingState, ApplyResult, BaseMintSnapshot,
    CoreError, DepositHoldResolution, DepositId, DepositRecord, EvmCallIntent, EvmOperationEvent,
    EvmOperationKind, EvmOperationRecord, EvmOperationState, EvmTransactionEnvelope,
    ExternalProgress, FeeKind, HoldId, ReconciliationHoldRecord, ReconciliationHoldState,
    ReconciliationScanProgress, ReconciliationTarget, WithdrawalEvent, WithdrawalHoldResolution,
    WithdrawalId, WithdrawalRecord, WithdrawalState,
};
use candid::{CandidType, Principal};
use ic_sqlite_vfs::db::migrate::Migration;
use ic_sqlite_vfs::{params, DbError, DbHandle, DefaultMemoryImpl, MemoryId, MemoryManager};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, io::Cursor, marker::PhantomData, ops::Bound as RangeBound, ops::RangeBounds};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum AcknowledgementBundleFailpoint {
    Encode,
    ExecutionPayload,
    EvmOperation,
    EvmStateIndex,
    OperationOwnerIndex,
    Withdrawal,
    ReleasePendingIndex,
    ReconciliationHold,
    OpenHoldIndex,
    ReconciliationScan,
    SingletonState,
}

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
    StateIndex,
    ReconciliationScan,
    Audit,
    SingletonState,
}

#[cfg(test)]
thread_local! {
    static ACKNOWLEDGEMENT_BUNDLE_FAILPOINT: std::cell::Cell<Option<AcknowledgementBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static OPERATION_BUNDLE_FAILPOINT: std::cell::Cell<Option<OperationBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static TERMINAL_BUNDLE_FAILPOINT: std::cell::Cell<Option<TerminalBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static HOLD_BUNDLE_FAILPOINT: std::cell::Cell<Option<HoldBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static RESOLVE_HOLD_BUNDLE_FAILPOINT: std::cell::Cell<Option<ResolveHoldBundleFailpoint>> = const { std::cell::Cell::new(None) };
    static FEE_PAYOUT_BUNDLE_FAILPOINT: std::cell::Cell<Option<FeePayoutBundleFailpoint>> = const { std::cell::Cell::new(None) };
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
fn set_acknowledgement_bundle_failpoint(value: Option<AcknowledgementBundleFailpoint>) {
    ACKNOWLEDGEMENT_BUNDLE_FAILPOINT.with(|slot| slot.set(value));
}

fn acknowledgement_bundle_storage_failpoint(
    point: AcknowledgementBundleFailpoint,
) -> Result<(), StorageError> {
    #[cfg(test)]
    {
        if ACKNOWLEDGEMENT_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
            return Err(StorageError::EncodeFailed);
        }
    }
    let _ = point;
    Ok(())
}

fn acknowledgement_bundle_db_failpoint(
    point: AcknowledgementBundleFailpoint,
) -> Result<(), DbError> {
    #[cfg(test)]
    {
        if ACKNOWLEDGEMENT_BUNDLE_FAILPOINT.with(|slot| slot.get()) == Some(point) {
            return Err(DbError::Constraint(
                "test acknowledgement bundle failpoint".into(),
            ));
        }
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

pub const SCHEMA_VERSION: u16 = 6;
const WIRE_VERSION: u8 = 6;
const MAX_STABLE_VALUE_BYTES: usize = 16 * 1024;
const MAX_AUDIT_EVENTS: u64 = 10_000;
const MAX_OWNER_DEPOSIT_INDEX_ENTRIES: usize = 100;
const AUDIT_DIGEST_DOMAIN: &[u8] = b"KINIC_BRIDGE_AUDIT_V1";

pub const RETIRED_STABLE_STRUCTURE_MEMORY_IDS: core::ops::RangeInclusive<u8> = 0..=32;
pub const SQLITE_MEMORY_ID: MemoryId = MemoryId::new(120);

const SQLITE_SCHEMA: &str = r#"
CREATE TABLE bridge_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    application_schema_version INTEGER NOT NULL,
    record_wire_version INTEGER NOT NULL
) STRICT;
INSERT INTO bridge_metadata VALUES (1, 6, 6);

CREATE TABLE singleton_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    schema BLOB NOT NULL,
    accounting BLOB NOT NULL,
    counters BLOB NOT NULL,
    external_progress BLOB NOT NULL,
    config BLOB NOT NULL,
    admin_state BLOB NOT NULL,
    deposit_admission BLOB NOT NULL,
    withdrawal_attempt_control BLOB NOT NULL,
    audit_retention BLOB NOT NULL,
    settlement_admission BLOB NOT NULL,
    confirmation_scheduler_health BLOB NOT NULL
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
CREATE TABLE deposit_intents (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE audit_events (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE fee_payouts (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE deposit_owner_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE fee_payout_state_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE operation_owner_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE evm_state_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE pull_pending_deposit_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE release_pending_withdrawal_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE open_hold_index (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE owner_deposit_sequences (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID;
CREATE TABLE settlement_jobs (
    settlement_kind INTEGER NOT NULL CHECK (settlement_kind IN (0, 1)),
    settlement_id BLOB NOT NULL CHECK (length(settlement_id) = 32),
    operation_id BLOB CHECK (operation_id IS NULL OR length(operation_id) = 8),
    status INTEGER NOT NULL CHECK (status IN (0, 1, 2)),
    next_run_at_ns BLOB CHECK (next_run_at_ns IS NULL OR length(next_run_at_ns) = 8),
    confirmation_checks INTEGER NOT NULL CHECK (confirmation_checks BETWEEN 0 AND 5),
    lease_generation BLOB NOT NULL CHECK (length(lease_generation) = 8),
    lease_until_ns BLOB CHECK (lease_until_ns IS NULL OR length(lease_until_ns) = 8),
    last_error TEXT,
    updated_at_ns BLOB NOT NULL CHECK (length(updated_at_ns) = 8),
    PRIMARY KEY (settlement_kind, settlement_id),
    CHECK ((status = 0 AND next_run_at_ns IS NOT NULL AND lease_until_ns IS NULL)
        OR (status = 1 AND next_run_at_ns IS NULL AND lease_until_ns IS NOT NULL)
        OR (status = 2 AND next_run_at_ns IS NULL AND lease_until_ns IS NULL))
) STRICT, WITHOUT ROWID;
CREATE INDEX settlement_jobs_due
ON settlement_jobs(status, next_run_at_ns, settlement_kind, settlement_id);

INSERT INTO table_counts(name, count) VALUES
 ('deposits', X'0000000000000000'),
 ('withdrawals', X'0000000000000000'),
 ('evm_operations', X'0000000000000000'),
 ('reconciliation_holds', X'0000000000000000'),
 ('evm_execution_payloads', X'0000000000000000'),
 ('reconciliation_scans', X'0000000000000000'),
 ('deposit_intents', X'0000000000000000'),
 ('audit_events', X'0000000000000000'),
 ('fee_payouts', X'0000000000000000'),
 ('deposit_owner_index', X'0000000000000000'),
 ('fee_payout_state_index', X'0000000000000000'),
 ('operation_owner_index', X'0000000000000000'),
 ('evm_state_index', X'0000000000000000'),
 ('pull_pending_deposit_index', X'0000000000000000'),
 ('release_pending_withdrawal_index', X'0000000000000000'),
 ('open_hold_index', X'0000000000000000'),
 ('owner_deposit_sequences', X'0000000000000000');
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

fn fee_payout_index_key_for_state(id: u64, state: u8) -> Result<StableBlob, StorageError> {
    let mut bytes = Vec::with_capacity(9);
    bytes.push(state);
    bytes.extend_from_slice(&id.to_be_bytes());
    StableBlob::new(bytes)
}

fn fee_payout_index_key(
    value: &crate::admin::FeePayoutRecord,
) -> Result<Option<StableBlob>, StorageError> {
    matches!(
        value.state,
        crate::admin::FeePayoutState::Pending | crate::admin::FeePayoutState::ReconciliationHold
    )
    .then(|| fee_payout_index_key_for_state(value.id, 0))
    .transpose()
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

fn evm_state_tag(state: EvmOperationState) -> Option<u8> {
    match state {
        EvmOperationState::Queued => Some(0),
        EvmOperationState::Prepared => Some(1),
        EvmOperationState::Submitted { .. } => Some(2),
        EvmOperationState::Confirmed { .. } | EvmOperationState::Reverted { .. } => None,
    }
}

fn evm_state_index_key(value: &EvmOperationRecord) -> Result<Option<StableBlob>, StorageError> {
    let Some(tag) = evm_state_tag(value.state) else {
        return Ok(None);
    };
    let mut bytes = Vec::with_capacity(10);
    bytes.push(tag);
    bytes.push(0);
    bytes.extend_from_slice(&value.id.get().to_be_bytes());
    StableBlob::new(bytes).map(Some)
}

fn first_evm_index_id(
    index: &SqlMap<StableBlob, u8>,
    tag: u8,
) -> Result<Option<u64>, StorageError> {
    let start = StableBlob::new(vec![tag])?;
    let end = StableBlob::new(vec![tag.saturating_add(1)])?;
    let Some(entry) = index.range(start..end).next() else {
        return Ok(None);
    };
    evm_index_id(entry.key()).map(Some)
}

fn evm_index_id(key: &StableBlob) -> Result<u64, StorageError> {
    let bytes: [u8; 8] = key
        .as_slice()
        .get(2..10)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or(StorageError::DecodeFailed)?;
    Ok(u64::from_be_bytes(bytes))
}

fn deposit_operation_id(value: &DepositRecord) -> Option<u64> {
    match value.state {
        bridge_core::DepositState::MintPending { operation_id, .. } => Some(operation_id.get()),
        _ => None,
    }
}

fn withdrawal_operation_id(value: &WithdrawalRecord) -> Option<u64> {
    match value.state {
        WithdrawalState::ReleaseCancellationPending { operation_id, .. }
        | WithdrawalState::AcknowledgePending { operation_id, .. }
        | WithdrawalState::RefundPending { operation_id, .. } => Some(operation_id.get()),
        _ => None,
    }
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
    handle: DbHandle,
    name: &'static str,
    value: T,
}

impl<T: SqlCodec> SqlCell<T> {
    fn load(handle: DbHandle, name: &'static str) -> Result<Self, StorageError> {
        let sql = format!("SELECT {name} FROM singleton_state WHERE id = 1");
        let bytes = handle
            .query(|connection| connection.query_optional_scalar::<Vec<u8>>(&sql, params![]))
            .map_err(StorageError::from)?
            .ok_or(StorageError::RecordNotFound)?;
        Ok(Self {
            handle,
            name,
            value: T::from_sql_bytes(bytes)?,
        })
    }

    fn get(&self) -> &T {
        &self.value
    }

    fn set(&mut self, value: T) -> T {
        let bytes = value.to_sql_bytes();
        let sql = format!("UPDATE singleton_state SET {} = ?1 WHERE id = 1", self.name);
        self.handle
            .update(|connection| connection.execute(&sql, params![bytes]))
            .unwrap_or_else(|error| panic!("SQLite cell update failed: {error}"));
        std::mem::replace(&mut self.value, value)
    }
}

struct SqlMap<K, V> {
    handle: DbHandle,
    table: &'static str,
    _types: PhantomData<(K, V)>,
}

impl<K, V> SqlMap<K, V>
where
    K: SqlCodec + Ord,
    V: SqlCodec,
{
    const fn new(handle: DbHandle, table: &'static str) -> Self {
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

    fn iter(&self) -> std::vec::IntoIter<SqlEntry<K, V>> {
        let sql = format!("SELECT key, value FROM {} ORDER BY key", self.table);
        self.query_entries(&sql, params![]).into_iter()
    }

    fn range<R: RangeBounds<K>>(&self, range: R) -> std::vec::IntoIter<SqlEntry<K, V>> {
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
        let entries = match (start, end) {
            (Some((start, start_inclusive)), Some((end, end_inclusive))) => {
                let sql = format!(
                    "SELECT key, value FROM {} WHERE key {} ?1 AND key {} ?2 ORDER BY key",
                    self.table,
                    if start_inclusive { ">=" } else { ">" },
                    if end_inclusive { "<=" } else { "<" }
                );
                self.query_entries(&sql, params![start, end])
            }
            (Some((start, inclusive)), None) => {
                let sql = format!(
                    "SELECT key, value FROM {} WHERE key {} ?1 ORDER BY key",
                    self.table,
                    if inclusive { ">=" } else { ">" }
                );
                self.query_entries(&sql, params![start])
            }
            (None, Some((end, inclusive))) => {
                let sql = format!(
                    "SELECT key, value FROM {} WHERE key {} ?1 ORDER BY key",
                    self.table,
                    if inclusive { "<=" } else { "<" }
                );
                self.query_entries(&sql, params![end])
            }
            (None, None) => self.iter().collect(),
        };
        entries.into_iter()
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

fn increment_table_count(
    connection: &ic_sqlite_vfs::db::UpdateConnection<'_>,
    table: &str,
) -> Result<(), DbError> {
    let bytes = connection.query_scalar::<Vec<u8>>(
        "SELECT count FROM table_counts WHERE name = ?1",
        params![table],
    )?;
    let count = u64::from_sql_bytes(bytes).map_err(|_| DbError::TypeMismatch {
        index: 0,
        expected: "u64 big-endian blob",
        actual: "invalid blob",
    })?;
    let next = count
        .checked_add(1)
        .ok_or_else(|| DbError::Constraint("table count overflow".into()))?;
    connection.execute(
        "UPDATE table_counts SET count = ?1 WHERE name = ?2",
        params![next.to_sql_bytes(), table],
    )
}

fn decrement_table_count(
    connection: &ic_sqlite_vfs::db::UpdateConnection<'_>,
    table: &str,
) -> Result<(), DbError> {
    let bytes = connection.query_scalar::<Vec<u8>>(
        "SELECT count FROM table_counts WHERE name = ?1",
        params![table],
    )?;
    let count = u64::from_sql_bytes(bytes).map_err(|_| DbError::TypeMismatch {
        index: 0,
        expected: "u64 big-endian blob",
        actual: "invalid blob",
    })?;
    let next = count
        .checked_sub(1)
        .ok_or_else(|| DbError::Constraint("table count underflow".into()))?;
    connection.execute(
        "UPDATE table_counts SET count = ?1 WHERE name = ?2",
        params![next.to_sql_bytes(), table],
    )
}

fn upsert_confirmation_schedule(
    connection: &ic_sqlite_vfs::db::UpdateConnection<'_>,
    kind: SettlementJobKind,
    settlement_id: [u8; 32],
    schedule: ConfirmationSchedule,
) -> Result<(), DbError> {
    connection.execute(
        "INSERT INTO settlement_jobs(
            settlement_kind, settlement_id, operation_id, status, next_run_at_ns,
            confirmation_checks, lease_generation, lease_until_ns, last_error, updated_at_ns
         ) VALUES(?1, ?2, ?3, 0, ?4, ?5, X'0000000000000000', NULL, NULL, ?6)
         ON CONFLICT(settlement_kind, settlement_id) DO UPDATE SET
            operation_id=excluded.operation_id, status=0, next_run_at_ns=excluded.next_run_at_ns,
            confirmation_checks=excluded.confirmation_checks, lease_until_ns=NULL,
            last_error=NULL, updated_at_ns=excluded.updated_at_ns",
        params![
            kind.sql(),
            settlement_id.to_sql_bytes(),
            schedule.operation_id.to_sql_bytes(),
            schedule.next_check_at_ns.to_sql_bytes(),
            i64::from(schedule.checks_completed),
            schedule.submitted_at_ns.to_sql_bytes()
        ],
    )
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
        "UPDATE settlement_jobs SET operation_id = NULL, confirmation_checks = 0,
         updated_at_ns = ?1 WHERE operation_id = ?2",
        params![updated_at_ns.to_sql_bytes(), operation_id.to_sql_bytes()],
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterState {
    pub next_evm_operation_id: u64,
    pub next_hold_id: u64,
    pub pending_evm_operations: u64,
    pub reconciliation_holds: u64,
    pub pending_ledger_operations: u64,
    pub next_audit_sequence: u64,
    pub next_fee_payout_id: u64,
    pub next_deposit_sequence: u64,
    pub reserved_deposit_mint_amount: u128,
    pub reserved_deposit_mint_operations: u64,
    pub reverted_evm_operations: u64,
    pub awaiting_nonce_evm_operations: u64,
    pub nonterminal_withdrawals: u64,
    pub pending_fee_payout_debit: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepositCallerQuota {
    pub caller: Vec<u8>,
    pub count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedBaseMintSnapshot {
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
    pub base_snapshot: Option<CachedBaseMintSnapshot>,
    pub refresh_started_at_ns: Option<u64>,
    pub next_refresh_allowed_at_ns: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositRateLimit {
    pub retry_after_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct WithdrawalAttemptCallerQuota {
    caller: Principal,
    count: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct WithdrawalAttemptControl {
    window_id: u64,
    global_count: u8,
    caller_counts: Vec<WithdrawalAttemptCallerQuota>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettlementAdmissionError {
    RateLimited { retry_after_seconds: u64 },
    Storage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementQuotaLimits {
    pub window_seconds: u64,
    pub global: u16,
    pub per_principal: u16,
    pub per_record: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationSchedule {
    pub operation_id: u64,
    pub submitted_at_ns: u64,
    pub next_check_at_ns: u64,
    pub checks_completed: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementJobKind {
    Deposit,
    Withdrawal,
}

impl SettlementJobKind {
    const fn sql(self) -> i64 {
        match self {
            Self::Deposit => 0,
            Self::Withdrawal => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettlementJobStatus {
    Scheduled,
    Leased,
    Stopped,
}

impl SettlementJobStatus {
    fn from_sql(value: i64) -> Result<Self, StorageError> {
        match value {
            0 => Ok(Self::Scheduled),
            1 => Ok(Self::Leased),
            2 => Ok(Self::Stopped),
            _ => Err(StorageError::DecodeFailed),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementJob {
    pub kind: SettlementJobKind,
    pub settlement_id: [u8; 32],
    pub operation_id: Option<u64>,
    pub status: SettlementJobStatus,
    pub next_run_at_ns: Option<u64>,
    pub confirmation_checks: u8,
    pub lease_generation: u64,
    pub lease_until_ns: Option<u64>,
    pub last_error: Option<String>,
    pub updated_at_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettlementJobClaim {
    Claimed(SettlementJob),
    ActiveLease { lease_until_ns: u64 },
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationSchedulerHealth {
    pub healthy: bool,
    pub last_run_ns: u64,
    pub last_error: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WithdrawalAttemptAdmissionError {
    RateLimited { retry_after_seconds: u64 },
    Storage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepositQuotaError {
    RateLimited(DepositRateLimit),
    Storage(StorageError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum OperationOwner {
    Deposit([u8; 32]),
    Withdrawal([u8; 32]),
}

#[derive(Clone, Copy)]
enum OperationBundleParent<'a> {
    Deposit {
        previous: &'a DepositRecord,
        next: &'a DepositRecord,
        resolved_hold: Option<(&'a ReconciliationHoldRecord, &'a ReconciliationHoldRecord)>,
    },
    Withdrawal {
        previous: Option<&'a WithdrawalRecord>,
        next: &'a WithdrawalRecord,
    },
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
    RuntimeAdministratorsRotated,
    FeeRecipientChanged {
        previous: FeeRecipientConfig,
        current: FeeRecipientConfig,
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
        confirmed_head_block_number: u64,
    },
}

#[derive(CandidType, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditedEvmOperationKind {
    MintDeposit,
    CancelRelease,
    AcknowledgeRelease,
    RefundWithdrawal,
}

impl From<bridge_core::EvmOperationKind> for AuditedEvmOperationKind {
    fn from(value: bridge_core::EvmOperationKind) -> Self {
        match value {
            bridge_core::EvmOperationKind::MintDeposit => Self::MintDeposit,
            bridge_core::EvmOperationKind::CancelRelease => Self::CancelRelease,
            bridge_core::EvmOperationKind::AcknowledgeRelease => Self::AcknowledgeRelease,
            bridge_core::EvmOperationKind::RefundWithdrawal => Self::RefundWithdrawal,
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
    pub reverted_evm_operations: u64,
    pub last_safe_base_block: u64,
    pub active_evm_payloads: u64,
    pub retained_audit_events: u64,
    pub pruned_audit_events: u64,
    pub retained_deposit_index_entries: u64,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepositReserveAdmission {
    pub audit_caller: Principal,
    pub expected_counters: CounterState,
    pub expected_observation_generation: u64,
    pub observed_at_ns: u64,
    pub eth_balance_wei: u128,
    pub cycles_balance: u128,
    pub reserve_policy: bridge_core::ReservePolicy,
    pub mint_snapshot: BaseMintSnapshot,
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
    RecordNotFound,
    DatabaseFailure,
    ReserveUnavailable,
    StaleReserveObservation,
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
        pause_principals: config.pause_principals.clone(),
        finance_administrator: config.finance_administrator,
        governance_principal: config.governance_principal,
        fee_recipient: config.fee_recipient.clone(),
    });
    let schema = SCHEMA_VERSION.to_sql_bytes();
    let accounting = encode(&AccountingState::default())?.to_sql_bytes();
    let counters = encode(&CounterState::default())?.to_sql_bytes();
    let external_progress = encode(&ExternalProgress::default())?.to_sql_bytes();
    let config = encode(&config.cloned())?.to_sql_bytes();
    let admin = encode(&admin)?.to_sql_bytes();
    let deposit_admission = encode(&DepositAdmissionControl::default())?.to_sql_bytes();
    let withdrawal_attempt_control = encode(&WithdrawalAttemptControl::default())?.to_sql_bytes();
    let audit_retention = encode(&AuditRetentionState::default())?.to_sql_bytes();
    let settlement_admission = encode(&SettlementAdmissionControl::default())?.to_sql_bytes();
    let confirmation_scheduler_health =
        encode(&ConfirmationSchedulerHealth::default())?.to_sql_bytes();
    handle.update(|connection| {
        connection.execute(
            "INSERT INTO singleton_state(
                id, schema, accounting, counters, external_progress, config, admin_state,
                deposit_admission, withdrawal_attempt_control, audit_retention,
                settlement_admission, confirmation_scheduler_health
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                schema,
                accounting,
                counters,
                external_progress,
                config,
                admin,
                deposit_admission,
                withdrawal_attempt_control,
                audit_retention,
                settlement_admission,
                confirmation_scheduler_health
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
    handle: DbHandle,
    schema: SqlCell<u16>,
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
    deposit_intents: SqlMap<[u8; 32], StableBlob>,
    admin_state: SqlCell<StableBlob>,
    audit_events: SqlMap<u64, StableBlob>,
    fee_payouts: SqlMap<u64, StableBlob>,
    deposit_owner_index: SqlMap<StableBlob, [u8; 32]>,
    deposit_admission: SqlCell<StableBlob>,
    #[allow(dead_code)]
    fee_payout_state_index: SqlMap<StableBlob, u8>,
    operation_owner_index: SqlMap<u64, StableBlob>,
    evm_state_index: SqlMap<StableBlob, u8>,
    pull_pending_deposit_index: SqlMap<[u8; 32], u8>,
    release_pending_withdrawal_index: SqlMap<[u8; 32], u8>,
    open_hold_index: SqlMap<u64, u8>,
    withdrawal_attempt_control: SqlCell<StableBlob>,
    owner_deposit_sequences: SqlMap<StableBlob, u64>,
    audit_retention: SqlCell<StableBlob>,
    settlement_admission: SqlCell<StableBlob>,
    confirmation_scheduler_health: SqlCell<StableBlob>,
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
        Ok(Self {
            handle,
            schema: SqlCell::load(handle, "schema")?,
            accounting: SqlCell::load(handle, "accounting")?,
            deposits: SqlMap::new(handle, "deposits"),
            withdrawals: SqlMap::new(handle, "withdrawals"),
            evm_operations: SqlMap::new(handle, "evm_operations"),
            reconciliation_holds: SqlMap::new(handle, "reconciliation_holds"),
            counters: SqlCell::load(handle, "counters")?,
            external_progress: SqlCell::load(handle, "external_progress")?,
            evm_execution_payloads: SqlMap::new(handle, "evm_execution_payloads"),
            reconciliation_scans: SqlMap::new(handle, "reconciliation_scans"),
            config: SqlCell::load(handle, "config")?,
            deposit_intents: SqlMap::new(handle, "deposit_intents"),
            admin_state: SqlCell::load(handle, "admin_state")?,
            audit_events: SqlMap::new(handle, "audit_events"),
            fee_payouts: SqlMap::new(handle, "fee_payouts"),
            deposit_owner_index: SqlMap::new(handle, "deposit_owner_index"),
            deposit_admission: SqlCell::load(handle, "deposit_admission")?,
            fee_payout_state_index: SqlMap::new(handle, "fee_payout_state_index"),
            operation_owner_index: SqlMap::new(handle, "operation_owner_index"),
            evm_state_index: SqlMap::new(handle, "evm_state_index"),
            pull_pending_deposit_index: SqlMap::new(handle, "pull_pending_deposit_index"),
            release_pending_withdrawal_index: SqlMap::new(
                handle,
                "release_pending_withdrawal_index",
            ),
            open_hold_index: SqlMap::new(handle, "open_hold_index"),
            withdrawal_attempt_control: SqlCell::load(handle, "withdrawal_attempt_control")?,
            owner_deposit_sequences: SqlMap::new(handle, "owner_deposit_sequences"),
            audit_retention: SqlCell::load(handle, "audit_retention")?,
            settlement_admission: SqlCell::load(handle, "settlement_admission")?,
            confirmation_scheduler_health: SqlCell::load(handle, "confirmation_scheduler_health")?,
        })
    }

    pub fn reopen(memory: DefaultMemoryImpl) -> Result<Self, StorageError> {
        #[cfg(test)]
        reset_sqlite_test_runtime();
        let handle = open_database(memory)?;
        verify_metadata(handle)?;
        let store = Self {
            handle,
            schema: SqlCell::load(handle, "schema")?,
            accounting: SqlCell::load(handle, "accounting")?,
            deposits: SqlMap::new(handle, "deposits"),
            withdrawals: SqlMap::new(handle, "withdrawals"),
            evm_operations: SqlMap::new(handle, "evm_operations"),
            reconciliation_holds: SqlMap::new(handle, "reconciliation_holds"),
            counters: SqlCell::load(handle, "counters")?,
            external_progress: SqlCell::load(handle, "external_progress")?,
            evm_execution_payloads: SqlMap::new(handle, "evm_execution_payloads"),
            reconciliation_scans: SqlMap::new(handle, "reconciliation_scans"),
            config: SqlCell::load(handle, "config")?,
            deposit_intents: SqlMap::new(handle, "deposit_intents"),
            admin_state: SqlCell::load(handle, "admin_state")?,
            audit_events: SqlMap::new(handle, "audit_events"),
            fee_payouts: SqlMap::new(handle, "fee_payouts"),
            deposit_owner_index: SqlMap::new(handle, "deposit_owner_index"),
            deposit_admission: SqlCell::load(handle, "deposit_admission")?,
            fee_payout_state_index: SqlMap::new(handle, "fee_payout_state_index"),
            operation_owner_index: SqlMap::new(handle, "operation_owner_index"),
            evm_state_index: SqlMap::new(handle, "evm_state_index"),
            pull_pending_deposit_index: SqlMap::new(handle, "pull_pending_deposit_index"),
            release_pending_withdrawal_index: SqlMap::new(
                handle,
                "release_pending_withdrawal_index",
            ),
            open_hold_index: SqlMap::new(handle, "open_hold_index"),
            withdrawal_attempt_control: SqlCell::load(handle, "withdrawal_attempt_control")?,
            owner_deposit_sequences: SqlMap::new(handle, "owner_deposit_sequences"),
            audit_retention: SqlCell::load(handle, "audit_retention")?,
            settlement_admission: SqlCell::load(handle, "settlement_admission")?,
            confirmation_scheduler_health: SqlCell::load(handle, "confirmation_scheduler_health")?,
        };
        if store.schema_version() != SCHEMA_VERSION {
            return Err(StorageError::UnsupportedSchemaVersion(
                store.schema_version(),
            ));
        }
        store.validate_singletons()?;
        Ok(store)
    }

    fn validate_singletons(&self) -> Result<(), StorageError> {
        self.accounting()?;
        self.counters()?;
        self.external_progress()?;
        self.config()?;
        decode::<Option<AdminState>>(self.admin_state.get())?;
        self.deposit_admission()?;
        decode::<WithdrawalAttemptControl>(self.withdrawal_attempt_control.get())?;
        decode::<AuditRetentionState>(self.audit_retention.get())?;
        decode::<SettlementAdmissionControl>(self.settlement_admission.get())?;
        decode::<ConfirmationSchedulerHealth>(self.confirmation_scheduler_health.get())?;
        Ok(())
    }

    pub fn schema_version(&self) -> u16 {
        *self.schema.get()
    }

    pub fn accounting(&self) -> Result<AccountingState, StorageError> {
        decode(self.accounting.get())
    }

    pub fn set_accounting(&mut self, value: &AccountingState) -> Result<(), StorageError> {
        self.accounting.set(encode(value)?);
        Ok(())
    }

    pub fn counters(&self) -> Result<CounterState, StorageError> {
        decode(self.counters.get())
    }

    fn deposit_admission(&self) -> Result<DepositAdmissionControl, StorageError> {
        decode(self.deposit_admission.get())
    }

    fn set_deposit_admission(
        &mut self,
        value: &DepositAdmissionControl,
    ) -> Result<(), StorageError> {
        self.deposit_admission.set(encode(value)?);
        Ok(())
    }

    pub fn reserve_deposit_quota(
        &mut self,
        caller: Principal,
        now_ns: u64,
        window_seconds: u64,
        global_limit: u16,
        per_principal_limit: u16,
    ) -> Result<(), DepositQuotaError> {
        let window_ns = window_seconds.saturating_mul(1_000_000_000);
        let window_id = now_ns / window_ns;
        let mut admission = self
            .deposit_admission()
            .map_err(DepositQuotaError::Storage)?;
        if admission.window_id != window_id {
            admission.window_id = window_id;
            admission.global_count = 0;
            admission.caller_counts.clear();
        }
        let retry_after_seconds = ((window_id + 1)
            .saturating_mul(window_ns)
            .saturating_sub(now_ns)
            .saturating_add(999_999_999)
            / 1_000_000_000)
            .max(1);
        let caller_bytes = caller.as_slice();
        let caller_count = admission
            .caller_counts
            .iter()
            .find(|entry| entry.caller == caller_bytes)
            .map(|entry| entry.count)
            .unwrap_or(0);
        if admission.global_count >= global_limit || caller_count >= per_principal_limit {
            return Err(DepositQuotaError::RateLimited(DepositRateLimit {
                retry_after_seconds,
            }));
        }
        admission.global_count = admission.global_count.saturating_add(1);
        match admission
            .caller_counts
            .iter_mut()
            .find(|entry| entry.caller == caller_bytes)
        {
            Some(entry) => entry.count = entry.count.saturating_add(1),
            None => admission.caller_counts.push(DepositCallerQuota {
                caller: caller_bytes.to_vec(),
                count: 1,
            }),
        }
        self.set_deposit_admission(&admission)
            .map_err(DepositQuotaError::Storage)
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
        let mut admission = decode::<SettlementAdmissionControl>(self.settlement_admission.get())
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
            .set(encode(&admission).map_err(|_| SettlementAdmissionError::Storage)?);
        Ok(())
    }

    pub fn confirmation_schedule(
        &self,
        operation_id: u64,
    ) -> Result<Option<ConfirmationSchedule>, StorageError> {
        let rows = self.handle.query(|connection| connection.query_all(
            "SELECT operation_id, updated_at_ns, COALESCE(next_run_at_ns, lease_until_ns), confirmation_checks
             FROM settlement_jobs WHERE operation_id = ?1 AND status IN (0, 1) LIMIT 1",
            params![operation_id.to_sql_bytes()],
            |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?, row.get::<Vec<u8>>(2)?, row.get::<i64>(3)?)),
        ))?;
        rows.into_iter()
            .next()
            .map(|(operation, submitted, next, checks)| {
                Ok(ConfirmationSchedule {
                    operation_id: u64::from_sql_bytes(operation)
                        .map_err(|_| StorageError::DecodeFailed)?,
                    submitted_at_ns: u64::from_sql_bytes(submitted)
                        .map_err(|_| StorageError::DecodeFailed)?,
                    next_check_at_ns: u64::from_sql_bytes(next)
                        .map_err(|_| StorageError::DecodeFailed)?,
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
                "SELECT operation_id FROM settlement_jobs WHERE status = 0
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
                    "SELECT COUNT(*) FROM settlement_jobs WHERE status = 0",
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

    pub fn withdrawal_confirmation_schedule(
        &self,
        withdrawal_id: [u8; 32],
    ) -> Result<Option<ConfirmationSchedule>, StorageError> {
        let Some(record) = self.withdrawal(withdrawal_id)? else {
            return Ok(None);
        };
        let Some(operation_id) = withdrawal_operation_id(&record) else {
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
        let (kind, id) = match decode::<OperationOwner>(&owner)? {
            OperationOwner::Deposit(id) => (SettlementJobKind::Deposit, id),
            OperationOwner::Withdrawal(id) => (SettlementJobKind::Withdrawal, id),
        };
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
                "SELECT operation_id, status, next_run_at_ns, confirmation_checks,
                    lease_generation, lease_until_ns, last_error, updated_at_ns
             FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2",
                params![kind.sql(), settlement_id.to_sql_bytes()],
                |row| {
                    Ok((
                        row.get::<Option<Vec<u8>>>(0)?,
                        row.get::<i64>(1)?,
                        row.get::<Option<Vec<u8>>>(2)?,
                        row.get::<i64>(3)?,
                        row.get::<Vec<u8>>(4)?,
                        row.get::<Option<Vec<u8>>>(5)?,
                        row.get::<Option<String>>(6)?,
                        row.get::<Vec<u8>>(7)?,
                    ))
                },
            )
        })?;
        let Some((operation, status, next, checks, generation, lease, error, updated)) =
            rows.into_iter().next()
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
            status: SettlementJobStatus::from_sql(status)?,
            next_run_at_ns: next
                .map(u64::from_sql_bytes)
                .transpose()
                .map_err(|_| StorageError::DecodeFailed)?,
            confirmation_checks: u8::try_from(checks).map_err(|_| StorageError::DecodeFailed)?,
            lease_generation: u64::from_sql_bytes(generation)
                .map_err(|_| StorageError::DecodeFailed)?,
            lease_until_ns: lease
                .map(u64::from_sql_bytes)
                .transpose()
                .map_err(|_| StorageError::DecodeFailed)?,
            last_error: error,
            updated_at_ns: u64::from_sql_bytes(updated).map_err(|_| StorageError::DecodeFailed)?,
        }))
    }

    pub fn next_settlement_wakeup_ns(&self) -> Result<Option<u64>, StorageError> {
        let values = self.handle.query(|connection| {
            connection.query_all(
                "SELECT next_run_at_ns FROM settlement_jobs WHERE status = 0
             UNION ALL SELECT lease_until_ns FROM settlement_jobs WHERE status = 1
             ORDER BY 1 LIMIT 1",
                params![],
                |row| row.get::<Vec<u8>>(0),
            )
        })?;
        values
            .into_iter()
            .next()
            .map(|value| u64::from_sql_bytes(value).map_err(|_| StorageError::DecodeFailed))
            .transpose()
    }

    pub fn claim_due_settlement_job(
        &mut self,
        now_ns: u64,
        lease_until_ns: u64,
    ) -> Result<SettlementJobClaim, StorageError> {
        self.handle.update(|connection| {
            if let Some(active) = connection.query_optional_scalar::<Vec<u8>>(
                "SELECT lease_until_ns FROM settlement_jobs WHERE status = 1 AND lease_until_ns > ?1
                 ORDER BY lease_until_ns LIMIT 1", params![now_ns.to_sql_bytes()])? {
                let lease_until_ns = u64::from_sql_bytes(active).map_err(|_| DbError::Constraint("invalid lease deadline".into()))?;
                return Ok(SettlementJobClaim::ActiveLease { lease_until_ns });
            }
            let rows = connection.query_all(
                "SELECT settlement_kind, settlement_id, operation_id, confirmation_checks, lease_generation
                 FROM settlement_jobs WHERE (status = 0 AND next_run_at_ns <= ?1)
                    OR (status = 1 AND lease_until_ns <= ?1)
                 ORDER BY CASE status WHEN 1 THEN lease_until_ns ELSE next_run_at_ns END,
                          settlement_kind, settlement_id LIMIT 1",
                params![now_ns.to_sql_bytes()],
                |row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?, row.get::<Option<Vec<u8>>>(2)?, row.get::<i64>(3)?, row.get::<Vec<u8>>(4)?)),
            )?;
            let Some((kind_raw, id_raw, operation_raw, checks, generation_raw)) = rows.into_iter().next() else { return Ok(SettlementJobClaim::None) };
            let kind = match kind_raw { 0 => SettlementJobKind::Deposit, 1 => SettlementJobKind::Withdrawal, _ => return Err(DbError::Constraint("invalid settlement kind".into())) };
            let settlement_id: [u8; 32] = id_raw.try_into().map_err(|_| DbError::Constraint("invalid settlement id".into()))?;
            let generation = u64::from_sql_bytes(generation_raw).map_err(|_| DbError::Constraint("invalid lease generation".into()))?.checked_add(1).ok_or_else(|| DbError::Constraint("lease generation overflow".into()))?;
            connection.execute(
                "UPDATE settlement_jobs SET status = 1, next_run_at_ns = NULL,
                 lease_generation = ?1, lease_until_ns = ?2, updated_at_ns = ?3
                 WHERE settlement_kind = ?4 AND settlement_id = ?5",
                params![generation.to_sql_bytes(), lease_until_ns.to_sql_bytes(), now_ns.to_sql_bytes(), kind.sql(), settlement_id.to_sql_bytes()],
            )?;
            Ok(SettlementJobClaim::Claimed(SettlementJob {
                kind, settlement_id,
                operation_id: operation_raw.map(u64::from_sql_bytes).transpose().map_err(|_| DbError::Constraint("invalid operation id".into()))?,
                status: SettlementJobStatus::Leased, next_run_at_ns: None,
                confirmation_checks: u8::try_from(checks).map_err(|_| DbError::Constraint("invalid confirmation count".into()))?,
                lease_generation: generation, lease_until_ns: Some(lease_until_ns), last_error: None, updated_at_ns: now_ns,
            }))
        }).map_err(Into::into)
    }

    pub fn finish_settlement_job(
        &mut self,
        job: &SettlementJob,
        next_run_at_ns: Option<u64>,
        confirmation_checks: u8,
        stop_error: Option<&str>,
    ) -> Result<(), StorageError> {
        self.handle.update(|connection| {
            let generation = connection.query_optional_scalar::<Vec<u8>>(
                "SELECT lease_generation FROM settlement_jobs WHERE settlement_kind = ?1 AND settlement_id = ?2 AND status = 1",
                params![job.kind.sql(), job.settlement_id.to_sql_bytes()])?;
            if generation.as_deref() != Some(job.lease_generation.to_sql_bytes().as_slice()) { return Ok(()) }
            if let Some(error) = stop_error {
                connection.execute(
                    "UPDATE settlement_jobs SET status = 2, next_run_at_ns = NULL, lease_until_ns = NULL,
                     confirmation_checks = ?1, last_error = ?2, updated_at_ns = ?3
                     WHERE settlement_kind = ?4 AND settlement_id = ?5",
                    params![i64::from(confirmation_checks), error, job.updated_at_ns.to_sql_bytes(), job.kind.sql(), job.settlement_id.to_sql_bytes()],
                )
            } else if let Some(next) = next_run_at_ns {
                connection.execute(
                    "UPDATE settlement_jobs SET status = 0, next_run_at_ns = ?1, lease_until_ns = NULL,
                     confirmation_checks = ?2, last_error = NULL, updated_at_ns = ?3
                     WHERE settlement_kind = ?4 AND settlement_id = ?5",
                    params![next.to_sql_bytes(), i64::from(confirmation_checks), job.updated_at_ns.to_sql_bytes(), job.kind.sql(), job.settlement_id.to_sql_bytes()],
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

    pub fn confirmation_scheduler_health(
        &self,
    ) -> Result<ConfirmationSchedulerHealth, StorageError> {
        decode(self.confirmation_scheduler_health.get())
    }

    pub fn set_confirmation_scheduler_health(
        &mut self,
        health: &ConfirmationSchedulerHealth,
    ) -> Result<(), StorageError> {
        self.confirmation_scheduler_health.set(encode(health)?);
        Ok(())
    }

    pub fn cached_base_mint_snapshot(
        &self,
        now_ns: u64,
        ttl_ns: u64,
        minimum_confirmed_block: u64,
    ) -> Result<Option<CachedBaseMintSnapshot>, StorageError> {
        Ok(self.deposit_admission()?.base_snapshot.and_then(|cached| {
            (now_ns.saturating_sub(cached.observed_at_ns) <= ttl_ns
                && cached.snapshot.confirmed_block_number >= minimum_confirmed_block)
                .then_some(cached)
        }))
    }

    pub fn begin_base_snapshot_refresh(
        &mut self,
        now_ns: u64,
        stale_lock_ns: u64,
        cooldown_ns: u64,
    ) -> Result<bool, StorageError> {
        let mut admission = self.deposit_admission()?;
        let locked = admission
            .refresh_started_at_ns
            .is_some_and(|started| now_ns.saturating_sub(started) < stale_lock_ns);
        if locked || now_ns < admission.next_refresh_allowed_at_ns {
            return Ok(false);
        }
        admission.refresh_started_at_ns = Some(now_ns);
        admission.next_refresh_allowed_at_ns = now_ns.saturating_add(cooldown_ns);
        self.set_deposit_admission(&admission)?;
        Ok(true)
    }

    pub fn finish_base_snapshot_refresh(
        &mut self,
        observed_at_ns: u64,
        snapshot: BaseMintSnapshot,
        bridge_signer: [u8; 20],
        deposits_paused: bool,
    ) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        admission.base_snapshot = Some(CachedBaseMintSnapshot {
            observed_at_ns,
            snapshot,
            bridge_signer,
            deposits_paused,
        });
        admission.refresh_started_at_ns = None;
        self.set_deposit_admission(&admission)
    }

    pub fn fail_base_snapshot_refresh(&mut self) -> Result<(), StorageError> {
        let mut admission = self.deposit_admission()?;
        admission.refresh_started_at_ns = None;
        self.set_deposit_admission(&admission)
    }

    pub fn signer_address(&self) -> Result<Option<[u8; 20]>, StorageError> {
        Ok(self.deposit_admission()?.signer_address)
    }

    pub fn signer_public_key(&self) -> Result<Option<Vec<u8>>, StorageError> {
        Ok(self.deposit_admission()?.signer_public_key)
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
        decode(self.external_progress.get())
    }

    pub fn config(&self) -> Result<Option<BridgeInitArgs>, StorageError> {
        decode(self.config.get())
    }

    pub fn set_config_once(&mut self, value: &BridgeInitArgs) -> Result<(), StorageError> {
        match self.config()? {
            None => {
                self.config.set(encode(&Some(value.clone()))?);
                Ok(())
            }
            Some(previous) if previous == *value => Ok(()),
            Some(_) => Err(StorageError::Core(CoreError::ConflictingReplay)),
        }
    }

    pub fn initialize_admin(&mut self, config: &BridgeInitArgs) -> Result<(), StorageError> {
        if decode::<Option<AdminState>>(self.admin_state.get())?.is_some() {
            return Ok(());
        }
        let state = AdminState {
            deposits_paused: true,
            pause_principals: config.pause_principals.clone(),
            finance_administrator: config.finance_administrator,
            governance_principal: config.governance_principal,
            fee_recipient: config.fee_recipient.clone(),
        };
        self.admin_state.set(encode(&Some(state))?);
        Ok(())
    }

    pub fn admin_state(&self) -> Result<AdminState, StorageError> {
        decode::<Option<AdminState>>(self.admin_state.get())?.ok_or(StorageError::RecordNotFound)
    }
    pub fn set_admin_state(&mut self, value: &AdminState) -> Result<(), StorageError> {
        self.admin_state.set(encode(&Some(value.clone()))?);
        Ok(())
    }
    pub fn append_audit_event(
        &mut self,
        caller: Principal,
        kind: AuditEventKind,
    ) -> Result<AuditEvent, StorageError> {
        self.append_audit_event_at(caller, kind, ic_cdk::api::time())
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
        let mut retention: AuditRetentionState = decode(self.audit_retention.get())?;
        let pruned = if self.audit_events.len() >= MAX_AUDIT_EVENTS {
            let (oldest_sequence, oldest_blob) = self
                .audit_events
                .iter()
                .next()
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
        self.counters.value = counters_blob;
        self.audit_retention.value = retention_blob;
        Ok(event)
    }

    pub fn audit_events(
        &self,
        requested_start: u64,
        limit: u16,
    ) -> Result<AuditEventPage, StorageError> {
        let retention: AuditRetentionState = decode(self.audit_retention.get())?;
        let oldest_available_sequence = self
            .audit_events
            .iter()
            .next()
            .map(|entry| *entry.key())
            .unwrap_or(retention.pruned_count);
        let start = requested_start.max(oldest_available_sequence);
        let mut entries = self
            .audit_events
            .range(start..)
            .take(usize::from(limit) + 1)
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
            .iter()
            .next_back()
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
        let sequence = counters.next_audit_sequence;
        counters.next_audit_sequence =
            bridge_core::audit_next(sequence).ok_or(StorageError::CounterOverflow)?;
        let event = AuditEvent {
            sequence,
            timestamp_ns,
            caller,
            kind: AuditEventKind::FeePayoutRequested {
                amount: value.amount,
            },
        };
        let mut retention: AuditRetentionState = decode(self.audit_retention.get())?;
        let pruned = if self.audit_events.len() >= MAX_AUDIT_EVENTS {
            let (oldest_sequence, oldest_blob) = self
                .audit_events
                .iter()
                .next()
                .map(|entry| (*entry.key(), entry.value()))
                .ok_or(StorageError::RecordNotFound)?;
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
            Some(oldest_sequence)
        } else {
            None
        };
        let value_blob = encode(value)?;
        let index_key = fee_payout_index_key(value)?.ok_or(StorageError::EncodeFailed)?;
        let event_blob = encode(&event)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let counters_blob = encode(&counters)?;
        let retention_blob = encode(&retention)?;
        fee_payout_bundle_storage_failpoint(FeePayoutBundleFailpoint::Encode)?;
        self.handle.update(|connection| {
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_counters != previous_counters_blob.to_sql_bytes() {
                return Err(DbError::Constraint("stale fee payout candidate".into()));
            }
            connection.execute(
                "INSERT INTO fee_payouts(key, value) VALUES (?1, ?2)",
                params![value.id.to_sql_bytes(), value_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "fee_payouts")?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::Record)?;
            connection.execute(
                "INSERT INTO fee_payout_state_index(key, value) VALUES (?1, ?2)",
                params![index_key.to_sql_bytes(), 0u8.to_sql_bytes()],
            )?;
            increment_table_count(connection, "fee_payout_state_index")?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::StateIndex)?;
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
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::Audit)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, audit_retention = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), retention_blob.to_sql_bytes()],
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::SingletonState)
        })?;
        self.counters.value = counters_blob;
        self.audit_retention.value = retention_blob;
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
        let previous_key = fee_payout_index_key(&previous)?;
        let next_key = fee_payout_index_key(&next)?;
        let scan_key = scan_target.map(reconciliation_scan_key);
        let scan_blob = scan.as_ref().map(encode).transpose()?;
        fee_payout_bundle_storage_failpoint(FeePayoutBundleFailpoint::Encode)?;
        self.handle.update(|connection| {
            let persisted_record = connection.query_scalar::<Vec<u8>>(
                "SELECT value FROM fee_payouts WHERE key = ?1",
                params![id.to_sql_bytes()],
            )?;
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_accounting = connection.query_scalar::<Vec<u8>>(
                "SELECT accounting FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_record != previous_blob.to_sql_bytes()
                || persisted_counters != previous_counters_blob.to_sql_bytes()
                || persisted_accounting != previous_accounting_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint("stale fee payout transition".into()));
            }
            connection.execute(
                "UPDATE fee_payouts SET value = ?1 WHERE key = ?2",
                params![next_blob.to_sql_bytes(), id.to_sql_bytes()],
            )?;
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::Record)?;
            if previous_key != next_key {
                if let Some(key) = &previous_key {
                    connection.execute(
                        "DELETE FROM fee_payout_state_index WHERE key = ?1",
                        params![key.to_sql_bytes()],
                    )?;
                    decrement_table_count(connection, "fee_payout_state_index")?;
                }
                if let Some(key) = &next_key {
                    connection.execute(
                        "INSERT INTO fee_payout_state_index(key, value) VALUES (?1, ?2)",
                        params![key.to_sql_bytes(), 0u8.to_sql_bytes()],
                    )?;
                    increment_table_count(connection, "fee_payout_state_index")?;
                }
            }
            fee_payout_bundle_db_failpoint(FeePayoutBundleFailpoint::StateIndex)?;
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
        self.counters.value = counters_blob;
        self.accounting.value = accounting_blob;
        Ok(())
    }

    pub fn set_external_progress(&mut self, value: &ExternalProgress) -> Result<(), StorageError> {
        self.external_progress.set(encode(value)?);
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
            connection.execute(
                "DELETE FROM evm_state_index WHERE key = ?1",
                params![previous_index.to_sql_bytes()],
            )?;
            connection.execute(
                "INSERT INTO evm_state_index(key, value) VALUES (?1, ?2)",
                params![next_index.to_sql_bytes(), 0u8.to_sql_bytes()],
            )?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, external_progress = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes()],
            )
        })?;
        self.counters.value = counters_blob;
        self.external_progress.value = progress_blob;
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
        self.counters.set(encode(value)?);
        Ok(())
    }

    #[cfg(test)]
    fn table_count(&self, table: &str) -> u64 {
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
            .expect("table count")
    }

    pub fn put_deposit(&mut self, value: &DepositRecord) -> Result<(), StorageError> {
        let previous = self.deposit(value.id.bytes())?;
        let mut counters = self.counters()?;
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
        let value_blob = encode(value)?;
        let counters_blob = encode(&counters)?;
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
                .is_some_and(|previous| previous != *owner)
            {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        if previous.as_ref().is_some_and(is_pending_deposit_ledger) {
            self.pull_pending_deposit_index.remove(&value.id.bytes());
        }
        if is_pending_deposit_ledger(value) {
            self.pull_pending_deposit_index.insert(value.id.bytes(), 0);
        }
        if let Some((operation_id, owner)) = operation_owner {
            self.operation_owner_index.insert(operation_id, owner);
        }
        self.deposits.insert(value.id.bytes(), value_blob);
        self.counters.set(counters_blob);
        Ok(())
    }

    pub fn put_deposit_intent(&mut self, value: &DepositIntent) -> Result<(), StorageError> {
        if let Some(previous) = self.deposit_intent(value.deposit_id)? {
            if previous == *value {
                return Ok(());
            }
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.deposit_intents
            .insert(value.deposit_id, encode(value)?);
        Ok(())
    }

    pub fn admit_deposit(
        &mut self,
        owner: Principal,
        intent: &DepositIntent,
        record: &DepositRecord,
        reserve_admission: Option<DepositReserveAdmission>,
    ) -> Result<(), StorageError> {
        if self.deposit(record.id.bytes())?.is_some()
            || self.deposit_intent(intent.deposit_id)?.is_some()
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        if intent.deposit_id != record.id.bytes() || intent.payload_hash != record.payload_hash {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }

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
        let mut counters = previous_counters;
        let previous_progress = self.external_progress()?;
        let mut progress = previous_progress;
        let mut emit_reserve_audit = false;
        if let Some(admission) = reserve_admission {
            if admission.expected_counters != previous_counters
                || admission.expected_observation_generation
                    != previous_progress.reserve_observation_generation
                || admission.observed_at_ns < previous_progress.last_reserve_observation_ns
            {
                return Err(StorageError::StaleReserveObservation);
            }
            let reserve = admission.reserve_policy.snapshot(
                previous_counters.nonterminal_withdrawals,
                previous_counters.reserved_deposit_mint_operations,
                1,
                admission.eth_balance_wei,
                admission.cycles_balance,
            )?;
            if !reserve.sufficient {
                return Err(StorageError::ReserveUnavailable);
            }
            let mint_total = bridge_core::mint_admission_total(
                admission.mint_snapshot.effective_minted_in_window().get(),
                previous_counters.reserved_deposit_mint_amount,
                record.net_amount.get(),
            )
            .ok_or(StorageError::CounterOverflow)?;
            if mint_total > admission.mint_snapshot.mint_window_limit.get() {
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
        let sequence = counters.next_deposit_sequence;
        counters.next_deposit_sequence = sequence
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            false,
            is_pending_deposit_ledger(record),
        )?;
        counters.reserved_deposit_mint_amount =
            adjust_reserved_mint_amount(counters.reserved_deposit_mint_amount, None, record)?;
        counters.reserved_deposit_mint_operations = adjust_reserved_mint_operations(
            counters.reserved_deposit_mint_operations,
            None,
            record,
        )?;

        let mut retention: AuditRetentionState = decode(self.audit_retention.get())?;
        let audit = if emit_reserve_audit {
            let sequence = counters.next_audit_sequence;
            counters.next_audit_sequence =
                bridge_core::audit_next(sequence).ok_or(StorageError::CounterOverflow)?;
            let event = AuditEvent {
                sequence,
                timestamp_ns: reserve_admission
                    .expect("reserve audit requires an observation")
                    .observed_at_ns,
                caller: reserve_admission
                    .expect("reserve audit requires an observation")
                    .audit_caller,
                kind: AuditEventKind::ReserveGateChanged { sufficient: true },
            };
            let pruned = if self.audit_events.len() >= MAX_AUDIT_EVENTS {
                let (oldest_sequence, oldest_blob) = self
                    .audit_events
                    .iter()
                    .next()
                    .map(|entry| (*entry.key(), entry.value()))
                    .ok_or(StorageError::RecordNotFound)?;
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
                Some(oldest_sequence)
            } else {
                None
            };
            Some((sequence, encode(&event)?, pruned))
        } else {
            None
        };

        let intent_blob = encode(intent)?;
        let record_blob = encode(record)?;
        let counters_blob = encode(&counters)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let previous_progress_blob = encode(&previous_progress)?;
        let progress_blob = encode(&progress)?;
        let retention_blob = encode(&retention)?;
        let index_key = deposit_owner_index_key(owner, sequence)?;
        let prefix = deposit_owner_index_prefix(owner);
        let range_start = StableBlob::new(deposit_owner_index_bytes(&prefix, 0))?;
        let range_end = StableBlob::new(deposit_owner_index_bytes(&prefix, u64::MAX))?;
        let excess_key = self
            .deposit_owner_index
            .range(range_start..=range_end)
            .nth(MAX_OWNER_DEPOSIT_INDEX_ENTRIES - 1)
            .map(|entry| entry.key().to_sql_bytes());
        let owner_sequence_exists = self
            .owner_deposit_sequences
            .get(&owner_sequence_key)
            .is_some();
        let deposit_key = record.id.bytes().to_sql_bytes();
        self.handle.update(|connection| {
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            let persisted_progress = connection.query_scalar::<Vec<u8>>(
                "SELECT external_progress FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_counters != previous_counters_blob.to_sql_bytes()
                || persisted_progress != previous_progress_blob.to_sql_bytes()
            {
                return Err(DbError::Constraint(
                    "stale deposit reserve observation".into(),
                ));
            }
            connection.execute(
                "INSERT INTO deposit_intents(key, value) VALUES (?1, ?2)",
                params![intent.deposit_id.to_sql_bytes(), intent_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "deposit_intents")?;
            connection.execute(
                "INSERT INTO deposits(key, value) VALUES (?1, ?2)",
                params![deposit_key.clone(), record_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "deposits")?;
            connection.execute(
                "INSERT INTO deposit_owner_index(key, value) VALUES (?1, ?2)",
                params![index_key.to_sql_bytes(), deposit_key.clone()],
            )?;
            increment_table_count(connection, "deposit_owner_index")?;
            if let Some(excess_key) = excess_key {
                connection.execute(
                    "DELETE FROM deposit_owner_index WHERE key = ?1",
                    params![excess_key],
                )?;
                decrement_table_count(connection, "deposit_owner_index")?;
            }
            if is_pending_deposit_ledger(record) {
                connection.execute(
                    "INSERT INTO pull_pending_deposit_index(key, value) VALUES (?1, ?2)",
                    params![deposit_key, 0u8.to_sql_bytes()],
                )?;
                increment_table_count(connection, "pull_pending_deposit_index")?;
            }
            if let Some((sequence, event_blob, pruned)) = &audit {
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
            }
            connection.execute(
                "INSERT INTO owner_deposit_sequences(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![
                    owner_sequence_key.to_sql_bytes(),
                    next_owner_sequence.to_sql_bytes()
                ],
            )?;
            if !owner_sequence_exists {
                increment_table_count(connection, "owner_deposit_sequences")?;
            }
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, external_progress = ?2, audit_retention = ?3 WHERE id = 1",
                params![
                    counters_blob.to_sql_bytes(),
                    progress_blob.to_sql_bytes(),
                    retention_blob.to_sql_bytes()
                ],
            )
        })?;
        self.counters.value = counters_blob;
        self.external_progress.value = progress_blob;
        self.audit_retention.value = retention_blob;
        Ok(())
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
            .range(range_start.clone()..=range_end.clone())
            .count() as u64;
        let oldest_available_cursor = self
            .deposit_owner_index
            .range(range_start..=range_end.clone())
            .next_back()
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
                })
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
            .range(start..=range_end)
            .take(usize::from(limit) + 1)
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
        self.deposit_intents
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
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
        let previous = self
            .deposit(deposit.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let hold_id = match deposit.state {
            bridge_core::DepositState::ReconciliationHold { hold_id } => hold_id,
            _ => return Err(StorageError::Core(CoreError::HoldMismatch)),
        };
        let mut expected = previous.clone();
        expected.apply(bridge_core::DepositEvent::PullAmbiguous { hold_id })?;
        if expected != *deposit
            || *hold
                != ReconciliationHoldRecord::open(
                    hold_id,
                    bridge_core::RequestReference::Deposit(deposit.id),
                    previous.transfer.clone(),
                )
        {
            return Err(StorageError::Core(CoreError::HoldMismatch));
        }
        self.commit_hold_bundle(
            hold,
            HoldBundleParent::Deposit {
                previous: &previous,
                next: deposit,
            },
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
        )
    }

    fn commit_hold_bundle(
        &mut self,
        hold: &ReconciliationHoldRecord,
        parent: HoldBundleParent<'_>,
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
        counters.reconciliation_holds = counters
            .reconciliation_holds
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        let (table, key, previous_blob, next_blob, previous_index, next_index) = match parent {
            HoldBundleParent::Deposit { previous, next } => {
                counters.pending_ledger_operations = adjust_active_count(
                    counters.pending_ledger_operations,
                    is_pending_deposit_ledger(previous),
                    is_pending_deposit_ledger(next),
                )?;
                (
                    "deposits",
                    next.id.bytes(),
                    encode(previous)?,
                    encode(next)?,
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
                counters.nonterminal_withdrawals = adjust_active_count(
                    counters.nonterminal_withdrawals,
                    is_nonterminal_withdrawal(previous),
                    is_nonterminal_withdrawal(next),
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
            let update_parent = if table == "deposits" {
                "UPDATE deposits SET value = ?1 WHERE key = ?2"
            } else {
                "UPDATE withdrawals SET value = ?1 WHERE key = ?2"
            };
            connection.execute(
                update_parent,
                params![next_blob.to_sql_bytes(), key.clone()],
            )?;
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
            connection.execute(
                "INSERT INTO open_hold_index(key, value) VALUES (?1, ?2)",
                params![hold.id.get().to_sql_bytes(), 0u8.to_sql_bytes()],
            )?;
            increment_table_count(connection, "open_hold_index")?;
            hold_bundle_db_failpoint(HoldBundleFailpoint::OpenHoldIndex)?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                params![counters_blob.to_sql_bytes()],
            )?;
            hold_bundle_db_failpoint(HoldBundleFailpoint::SingletonState)
        })?;
        self.counters.value = counters_blob;
        Ok(())
    }

    /// Returns the operation ID that an acknowledgement bundle must use.
    ///
    /// This does not reserve or persist the ID. `commit_acknowledgement_bundle` rechecks the
    /// candidate inside the same transaction that advances the counter and persists the bundle.
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
        self.commit_deposit_mint_bundle_and_scan(deposit, operation, intent, None)
    }

    pub fn commit_deposit_mint_bundle_and_scan(
        &mut self,
        deposit: &DepositRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<(), StorageError> {
        let previous = self
            .deposit(deposit.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let (ledger_block_index, operation_id) = match deposit.state {
            bridge_core::DepositState::MintPending {
                ledger_block_index,
                operation_id,
            } => (ledger_block_index, operation_id),
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        };
        let mut expected = previous.clone();
        let mut resolved_hold = None;
        if let bridge_core::DepositState::ReconciliationHold { hold_id } = previous.state {
            let mut hold = self
                .reconciliation_hold(hold_id.get())?
                .ok_or(StorageError::RecordNotFound)?;
            let previous_hold = hold.clone();
            resolve_deposit_hold(
                &mut expected,
                &mut hold,
                DepositHoldResolution::Succeeded { ledger_block_index },
            )?;
            resolved_hold = Some((previous_hold, hold));
        } else {
            expected.apply(bridge_core::DepositEvent::PullSucceeded { ledger_block_index })?;
        }
        expected.apply(bridge_core::DepositEvent::PrepareMint { operation_id })?;
        if expected != *deposit || operation.kind != EvmOperationKind::MintDeposit {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.commit_operation_bundle(
            operation,
            intent,
            OperationBundleParent::Deposit {
                previous: &previous,
                next: deposit,
                resolved_hold: resolved_hold
                    .as_ref()
                    .map(|(previous, next)| (previous, next)),
            },
            None,
            scan_target,
        )
    }

    pub fn commit_new_withdrawal_operation_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
        progress: &ExternalProgress,
    ) -> Result<bool, StorageError> {
        if let Some(previous) = self.withdrawal(withdrawal.id.bytes())? {
            if previous == *withdrawal {
                return Ok(false);
            }
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let mut expected = withdrawal.clone();
        expected.state = WithdrawalState::Observed;
        match &withdrawal.state {
            WithdrawalState::ReleaseCancellationPending {
                operation_id,
                expected_ledger_fee,
                ..
            } => {
                expected.apply(WithdrawalEvent::PrepareReleaseCancellation {
                    operation_id: *operation_id,
                    expected_ledger_fee: *expected_ledger_fee,
                })?;
                if operation.kind != EvmOperationKind::CancelRelease {
                    return Err(StorageError::Core(CoreError::PayloadConflict));
                }
            }
            WithdrawalState::RefundPending {
                operation_id,
                eligibility,
            } => {
                expected.apply(WithdrawalEvent::StartRefund {
                    operation_id: *operation_id,
                    eligibility: *eligibility,
                })?;
                if !withdrawal.refund_operation_matches(operation, intent) {
                    return Err(StorageError::Core(CoreError::PayloadConflict));
                }
            }
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        }
        if expected != *withdrawal {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.commit_operation_bundle(
            operation,
            intent,
            OperationBundleParent::Withdrawal {
                previous: None,
                next: withdrawal,
            },
            Some(progress),
            None,
        )?;
        Ok(true)
    }

    /// Atomically ingests a safe Base withdrawal and makes its Ledger release runnable.
    pub fn commit_new_withdrawal_release_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        progress: &ExternalProgress,
    ) -> Result<bool, StorageError> {
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
        counters.nonterminal_withdrawals = counters
            .nonterminal_withdrawals
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        let withdrawal_blob = encode(withdrawal)?;
        let counters_blob = encode(&counters)?;
        let progress_blob = encode(progress)?;
        let key = withdrawal.id.bytes().to_sql_bytes();

        self.handle.update(|connection| {
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_counters != previous_counters_blob.to_sql_bytes() {
                return Err(DbError::Constraint("stale withdrawal ingest".into()));
            }
            connection.execute(
                "INSERT INTO withdrawals(key, value) VALUES (?1, ?2)",
                params![key.clone(), withdrawal_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "withdrawals")?;
            connection.execute(
                "INSERT INTO release_pending_withdrawal_index(key, value) VALUES (?1, ?2)",
                params![key, 0u8.to_sql_bytes()],
            )?;
            increment_table_count(connection, "release_pending_withdrawal_index")?;
            connection.execute(
                "UPDATE singleton_state SET counters = ?1, external_progress = ?2 WHERE id = 1",
                params![counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes()],
            )?;
            Ok(())
        })?;
        self.counters.value = counters_blob;
        self.external_progress.value = progress_blob;
        Ok(true)
    }

    pub fn commit_withdrawal_operation_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
    ) -> Result<(), StorageError> {
        let previous = self
            .withdrawal(withdrawal.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let mut expected = previous.clone();
        match &withdrawal.state {
            WithdrawalState::ReleaseCancellationPending {
                operation_id,
                expected_ledger_fee,
                ..
            } => {
                expected.apply(WithdrawalEvent::PrepareReleaseCancellation {
                    operation_id: *operation_id,
                    expected_ledger_fee: *expected_ledger_fee,
                })?;
                if operation.kind != EvmOperationKind::CancelRelease {
                    return Err(StorageError::Core(CoreError::PayloadConflict));
                }
            }
            WithdrawalState::RefundPending {
                operation_id,
                eligibility,
            } => {
                expected.apply(WithdrawalEvent::StartRefund {
                    operation_id: *operation_id,
                    eligibility: *eligibility,
                })?;
                if !withdrawal.refund_operation_matches(operation, intent) {
                    return Err(StorageError::Core(CoreError::PayloadConflict));
                }
            }
            _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
        }
        if expected != *withdrawal {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        self.commit_operation_bundle(
            operation,
            intent,
            OperationBundleParent::Withdrawal {
                previous: Some(&previous),
                next: withdrawal,
            },
            None,
            None,
        )
    }

    fn commit_operation_bundle(
        &mut self,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
        parent: OperationBundleParent<'_>,
        progress: Option<&ExternalProgress>,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<(), StorageError> {
        if !matches!(operation.state, EvmOperationState::Queued)
            || intent.operation_id != operation.id
            || intent.payload_hash != operation.payload_hash
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }
        let mut counters = self.counters()?;
        let previous_counters = counters;
        if counters.next_evm_operation_id != operation.id.get() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        counters.next_evm_operation_id = counters
            .next_evm_operation_id
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.pending_evm_operations = counters
            .pending_evm_operations
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
                resolved_hold,
            } => {
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
                if let Some((previous_hold, next_hold)) = resolved_hold {
                    counters.reconciliation_holds = adjust_active_count(
                        counters.reconciliation_holds,
                        is_open_hold(previous_hold),
                        is_open_hold(next_hold),
                    )?;
                }
                (
                    "deposits",
                    next.id.bytes(),
                    Some(encode(previous)?),
                    encode(next)?,
                    OperationOwner::Deposit(next.id.bytes()),
                    true,
                    is_pending_deposit_ledger(previous),
                    is_pending_deposit_ledger(next),
                )
            }
            OperationBundleParent::Withdrawal { previous, next } => {
                counters.pending_ledger_operations = adjust_active_count(
                    counters.pending_ledger_operations,
                    previous.is_some_and(is_pending_withdrawal_ledger),
                    is_pending_withdrawal_ledger(next),
                )?;
                counters.nonterminal_withdrawals = adjust_active_count(
                    counters.nonterminal_withdrawals,
                    previous.is_some_and(is_nonterminal_withdrawal),
                    is_nonterminal_withdrawal(next),
                )?;
                (
                    "withdrawals",
                    next.id.bytes(),
                    previous.map(encode).transpose()?,
                    encode(next)?,
                    OperationOwner::Withdrawal(next.id.bytes()),
                    previous.is_some(),
                    previous.is_some_and(is_pending_withdrawal_ledger),
                    is_pending_withdrawal_ledger(next),
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
        let payload_blob = encode(&EvmExecutionPayload::AwaitingNonce(intent.clone()))?;
        let operation_blob = encode(operation)?;
        let owner_blob = encode(&owner)?;
        let previous_counters_blob = encode(&previous_counters)?;
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
            OperationBundleParent::Withdrawal { .. } => None,
        };
        let operation_key = operation.id.get().to_sql_bytes();
        let parent_key = parent_key.to_sql_bytes();

        self.handle.update(|connection| {
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_counters != previous_counters_blob.to_sql_bytes() {
                return Err(DbError::Constraint("stale operation ID candidate".into()));
            }
            if parent_was_present {
                let select_sql = match parent_table {
                    "deposits" => "SELECT value FROM deposits WHERE key = ?1",
                    _ => "SELECT value FROM withdrawals WHERE key = ?1",
                };
                let persisted_parent =
                    connection.query_scalar::<Vec<u8>>(select_sql, params![parent_key.clone()])?;
                if persisted_parent
                    != previous_parent_blob
                        .as_ref()
                        .expect("present parent")
                        .to_sql_bytes()
                {
                    return Err(DbError::Constraint("stale operation parent".into()));
                }
                let sql = match parent_table {
                    "deposits" => "UPDATE deposits SET value = ?1 WHERE key = ?2",
                    _ => "UPDATE withdrawals SET value = ?1 WHERE key = ?2",
                };
                connection.execute(sql, params![parent_blob.to_sql_bytes(), parent_key.clone()])?;
            } else {
                connection.execute(
                    "INSERT INTO withdrawals(key, value) VALUES (?1, ?2)",
                    params![parent_key.clone(), parent_blob.to_sql_bytes()],
                )?;
                increment_table_count(connection, "withdrawals")?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::Parent)?;

            let parent_index_table = if parent_table == "deposits" {
                "pull_pending_deposit_index"
            } else {
                "release_pending_withdrawal_index"
            };
            if previous_parent_index {
                let sql = if parent_table == "deposits" {
                    "DELETE FROM pull_pending_deposit_index WHERE key = ?1"
                } else {
                    "DELETE FROM release_pending_withdrawal_index WHERE key = ?1"
                };
                connection.execute(sql, params![parent_key.clone()])?;
                decrement_table_count(connection, parent_index_table)?;
            }
            if next_parent_index {
                let sql = if parent_table == "deposits" {
                    "INSERT INTO pull_pending_deposit_index(key, value) VALUES (?1, ?2)"
                } else {
                    "INSERT INTO release_pending_withdrawal_index(key, value) VALUES (?1, ?2)"
                };
                connection.execute(sql, params![parent_key, 0u8.to_sql_bytes()])?;
                increment_table_count(connection, parent_index_table)?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::ParentIndex)?;

            if let Some((hold_id, previous_hold_blob, next_hold_blob)) = &resolved_hold_blobs {
                let persisted_hold = connection.query_scalar::<Vec<u8>>(
                    "SELECT value FROM reconciliation_holds WHERE key = ?1",
                    params![hold_id.to_sql_bytes()],
                )?;
                if persisted_hold != previous_hold_blob.to_sql_bytes() {
                    return Err(DbError::Constraint("stale reconciliation hold".into()));
                }
                connection.execute(
                    "UPDATE reconciliation_holds SET value = ?1 WHERE key = ?2",
                    params![next_hold_blob.to_sql_bytes(), hold_id.to_sql_bytes()],
                )?;
                operation_bundle_db_failpoint(OperationBundleFailpoint::ReconciliationHold)?;
                connection.execute(
                    "DELETE FROM open_hold_index WHERE key = ?1",
                    params![hold_id.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "open_hold_index")?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::OpenHoldIndex)?;

            if let (Some(scan_key), Some(scan_blob)) = (scan_key, &scan_blob) {
                let persisted_scan = connection.query_scalar::<Vec<u8>>(
                    "SELECT value FROM reconciliation_scans WHERE key = ?1",
                    params![scan_key.to_sql_bytes()],
                )?;
                if persisted_scan != scan_blob.to_sql_bytes() {
                    return Err(DbError::Constraint("stale reconciliation scan".into()));
                }
                connection.execute(
                    "DELETE FROM reconciliation_scans WHERE key = ?1",
                    params![scan_key.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "reconciliation_scans")?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::ReconciliationScan)?;

            connection.execute(
                "INSERT INTO evm_execution_payloads(key, value) VALUES (?1, ?2)",
                params![operation_key.clone(), payload_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "evm_execution_payloads")?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::ExecutionPayload)?;
            connection.execute(
                "INSERT INTO evm_operations(key, value) VALUES (?1, ?2)",
                params![operation_key.clone(), operation_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "evm_operations")?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::EvmOperation)?;
            connection.execute(
                "INSERT INTO evm_state_index(key, value) VALUES (?1, ?2)",
                params![evm_index_key.to_sql_bytes(), 0u8.to_sql_bytes()],
            )?;
            increment_table_count(connection, "evm_state_index")?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::EvmStateIndex)?;
            connection.execute(
                "INSERT INTO operation_owner_index(key, value) VALUES (?1, ?2)",
                params![operation_key, owner_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "operation_owner_index")?;
            operation_bundle_db_failpoint(OperationBundleFailpoint::OperationOwnerIndex)?;

            if let Some(progress_blob) = &progress_blob {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1, external_progress = ?2 WHERE id = 1",
                    params![counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes()],
                )?;
            } else {
                connection.execute(
                    "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                    params![counters_blob.to_sql_bytes()],
                )?;
            }
            operation_bundle_db_failpoint(OperationBundleFailpoint::SingletonState)
        })?;
        self.counters.value = counters_blob;
        if let Some(progress_blob) = progress_blob {
            self.external_progress.value = progress_blob;
        }
        Ok(())
    }

    /// Atomically confirms a successful withdrawal release and prepares its acknowledgement.
    ///
    /// The caller constructs `withdrawal`, `operation`, and `intent` using the candidate returned
    /// by `next_evm_operation_id`. This method treats those values as an untrusted proposal: it
    /// replays the domain transitions from the persisted withdrawal, derives the fee delta, and
    /// verifies the current operation counter before performing any write.
    pub fn commit_acknowledgement_bundle(
        &mut self,
        withdrawal: &WithdrawalRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
    ) -> Result<(), StorageError> {
        self.commit_acknowledgement_bundle_and_scan(withdrawal, operation, intent, None)
    }

    pub fn commit_acknowledgement_bundle_and_scan(
        &mut self,
        withdrawal: &WithdrawalRecord,
        operation: &EvmOperationRecord,
        intent: &EvmCallIntent,
        scan_target: Option<&ReconciliationTarget>,
    ) -> Result<(), StorageError> {
        let previous = self
            .withdrawal(withdrawal.id.bytes())?
            .ok_or(StorageError::RecordNotFound)?;
        let (operation_id, ledger_block_index) = match &withdrawal.state {
            WithdrawalState::AcknowledgePending {
                operation_id,
                ledger_block_index,
                ..
            } => (*operation_id, *ledger_block_index),
            _ => {
                return Err(StorageError::Core(CoreError::InvalidTransition {
                    entity: "withdrawal",
                    event: "commit_acknowledgement_bundle",
                }))
            }
        };

        let mut expected = previous.clone();
        let mut resolved_hold = None;
        let fee_delta = match &previous.state {
            WithdrawalState::ReleasePending { .. } => {
                expected
                    .apply(WithdrawalEvent::ReleaseSucceeded { ledger_block_index })?
                    .fee_delta
            }
            WithdrawalState::ReleaseTransferred { settlement, .. } => bridge_core::Amount::new(
                bridge_core::fee_delta_once(false, true, settlement.service_fee.get()),
            ),
            WithdrawalState::ReconciliationHold { hold_id, .. } => {
                let mut hold = self
                    .reconciliation_hold(hold_id.get())?
                    .ok_or(StorageError::RecordNotFound)?;
                let previous_hold = hold.clone();
                let result = resolve_withdrawal_hold(
                    &mut expected,
                    &mut hold,
                    WithdrawalHoldResolution::Succeeded { ledger_block_index },
                )?;
                resolved_hold = Some((previous_hold, hold));
                result.fee_delta
            }
            _ => {
                return Err(StorageError::Core(CoreError::InvalidTransition {
                    entity: "withdrawal",
                    event: "commit_acknowledgement_bundle",
                }))
            }
        };
        expected.apply(WithdrawalEvent::PrepareAcknowledgement { operation_id })?;
        if expected != *withdrawal
            || *operation
                != EvmOperationRecord::queued(
                    operation_id,
                    withdrawal.payload_hash,
                    EvmOperationKind::AcknowledgeRelease,
                )
            || intent.operation_id != operation_id
            || intent.payload_hash != withdrawal.payload_hash
        {
            return Err(StorageError::Core(CoreError::PayloadConflict));
        }

        let mut accounting = self.accounting()?;
        accounting.confirm_fee(FeeKind::Withdrawal, fee_delta)?;
        let mut counters = self.counters()?;
        if counters.next_evm_operation_id != operation_id.get() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let previous_counters = counters;
        counters.next_evm_operation_id = counters
            .next_evm_operation_id
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.pending_evm_operations = counters
            .pending_evm_operations
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.awaiting_nonce_evm_operations = counters
            .awaiting_nonce_evm_operations
            .checked_add(1)
            .ok_or(StorageError::CounterOverflow)?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            is_pending_withdrawal_ledger(&previous),
            is_pending_withdrawal_ledger(withdrawal),
        )?;
        counters.nonterminal_withdrawals = adjust_active_count(
            counters.nonterminal_withdrawals,
            is_nonterminal_withdrawal(&previous),
            is_nonterminal_withdrawal(withdrawal),
        )?;
        if let Some((previous_hold, next_hold)) = &resolved_hold {
            counters.reconciliation_holds = adjust_active_count(
                counters.reconciliation_holds,
                is_open_hold(previous_hold),
                is_open_hold(next_hold),
            )?;
        }

        if self.evm_execution_payload(operation_id.get())?.is_some()
            || self.evm_operation(operation_id.get())?.is_some()
            || self
                .operation_owner_index
                .get(&operation_id.get())
                .is_some()
        {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let evm_index_key = evm_state_index_key(operation)?.ok_or(StorageError::RecordNotFound)?;
        if self.evm_state_index.get(&evm_index_key).is_some() {
            return Err(StorageError::Core(CoreError::ConflictingReplay));
        }
        let release_index_present = self
            .release_pending_withdrawal_index
            .get(&withdrawal.id.bytes())
            .is_some();
        if release_index_present != is_pending_withdrawal_ledger(&previous) {
            return Err(StorageError::RecordNotFound);
        }
        let open_hold_index_present = resolved_hold
            .as_ref()
            .is_some_and(|(previous, _)| self.open_hold_index.get(&previous.id.get()).is_some());
        if resolved_hold.is_some() != open_hold_index_present {
            return Err(StorageError::RecordNotFound);
        }

        acknowledgement_bundle_storage_failpoint(AcknowledgementBundleFailpoint::Encode)?;
        let payload_blob = encode(&EvmExecutionPayload::AwaitingNonce(intent.clone()))?;
        let operation_blob = encode(operation)?;
        let owner_blob = encode(&OperationOwner::Withdrawal(withdrawal.id.bytes()))?;
        let withdrawal_blob = encode(withdrawal)?;
        let accounting_blob = encode(&accounting)?;
        let previous_counters_blob = encode(&previous_counters)?;
        let counters_blob = encode(&counters)?;
        let scan_blob = scan_target
            .map(|target| {
                self.reconciliation_scan(target)?
                    .map(|scan| encode(&scan))
                    .transpose()?
                    .ok_or(StorageError::RecordNotFound)
            })
            .transpose()?;
        let scan_key = scan_target.map(reconciliation_scan_key);
        let resolved_hold_blobs = resolved_hold
            .as_ref()
            .map(|(previous, next)| -> Result<_, StorageError> {
                Ok((previous.id.get(), encode(previous)?, encode(next)?))
            })
            .transpose()?;
        let operation_key = operation_id.get().to_sql_bytes();
        let withdrawal_key = withdrawal.id.bytes().to_sql_bytes();

        self.handle.update(|connection| {
            let persisted_counters = connection.query_scalar::<Vec<u8>>(
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
            )?;
            if persisted_counters != previous_counters_blob.to_sql_bytes() {
                return Err(DbError::Constraint(
                    "stale acknowledgement operation ID candidate".into(),
                ));
            }
            connection.execute(
                "INSERT INTO evm_execution_payloads(key, value) VALUES (?1, ?2)",
                params![operation_key.clone(), payload_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "evm_execution_payloads")?;
            acknowledgement_bundle_db_failpoint(AcknowledgementBundleFailpoint::ExecutionPayload)?;

            connection.execute(
                "INSERT INTO evm_operations(key, value) VALUES (?1, ?2)",
                params![operation_key.clone(), operation_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "evm_operations")?;
            acknowledgement_bundle_db_failpoint(AcknowledgementBundleFailpoint::EvmOperation)?;

            connection.execute(
                "INSERT INTO evm_state_index(key, value) VALUES (?1, ?2)",
                params![evm_index_key.to_sql_bytes(), 0u8.to_sql_bytes()],
            )?;
            increment_table_count(connection, "evm_state_index")?;
            acknowledgement_bundle_db_failpoint(AcknowledgementBundleFailpoint::EvmStateIndex)?;

            connection.execute(
                "INSERT INTO operation_owner_index(key, value) VALUES (?1, ?2)",
                params![operation_key, owner_blob.to_sql_bytes()],
            )?;
            increment_table_count(connection, "operation_owner_index")?;
            acknowledgement_bundle_db_failpoint(
                AcknowledgementBundleFailpoint::OperationOwnerIndex,
            )?;

            connection.execute(
                "UPDATE withdrawals SET value = ?1 WHERE key = ?2",
                params![withdrawal_blob.to_sql_bytes(), withdrawal_key.clone()],
            )?;
            acknowledgement_bundle_db_failpoint(AcknowledgementBundleFailpoint::Withdrawal)?;

            if release_index_present {
                connection.execute(
                    "DELETE FROM release_pending_withdrawal_index WHERE key = ?1",
                    params![withdrawal_key],
                )?;
                decrement_table_count(connection, "release_pending_withdrawal_index")?;
            }
            acknowledgement_bundle_db_failpoint(
                AcknowledgementBundleFailpoint::ReleasePendingIndex,
            )?;

            if let Some((hold_id, previous_hold_blob, next_hold_blob)) = &resolved_hold_blobs {
                let persisted_hold = connection.query_scalar::<Vec<u8>>(
                    "SELECT value FROM reconciliation_holds WHERE key = ?1",
                    params![hold_id.to_sql_bytes()],
                )?;
                if persisted_hold != previous_hold_blob.to_sql_bytes() {
                    return Err(DbError::Constraint("stale reconciliation hold".into()));
                }
                connection.execute(
                    "UPDATE reconciliation_holds SET value = ?1 WHERE key = ?2",
                    params![next_hold_blob.to_sql_bytes(), hold_id.to_sql_bytes()],
                )?;
                acknowledgement_bundle_db_failpoint(
                    AcknowledgementBundleFailpoint::ReconciliationHold,
                )?;
                connection.execute(
                    "DELETE FROM open_hold_index WHERE key = ?1",
                    params![hold_id.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "open_hold_index")?;
            }
            acknowledgement_bundle_db_failpoint(AcknowledgementBundleFailpoint::OpenHoldIndex)?;

            if let (Some(scan_key), Some(scan_blob)) = (scan_key, &scan_blob) {
                let persisted_scan = connection.query_scalar::<Vec<u8>>(
                    "SELECT value FROM reconciliation_scans WHERE key = ?1",
                    params![scan_key.to_sql_bytes()],
                )?;
                if persisted_scan != scan_blob.to_sql_bytes() {
                    return Err(DbError::Constraint("stale reconciliation scan".into()));
                }
                connection.execute(
                    "DELETE FROM reconciliation_scans WHERE key = ?1",
                    params![scan_key.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "reconciliation_scans")?;
            }
            acknowledgement_bundle_db_failpoint(
                AcknowledgementBundleFailpoint::ReconciliationScan,
            )?;

            connection.execute(
                "UPDATE singleton_state SET accounting = ?1, counters = ?2 WHERE id = 1",
                params![accounting_blob.to_sql_bytes(), counters_blob.to_sql_bytes()],
            )?;
            acknowledgement_bundle_db_failpoint(AcknowledgementBundleFailpoint::SingletonState)
        })?;
        self.accounting.value = accounting_blob;
        self.counters.value = counters_blob;
        Ok(())
    }

    pub fn commit_evm_terminal_bundle(
        &mut self,
        operation: &EvmOperationRecord,
        progress: &ExternalProgress,
        revert_audit: Option<(Principal, u64, u64)>,
    ) -> Result<(), StorageError> {
        let previous_operation = self
            .evm_operation(operation.id.get())?
            .ok_or(StorageError::RecordNotFound)?;
        let transition = match operation.state {
            EvmOperationState::Confirmed {
                transaction_hash,
                receipt_block_number,
                confirmed_block_number,
            } => EvmOperationEvent::Confirmed {
                transaction_hash,
                receipt_block_number,
                confirmed_block_number,
            },
            EvmOperationState::Reverted {
                transaction_hash,
                receipt_block_number,
                confirmed_block_number,
            } => EvmOperationEvent::Reverted {
                transaction_hash,
                receipt_block_number,
                confirmed_block_number,
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

        let mut accounting = self.accounting()?;
        let mut counters = self.counters()?;
        counters.pending_evm_operations = counters
            .pending_evm_operations
            .checked_sub(1)
            .ok_or(StorageError::CounterUnderflow)?;
        if !is_confirmed {
            counters.reverted_evm_operations = counters
                .reverted_evm_operations
                .checked_add(1)
                .ok_or(StorageError::CounterOverflow)?;
        }

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
                (
                    "deposits",
                    id,
                    encode(&previous)?,
                    encode(&next)?,
                    is_pending_deposit_ledger(&previous),
                    is_pending_deposit_ledger(&next),
                )
            }
            OperationOwner::Withdrawal(id) => {
                let previous = self.withdrawal(id)?.ok_or(StorageError::RecordNotFound)?;
                let mut next = previous.clone();
                let event = match (operation.kind, is_confirmed) {
                    (EvmOperationKind::CancelRelease, true) => {
                        Some(WithdrawalEvent::ReleaseCancellationConfirmed {
                            operation_id: operation.id,
                        })
                    }
                    (EvmOperationKind::AcknowledgeRelease, true) => {
                        Some(WithdrawalEvent::AcknowledgementConfirmed {
                            operation_id: operation.id,
                        })
                    }
                    (EvmOperationKind::RefundWithdrawal, true) => {
                        Some(WithdrawalEvent::RefundConfirmed {
                            operation_id: operation.id,
                        })
                    }
                    (EvmOperationKind::AcknowledgeRelease, false) => {
                        Some(WithdrawalEvent::AcknowledgementReverted {
                            operation_id: operation.id,
                        })
                    }
                    (EvmOperationKind::RefundWithdrawal, false) => {
                        Some(WithdrawalEvent::RefundReverted {
                            operation_id: operation.id,
                        })
                    }
                    (EvmOperationKind::CancelRelease, false) => {
                        // A reverted lock/cancellation deliberately leaves the parent pending.
                        // The atomic automatic pause below prevents unsafe progress.
                        None
                    }
                    (EvmOperationKind::MintDeposit, _) => {
                        return Err(StorageError::Core(CoreError::PayloadConflict))
                    }
                };
                if let Some(event) = event {
                    next.apply(event)?;
                }
                counters.pending_ledger_operations = adjust_active_count(
                    counters.pending_ledger_operations,
                    is_pending_withdrawal_ledger(&previous),
                    is_pending_withdrawal_ledger(&next),
                )?;
                counters.nonterminal_withdrawals = adjust_active_count(
                    counters.nonterminal_withdrawals,
                    is_nonterminal_withdrawal(&previous),
                    is_nonterminal_withdrawal(&next),
                )?;
                (
                    "withdrawals",
                    id,
                    encode(&previous)?,
                    encode(&next)?,
                    is_pending_withdrawal_ledger(&previous),
                    is_pending_withdrawal_ledger(&next),
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

        let (admin_blob, audit_event, retention_blob, pruned_sequence) =
            if let Some((caller, timestamp_ns, confirmed_block_number)) = revert_audit {
                let mut admin = self.admin_state()?;
                admin.deposits_paused = true;
                let sequence = counters.next_audit_sequence;
                counters.next_audit_sequence =
                    bridge_core::audit_next(sequence).ok_or(StorageError::CounterOverflow)?;
                let transaction_hash = match operation.state {
                    EvmOperationState::Reverted {
                        transaction_hash, ..
                    } => transaction_hash,
                    _ => return Err(StorageError::Core(CoreError::PayloadConflict)),
                };
                let event = AuditEvent {
                    sequence,
                    timestamp_ns,
                    caller,
                    kind: AuditEventKind::EvmOperationReverted {
                        operation_id: operation.id.get(),
                        kind: operation.kind.into(),
                        transaction_hash: transaction_hash.to_vec(),
                        confirmed_head_block_number: confirmed_block_number,
                    },
                };
                let mut retention: AuditRetentionState = decode(self.audit_retention.get())?;
                let pruned = if self.audit_events.len() >= MAX_AUDIT_EVENTS {
                    let (oldest_sequence, oldest_blob) = self
                        .audit_events
                        .iter()
                        .next()
                        .map(|entry| (*entry.key(), entry.value()))
                        .ok_or(StorageError::RecordNotFound)?;
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
                    Some(oldest_sequence)
                } else {
                    None
                };
                (
                    Some(encode(&Some(admin))?),
                    Some((sequence, encode(&event)?)),
                    Some(encode(&retention)?),
                    pruned,
                )
            } else {
                (None, None, None, None)
            };
        let counters_blob = encode(&counters)?;

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
            let update_parent = if parent_table == "deposits" {
                "UPDATE deposits SET value = ?1 WHERE key = ?2"
            } else {
                "UPDATE withdrawals SET value = ?1 WHERE key = ?2"
            };
            connection.execute(update_parent, params![parent_blob.to_sql_bytes(), parent_key_sql.clone()])?;
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
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::EvmOperation)?;
            connection.execute(
                "DELETE FROM evm_state_index WHERE key = ?1",
                params![previous_evm_index.to_sql_bytes()],
            )?;
            decrement_table_count(connection, "evm_state_index")?;
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::EvmStateIndex)?;
            connection.execute("DELETE FROM operation_owner_index WHERE key = ?1", params![operation_key])?;
            decrement_table_count(connection, "operation_owner_index")?;
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::OperationOwnerIndex)?;
            detach_confirmed_operation(
                connection,
                previous_schedule.operation_id,
                progress.last_safe_observation_ns,
            )?;
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::ConfirmationSchedule)?;
            if let Some((sequence, event_blob)) = &audit_event {
                connection.execute(
                    "INSERT INTO audit_events(key, value) VALUES (?1, ?2)",
                    params![sequence.to_sql_bytes(), event_blob.to_sql_bytes()],
                )?;
                increment_table_count(connection, "audit_events")?;
                if let Some(pruned) = pruned_sequence {
                    connection.execute("DELETE FROM audit_events WHERE key = ?1", params![pruned.to_sql_bytes()])?;
                    decrement_table_count(connection, "audit_events")?;
                }
            }
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::Audit)?;
            if let (Some(admin_blob), Some(retention_blob)) = (&admin_blob, &retention_blob) {
                connection.execute(
                    "UPDATE singleton_state SET accounting = ?1, counters = ?2, external_progress = ?3,
                        admin_state = ?4, audit_retention = ?5 WHERE id = 1",
                    params![accounting_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes(), admin_blob.to_sql_bytes(), retention_blob.to_sql_bytes()],
                )?;
            } else {
                connection.execute(
                    "UPDATE singleton_state SET accounting = ?1, counters = ?2, external_progress = ?3 WHERE id = 1",
                    params![accounting_blob.to_sql_bytes(), counters_blob.to_sql_bytes(), progress_blob.to_sql_bytes()],
                )?;
            }
            terminal_bundle_db_failpoint(TerminalBundleFailpoint::SingletonState)
        })?;
        self.accounting.value = accounting_blob;
        self.counters.value = counters_blob;
        self.external_progress.value = progress_blob;
        if let Some(blob) = admin_blob {
            self.admin_state.value = blob;
        }
        if let Some(blob) = retention_blob {
            self.audit_retention.value = blob;
        }
        Ok(())
    }

    pub fn deposit(&self, id: [u8; 32]) -> Result<Option<DepositRecord>, StorageError> {
        self.deposits.get(&id).map(|blob| decode(&blob)).transpose()
    }

    pub fn put_withdrawal(&mut self, value: &WithdrawalRecord) -> Result<(), StorageError> {
        let previous = self.withdrawal(value.id.bytes())?;
        let mut counters = self.counters()?;
        counters.pending_ledger_operations = adjust_active_count(
            counters.pending_ledger_operations,
            previous
                .as_ref()
                .map(is_pending_withdrawal_ledger)
                .unwrap_or(false),
            is_pending_withdrawal_ledger(value),
        )?;
        counters.nonterminal_withdrawals = adjust_active_count(
            counters.nonterminal_withdrawals,
            previous.as_ref().is_some_and(is_nonterminal_withdrawal),
            is_nonterminal_withdrawal(value),
        )?;
        let value_blob = encode(value)?;
        let counters_blob = encode(&counters)?;
        let operation_owner = withdrawal_operation_id(value)
            .map(|operation_id| {
                encode(&OperationOwner::Withdrawal(value.id.bytes()))
                    .map(|owner| (operation_id, owner))
            })
            .transpose()?;
        if let Some((operation_id, owner)) = operation_owner.as_ref() {
            if self
                .operation_owner_index
                .get(operation_id)
                .is_some_and(|previous| previous != *owner)
            {
                return Err(StorageError::Core(CoreError::ConflictingReplay));
            }
        }
        if previous.as_ref().is_some_and(is_pending_withdrawal_ledger) {
            self.release_pending_withdrawal_index
                .remove(&value.id.bytes());
        }
        if is_pending_withdrawal_ledger(value) {
            self.release_pending_withdrawal_index
                .insert(value.id.bytes(), 0);
        }
        if let Some((operation_id, owner)) = operation_owner {
            self.operation_owner_index.insert(operation_id, owner);
        }
        self.withdrawals.insert(value.id.bytes(), value_blob);
        self.counters.set(counters_blob);
        Ok(())
    }

    pub fn withdrawal(&self, id: [u8; 32]) -> Result<Option<WithdrawalRecord>, StorageError> {
        self.withdrawals
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn nonterminal_withdrawal_count(&self) -> Result<u64, StorageError> {
        Ok(self.counters()?.nonterminal_withdrawals)
    }

    pub fn deposit_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<DepositRecord>, StorageError> {
        let Some(owner) = self.operation_owner_index.get(&operation_id.get()) else {
            return Ok(None);
        };
        match decode::<OperationOwner>(&owner)? {
            OperationOwner::Deposit(id) => self.deposit(id),
            OperationOwner::Withdrawal(_) => Ok(None),
        }
    }

    pub fn withdrawal_for_operation(
        &self,
        operation_id: bridge_core::EvmOperationId,
    ) -> Result<Option<WithdrawalRecord>, StorageError> {
        let Some(owner) = self.operation_owner_index.get(&operation_id.get()) else {
            return Ok(None);
        };
        match decode::<OperationOwner>(&owner)? {
            OperationOwner::Withdrawal(id) => self.withdrawal(id),
            OperationOwner::Deposit(_) => Ok(None),
        }
    }

    pub fn put_evm_operation(&mut self, value: &EvmOperationRecord) -> Result<(), StorageError> {
        self.put_evm_operation_inner(value, None)
    }

    pub fn put_submitted_evm_operation(
        &mut self,
        value: &EvmOperationRecord,
        submitted_at_ns: u64,
        next_check_at_ns: u64,
    ) -> Result<(), StorageError> {
        self.put_evm_operation_inner(
            value,
            Some(ConfirmationSchedule {
                operation_id: value.id.get(),
                submitted_at_ns,
                next_check_at_ns,
                checks_completed: 0,
            }),
        )
    }

    fn put_evm_operation_inner(
        &mut self,
        value: &EvmOperationRecord,
        submission_schedule: Option<ConfirmationSchedule>,
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
                | EvmOperationState::Reverted { .. },
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
        counters.pending_evm_operations = adjust_active_count(
            counters.pending_evm_operations,
            previous.as_ref().map(is_pending_evm).unwrap_or(false),
            is_pending_evm(value),
        )?;
        counters.reverted_evm_operations = adjust_active_count(
            counters.reverted_evm_operations,
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
        let encoded_counters = encode(&counters)?;
        let previous_key = previous
            .as_ref()
            .map(evm_state_index_key)
            .transpose()?
            .flatten();
        let next_key = evm_state_index_key(value)?;
        let removes_payload = matches!(
            value.state,
            EvmOperationState::Submitted { .. }
                | EvmOperationState::Confirmed { .. }
                | EvmOperationState::Reverted { .. }
        );
        let removes_owner = matches!(
            value.state,
            EvmOperationState::Confirmed { .. } | EvmOperationState::Reverted { .. }
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
            Some(match decode::<OperationOwner>(&owner)? {
                OperationOwner::Deposit(id) => (SettlementJobKind::Deposit, id),
                OperationOwner::Withdrawal(id) => (SettlementJobKind::Withdrawal, id),
            })
        } else {
            None
        };
        let removes_schedule = matches!(
            value.state,
            EvmOperationState::Confirmed { .. } | EvmOperationState::Reverted { .. }
        );
        let operation_key = value.id.get().to_sql_bytes();
        self.handle.update(|connection| {
            if let Some(previous_key) = previous_key {
                connection.execute(
                    "DELETE FROM evm_state_index WHERE key = ?1",
                    params![previous_key.to_sql_bytes()],
                )?;
                decrement_table_count(connection, "evm_state_index")?;
            }
            if let Some(next_key) = next_key {
                connection.execute(
                    "INSERT INTO evm_state_index(key, value) VALUES (?1, ?2)",
                    params![next_key.to_sql_bytes(), 0u8.to_sql_bytes()],
                )?;
                increment_table_count(connection, "evm_state_index")?;
            }
            connection.execute(
                "INSERT INTO evm_operations(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![operation_key.clone(), encoded_value.to_sql_bytes()],
            )?;
            if previous.is_none() {
                increment_table_count(connection, "evm_operations")?;
            }
            if removes_payload && payload_present {
                connection.execute(
                    "DELETE FROM evm_execution_payloads WHERE key = ?1",
                    params![operation_key.clone()],
                )?;
                decrement_table_count(connection, "evm_execution_payloads")?;
            }
            if owner_present {
                connection.execute(
                    "DELETE FROM operation_owner_index WHERE key = ?1",
                    params![operation_key],
                )?;
                decrement_table_count(connection, "operation_owner_index")?;
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
            connection.execute(
                "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                params![encoded_counters.to_sql_bytes()],
            )
        })?;
        self.counters.value = encoded_counters;
        Ok(())
    }

    pub fn evm_operation(&self, id: u64) -> Result<Option<EvmOperationRecord>, StorageError> {
        self.evm_operations
            .get(&id)
            .map(|blob| decode(&blob))
            .transpose()
    }

    pub fn admit_withdrawal_notification_attempt(
        &mut self,
        caller: Principal,
        now_ns: u64,
    ) -> Result<(), WithdrawalAttemptAdmissionError> {
        const WINDOW_NS: u64 = 10 * 60 * 1_000_000_000;
        const GLOBAL_WINDOW_LIMIT: u8 = 32;
        const CALLER_WINDOW_LIMIT: u8 = 4;

        let window_id = now_ns / WINDOW_NS;
        let mut control = decode::<WithdrawalAttemptControl>(self.withdrawal_attempt_control.get())
            .map_err(|_| WithdrawalAttemptAdmissionError::Storage)?;
        if control.window_id != window_id {
            control = WithdrawalAttemptControl {
                window_id,
                ..WithdrawalAttemptControl::default()
            };
        }
        let caller_count = control
            .caller_counts
            .iter()
            .find(|quota| quota.caller == caller)
            .map(|quota| quota.count)
            .unwrap_or(0);
        if control.global_count >= GLOBAL_WINDOW_LIMIT || caller_count >= CALLER_WINDOW_LIMIT {
            let retry_after_ns = WINDOW_NS.saturating_sub(now_ns % WINDOW_NS);
            return Err(WithdrawalAttemptAdmissionError::RateLimited {
                retry_after_seconds: retry_after_ns.saturating_add(999_999_999) / 1_000_000_000,
            });
        }

        control.global_count = control
            .global_count
            .checked_add(1)
            .ok_or(WithdrawalAttemptAdmissionError::Storage)?;
        match control
            .caller_counts
            .iter_mut()
            .find(|quota| quota.caller == caller)
        {
            Some(quota) => {
                quota.count = quota
                    .count
                    .checked_add(1)
                    .ok_or(WithdrawalAttemptAdmissionError::Storage)?;
            }
            None => control
                .caller_counts
                .push(WithdrawalAttemptCallerQuota { caller, count: 1 }),
        }

        let encoded_control =
            encode(&control).map_err(|_| WithdrawalAttemptAdmissionError::Storage)?;
        self.withdrawal_attempt_control.set(encoded_control);
        Ok(())
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
        let mut counters = self.counters()?;
        counters.reconciliation_holds = adjust_active_count(
            counters.reconciliation_holds,
            previous.as_ref().map(is_open_hold).unwrap_or(false),
            is_open_hold(value),
        )?;
        let encoded_counters = encode(&counters)?;
        if previous.as_ref().is_some_and(is_open_hold) {
            self.open_hold_index.remove(&value.id.get());
        }
        if is_open_hold(value) {
            self.open_hold_index.insert(value.id.get(), 0);
        }
        self.reconciliation_holds
            .insert(value.id.get(), encoded_value);
        self.counters.set(encoded_counters);
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
        let previous_counters = self.counters()?;
        let mut counters = previous_counters;
        counters.reconciliation_holds = counters
            .reconciliation_holds
            .checked_sub(1)
            .ok_or(StorageError::CounterUnderflow)?;
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
                    (
                        "deposits",
                        next.id.bytes(),
                        encode(previous)?,
                        encode(next)?,
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
                    counters.nonterminal_withdrawals = adjust_active_count(
                        counters.nonterminal_withdrawals,
                        is_nonterminal_withdrawal(previous),
                        is_nonterminal_withdrawal(next),
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
            let update_parent = if table == "deposits" {
                "UPDATE deposits SET value = ?1 WHERE key = ?2"
            } else {
                "UPDATE withdrawals SET value = ?1 WHERE key = ?2"
            };
            connection.execute(
                update_parent,
                params![parent_blob.to_sql_bytes(), key.clone()],
            )?;
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
            connection.execute(
                "DELETE FROM open_hold_index WHERE key = ?1",
                params![hold.id.get().to_sql_bytes()],
            )?;
            decrement_table_count(connection, "open_hold_index")?;
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
                "UPDATE singleton_state SET counters = ?1 WHERE id = 1",
                params![counters_blob.to_sql_bytes()],
            )?;
            resolve_hold_bundle_db_failpoint(ResolveHoldBundleFailpoint::SingletonState)
        })?;
        self.counters.value = counters_blob;
        Ok(())
    }

    pub fn status_counts(&self) -> Result<StorageCounts, StorageError> {
        let counters = self.counters()?;
        let audit_retention: AuditRetentionState = decode(self.audit_retention.get())?;

        Ok(StorageCounts {
            deposits: self.deposits.len(),
            withdrawals: self.withdrawals.len(),
            pending_evm_operations: counters.pending_evm_operations,
            reconciliation_holds: counters.reconciliation_holds,
            pending_ledger_operations: counters.pending_ledger_operations,
            reserved_deposit_mint_amount: counters.reserved_deposit_mint_amount,
            reserved_deposit_mint_operations: counters.reserved_deposit_mint_operations,
            reverted_evm_operations: counters.reverted_evm_operations,
            last_safe_base_block: self.external_progress()?.last_safe_base_block,
            active_evm_payloads: self.evm_execution_payloads.len(),
            retained_audit_events: self.audit_events.len(),
            pruned_audit_events: audit_retention.pruned_count,
            retained_deposit_index_entries: self.deposit_owner_index.len(),
        })
    }
}

fn is_pending_evm(value: &EvmOperationRecord) -> bool {
    !matches!(
        value.state,
        EvmOperationState::Confirmed { .. } | EvmOperationState::Reverted { .. }
    )
}

fn is_reverted_evm(value: &EvmOperationRecord) -> bool {
    matches!(value.state, EvmOperationState::Reverted { .. })
}

fn is_open_hold(value: &ReconciliationHoldRecord) -> bool {
    matches!(value.state, ReconciliationHoldState::Open)
}

fn is_pending_deposit_ledger(value: &DepositRecord) -> bool {
    matches!(value.state, bridge_core::DepositState::PullPending)
}

fn is_deposit_mint_reserved(value: &DepositRecord) -> bool {
    !matches!(
        value.state,
        bridge_core::DepositState::Minted { .. }
            | bridge_core::DepositState::MintReverted { .. }
            | bridge_core::DepositState::Cancelled { .. }
    )
}

fn adjust_reserved_mint_amount(
    current: u128,
    previous: Option<&DepositRecord>,
    next: &DepositRecord,
) -> Result<u128, StorageError> {
    let without_previous = if previous.is_some_and(is_deposit_mint_reserved) {
        current
            .checked_sub(previous.expect("checked previous").net_amount.get())
            .ok_or(StorageError::CounterUnderflow)?
    } else {
        current
    };
    if is_deposit_mint_reserved(next) {
        without_previous
            .checked_add(next.net_amount.get())
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
    !matches!(
        value.state,
        WithdrawalState::Released { .. } | WithdrawalState::Refunded { .. }
    )
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
        DepositId, DepositRequest, DepositState, EvmCallIntent, EvmOperationEvent, EvmOperationId,
        EvmOperationKind, HoldId, LedgerOperation, LedgerTransferIdentity,
        ReconciliationArchiveRange, ReconciliationHoldRecord, ReconciliationHoldState,
        ReconciliationLedgerPage, ReconciliationScanPhase, ReconciliationScanProgress,
        ReconciliationTarget, RefundEligibility, RefundReason, RequestReference, Settlement,
        TransferAttempt, WithdrawalEvent, WithdrawalHoldResolution, WithdrawalId,
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

    fn deposit() -> DepositRecord {
        let mut deposit = DepositRecord::accept(
            DepositRequest {
                id: DepositId::new([1; 32]),
                payload_hash: [2; 32],
                gross_amount: Amount::new(110),
                user_max_service_fee: Amount::new(10),
                transfer: transfer(LedgerOperation::PullDeposit, 110, 10),
            },
            BaseMintSnapshot {
                confirmed_block_number: 1,
                confirmed_block_timestamp: 1,
                service_fee: Amount::new(10),
                max_service_fee: Amount::new(20),
                per_deposit_limit: Amount::new(1_000),
                mint_window_limit: Amount::new(10_000),
                mint_window_started_at: 0,
                mint_window_duration: 100,
                minted_in_window: Amount::ZERO,
            },
        )
        .expect("valid deposit");
        deposit
            .apply(DepositEvent::PullSucceeded {
                ledger_block_index: 4,
            })
            .expect("escrowed");
        deposit
    }

    fn mint_snapshot() -> BaseMintSnapshot {
        BaseMintSnapshot {
            confirmed_block_number: 1,
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
            vec![1],
            [4; 32],
            Amount::new(100),
            Amount::new(80),
            Amount::new(10),
        )
        .expect("valid withdrawal");
        withdrawal
            .apply(WithdrawalEvent::StartRelease {
                attempt: Box::new(TransferAttempt {
                    attempt_no: 0,
                    identity: transfer(LedgerOperation::ReleaseWithdrawal, 85, 20),
                }),
                settlement: Settlement {
                    amount_out: Amount::new(85),
                    service_fee: Amount::new(10),
                    ledger_fee: Amount::new(5),
                },
            })
            .expect("release pending");
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
            ecdsa_key_name: "test_key".into(),
            ecdsa_derivation_path: vec![],
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
            eth_floor_wei: 1,
            cycles_floor: 1,
            settlement_cycle_ceiling: 1,
            governance_principal: principal,
            pause_principals: vec![principal],
            finance_administrator: principal,
            fee_recipient: FeeRecipientConfig {
                owner: principal,
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

    #[test]
    #[serial]
    fn configured_store_starts_with_new_deposits_paused() {
        let store = StableStore::init_configured(VectorMemory::default(), &config())
            .expect("initialize configured store");
        assert!(
            store
                .admin_state()
                .expect("read administrator state")
                .deposits_paused
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
            let mut record = deposit();
            record.id = DepositId::new([tag; 32]);
            record.payload_hash = [2; 32];
            let mut deposit_intent = intent([tag; 32], principal);
            deposit_intent.owner_sequence = store
                .next_deposit_sequence(principal)
                .expect("read owner sequence");
            store
                .admit_deposit(principal, &deposit_intent, &record, None)
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
            let mut record = deposit();
            record.id = DepositId::new([tag; 32]);
            let mut deposit_intent = intent([tag; 32], owner);
            deposit_intent.owner_sequence = store
                .next_deposit_sequence(owner)
                .expect("read owner sequence");
            store
                .admit_deposit(owner, &deposit_intent, &record, None)
                .expect("admit deposit");
        }
        let mut other_record = deposit();
        other_record.id = DepositId::new([200; 32]);
        let other_intent = intent([200; 32], other);
        store
            .admit_deposit(other, &other_intent, &other_record, None)
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
    fn deposit_admission_rejects_replay_without_duplicate_index_entry() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize stable store");
        let owner = Principal::self_authenticating([3; 32]);
        let record = deposit();
        let intent = intent(record.id.bytes(), owner);
        store
            .admit_deposit(owner, &intent, &record, None)
            .expect("first admission");
        assert!(matches!(
            store.admit_deposit(owner, &intent, &record, None),
            Err(StorageError::Core(CoreError::ConflictingReplay))
        ));
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
    fn deposit_admission_constraint_failure_rolls_back_records_indexes_and_counters() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize rollback fixture");
        let owner = Principal::self_authenticating([31; 32]);
        let record = deposit();
        let intent = intent(record.id.bytes(), owner);
        let conflicting_index = deposit_owner_index_key(owner, 0).expect("index key");
        store
            .deposit_owner_index
            .insert(conflicting_index, [99; 32]);
        let before = store.counters().expect("counters before");

        assert_eq!(
            store.admit_deposit(owner, &intent, &record, None),
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
        let record = deposit();
        let intent = intent(record.id.bytes(), owner);

        // Model the state immediately before the final ETH balance await.
        let mut before_observation = store.counters().expect("counters before observation");
        before_observation.nonterminal_withdrawals = 1;
        store
            .set_counters(&before_observation)
            .expect("seed withdrawal reservation");
        let progress_before = store
            .external_progress()
            .expect("progress before observation");

        // The ETH response was obtained, but a competing finalization message ran while the
        // caller was suspended and released the withdrawal reservation. The old implementation
        // could combine that pre-finalization ETH balance with this newer, smaller counter.
        let mut after_interrupt = before_observation;
        after_interrupt.nonterminal_withdrawals = 0;
        store
            .set_counters(&after_interrupt)
            .expect("model competing finalization");

        let result = store.admit_deposit(
            owner,
            &intent,
            &record,
            Some(DepositReserveAdmission {
                audit_caller: owner,
                expected_counters: before_observation,
                expected_observation_generation: progress_before.reserve_observation_generation,
                observed_at_ns: 10,
                eth_balance_wei: 20_000_000,
                cycles_balance: 20_000_000,
                reserve_policy: config().reserve_policy(),
                mint_snapshot: mint_snapshot(),
            }),
        );

        assert_eq!(result, Err(StorageError::StaleReserveObservation));
        assert_eq!(
            store.counters().expect("counters after rejection"),
            after_interrupt
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
    fn deposit_admission_atomically_commits_reserve_observation_and_audit() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory.clone()).expect("initialize admission fixture");
        let owner = Principal::self_authenticating([33; 32]);
        let record = deposit();
        let intent = intent(record.id.bytes(), owner);
        let expected_counters = store.counters().expect("counters before admission");
        let expected_progress = store
            .external_progress()
            .expect("progress before admission");

        store
            .admit_deposit(
                owner,
                &intent,
                &record,
                Some(DepositReserveAdmission {
                    audit_caller: owner,
                    expected_counters,
                    expected_observation_generation: expected_progress
                        .reserve_observation_generation,
                    observed_at_ns: 42,
                    eth_balance_wei: 20_000_000,
                    cycles_balance: 20_000_000,
                    reserve_policy: config().reserve_policy(),
                    mint_snapshot: mint_snapshot(),
                }),
            )
            .expect("admit with fresh resource observation");

        let progress = store.external_progress().expect("progress after admission");
        assert_eq!(progress.reserve_observation_generation, 1);
        assert_eq!(progress.last_reserve_observation_ns, 42);
        assert_eq!(progress.last_eth_balance_wei, 20_000_000);
        assert!(progress.reserve_sufficient);
        assert_eq!(
            store
                .counters()
                .expect("counters after admission")
                .reserved_deposit_mint_operations,
            1
        );
        assert!(matches!(
            store
                .audit_events(0, 10)
                .expect("reserve audit")
                .events
                .as_slice(),
            [AuditEvent {
                kind: AuditEventKind::ReserveGateChanged { sufficient: true },
                ..
            }]
        ));

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
            1
        );
        assert!(reopened
            .deposit(record.id.bytes())
            .expect("reopened deposit")
            .is_some());
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

        let mut record = deposit();
        record.id = DepositId::new([21; 32]);
        let mut gap = intent(record.id.bytes(), owner);
        gap.owner_sequence = 1;
        assert!(matches!(
            store.admit_deposit(owner, &gap, &record, None),
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
            .admit_deposit(owner, &accepted, &record, None)
            .expect("accept sequence zero");
        drop(store);

        let reopened = StableStore::reopen(memory).expect("reopen stable store");
        assert_eq!(
            reopened
                .next_deposit_sequence(owner)
                .expect("reopened sequence"),
            1
        );
    }

    fn held_deposit() -> (DepositRecord, ReconciliationHoldRecord) {
        let mut deposit = DepositRecord::accept(
            DepositRequest {
                id: DepositId::new([11; 32]),
                payload_hash: [12; 32],
                gross_amount: Amount::new(110),
                user_max_service_fee: Amount::new(10),
                transfer: transfer(LedgerOperation::PullDeposit, 110, 40),
            },
            BaseMintSnapshot {
                confirmed_block_number: 1,
                confirmed_block_timestamp: 1,
                service_fee: Amount::new(10),
                max_service_fee: Amount::new(20),
                per_deposit_limit: Amount::new(1_000),
                mint_window_limit: Amount::new(10_000),
                mint_window_started_at: 0,
                mint_window_duration: 100,
                minted_in_window: Amount::ZERO,
            },
        )
        .expect("valid deposit");
        let hold_id = HoldId::new(12);
        deposit
            .apply(DepositEvent::PullAmbiguous { hold_id })
            .expect("hold deposit");
        let hold = ReconciliationHoldRecord::open(
            hold_id,
            RequestReference::Deposit(deposit.id),
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
            previous.state = DepositState::PullPending;
            store.put_deposit(&previous).expect("seed parent");
            let hold_id = store.next_hold_id().expect("candidate");
            let mut next = previous.clone();
            next.apply(DepositEvent::PullAmbiguous { hold_id })
                .expect("ambiguous");
            let hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::Deposit(next.id),
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
            previous.state = DepositState::PullPending;
            store.put_deposit(&previous).expect("seed");
            let hold_id = store.next_hold_id().expect("candidate");
            let mut held = previous.clone();
            held.apply(DepositEvent::PullAmbiguous { hold_id })
                .expect("held");
            let hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::Deposit(held.id),
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
                    DepositHoldResolution::Absent {
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
        let deposit = deposit();
        let withdrawal = withdrawal();
        let mut evm = EvmOperationRecord::prepared(
            EvmOperationId::new(6),
            [6; 32],
            EvmOperationKind::AcknowledgeRelease,
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
            pending_evm_operations: 1,
            reconciliation_holds: 1,
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
                reverted_evm_operations: 0,
                last_safe_base_block: 0,
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

        let store = StableStore::init(VectorMemory::default()).expect("initialize schema v5");
        assert_eq!(store.schema_version(), SCHEMA_VERSION);
    }

    #[test]
    #[serial]
    fn non_current_schema_is_rejected_without_migration() {
        assert_ne!(SCHEMA_VERSION, 2);
        assert_eq!(WIRE_VERSION, 6);
    }

    #[test]
    #[serial]
    fn submitted_evm_schedule_is_created_ordered_and_removed_atomically() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = EvmOperationId::new(41);
        let mut owner = deposit();
        owner
            .apply(DepositEvent::PrepareMint { operation_id })
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
            .put_submitted_evm_operation(&operation, 10, 200)
            .expect("persist operation and schedule");
        assert_eq!(
            store.evm_operation(operation_id.get()).expect("operation"),
            Some(operation)
        );
        assert_eq!(
            store.earliest_confirmation_schedule().expect("earliest"),
            Some(ConfirmationSchedule {
                operation_id: operation_id.get(),
                submitted_at_ns: 10,
                next_check_at_ns: 200,
                checks_completed: 0,
            })
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
            .apply(DepositEvent::PrepareMint {
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
            .put_submitted_evm_operation(&earlier, 20, 100)
            .expect("insert earlier schedule");
        assert_eq!(
            store
                .earliest_confirmation_schedule()
                .expect("ordered earliest")
                .map(|schedule| schedule.operation_id),
            Some(99)
        );
        store
            .remove_confirmation_schedule(99)
            .expect("remove earlier");

        operation.state = EvmOperationState::Confirmed {
            transaction_hash: [4; 32],
            receipt_block_number: 7,
            confirmed_block_number: 8,
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
    fn leased_confirmation_schedule_remains_visible_to_terminal_bundle() {
        let mut store = StableStore::init(VectorMemory::default()).expect("initialize");
        let operation_id = EvmOperationId::new(42);
        let mut owner = deposit();
        owner
            .apply(DepositEvent::PrepareMint { operation_id })
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
            .put_submitted_evm_operation(&operation, 10, 200)
            .expect("persist operation and schedule");

        let SettlementJobClaim::Claimed(job) = store
            .claim_due_settlement_job(200, 300)
            .expect("lease due confirmation")
        else {
            panic!("due confirmation was not leased")
        };
        assert_eq!(
            store
                .confirmation_schedule(operation_id.get())
                .expect("leased schedule"),
            Some(ConfirmationSchedule {
                operation_id: operation_id.get(),
                submitted_at_ns: 200,
                next_check_at_ns: 300,
                checks_completed: 0,
            })
        );
        store
            .finish_settlement_job(&job, None, 1, Some("checks exhausted"))
            .expect("stop leased confirmation");
        assert_eq!(
            store
                .confirmation_schedule(operation_id.get())
                .expect("stopped schedule lookup"),
            None
        );
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
    fn schema_v6_reopen_preserves_schedule_quota_and_scheduler_health() {
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
                .apply(DepositEvent::PrepareMint {
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
                .put_submitted_evm_operation(
                    &operation,
                    schedule.submitted_at_ns,
                    schedule.next_check_at_ns,
                )
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

        let mut reopened = StableStore::reopen(memory).expect("reopen v6");
        assert_eq!(reopened.schema_version(), 6);
        assert_eq!(
            reopened.confirmation_schedule(43).expect("schedule"),
            Some(schedule)
        );
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
        assert_eq!(
            store.counters().expect("counters").pending_evm_operations,
            1
        );
        evm.state = EvmOperationState::Confirmed {
            transaction_hash: [2; 32],
            receipt_block_number: 2,
            confirmed_block_number: 3,
        };
        store.put_evm_operation(&evm).expect("confirm EVM");
        store.put_evm_operation(&evm).expect("replay confirmed EVM");
        assert_eq!(
            store.counters().expect("counters").pending_evm_operations,
            0
        );

        let (_, mut hold) = held_deposit();
        store
            .put_open_reconciliation_hold(&hold)
            .expect("insert open hold");
        store
            .put_open_reconciliation_hold(&hold)
            .expect("replay open hold");
        assert_eq!(store.counters().expect("counters").reconciliation_holds, 1);
        hold.state = ReconciliationHoldState::ResolvedAbsent {
            history_watermark: 9,
        };
        store
            .put_reconciliation_hold(&hold)
            .expect("resolve hold internally");
        store
            .put_reconciliation_hold(&hold)
            .expect("replay resolved hold internally");
        assert_eq!(store.counters().expect("counters").reconciliation_holds, 0);
    }

    #[test]
    #[serial]
    fn evm_payload_is_exclusive_retained_after_signing_and_removed_after_submission() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = EvmOperationId::new(77);
        let mut deposit = deposit();
        deposit
            .apply(DepositEvent::PrepareMint { operation_id })
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
            .expect("remove payload with submitted state");
        assert!(store
            .evm_execution_payload(operation_id.get())
            .expect("read submitted payload")
            .is_none());
        assert!(store
            .deposit_for_operation(operation_id)
            .expect("read nonterminal owner")
            .is_some());

        operation
            .apply(EvmOperationEvent::Confirmed {
                transaction_hash: [8; 32],
                receipt_block_number: 20,
                confirmed_block_number: 21,
            })
            .expect("confirm operation");
        store
            .put_evm_operation(&operation)
            .expect("remove terminal owner index");
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
                confirmed_block_number: 21,
            })
        );
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
            100
        );
        let operation_id = EvmOperationId::new(41);
        pending
            .apply(DepositEvent::PrepareMint { operation_id })
            .expect("prepare mint");
        store
            .put_deposit(&pending)
            .expect("retain pending reservation");
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
            .apply(DepositEvent::PrepareMint { operation_id })
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
        pending.state = DepositState::PullPending;
        store
            .put_deposit(&pending)
            .expect("reserve pending deposit");
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .reserved_deposit_mint_amount,
            100
        );
        assert_eq!(
            store
                .counters()
                .expect("counters")
                .pending_ledger_operations,
            1
        );
        pending
            .apply(DepositEvent::PullFailed {
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
            confirmed_block_number: 99,
        };
        store.put_evm_operation(&evm).expect("mark reverted");
        store.put_evm_operation(&evm).expect("replay reverted");
        let counts = store.status_counts().expect("status");
        assert_eq!(counts.pending_evm_operations, 0);
        assert_eq!(counts.reverted_evm_operations, 1);
    }

    #[test]
    #[serial]
    fn acknowledgement_bundle_is_all_or_error_on_operation_id_overflow() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut withdrawal = withdrawal();
        withdrawal
            .apply(WithdrawalEvent::ReleaseSucceeded {
                ledger_block_index: 77,
            })
            .expect("release transferred");
        store
            .put_withdrawal(&withdrawal)
            .expect("persist release transfer");
        store
            .set_counters(&CounterState {
                next_evm_operation_id: u64::MAX,
                ..store.counters().expect("counters")
            })
            .expect("seed operation id overflow");
        let before = withdrawal.clone();
        assert_eq!(
            crate::tasks::prepare_acknowledgement_in_store(&mut store, &config(), &mut withdrawal,),
            Err(StorageError::CounterOverflow)
        );
        assert_eq!(withdrawal, before);
        assert_eq!(
            store
                .withdrawal(withdrawal.id.bytes())
                .expect("read withdrawal"),
            Some(before)
        );
        assert_eq!(
            store
                .awaiting_nonce_evm_count()
                .expect("awaiting nonce count"),
            0
        );
    }

    #[test]
    #[serial]
    fn acknowledgement_bundle_persists_parent_operation_and_intent_together() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let mut withdrawal = withdrawal();
        withdrawal
            .apply(WithdrawalEvent::ReleaseSucceeded {
                ledger_block_index: 78,
            })
            .expect("release transferred");
        store
            .put_withdrawal(&withdrawal)
            .expect("persist release transfer");
        crate::tasks::prepare_acknowledgement_in_store(&mut store, &config(), &mut withdrawal)
            .expect("prepare acknowledgement");
        let operation_id = match &withdrawal.state {
            WithdrawalState::AcknowledgePending { operation_id, .. } => *operation_id,
            _ => panic!("withdrawal must be acknowledgement pending"),
        };
        assert!(store
            .evm_operation(operation_id.get())
            .expect("operation")
            .is_some());
        assert!(store
            .evm_call_intent(operation_id.get())
            .expect("intent")
            .is_some());
        assert_eq!(
            store
                .withdrawal(withdrawal.id.bytes())
                .expect("stored withdrawal"),
            Some(withdrawal)
        );
    }

    fn acknowledgement_bundle_fixture(
        store: &mut StableStore,
        operation_id: EvmOperationId,
    ) -> (WithdrawalRecord, EvmOperationRecord, EvmCallIntent) {
        let current = withdrawal();
        store
            .put_withdrawal(&current)
            .expect("persist release-pending withdrawal");
        let mut next = current;
        next.apply(WithdrawalEvent::ReleaseSucceeded {
            ledger_block_index: 77,
        })
        .expect("confirm ledger release");
        next.apply(WithdrawalEvent::PrepareAcknowledgement { operation_id })
            .expect("prepare acknowledgement");
        let operation = EvmOperationRecord::queued(
            operation_id,
            next.payload_hash,
            EvmOperationKind::AcknowledgeRelease,
        );
        let intent = evm_intent(operation_id, next.payload_hash);
        (next, operation, intent)
    }

    #[derive(Debug, PartialEq, Eq)]
    struct AcknowledgementBundleSnapshot {
        accounting: AccountingState,
        counters: CounterState,
        withdrawal: Option<WithdrawalRecord>,
        operation: Option<EvmOperationRecord>,
        payload: Option<EvmExecutionPayload>,
        owner: Option<StableBlob>,
        evm_index_present: bool,
        release_index_present: bool,
        hold: Option<ReconciliationHoldRecord>,
        open_hold_index_present: bool,
        scan: Option<ReconciliationScanProgress>,
        scan_table_count: u64,
        counts: StorageCounts,
    }

    fn acknowledgement_bundle_snapshot(
        store: &StableStore,
        withdrawal_id: WithdrawalId,
        operation_id: EvmOperationId,
    ) -> AcknowledgementBundleSnapshot {
        let operation =
            EvmOperationRecord::queued(operation_id, [4; 32], EvmOperationKind::AcknowledgeRelease);
        let index_key = evm_state_index_key(&operation)
            .expect("encode EVM index")
            .expect("queued operation is indexed");
        let hold_id = store
            .withdrawal(withdrawal_id.bytes())
            .expect("read withdrawal for hold")
            .and_then(|withdrawal| match withdrawal.state {
                WithdrawalState::ReconciliationHold { hold_id, .. } => Some(hold_id),
                WithdrawalState::ReleaseTransferred { source_hold, .. }
                | WithdrawalState::AcknowledgePending { source_hold, .. } => source_hold,
                _ => None,
            });
        AcknowledgementBundleSnapshot {
            accounting: store.accounting().expect("read accounting"),
            counters: store.counters().expect("read counters"),
            withdrawal: store
                .withdrawal(withdrawal_id.bytes())
                .expect("read withdrawal"),
            operation: store
                .evm_operation(operation_id.get())
                .expect("read EVM operation"),
            payload: store
                .evm_execution_payload(operation_id.get())
                .expect("read EVM payload"),
            owner: store.operation_owner_index.get(&operation_id.get()),
            evm_index_present: store.evm_state_index.get(&index_key).is_some(),
            release_index_present: store
                .release_pending_withdrawal_index
                .get(&withdrawal_id.bytes())
                .is_some(),
            hold: hold_id.and_then(|id| store.reconciliation_hold(id.get()).expect("read hold")),
            open_hold_index_present: hold_id
                .is_some_and(|id| store.open_hold_index.get(&id.get()).is_some()),
            scan: hold_id.and_then(|id| {
                store
                    .reconciliation_scan(&ReconciliationTarget::Hold(id))
                    .expect("scan")
            }),
            scan_table_count: store.table_count("reconciliation_scans"),
            counts: store.status_counts().expect("read status counts"),
        }
    }

    #[test]
    #[serial]
    fn acknowledgement_bundle_atomically_confirms_fee_and_prepares_evm_operation() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = store
            .next_evm_operation_id()
            .expect("read candidate operation ID");
        let (withdrawal, operation, intent) =
            acknowledgement_bundle_fixture(&mut store, operation_id);

        store
            .commit_acknowledgement_bundle(&withdrawal, &operation, &intent)
            .expect("commit acknowledgement bundle");

        let snapshot = acknowledgement_bundle_snapshot(&store, withdrawal.id, operation_id);
        assert_eq!(snapshot.accounting.fee_reserve, Amount::new(10));
        assert_eq!(
            snapshot.accounting.confirmed_withdrawal_fees,
            Amount::new(10)
        );
        assert_eq!(
            snapshot.counters.next_evm_operation_id,
            operation_id.get() + 1
        );
        assert_eq!(snapshot.counters.pending_evm_operations, 1);
        assert_eq!(snapshot.counters.awaiting_nonce_evm_operations, 1);
        assert_eq!(snapshot.counters.pending_ledger_operations, 0);
        assert_eq!(snapshot.withdrawal, Some(withdrawal));
        assert_eq!(snapshot.operation, Some(operation));
        assert_eq!(
            snapshot.payload,
            Some(EvmExecutionPayload::AwaitingNonce(intent))
        );
        assert!(snapshot.owner.is_some());
        assert!(snapshot.evm_index_present);
        assert!(!snapshot.release_index_present);
    }

    #[test]
    #[serial]
    fn acknowledgement_bundle_confirms_fee_after_a_resolved_hold_transfer() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = store
            .next_evm_operation_id()
            .expect("read candidate operation ID");
        let mut withdrawal = withdrawal();
        withdrawal
            .apply(WithdrawalEvent::ReleaseSucceeded {
                ledger_block_index: 79,
            })
            .expect("resolve held release as transferred");
        store
            .put_withdrawal(&withdrawal)
            .expect("persist resolved transfer and hold");
        withdrawal
            .apply(WithdrawalEvent::PrepareAcknowledgement { operation_id })
            .expect("prepare acknowledgement");
        let operation = EvmOperationRecord::queued(
            operation_id,
            withdrawal.payload_hash,
            EvmOperationKind::AcknowledgeRelease,
        );
        let intent = evm_intent(operation_id, withdrawal.payload_hash);

        store
            .commit_acknowledgement_bundle(&withdrawal, &operation, &intent)
            .expect("commit acknowledgement after hold resolution");

        let accounting = store.accounting().expect("read accounting");
        assert_eq!(accounting.fee_reserve, Amount::new(10));
        assert_eq!(accounting.confirmed_withdrawal_fees, Amount::new(10));
    }

    #[test]
    #[serial]
    fn acknowledgement_bundle_rolls_back_every_write_failpoint() {
        for failpoint in [
            AcknowledgementBundleFailpoint::Encode,
            AcknowledgementBundleFailpoint::ExecutionPayload,
            AcknowledgementBundleFailpoint::EvmOperation,
            AcknowledgementBundleFailpoint::EvmStateIndex,
            AcknowledgementBundleFailpoint::OperationOwnerIndex,
            AcknowledgementBundleFailpoint::Withdrawal,
            AcknowledgementBundleFailpoint::ReleasePendingIndex,
            AcknowledgementBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let operation_id = store
                .next_evm_operation_id()
                .expect("read candidate operation ID");
            let (withdrawal, operation, intent) =
                acknowledgement_bundle_fixture(&mut store, operation_id);
            let before = acknowledgement_bundle_snapshot(&store, withdrawal.id, operation_id);
            set_acknowledgement_bundle_failpoint(Some(failpoint));
            assert!(store
                .commit_acknowledgement_bundle(&withdrawal, &operation, &intent)
                .is_err());
            set_acknowledgement_bundle_failpoint(None);
            assert_eq!(
                acknowledgement_bundle_snapshot(&store, withdrawal.id, operation_id),
                before,
                "failpoint {failpoint:?} changed stable state"
            );
        }
    }

    #[test]
    #[serial]
    fn held_acknowledgement_bundle_rolls_back_hold_and_operation_together() {
        for failpoint in [
            AcknowledgementBundleFailpoint::Encode,
            AcknowledgementBundleFailpoint::ExecutionPayload,
            AcknowledgementBundleFailpoint::EvmOperation,
            AcknowledgementBundleFailpoint::EvmStateIndex,
            AcknowledgementBundleFailpoint::OperationOwnerIndex,
            AcknowledgementBundleFailpoint::Withdrawal,
            AcknowledgementBundleFailpoint::ReleasePendingIndex,
            AcknowledgementBundleFailpoint::ReconciliationHold,
            AcknowledgementBundleFailpoint::OpenHoldIndex,
            AcknowledgementBundleFailpoint::ReconciliationScan,
            AcknowledgementBundleFailpoint::SingletonState,
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory.clone()).expect("initialize");
            let hold_id = HoldId::new(55);
            let mut previous = withdrawal();
            let transfer = match &previous.state {
                WithdrawalState::ReleasePending { attempt, .. } => attempt.identity.clone(),
                _ => panic!("release pending fixture"),
            };
            previous
                .apply(WithdrawalEvent::ReleaseAmbiguous { hold_id })
                .expect("hold withdrawal");
            let previous_hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::Withdrawal(previous.id),
                transfer,
            );
            store
                .put_withdrawal(&previous)
                .expect("persist held withdrawal");
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
            resolve_withdrawal_hold(
                &mut next,
                &mut next_hold,
                WithdrawalHoldResolution::Succeeded {
                    ledger_block_index: 91,
                },
            )
            .expect("resolve hold in memory");
            next.apply(WithdrawalEvent::PrepareAcknowledgement { operation_id })
                .expect("prepare acknowledgement");
            let operation = EvmOperationRecord::queued(
                operation_id,
                next.payload_hash,
                EvmOperationKind::AcknowledgeRelease,
            );
            let intent = evm_intent(operation_id, next.payload_hash);
            let before = acknowledgement_bundle_snapshot(&store, next.id, operation_id);
            set_acknowledgement_bundle_failpoint(Some(failpoint));
            assert!(store
                .commit_acknowledgement_bundle_and_scan(
                    &next,
                    &operation,
                    &intent,
                    Some(&scan_target),
                )
                .is_err());
            set_acknowledgement_bundle_failpoint(None);
            assert_eq!(
                acknowledgement_bundle_snapshot(&store, next.id, operation_id),
                before,
                "failpoint {failpoint:?} changed held acknowledgement state"
            );
            drop(store);
            let reopened = StableStore::reopen(memory).expect("reopen");
            assert_eq!(
                acknowledgement_bundle_snapshot(&reopened, next.id, operation_id),
                before,
                "failpoint {failpoint:?} changed reopened held state"
            );
        }
    }

    #[test]
    #[serial]
    fn acknowledgement_bundle_rejects_stale_candidate_and_overflow_without_writes() {
        for (operation_id, next_counter) in [
            (EvmOperationId::new(0), 1),
            (EvmOperationId::new(u64::MAX), u64::MAX),
        ] {
            let memory = VectorMemory::default();
            let mut store = StableStore::init(memory).expect("initialize");
            let (withdrawal, operation, intent) =
                acknowledgement_bundle_fixture(&mut store, operation_id);
            let mut counters = store.counters().expect("read counters");
            counters.next_evm_operation_id = next_counter;
            store
                .set_counters(&counters)
                .expect("seed counter boundary");
            let before = acknowledgement_bundle_snapshot(&store, withdrawal.id, operation_id);
            assert!(store
                .commit_acknowledgement_bundle(&withdrawal, &operation, &intent)
                .is_err());
            assert_eq!(
                acknowledgement_bundle_snapshot(&store, withdrawal.id, operation_id),
                before
            );
        }
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
            next.apply(DepositEvent::PrepareMint { operation_id })
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
            previous.state = DepositState::PullPending;
            previous
                .apply(DepositEvent::PullAmbiguous { hold_id })
                .expect("hold deposit");
            let previous_hold = ReconciliationHoldRecord::open(
                hold_id,
                RequestReference::Deposit(previous.id),
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
                DepositHoldResolution::Succeeded {
                    ledger_block_index: 92,
                },
            )
            .expect("resolve hold in memory");
            next.apply(DepositEvent::PrepareMint { operation_id })
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
            assert!(
                store
                    .commit_deposit_mint_bundle_and_scan(
                        &next,
                        &operation,
                        &intent,
                        Some(&scan_target),
                    )
                    .is_err()
            );
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
        next.apply(DepositEvent::PrepareMint {
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

    #[derive(Debug, PartialEq, Eq)]
    struct TerminalBundleSnapshot {
        counters: CounterState,
        accounting: AccountingState,
        progress: ExternalProgress,
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
        next.apply(DepositEvent::PrepareMint { operation_id })
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
        let envelope = intent.assign_nonce(0);
        operation
            .apply(EvmOperationEvent::Prepared)
            .expect("prepared");
        let mut progress = store.external_progress().expect("progress");
        progress.next_evm_nonce = 1;
        store
            .prepare_evm_operation(&operation, &envelope, &progress)
            .expect("persist prepared");
        operation
            .apply(EvmOperationEvent::Submitted {
                transaction_hash: [9; 32],
            })
            .expect("submitted");
        store
            .put_submitted_evm_operation(&operation, 10, 20)
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
                    confirmed_block_number: 40,
                })
                .expect("reverted");
            let mut progress = store.external_progress().expect("progress");
            progress.last_safe_base_block = 40;
            let before = terminal_bundle_snapshot(&store, deposit_id, submitted.id);
            set_terminal_bundle_failpoint(Some(failpoint));
            assert!(store
                .commit_evm_terminal_bundle(
                    &terminal,
                    &progress,
                    Some((Principal::self_authenticating([5; 32]), 50, 40)),
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
    fn refund_bundle_is_inserted_only_once_for_a_withdrawal() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let operation_id = store.next_evm_operation_id().expect("candidate");
        let mut withdrawal = WithdrawalRecord::observed(
            WithdrawalId::new([8; 32]),
            Principal::self_authenticating([8; 32]).as_slice().to_vec(),
            [7; 32],
            Amount::new(2),
            Amount::new(2),
            Amount::new(10),
        )
        .expect("observed withdrawal");
        withdrawal
            .apply(WithdrawalEvent::StartRefund {
                operation_id,
                eligibility: RefundEligibility {
                    confirmed_base_block: 100,
                    base_status_pending: true,
                    release_transfer_proven_absent: true,
                    reason: RefundReason::AmountBelowMinimum,
                },
            })
            .expect("prepare refund");
        let operation = EvmOperationRecord::queued(
            operation_id,
            withdrawal.payload_hash,
            EvmOperationKind::RefundWithdrawal,
        );
        let intent = EvmCallIntent {
            operation_id,
            payload_hash: withdrawal.payload_hash,
            chain_id: 8453,
            contract: [1; 20],
            calldata: {
                let mut calldata = vec![0xf0, 0x65, 0xe1, 0xff];
                calldata.extend_from_slice(&withdrawal.id.bytes());
                calldata
            },
            gas_limit: 100_000,
            max_fee_per_gas: 10,
            max_priority_fee_per_gas: 1,
        };

        let progress = store.external_progress().expect("progress");
        assert!(store
            .commit_new_withdrawal_operation_bundle(&withdrawal, &operation, &intent, &progress)
            .expect("first insert"));
        assert!(!store
            .commit_new_withdrawal_operation_bundle(&withdrawal, &operation, &intent, &progress)
            .expect("idempotent replay"));
        assert_eq!(
            store
                .awaiting_nonce_evm_count()
                .expect("awaiting nonce count"),
            1
        );
        assert_eq!(store.withdrawals.len(), 1);
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
                        DepositHoldResolution::Succeeded {
                            ledger_block_index: 88,
                        },
                    )
                    .expect("resolve")
                    .outcome,
                ApplyOutcome::Applied
            );
            assert_eq!(store.counters().expect("counters").reconciliation_holds, 0);
        }

        let mut reopened = StableStore::reopen(memory).expect("reopen");
        assert!(matches!(
            reopened
                .deposit(deposit.id.bytes())
                .expect("read deposit")
                .expect("deposit exists")
                .state,
            DepositState::Escrowed {
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
                    DepositHoldResolution::Succeeded {
                        ledger_block_index: 88,
                    },
                )
                .expect("retry resolution")
                .outcome,
            ApplyOutcome::Idempotent
        );
        assert_eq!(
            reopened.counters().expect("counters").reconciliation_holds,
            0
        );
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
        assert_eq!(store.counters().expect("counters").reconciliation_holds, 0);
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
                confirmed_block_number: id,
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
                reverted_evm_operations: 0,
                last_safe_base_block: 0,
                active_evm_payloads: 0,
                retained_audit_events: 0,
                pruned_audit_events: 0,
                retained_deposit_index_entries: 0,
            }
        );
    }

    #[test]
    #[serial]
    fn counter_overflow_and_underflow_fail_before_record_write() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        store
            .set_counters(&CounterState {
                pending_evm_operations: u64::MAX,
                ..CounterState::default()
            })
            .expect("seed overflow counter");
        let evm = EvmOperationRecord::queued(
            EvmOperationId::new(1),
            [1; 32],
            EvmOperationKind::MintDeposit,
        );
        store
            .put_evm_call_intent(&evm_intent(evm.id, evm.payload_hash))
            .expect("insert EVM intent");
        assert_eq!(
            store.put_evm_operation(&evm),
            Err(StorageError::CounterOverflow)
        );
        assert_eq!(store.evm_operation(evm.id.get()).expect("read EVM"), None);

        store
            .set_counters(&CounterState::default())
            .expect("reset counters");
        store.put_evm_operation(&evm).expect("insert pending EVM");
        store
            .set_counters(&CounterState::default())
            .expect("corrupt count for underflow fixture");
        let mut confirmed = evm;
        confirmed.state = EvmOperationState::Confirmed {
            transaction_hash: [2; 32],
            receipt_block_number: 2,
            confirmed_block_number: 3,
        };
        assert_eq!(
            store.put_evm_operation(&confirmed),
            Err(StorageError::CounterUnderflow)
        );
        assert_eq!(
            store.evm_operation(evm.id.get()).expect("read EVM"),
            Some(evm)
        );
    }

    #[test]
    #[serial]
    fn deposit_quota_is_principal_scoped_and_resets_by_window() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let caller = Principal::self_authenticating([1; 32]);
        let other = Principal::self_authenticating([2; 32]);
        store
            .reserve_deposit_quota(caller, 1, 60, 3, 2)
            .expect("first admission");
        store
            .reserve_deposit_quota(caller, 2, 60, 3, 2)
            .expect("second admission");
        assert_eq!(
            store.reserve_deposit_quota(caller, 3, 60, 3, 2),
            Err(DepositQuotaError::RateLimited(DepositRateLimit {
                retry_after_seconds: 60
            }))
        );
        store
            .reserve_deposit_quota(other, 4, 60, 3, 2)
            .expect("global final slot");
        assert!(store
            .reserve_deposit_quota(Principal::self_authenticating([3; 32]), 5, 60, 3, 2)
            .is_err());
        store
            .reserve_deposit_quota(caller, 60_000_000_000, 60, 3, 2)
            .expect("new window resets quota");
    }

    #[test]
    #[serial]
    fn withdrawal_notification_attempts_are_rate_limited_per_caller() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let caller = Principal::self_authenticating([1; 32]);
        for now in 1..=4 {
            store
                .admit_withdrawal_notification_attempt(caller, now)
                .expect("attempt admitted");
        }
        assert!(matches!(
            store.admit_withdrawal_notification_attempt(caller, 5),
            Err(WithdrawalAttemptAdmissionError::RateLimited { .. })
        ));
        store
            .admit_withdrawal_notification_attempt(caller, 600_000_000_000)
            .expect("new window resets quota");
    }

    #[test]
    #[serial]
    fn withdrawal_notification_attempt_global_window_limit_is_enforced() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        for tag in 1..=32u8 {
            let caller = Principal::self_authenticating([tag; 32]);
            store
                .admit_withdrawal_notification_attempt(caller, 1)
                .expect("attempt admitted");
        }
        assert!(matches!(
            store
                .admit_withdrawal_notification_attempt(Principal::self_authenticating([99; 32]), 1),
            Err(WithdrawalAttemptAdmissionError::RateLimited { .. })
        ));
    }

    #[test]
    #[serial]
    fn base_snapshot_cache_is_bounded_by_ttl_progress_and_singleflight() {
        let memory = VectorMemory::default();
        let mut store = StableStore::init(memory).expect("initialize");
        let snapshot = BaseMintSnapshot {
            confirmed_block_number: 10,
            confirmed_block_timestamp: 10,
            service_fee: Amount::new(1),
            max_service_fee: Amount::new(2),
            per_deposit_limit: Amount::new(100),
            mint_window_limit: Amount::new(1_000),
            mint_window_started_at: 0,
            mint_window_duration: 100,
            minted_in_window: Amount::ZERO,
        };
        assert!(store
            .begin_base_snapshot_refresh(100, 300, 60)
            .expect("begin refresh"));
        assert!(!store
            .begin_base_snapshot_refresh(101, 300, 60)
            .expect("singleflight rejects overlap"));
        store
            .finish_base_snapshot_refresh(110, snapshot, [7; 20], false)
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

    #[allow(clippy::type_complexity)]
    fn fee_payout_bundle_snapshot(
        store: &StableStore,
        payout_id: u64,
        target: &ReconciliationTarget,
    ) -> (
        Option<crate::admin::FeePayoutRecord>,
        CounterState,
        AccountingState,
        Option<u8>,
        Option<ReconciliationScanProgress>,
        Option<u64>,
        AuditRetentionState,
        Vec<AuditEvent>,
        [u64; 4],
    ) {
        (
            store.fee_payout(payout_id).expect("payout"),
            store.counters().expect("counters"),
            store.accounting().expect("accounting"),
            store
                .fee_payout_state_index
                .get(&fee_payout_index_key_for_state(payout_id, 0).expect("index key")),
            store.reconciliation_scan(target).expect("scan"),
            store.last_audit_sequence().expect("audit sequence"),
            decode(store.audit_retention.get()).expect("audit retention"),
            store.audit_events(0, 100).expect("audit events").events,
            [
                store.table_count("fee_payouts"),
                store.table_count("fee_payout_state_index"),
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
    fn fee_payout_request_rolls_back_every_write_failpoint() {
        for failpoint in [
            FeePayoutBundleFailpoint::Encode,
            FeePayoutBundleFailpoint::Record,
            FeePayoutBundleFailpoint::StateIndex,
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
    fn fee_payout_success_and_scan_roll_back_every_write_failpoint() {
        for failpoint in [
            FeePayoutBundleFailpoint::Encode,
            FeePayoutBundleFailpoint::Record,
            FeePayoutBundleFailpoint::StateIndex,
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
            FeePayoutBundleFailpoint::StateIndex,
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
                FeePayoutBundleFailpoint::StateIndex,
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
                    "UPDATE bridge_metadata SET application_schema_version = 7 WHERE id = 1",
                    params![],
                )
            })
            .expect("corrupt schema");
        drop(store);
        assert_eq!(
            StableStore::reopen(unknown).err(),
            Some(StorageError::UnsupportedSchemaVersion(7))
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
                    "UPDATE singleton_state SET counters = X'06FF' WHERE id = 1",
                    params![],
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
    fn reserved_memory_ids_are_never_reassigned() {
        assert_eq!(RETIRED_STABLE_STRUCTURE_MEMORY_IDS, 0..=32);
        assert_eq!(SQLITE_MEMORY_ID, MemoryId::new(120));
    }
}
